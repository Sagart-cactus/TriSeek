use crate::build::BuildConfig;
use crate::error::SearchIndexError;
use ignore::WalkBuilder;
use search_core::{RepoStats, classify_repo};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use xxhash_rust::xxh3::xxh3_64;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub include_hidden: bool,
    pub include_binary: bool,
    pub max_file_size: Option<u64>,
}

impl From<&BuildConfig> for ScanOptions {
    fn from(value: &BuildConfig) -> Self {
        Self {
            include_hidden: value.include_hidden,
            include_binary: value.include_binary,
            max_file_size: value.max_file_size,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub absolute_path: PathBuf,
    pub relative_path: String,
    pub file_name: String,
    pub extension: Option<String>,
    pub contents: Vec<u8>,
    pub file_size: u64,
    pub modified_unix_secs: i64,
    pub content_hash: u64,
}

#[derive(Debug, Clone)]
pub struct ScanSummary {
    pub repo_stats: RepoStats,
    pub files: Vec<ScannedFile>,
}

pub fn scan_repository(
    repo_root: &Path,
    options: &ScanOptions,
) -> Result<ScanSummary, SearchIndexError> {
    let mut files = Vec::new();
    let repo_stats = walk_repository(repo_root, options, |file| {
        files.push(file);
        Ok(())
    })?;
    Ok(ScanSummary { repo_stats, files })
}

pub fn walk_repository<F>(
    repo_root: &Path,
    options: &ScanOptions,
    mut on_file: F,
) -> Result<RepoStats, SearchIndexError>
where
    F: FnMut(ScannedFile) -> Result<(), SearchIndexError>,
{
    let repo_name = repo_root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo_root.display().to_string());
    let commit_sha = git_head(repo_root).unwrap_or_else(|_| "unresolved".to_string());
    let mut repo_stats = RepoStats {
        repo_name,
        repo_root: repo_root.display().to_string(),
        commit_sha,
        ..RepoStats::default()
    };
    let mut languages = HashMap::<String, u64>::new();

    let mut builder = WalkBuilder::new(repo_root);
    builder.hidden(!options.include_hidden);
    builder.git_ignore(true);
    builder.git_exclude(true);
    builder.git_global(true);
    builder.ignore(true);
    builder.follow_links(false);
    builder.standard_filters(true);
    let walker = builder.build();

    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
        {
            continue;
        }

        let path = entry.into_path();
        let metadata = fs::metadata(&path)?;
        let size = metadata.len();
        repo_stats.tracked_files += 1;
        repo_stats.total_disk_bytes += size;

        if let Some(max_file_size) = options.max_file_size {
            if size > max_file_size {
                continue;
            }
        }

        let contents = fs::read(&path)?;
        let is_binary = !options.include_binary && looks_binary(&contents);
        if is_binary {
            repo_stats.skipped_binary_files += 1;
            continue;
        }

        let relative_path = normalize_relative(repo_root, &path);
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let extension = path
            .extension()
            .map(|ext| ext.to_string_lossy().to_ascii_lowercase());
        *languages
            .entry(extension.clone().unwrap_or_else(|| "<none>".to_string()))
            .or_default() += 1;

        repo_stats.searchable_files += 1;
        repo_stats.searchable_bytes += size;

        let modified_unix_secs = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default();

        on_file(ScannedFile {
            absolute_path: path,
            relative_path,
            file_name,
            extension,
            file_size: size,
            modified_unix_secs,
            content_hash: xxh3_64(&contents),
            contents,
        })?;
    }

    let mut languages: Vec<(String, u64)> = languages.into_iter().collect();
    languages.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    repo_stats.languages = languages;
    repo_stats.category = Some(classify_repo(
        repo_stats.searchable_files,
        repo_stats.searchable_bytes,
    ));

    Ok(repo_stats)
}

/// Parallel walk for index building — collects all files using multiple threads.
pub fn walk_repository_parallel(
    repo_root: &Path,
    options: &ScanOptions,
) -> Result<(RepoStats, Vec<ScannedFile>), SearchIndexError> {
    let repo_name = repo_root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo_root.display().to_string());
    let commit_sha = git_head(repo_root).unwrap_or_else(|_| "unresolved".to_string());

    let mut builder = WalkBuilder::new(repo_root);
    builder.hidden(!options.include_hidden);
    builder.git_ignore(true);
    builder.git_exclude(true);
    builder.git_global(true);
    builder.ignore(true);
    builder.follow_links(false);
    builder.standard_filters(true);
    builder.threads(num_cpus::get().min(8));

    let files = Mutex::new(Vec::new());
    let stats = Mutex::new(RepoStats {
        repo_name,
        repo_root: repo_root.display().to_string(),
        commit_sha,
        ..RepoStats::default()
    });
    let languages = Mutex::new(HashMap::<String, u64>::new());
    let opts = options.clone();

    builder.build_parallel().run(|| {
        Box::new(|entry| {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => return ignore::WalkState::Continue,
            };
            if !entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
            {
                return ignore::WalkState::Continue;
            }

            let path = entry.into_path();
            let metadata = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => return ignore::WalkState::Continue,
            };
            let size = metadata.len();

            {
                let mut s = stats.lock().unwrap();
                s.tracked_files += 1;
                s.total_disk_bytes += size;
            }

            if let Some(max_file_size) = opts.max_file_size {
                if size > max_file_size {
                    return ignore::WalkState::Continue;
                }
            }

            let contents = match fs::read(&path) {
                Ok(c) => c,
                Err(_) => return ignore::WalkState::Continue,
            };
            let is_binary = !opts.include_binary && looks_binary(&contents);
            if is_binary {
                stats.lock().unwrap().skipped_binary_files += 1;
                return ignore::WalkState::Continue;
            }

            let relative_path = normalize_relative(repo_root, &path);
            let file_name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let extension = path
                .extension()
                .map(|ext| ext.to_string_lossy().to_ascii_lowercase());

            {
                let mut s = stats.lock().unwrap();
                s.searchable_files += 1;
                s.searchable_bytes += size;
            }
            {
                *languages
                    .lock()
                    .unwrap()
                    .entry(extension.clone().unwrap_or_else(|| "<none>".to_string()))
                    .or_default() += 1;
            }

            let modified_unix_secs = metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or_default();

            let content_hash = xxh3_64(&contents);
            files.lock().unwrap().push(ScannedFile {
                absolute_path: path,
                relative_path,
                file_name,
                extension,
                file_size: size,
                modified_unix_secs,
                content_hash,
                contents,
            });

            ignore::WalkState::Continue
        })
    });

    let mut repo_stats = stats.into_inner().unwrap();
    let mut lang_vec: Vec<(String, u64)> = languages.into_inner().unwrap().into_iter().collect();
    lang_vec.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    repo_stats.languages = lang_vec;
    repo_stats.category = Some(classify_repo(
        repo_stats.searchable_files,
        repo_stats.searchable_bytes,
    ));

    Ok((repo_stats, files.into_inner().unwrap()))
}

fn git_head(repo_root: &Path) -> Result<String, SearchIndexError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("rev-parse")
        .arg("HEAD")
        .output()?;
    if !output.status.success() {
        return Err(SearchIndexError::Command(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn normalize_relative(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn looks_binary(contents: &[u8]) -> bool {
    if contents.iter().take(4096).any(|byte| *byte == 0) {
        return true;
    }
    if std::str::from_utf8(contents).is_ok() {
        return false;
    }

    let sample = &contents[..contents.len().min(4096)];
    let printable = sample
        .iter()
        .filter(|byte| matches!(byte, b'\n' | b'\r' | b'\t' | 0x20..=0x7e))
        .count();
    let suspicious_controls = sample
        .iter()
        .filter(|byte| matches!(byte, 0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f))
        .count();
    let ratio = printable as f64 / sample.len().max(1) as f64;
    suspicious_controls * 200 >= sample.len().max(1)
        || (suspicious_controls > 0 && ratio < 0.95)
        || ratio < 0.85
}
