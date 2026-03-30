use crate::error::SearchIndexError;
use crate::fastindex::write_fast_index;
use crate::model::{
    DeltaSnapshot, DocumentRecord, NamePostingEntry, PersistedIndex, PostingListEntry,
    SCHEMA_VERSION,
};
use crate::storage::{
    fast_index_path, load_base, persist_base, persist_delta, persist_metadata, remove_delta,
};
use crate::walker::{
    ScanOptions, ScanSummary, ScannedFile, scan_repository, walk_repository,
    walk_repository_parallel,
};
use search_core::{BuildStats, FileFingerprint, IndexMetadata, RepoStats, trigrams_from_bytes};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct BuildConfig {
    pub include_hidden: bool,
    pub include_binary: bool,
    pub max_file_size: Option<u64>,
    pub merge_threshold_ratio: f32,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            include_hidden: false,
            include_binary: false,
            max_file_size: None,
            merge_threshold_ratio: 0.25,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateOutcome {
    pub metadata: IndexMetadata,
    pub rebuilt_full: bool,
}

pub fn measure_repository(
    repo_root: &Path,
    config: &BuildConfig,
) -> Result<RepoStats, SearchIndexError> {
    walk_repository(repo_root, &ScanOptions::from(config), |_| Ok(()))
}

pub fn build_index(
    repo_root: &Path,
    index_dir: &Path,
    config: &BuildConfig,
) -> Result<IndexMetadata, SearchIndexError> {
    let started = Instant::now();
    let (repo_stats, files) = walk_repository_parallel(repo_root, &ScanOptions::from(config))?;
    let mut accumulator = BuildAccumulator::new(1);
    for file in files {
        accumulator.push(file);
    }
    let persisted = accumulator.finish(repo_root, repo_stats, started.elapsed().as_millis());
    persist_full_index(index_dir, persisted)
}

pub fn update_index(
    repo_root: &Path,
    index_dir: &Path,
    config: &BuildConfig,
) -> Result<UpdateOutcome, SearchIndexError> {
    let base = load_base(index_dir)?;
    let started = Instant::now();
    let scan = scan_repository(repo_root, &ScanOptions::from(config))?;

    let base_by_path: HashMap<&str, &DocumentRecord> = base
        .docs
        .iter()
        .map(|doc| (doc.relative_path.as_str(), doc))
        .collect();
    let mut current_paths = HashMap::<&str, &ScannedFile>::new();
    let mut delta_files = Vec::new();
    let mut removed_paths = Vec::new();

    for file in &scan.files {
        current_paths.insert(file.relative_path.as_str(), file);
        match base_by_path.get(file.relative_path.as_str()) {
            Some(previous)
                if previous.fingerprint.size == file.file_size
                    && previous.fingerprint.modified_unix_secs == file.modified_unix_secs
                    && previous.fingerprint.hash == file.content_hash => {}
            Some(_) => {
                removed_paths.push(file.relative_path.clone());
                delta_files.push(file.clone());
            }
            None => delta_files.push(file.clone()),
        }
    }

    for path in base_by_path.keys() {
        if !current_paths.contains_key(path) {
            removed_paths.push((*path).to_string());
        }
    }

    let delta_ratio = if base.docs.is_empty() {
        1.0
    } else {
        (delta_files.len() + removed_paths.len()) as f32 / base.docs.len() as f32
    };

    if delta_ratio >= config.merge_threshold_ratio {
        let metadata = build_index(repo_root, index_dir, config)?;
        return Ok(UpdateOutcome {
            metadata,
            rebuilt_full: true,
        });
    }

    if delta_files.is_empty() && removed_paths.is_empty() {
        let metadata = IndexMetadata {
            schema_version: SCHEMA_VERSION,
            repo_stats: scan.repo_stats,
            build_stats: base.build_stats.clone(),
            delta_docs: 0,
            delta_removed_paths: 0,
        };
        remove_delta(index_dir)?;
        persist_metadata(index_dir, &metadata)?;
        return Ok(UpdateOutcome {
            metadata,
            rebuilt_full: false,
        });
    }

    let next_doc_id = base.docs.iter().map(|doc| doc.doc_id).max().unwrap_or(0) + 1;
    let build_stats = BuildStats {
        completed_at: BuildStats::completed_now(),
        docs_indexed: delta_files.len() as u64,
        files_skipped: scan
            .repo_stats
            .tracked_files
            .saturating_sub(scan.repo_stats.searchable_files),
        total_postings: 0,
        index_bytes: 0,
        build_millis: 0,
        update_millis: Some(started.elapsed().as_millis()),
    };

    let mut delta = build_delta_snapshot(
        repo_root,
        scan,
        delta_files,
        removed_paths,
        next_doc_id,
        build_stats,
    );
    let delta_size = persist_delta(index_dir, &delta)?;
    delta.build_stats.index_bytes = delta_size;
    let delta_size = persist_delta(index_dir, &delta)?;
    let metadata = IndexMetadata {
        schema_version: SCHEMA_VERSION,
        repo_stats: delta.repo_stats.clone(),
        build_stats: delta.build_stats.clone(),
        delta_docs: delta.docs.len() as u64,
        delta_removed_paths: delta.removed_paths.len() as u64,
    };
    persist_metadata(index_dir, &metadata)?;

    Ok(UpdateOutcome {
        metadata: IndexMetadata {
            build_stats: BuildStats {
                index_bytes: delta_size,
                ..metadata.build_stats
            },
            ..metadata
        },
        rebuilt_full: false,
    })
}

fn persist_full_index(
    index_dir: &Path,
    mut persisted: PersistedIndex,
) -> Result<IndexMetadata, SearchIndexError> {
    let size = persist_base(index_dir, &persisted)?;
    persisted.build_stats.index_bytes = size;
    let size = persist_base(index_dir, &persisted)?;
    // Also write fast binary index for mmap-based loading
    let fast_size = write_fast_index(&fast_index_path(index_dir), &persisted, None)?;
    remove_delta(index_dir)?;
    let metadata = IndexMetadata {
        schema_version: SCHEMA_VERSION,
        repo_stats: persisted.repo_stats.clone(),
        build_stats: BuildStats {
            index_bytes: size + fast_size,
            ..persisted.build_stats.clone()
        },
        delta_docs: 0,
        delta_removed_paths: 0,
    };
    persist_metadata(index_dir, &metadata)?;
    Ok(metadata)
}

fn build_delta_snapshot(
    repo_root: &Path,
    scan: ScanSummary,
    files: Vec<ScannedFile>,
    removed_paths: Vec<String>,
    next_doc_id: u32,
    build_stats: BuildStats,
) -> DeltaSnapshot {
    let indexes = build_postings(&files, next_doc_id);
    DeltaSnapshot {
        schema_version: SCHEMA_VERSION,
        repo_root: repo_root.display().to_string(),
        repo_stats: scan.repo_stats,
        build_stats: BuildStats {
            total_postings: indexes.total_postings,
            ..build_stats
        },
        removed_paths,
        docs: indexes.docs,
        content_postings: indexes.content_postings,
        path_postings: indexes.path_postings,
        filename_map: indexes.filename_map,
        extension_map: indexes.extension_map,
    }
}

struct BuiltIndexes {
    docs: Vec<DocumentRecord>,
    content_postings: Vec<PostingListEntry>,
    path_postings: Vec<PostingListEntry>,
    filename_map: Vec<NamePostingEntry>,
    extension_map: Vec<NamePostingEntry>,
    total_postings: u64,
}

struct BuildAccumulator {
    next_doc_id: u32,
    docs: Vec<DocumentRecord>,
    content_postings: HashMap<u32, Vec<u32>>,
    path_postings: HashMap<u32, Vec<u32>>,
    filename_map: HashMap<String, Vec<u32>>,
    extension_map: HashMap<String, Vec<u32>>,
    total_postings: u64,
}

impl BuildAccumulator {
    fn new(next_doc_id: u32) -> Self {
        Self {
            next_doc_id,
            docs: Vec::new(),
            content_postings: HashMap::new(),
            path_postings: HashMap::new(),
            filename_map: HashMap::new(),
            extension_map: HashMap::new(),
            total_postings: 0,
        }
    }

    fn push(&mut self, file: ScannedFile) {
        let doc_id = self.next_doc_id;
        self.next_doc_id += 1;
        self.docs.push(DocumentRecord {
            doc_id,
            relative_path: file.relative_path.clone(),
            file_name: file.file_name.clone(),
            extension: file.extension.clone(),
            fingerprint: FileFingerprint {
                size: file.file_size,
                modified_unix_secs: file.modified_unix_secs,
                hash: file.content_hash,
            },
        });
        for trigram in trigrams_from_bytes(&file.contents) {
            self.content_postings
                .entry(trigram)
                .or_default()
                .push(doc_id);
            self.total_postings += 1;
        }
        for trigram in trigrams_from_bytes(file.relative_path.as_bytes()) {
            self.path_postings.entry(trigram).or_default().push(doc_id);
        }
        self.filename_map
            .entry(file.file_name.to_ascii_lowercase())
            .or_default()
            .push(doc_id);
        if let Some(extension) = &file.extension {
            self.extension_map
                .entry(extension.to_ascii_lowercase())
                .or_default()
                .push(doc_id);
        }
    }

    fn finish(self, repo_root: &Path, repo_stats: RepoStats, build_millis: u128) -> PersistedIndex {
        PersistedIndex {
            schema_version: SCHEMA_VERSION,
            repo_root: repo_root.display().to_string(),
            repo_stats: repo_stats.clone(),
            build_stats: BuildStats {
                completed_at: BuildStats::completed_now(),
                docs_indexed: self.docs.len() as u64,
                files_skipped: repo_stats
                    .tracked_files
                    .saturating_sub(repo_stats.searchable_files),
                total_postings: self.total_postings,
                index_bytes: 0,
                build_millis,
                update_millis: None,
            },
            docs: self.docs,
            content_postings: postings_to_entries(self.content_postings),
            path_postings: postings_to_entries(self.path_postings),
            filename_map: names_to_entries(self.filename_map),
            extension_map: names_to_entries(self.extension_map),
        }
    }
}

fn build_postings(files: &[ScannedFile], starting_doc_id: u32) -> BuiltIndexes {
    let mut docs = Vec::with_capacity(files.len());
    let mut content_postings = HashMap::<u32, Vec<u32>>::new();
    let mut path_postings = HashMap::<u32, Vec<u32>>::new();
    let mut filename_map = HashMap::<String, Vec<u32>>::new();
    let mut extension_map = HashMap::<String, Vec<u32>>::new();
    let mut total_postings = 0_u64;

    for (offset, file) in files.iter().enumerate() {
        let doc_id = starting_doc_id + offset as u32;
        docs.push(DocumentRecord {
            doc_id,
            relative_path: file.relative_path.clone(),
            file_name: file.file_name.clone(),
            extension: file.extension.clone(),
            fingerprint: FileFingerprint {
                size: file.file_size,
                modified_unix_secs: file.modified_unix_secs,
                hash: file.content_hash,
            },
        });

        for trigram in trigrams_from_bytes(&file.contents) {
            content_postings.entry(trigram).or_default().push(doc_id);
            total_postings += 1;
        }
        for trigram in trigrams_from_bytes(file.relative_path.as_bytes()) {
            path_postings.entry(trigram).or_default().push(doc_id);
        }
        filename_map
            .entry(file.file_name.to_ascii_lowercase())
            .or_default()
            .push(doc_id);
        if let Some(extension) = &file.extension {
            extension_map
                .entry(extension.to_ascii_lowercase())
                .or_default()
                .push(doc_id);
        }
    }

    BuiltIndexes {
        docs,
        content_postings: postings_to_entries(content_postings),
        path_postings: postings_to_entries(path_postings),
        filename_map: names_to_entries(filename_map),
        extension_map: names_to_entries(extension_map),
        total_postings,
    }
}

fn postings_to_entries(mut postings: HashMap<u32, Vec<u32>>) -> Vec<PostingListEntry> {
    let mut entries: Vec<_> = postings
        .drain()
        .map(|(trigram, mut docs)| {
            docs.sort_unstable();
            docs.dedup();
            PostingListEntry { trigram, docs }
        })
        .collect();
    entries.sort_by(|left, right| left.trigram.cmp(&right.trigram));
    entries
}

fn names_to_entries(mut postings: HashMap<String, Vec<u32>>) -> Vec<NamePostingEntry> {
    let mut entries: Vec<_> = postings
        .drain()
        .map(|(key, mut docs)| {
            docs.sort_unstable();
            docs.dedup();
            NamePostingEntry { key, docs }
        })
        .collect();
    entries.sort_by(|left, right| left.key.cmp(&right.key));
    entries
}
