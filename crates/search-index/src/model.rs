use search_core::{
    BuildStats, FileFingerprint, IndexMetadata, RepoStats, SearchHit, SearchMetrics, SearchSummary,
    Trigram,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentRecord {
    pub doc_id: u32,
    pub relative_path: String,
    pub file_name: String,
    pub extension: Option<String>,
    pub fingerprint: FileFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostingListEntry {
    pub trigram: Trigram,
    pub docs: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamePostingEntry {
    pub key: String,
    pub docs: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedIndex {
    pub schema_version: u32,
    pub repo_root: String,
    pub repo_stats: RepoStats,
    pub build_stats: BuildStats,
    pub docs: Vec<DocumentRecord>,
    pub content_postings: Vec<PostingListEntry>,
    pub path_postings: Vec<PostingListEntry>,
    pub filename_map: Vec<NamePostingEntry>,
    pub extension_map: Vec<NamePostingEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaSnapshot {
    pub schema_version: u32,
    pub repo_root: String,
    pub repo_stats: RepoStats,
    pub build_stats: BuildStats,
    pub removed_paths: Vec<String>,
    pub docs: Vec<DocumentRecord>,
    pub content_postings: Vec<PostingListEntry>,
    pub path_postings: Vec<PostingListEntry>,
    pub filename_map: Vec<NamePostingEntry>,
    pub extension_map: Vec<NamePostingEntry>,
}

#[derive(Debug, Clone)]
pub struct RuntimeIndex {
    pub repo_root: PathBuf,
    pub metadata: IndexMetadata,
    pub docs: Vec<DocumentRecord>,
    pub doc_lookup: HashMap<u32, usize>,
    pub path_lookup: HashMap<String, u32>,
    pub content_postings: HashMap<Trigram, Vec<u32>>,
    pub path_postings: HashMap<Trigram, Vec<u32>>,
    pub filename_map: HashMap<String, Vec<u32>>,
    pub extension_map: HashMap<String, Vec<u32>>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchExecution {
    pub hits: Vec<SearchHit>,
    pub summary: SearchSummary,
    pub metrics: SearchMetrics,
}

impl RuntimeIndex {
    pub fn from_snapshots(base: PersistedIndex, delta: Option<DeltaSnapshot>) -> Self {
        let mut active_docs_by_path: HashMap<String, DocumentRecord> = base
            .docs
            .iter()
            .cloned()
            .map(|doc| (doc.relative_path.clone(), doc))
            .collect();
        let mut active_doc_ids: HashSet<u32> = base.docs.iter().map(|doc| doc.doc_id).collect();
        let mut metadata = IndexMetadata {
            schema_version: SCHEMA_VERSION,
            repo_stats: base.repo_stats.clone(),
            build_stats: base.build_stats.clone(),
            delta_docs: 0,
            delta_removed_paths: 0,
        };

        if let Some(delta_snapshot) = delta.as_ref() {
            metadata.repo_stats = delta_snapshot.repo_stats.clone();
            metadata.delta_docs = delta_snapshot.docs.len() as u64;
            metadata.delta_removed_paths = delta_snapshot.removed_paths.len() as u64;

            for removed in &delta_snapshot.removed_paths {
                if let Some(previous) = active_docs_by_path.remove(removed) {
                    active_doc_ids.remove(&previous.doc_id);
                }
            }
            for doc in delta_snapshot.docs.iter().cloned() {
                active_doc_ids.insert(doc.doc_id);
                active_docs_by_path.insert(doc.relative_path.clone(), doc);
            }
        }

        let mut docs: Vec<_> = active_docs_by_path.into_values().collect();
        docs.sort_by(|left, right| left.doc_id.cmp(&right.doc_id));

        let doc_lookup = docs
            .iter()
            .enumerate()
            .map(|(idx, doc)| (doc.doc_id, idx))
            .collect();
        let path_lookup = docs
            .iter()
            .map(|doc| (doc.relative_path.clone(), doc.doc_id))
            .collect();

        let mut content_postings = rebuild_postings(&base.content_postings, &active_doc_ids);
        let mut path_postings = rebuild_postings(&base.path_postings, &active_doc_ids);
        let mut filename_map = rebuild_name_map(&base.filename_map, &active_doc_ids);
        let mut extension_map = rebuild_name_map(&base.extension_map, &active_doc_ids);

        if let Some(delta_snapshot) = delta.as_ref() {
            merge_postings(&mut content_postings, &delta_snapshot.content_postings);
            merge_postings(&mut path_postings, &delta_snapshot.path_postings);
            merge_name_map(&mut filename_map, &delta_snapshot.filename_map);
            merge_name_map(&mut extension_map, &delta_snapshot.extension_map);
        }

        Self {
            repo_root: PathBuf::from(base.repo_root),
            metadata,
            docs,
            doc_lookup,
            path_lookup,
            content_postings,
            path_postings,
            filename_map,
            extension_map,
        }
    }

    pub fn doc(&self, doc_id: u32) -> Option<&DocumentRecord> {
        self.doc_lookup
            .get(&doc_id)
            .and_then(|index| self.docs.get(*index))
    }
}

fn rebuild_postings(
    entries: &[PostingListEntry],
    active_doc_ids: &HashSet<u32>,
) -> HashMap<Trigram, Vec<u32>> {
    let mut map = HashMap::new();
    for entry in entries {
        let mut docs: Vec<u32> = entry
            .docs
            .iter()
            .copied()
            .filter(|doc_id| active_doc_ids.contains(doc_id))
            .collect();
        docs.sort_unstable();
        docs.dedup();
        if !docs.is_empty() {
            map.insert(entry.trigram, docs);
        }
    }
    map
}

fn rebuild_name_map(
    entries: &[NamePostingEntry],
    active_doc_ids: &HashSet<u32>,
) -> HashMap<String, Vec<u32>> {
    let mut map = HashMap::new();
    for entry in entries {
        let mut docs: Vec<u32> = entry
            .docs
            .iter()
            .copied()
            .filter(|doc_id| active_doc_ids.contains(doc_id))
            .collect();
        docs.sort_unstable();
        docs.dedup();
        if !docs.is_empty() {
            map.insert(entry.key.clone(), docs);
        }
    }
    map
}

fn merge_postings(destination: &mut HashMap<Trigram, Vec<u32>>, delta: &[PostingListEntry]) {
    for entry in delta {
        destination
            .entry(entry.trigram)
            .or_default()
            .extend(entry.docs.iter().copied());
        if let Some(docs) = destination.get_mut(&entry.trigram) {
            docs.sort_unstable();
            docs.dedup();
        }
    }
}

fn merge_name_map(destination: &mut HashMap<String, Vec<u32>>, delta: &[NamePostingEntry]) {
    for entry in delta {
        destination
            .entry(entry.key.clone())
            .or_default()
            .extend(entry.docs.iter().copied());
        if let Some(docs) = destination.get_mut(&entry.key) {
            docs.sort_unstable();
            docs.dedup();
        }
    }
}
