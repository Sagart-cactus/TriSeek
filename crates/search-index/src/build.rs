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
    walk_repository_parallel_with_progress,
};
use rayon::prelude::*;
use search_core::{BuildStats, FileFingerprint, IndexMetadata, RepoStats, trigrams_from_bytes};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicU64, Ordering},
};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum BuildPhase {
    #[default]
    Idle = 0,
    Scanning = 1,
    Indexing = 2,
    Persisting = 3,
    Finished = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BuildProgressSnapshot {
    pub phase: BuildPhase,
    pub tracked_files: u64,
    pub searchable_files: u64,
    pub searchable_bytes: u64,
    pub total_disk_bytes: u64,
    pub total_files: u64,
    pub total_bytes: u64,
    pub indexed_files: u64,
    pub indexed_bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub struct BuildProgress {
    inner: Arc<BuildProgressInner>,
}

#[derive(Debug, Default)]
struct BuildProgressInner {
    phase: AtomicU8,
    tracked_files: AtomicU64,
    searchable_files: AtomicU64,
    searchable_bytes: AtomicU64,
    total_disk_bytes: AtomicU64,
    total_files: AtomicU64,
    total_bytes: AtomicU64,
    indexed_files: AtomicU64,
    indexed_bytes: AtomicU64,
}

impl BuildProgress {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> BuildProgressSnapshot {
        BuildProgressSnapshot {
            phase: BuildPhase::from_u8(self.inner.phase.load(Ordering::Relaxed)),
            tracked_files: self.inner.tracked_files.load(Ordering::Relaxed),
            searchable_files: self.inner.searchable_files.load(Ordering::Relaxed),
            searchable_bytes: self.inner.searchable_bytes.load(Ordering::Relaxed),
            total_disk_bytes: self.inner.total_disk_bytes.load(Ordering::Relaxed),
            total_files: self.inner.total_files.load(Ordering::Relaxed),
            total_bytes: self.inner.total_bytes.load(Ordering::Relaxed),
            indexed_files: self.inner.indexed_files.load(Ordering::Relaxed),
            indexed_bytes: self.inner.indexed_bytes.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn set_phase(&self, phase: BuildPhase) {
        self.inner.phase.store(phase as u8, Ordering::Relaxed);
    }

    pub(crate) fn record_tracked_file(&self, file_size: u64) {
        self.inner.tracked_files.fetch_add(1, Ordering::Relaxed);
        self.inner
            .total_disk_bytes
            .fetch_add(file_size, Ordering::Relaxed);
    }

    pub(crate) fn record_searchable_file(&self, file_size: u64) {
        self.inner.searchable_files.fetch_add(1, Ordering::Relaxed);
        self.inner
            .searchable_bytes
            .fetch_add(file_size, Ordering::Relaxed);
    }

    pub(crate) fn set_index_totals(&self, total_files: u64, total_bytes: u64) {
        self.inner.total_files.store(total_files, Ordering::Relaxed);
        self.inner.total_bytes.store(total_bytes, Ordering::Relaxed);
        self.inner.indexed_files.store(0, Ordering::Relaxed);
        self.inner.indexed_bytes.store(0, Ordering::Relaxed);
    }

    pub(crate) fn record_indexed_file(&self, file_size: u64) {
        self.inner.indexed_files.fetch_add(1, Ordering::Relaxed);
        self.inner
            .indexed_bytes
            .fetch_add(file_size, Ordering::Relaxed);
    }
}

impl BuildPhase {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Scanning,
            2 => Self::Indexing,
            3 => Self::Persisting,
            4 => Self::Finished,
            _ => Self::Idle,
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
    build_index_with_progress(repo_root, index_dir, config, None)
}

pub fn build_index_with_progress(
    repo_root: &Path,
    index_dir: &Path,
    config: &BuildConfig,
    progress: Option<&BuildProgress>,
) -> Result<IndexMetadata, SearchIndexError> {
    let started = Instant::now();
    if let Some(progress) = progress {
        progress.set_phase(BuildPhase::Scanning);
    }
    let (repo_stats, files) =
        walk_repository_parallel_with_progress(repo_root, &ScanOptions::from(config), progress)?;
    if let Some(progress) = progress {
        progress.set_phase(BuildPhase::Indexing);
        progress.set_index_totals(repo_stats.searchable_files, repo_stats.searchable_bytes);
    }
    let accumulator = files
        .into_par_iter()
        .enumerate()
        .fold(
            || BuildAccumulator::new(0),
            |mut acc, (idx, file)| {
                let doc_id = 1u32 + idx as u32;
                let file_size = file.file_size;
                acc.push_with_id(file, doc_id);
                if let Some(p) = progress {
                    p.record_indexed_file(file_size);
                }
                acc
            },
        )
        .reduce_with(|mut a, b| {
            a.merge(b);
            a
        })
        .unwrap_or_else(|| BuildAccumulator::new(1));
    if let Some(progress) = progress {
        progress.set_phase(BuildPhase::Persisting);
    }
    let persisted = accumulator.finish(repo_root, repo_stats, started.elapsed().as_millis());
    let metadata = persist_full_index(index_dir, persisted)?;
    if let Some(progress) = progress {
        progress.set_phase(BuildPhase::Finished);
    }
    Ok(metadata)
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

    let delta = build_delta_snapshot(
        repo_root,
        scan,
        delta_files,
        removed_paths,
        next_doc_id,
        build_stats,
    );
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

/// Apply a pre-computed set of changes to the index without a full repository scan.
/// Used by the background watcher to avoid re-scanning unchanged files.
///
/// `added_or_modified`: files that were created or changed (already scanned).
/// `removed_relative_paths`: repo-relative paths of deleted files.
pub fn apply_incremental_changes(
    repo_root: &Path,
    index_dir: &Path,
    added_or_modified: Vec<ScannedFile>,
    removed_relative_paths: Vec<String>,
    config: &BuildConfig,
) -> Result<UpdateOutcome, SearchIndexError> {
    let base = load_base(index_dir)?;

    // For changed files, also remove the old version from the base.
    let base_paths: std::collections::HashSet<&str> =
        base.docs.iter().map(|d| d.relative_path.as_str()).collect();
    let mut removed_paths = removed_relative_paths;
    let mut delta_files = Vec::new();

    for file in added_or_modified {
        // If the file already exists in base, remove the old version.
        if base_paths.contains(file.relative_path.as_str()) {
            removed_paths.push(file.relative_path.clone());
        }
        delta_files.push(file);
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
        // Nothing actually changed; return the existing base metadata.
        let metadata = IndexMetadata {
            schema_version: SCHEMA_VERSION,
            repo_stats: base.repo_stats.clone(),
            build_stats: base.build_stats.clone(),
            delta_docs: 0,
            delta_removed_paths: 0,
        };
        return Ok(UpdateOutcome {
            metadata,
            rebuilt_full: false,
        });
    }

    let next_doc_id = base.docs.iter().map(|doc| doc.doc_id).max().unwrap_or(0) + 1;
    let build_stats = BuildStats {
        completed_at: BuildStats::completed_now(),
        docs_indexed: delta_files.len() as u64,
        files_skipped: 0,
        total_postings: 0,
        index_bytes: 0,
        build_millis: 0,
        update_millis: Some(0),
    };

    // Reuse existing repo_stats from the base — watcher doesn't do a full scan.
    let fake_scan = ScanSummary {
        repo_stats: base.repo_stats.clone(),
        files: vec![],
    };
    let delta = build_delta_snapshot(
        repo_root,
        fake_scan,
        delta_files,
        removed_paths,
        next_doc_id,
        build_stats,
    );
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
    persisted: PersistedIndex,
) -> Result<IndexMetadata, SearchIndexError> {
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

    fn push_with_id(&mut self, file: ScannedFile, doc_id: u32) {
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

    fn merge(&mut self, other: BuildAccumulator) {
        self.docs.extend(other.docs);
        for (trigram, ids) in other.content_postings {
            self.content_postings
                .entry(trigram)
                .or_default()
                .extend(ids);
        }
        for (trigram, ids) in other.path_postings {
            self.path_postings.entry(trigram).or_default().extend(ids);
        }
        for (name, ids) in other.filename_map {
            self.filename_map.entry(name).or_default().extend(ids);
        }
        for (ext, ids) in other.extension_map {
            self.extension_map.entry(ext).or_default().extend(ids);
        }
        self.total_postings += other.total_postings;
        self.next_doc_id = self.next_doc_id.max(other.next_doc_id);
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
    entries.sort_by_key(|e| e.trigram);
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
