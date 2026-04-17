use crate::build::BuildConfig;
use crate::error::SearchIndexError;
use ignore::WalkBuilder;
use search_core::{RepoStats, classify_repo};
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use xxhash_rust::xxh3::xxh3_64;

/// Top-level hidden directories that are typically committed, human-authored,
/// and useful to search by default.
pub const DEFAULT_SEARCHABLE_HIDDEN_DIRS: &[&str] = &[
    ".github",
    ".gitlab",
    ".circleci",
    ".buildkite",
    ".azuredevops",
    ".ci",
    ".devcontainer",
    ".vscode",
    ".idea",
    ".run",
    ".husky",
    ".changeset",
    ".storybook",
    ".cargo",
    ".mvn",
    ".claude",
    ".cursor",
    ".codex",
    ".well-known",
    ".platform",
    ".ebextensions",
    ".streamlit",
    ".dvc",
];

/// Top-level hidden files that are usually important repo config and should be
/// searchable without opting into every hidden path.
pub const DEFAULT_SEARCHABLE_HIDDEN_FILES: &[&str] = &[
    ".gitignore",
    ".gitattributes",
    ".gitmodules",
    ".git-blame-ignore-revs",
    ".mailmap",
    ".editorconfig",
    ".gitlab-ci.yml",
    ".dockerignore",
    ".npmrc",
    ".nvmrc",
    ".node-version",
    ".python-version",
    ".ruby-version",
    ".tool-versions",
    ".terraform-version",
    ".terraform.lock.hcl",
    ".bazelrc",
    ".buckconfig",
    ".clang-format",
    ".clang-tidy",
    ".coveragerc",
    ".ruff.toml",
    ".yamllint",
    ".prettierignore",
    ".prettierrc",
    ".prettierrc.json",
    ".prettierrc.yml",
    ".prettierrc.yaml",
    ".prettierrc.js",
    ".prettierrc.cjs",
    ".prettierrc.mjs",
    ".eslintignore",
    ".eslintrc",
    ".eslintrc.json",
    ".eslintrc.yml",
    ".eslintrc.yaml",
    ".eslintrc.js",
    ".eslintrc.cjs",
    ".stylelintignore",
    ".stylelintrc",
    ".stylelintrc.json",
    ".stylelintrc.yml",
    ".stylelintrc.yaml",
    ".stylelintrc.js",
    ".stylelintrc.cjs",
    ".markdownlint.json",
    ".markdownlint.jsonc",
    ".env.example",
    ".env.sample",
    ".env.template",
];

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

#[derive(Debug)]
enum CandidateFile {
    Searchable(ScannedFile),
    Skipped { size: u64, binary: bool },
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

    let mut process_path = |path: PathBuf| -> Result<(), SearchIndexError> {
        match inspect_file_candidate(repo_root, &path, options)? {
            Some(CandidateFile::Searchable(file)) => {
                repo_stats.tracked_files += 1;
                repo_stats.total_disk_bytes += file.file_size;
                repo_stats.searchable_files += 1;
                repo_stats.searchable_bytes += file.file_size;
                *languages
                    .entry(
                        file.extension
                            .clone()
                            .unwrap_or_else(|| "<none>".to_string()),
                    )
                    .or_default() += 1;
                on_file(file)?;
            }
            Some(CandidateFile::Skipped { size, binary }) => {
                repo_stats.tracked_files += 1;
                repo_stats.total_disk_bytes += size;
                if binary {
                    repo_stats.skipped_binary_files += 1;
                }
            }
            None => {}
        }
        Ok(())
    };

    let mut builder = WalkBuilder::new(repo_root);
    configure_walk_builder(&mut builder, options.include_hidden);
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
        process_path(entry.into_path())?;
    }

    if !options.include_hidden {
        walk_default_searchable_hidden_files(repo_root, process_path)?;
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
    configure_walk_builder(&mut builder, options.include_hidden);
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

            match inspect_file_candidate(repo_root, &entry.into_path(), &opts) {
                Ok(Some(CandidateFile::Searchable(file))) => {
                    {
                        let mut s = stats.lock().unwrap();
                        s.tracked_files += 1;
                        s.total_disk_bytes += file.file_size;
                        s.searchable_files += 1;
                        s.searchable_bytes += file.file_size;
                    }
                    {
                        *languages
                            .lock()
                            .unwrap()
                            .entry(
                                file.extension
                                    .clone()
                                    .unwrap_or_else(|| "<none>".to_string()),
                            )
                            .or_default() += 1;
                    }
                    files.lock().unwrap().push(file);
                }
                Ok(Some(CandidateFile::Skipped { size, binary })) => {
                    let mut s = stats.lock().unwrap();
                    s.tracked_files += 1;
                    s.total_disk_bytes += size;
                    if binary {
                        s.skipped_binary_files += 1;
                    }
                }
                Ok(None) | Err(_) => {}
            }

            ignore::WalkState::Continue
        })
    });

    if !options.include_hidden {
        walk_default_searchable_hidden_files(repo_root, |path| {
            match inspect_file_candidate(repo_root, &path, options)? {
                Some(CandidateFile::Searchable(file)) => {
                    {
                        let mut s = stats.lock().unwrap();
                        s.tracked_files += 1;
                        s.total_disk_bytes += file.file_size;
                        s.searchable_files += 1;
                        s.searchable_bytes += file.file_size;
                    }
                    {
                        *languages
                            .lock()
                            .unwrap()
                            .entry(
                                file.extension
                                    .clone()
                                    .unwrap_or_else(|| "<none>".to_string()),
                            )
                            .or_default() += 1;
                    }
                    files.lock().unwrap().push(file);
                }
                Some(CandidateFile::Skipped { size, binary }) => {
                    let mut s = stats.lock().unwrap();
                    s.tracked_files += 1;
                    s.total_disk_bytes += size;
                    if binary {
                        s.skipped_binary_files += 1;
                    }
                }
                None => {}
            }
            Ok(())
        })?;
    }

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

pub fn default_searchable_hidden_roots(repo_root: &Path) -> Vec<PathBuf> {
    DEFAULT_SEARCHABLE_HIDDEN_FILES
        .iter()
        .chain(DEFAULT_SEARCHABLE_HIDDEN_DIRS.iter())
        .map(|name| repo_root.join(name))
        .filter(|path| path.exists())
        .collect()
}

fn configure_walk_builder(builder: &mut WalkBuilder, include_hidden: bool) {
    builder.hidden(!include_hidden);
    builder.git_ignore(true);
    builder.git_exclude(true);
    builder.git_global(true);
    builder.ignore(true);
    builder.follow_links(false);
    builder.standard_filters(true);
}

fn walk_default_searchable_hidden_files<F>(
    repo_root: &Path,
    mut on_path: F,
) -> Result<(), SearchIndexError>
where
    F: FnMut(PathBuf) -> Result<(), SearchIndexError>,
{
    for root in default_searchable_hidden_roots(repo_root) {
        if root.is_file() {
            on_path(root)?;
            continue;
        }

        let mut builder = WalkBuilder::new(&root);
        configure_walk_builder(&mut builder, true);
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
            let relative_path = normalize_relative(repo_root, &path);
            if !is_path_searchable(Path::new(&relative_path), false) {
                continue;
            }

            on_path(path)?;
        }
    }

    Ok(())
}

fn inspect_file_candidate(
    repo_root: &Path,
    path: &Path,
    options: &ScanOptions,
) -> Result<Option<CandidateFile>, SearchIndexError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => return Ok(None),
    };

    let relative_path = normalize_relative(repo_root, path);
    if !is_path_searchable(Path::new(&relative_path), options.include_hidden) {
        return Ok(None);
    }

    let size = metadata.len();
    if let Some(max_file_size) = options.max_file_size
        && size > max_file_size
    {
        return Ok(Some(CandidateFile::Skipped {
            size,
            binary: false,
        }));
    }

    let contents = fs::read(path)?;
    if !options.include_binary && looks_binary(&contents) {
        return Ok(Some(CandidateFile::Skipped { size, binary: true }));
    }

    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase());
    let modified_unix_secs = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();

    Ok(Some(CandidateFile::Searchable(ScannedFile {
        absolute_path: path.to_path_buf(),
        relative_path,
        file_name,
        extension,
        file_size: size,
        modified_unix_secs,
        content_hash: xxh3_64(&contents),
        contents,
    })))
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

pub(crate) fn normalize_relative(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn is_path_searchable(relative_path: &Path, include_hidden: bool) -> bool {
    if include_hidden {
        return true;
    }

    let mut components = Vec::new();
    for component in relative_path.components() {
        let Component::Normal(component) = component else {
            continue;
        };
        components.push(component.to_string_lossy().into_owned());
    }

    for (index, component) in components.iter().enumerate() {
        if !is_hidden_component(component) {
            continue;
        }

        if index == 0 && DEFAULT_SEARCHABLE_HIDDEN_DIRS.contains(&component.as_str()) {
            continue;
        }
        if index == 0 && DEFAULT_SEARCHABLE_HIDDEN_FILES.contains(&component.as_str()) {
            return components.len() == 1;
        }
        return false;
    }

    true
}

fn is_hidden_component(component: &str) -> bool {
    component.starts_with('.') && component != "." && component != ".."
}

pub(crate) fn looks_binary(contents: &[u8]) -> bool {
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

/// Scan a single file relative to `repo_root`.
/// Returns `None` if the file doesn't exist, exceeds `max_file_size`, or should be skipped.
pub(crate) fn scan_single_file(
    repo_root: &Path,
    abs_path: &Path,
    opts: &ScanOptions,
) -> Result<Option<ScannedFile>, SearchIndexError> {
    match inspect_file_candidate(repo_root, abs_path, opts)? {
        Some(CandidateFile::Searchable(file)) => Ok(Some(file)),
        Some(CandidateFile::Skipped { .. }) | None => Ok(None),
    }
}
