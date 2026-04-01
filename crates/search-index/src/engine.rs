use crate::build::{BuildConfig, build_index, update_index};
use crate::error::SearchIndexError;
use crate::fastindex::FastIndex;
use crate::model::{RuntimeIndex, SearchExecution};
use crate::storage::{
    default_index_dir, fast_index_exists, fast_index_path, load_base, load_delta,
};
use crate::walker::{ScanOptions, scan_repository};
use globset::{Glob, GlobSet, GlobSetBuilder};
use memmap2::Mmap;
use rayon::prelude::*;
use regex::{Regex, RegexBuilder};
use search_core::{
    CaseMode, QueryRequest, SearchHit, SearchKind, SearchLineMatch, SearchMetrics, SearchSummary,
    plan_query, trigrams_from_text,
};
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

enum IndexBackend {
    Fast(Box<FastIndex>),
    Legacy(Box<RuntimeIndex>),
}

pub struct SearchEngine {
    backend: IndexBackend,
    repo_root: std::path::PathBuf,
    metadata: search_core::IndexMetadata,
}

impl SearchEngine {
    pub fn build(
        repo_root: &Path,
        index_dir: Option<&Path>,
        config: &BuildConfig,
    ) -> Result<search_core::IndexMetadata, SearchIndexError> {
        let index_dir = index_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| default_index_dir(repo_root));
        build_index(repo_root, &index_dir, config)
    }

    pub fn update(
        repo_root: &Path,
        index_dir: Option<&Path>,
        config: &BuildConfig,
    ) -> Result<crate::build::UpdateOutcome, SearchIndexError> {
        let index_dir = index_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| default_index_dir(repo_root));
        update_index(repo_root, &index_dir, config)
    }

    pub fn open(index_dir: &Path) -> Result<Self, SearchIndexError> {
        let delta = load_delta(index_dir)?;

        // Prefer fast mmap-based index only when there is no delta overlay.
        // The fast index is a materialized base snapshot; if a delta exists,
        // fall back to the legacy runtime that merges base + delta correctly.
        if fast_index_exists(index_dir) && delta.is_none() {
            let metadata = crate::storage::read_index_metadata(index_dir)?;
            let repo_root = std::path::PathBuf::from(&metadata.repo_stats.repo_root);
            let fast = FastIndex::open(&fast_index_path(index_dir))?;
            return Ok(Self {
                backend: IndexBackend::Fast(Box::new(fast)),
                repo_root,
                metadata,
            });
        }

        // Fall back to legacy bincode index
        let base = load_base(index_dir)?;
        let runtime = RuntimeIndex::from_snapshots(base, delta);
        let metadata = runtime.metadata.clone();
        let repo_root = runtime.repo_root.clone();
        Ok(Self {
            backend: IndexBackend::Legacy(Box::new(runtime)),
            repo_root,
            metadata,
        })
    }

    pub fn metadata(&self) -> &search_core::IndexMetadata {
        &self.metadata
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    pub fn search(&self, request: &QueryRequest) -> Result<SearchExecution, SearchIndexError> {
        let started = Instant::now();
        let plan = plan_query(request);

        let (_filtered_docs, candidate_docs) = match &self.backend {
            IndexBackend::Fast(fast) => {
                let filtered = self.fast_path_filtered_docs(fast, request)?;
                let candidates = match request.kind {
                    SearchKind::Path => filtered.clone(),
                    _ if matches!(plan.strategy, search_core::SearchExecutionStrategy::Indexed) => {
                        self.fast_index_candidates(fast, request, &filtered)?
                    }
                    _ => filtered.clone(),
                };
                (filtered, candidates)
            }
            IndexBackend::Legacy(runtime) => {
                let filtered = self.legacy_path_filtered_docs(runtime, request)?;
                let candidates = match request.kind {
                    SearchKind::Path => filtered.clone(),
                    _ if matches!(plan.strategy, search_core::SearchExecutionStrategy::Indexed) => {
                        self.legacy_index_candidates(runtime, request, &filtered)?
                    }
                    _ => filtered.clone(),
                };
                (filtered, candidates)
            }
        };

        let execution = if matches!(request.kind, SearchKind::Path) {
            self.collect_path_hits(&candidate_docs)
        } else {
            self.collect_content_hits_parallel(request, &candidate_docs)?
        };

        Ok(SearchExecution {
            metrics: SearchMetrics {
                process: search_core::ProcessMetrics {
                    wall_millis: started.elapsed().as_secs_f64() * 1_000.0,
                    user_cpu_millis: None,
                    system_cpu_millis: None,
                    max_rss_kib: None,
                },
                candidate_docs: candidate_docs.len(),
                verified_docs: execution.metrics.verified_docs,
                matches_returned: execution
                    .summary
                    .total_line_matches
                    .max(execution.summary.files_with_matches),
                bytes_scanned: execution.metrics.bytes_scanned,
                index_bytes_read: Some(self.metadata.build_stats.index_bytes),
            },
            ..execution
        })
    }

    pub fn search_direct(
        repo_root: &Path,
        request: &QueryRequest,
        config: &BuildConfig,
    ) -> Result<SearchExecution, SearchIndexError> {
        let started = Instant::now();
        let scan = scan_repository(repo_root, &ScanOptions::from(config))?;
        let matcher = if matches!(request.kind, SearchKind::Path) {
            None
        } else {
            Some(build_matcher(request)?)
        };
        let globset = build_globset(&request.globs)?;
        let mut hits = Vec::new();
        let mut files_with_matches = 0;
        let mut total_line_matches = 0;
        let mut bytes_scanned = 0_u64;
        let mut verified_docs = 0_usize;

        for file in scan.files {
            if !path_matches_filters(
                &file.relative_path,
                &file.file_name,
                file.extension.as_deref(),
                request,
                globset.as_ref(),
            ) {
                continue;
            }

            if matches!(request.kind, SearchKind::Path) {
                files_with_matches += 1;
                hits.push(SearchHit::Path {
                    path: file.relative_path,
                });
                continue;
            }

            verified_docs += 1;
            bytes_scanned += file.file_size;
            let text = String::from_utf8_lossy(&file.contents);
            if let Some(lines) = match_lines(
                &text,
                matcher.as_ref().expect("matcher"),
                request.max_results,
            ) {
                total_line_matches += lines.len();
                files_with_matches += 1;
                hits.push(SearchHit::Content {
                    path: file.relative_path,
                    lines,
                });
            }
        }

        Ok(SearchExecution {
            hits,
            summary: SearchSummary {
                files_with_matches,
                total_line_matches,
            },
            metrics: SearchMetrics {
                process: search_core::ProcessMetrics {
                    wall_millis: started.elapsed().as_secs_f64() * 1_000.0,
                    user_cpu_millis: None,
                    system_cpu_millis: None,
                    max_rss_kib: None,
                },
                candidate_docs: scan.repo_stats.searchable_files as usize,
                verified_docs,
                matches_returned: total_line_matches.max(files_with_matches),
                bytes_scanned,
                index_bytes_read: None,
            },
        })
    }

    // ---- Fast index methods ----

    fn fast_path_filtered_docs(
        &self,
        fast: &FastIndex,
        request: &QueryRequest,
    ) -> Result<Vec<u32>, SearchIndexError> {
        let mut current: Option<Vec<u32>> = None;

        if !request.exact_paths.is_empty() {
            let mut matched = Vec::new();
            for path in &request.exact_paths {
                if let Some(doc_id) = fast.path_to_doc_id(path) {
                    matched.push(doc_id);
                }
            }
            matched.sort_unstable();
            matched.dedup();
            current = intersect_sorted(current, matched);
        }

        if !request.exact_names.is_empty() {
            let mut matched = Vec::new();
            for name in &request.exact_names {
                if let Some(doc_ids) = fast.docs_by_filename(&name.to_ascii_lowercase()) {
                    matched.extend(doc_ids.iter().copied());
                }
            }
            matched.sort_unstable();
            matched.dedup();
            current = intersect_sorted(current, matched);
        }

        let extensions = request.normalized_extensions();
        if !extensions.is_empty() {
            let mut matched = Vec::new();
            for extension in extensions {
                if let Some(doc_ids) = fast.docs_by_extension(&extension) {
                    matched.extend(doc_ids.iter().copied());
                }
            }
            matched.sort_unstable();
            matched.dedup();
            current = intersect_sorted(current, matched);
        }

        let mut path_substrings = request.path_substrings.clone();
        if matches!(request.kind, SearchKind::Path) && !request.pattern.is_empty() {
            path_substrings.push(request.pattern.clone());
        }
        if !path_substrings.is_empty() {
            let mut matched = Vec::new();
            for pattern in path_substrings {
                matched.extend(self.fast_path_candidates_for_substring(fast, &pattern)?);
            }
            matched.sort_unstable();
            matched.dedup();
            current = intersect_sorted(current, matched);
        }

        let globset = build_globset(&request.globs)?;
        if !request.path_prefixes.is_empty() || globset.is_some() {
            let all_docs = fast.all_docs();
            let mut matched = Vec::new();
            for (doc_id, path) in &all_docs {
                let prefix_match = request.path_prefixes.is_empty()
                    || request
                        .path_prefixes
                        .iter()
                        .any(|prefix| path.starts_with(prefix));
                let glob_match = globset
                    .as_ref()
                    .map(|set| set.is_match(path.as_str()))
                    .unwrap_or(true);
                if prefix_match && glob_match {
                    matched.push(*doc_id);
                }
            }
            matched.sort_unstable();
            current = intersect_sorted(current, matched);
        }

        Ok(current.unwrap_or_else(|| fast.all_doc_ids()))
    }

    fn fast_path_candidates_for_substring(
        &self,
        fast: &FastIndex,
        pattern: &str,
    ) -> Result<Vec<u32>, SearchIndexError> {
        if pattern.is_empty() {
            return Ok(fast.all_doc_ids());
        }
        let lower = pattern.to_ascii_lowercase();
        if lower.len() < 3 {
            return Ok(fast
                .all_docs()
                .iter()
                .filter(|(_, path)| path.to_ascii_lowercase().contains(&lower))
                .map(|(id, _)| *id)
                .collect());
        }

        let grams = trigrams_from_text(&lower);
        let mut candidate_lists: Vec<Vec<u32>> = Vec::new();
        for gram in &grams {
            let Some(postings) = fast.path_postings(*gram) else {
                return Ok(Vec::new());
            };
            candidate_lists.push(postings);
        }
        candidate_lists.sort_by_key(|l| l.len());

        let mut result = candidate_lists[0].clone();
        for list in &candidate_lists[1..] {
            result = sorted_intersect(&result, list);
            if result.is_empty() {
                return Ok(Vec::new());
            }
        }

        Ok(result
            .into_iter()
            .filter(|doc_id| {
                fast.doc_path(*doc_id)
                    .map(|p| p.to_ascii_lowercase().contains(&lower))
                    .unwrap_or(false)
            })
            .collect())
    }

    fn fast_index_candidates(
        &self,
        fast: &FastIndex,
        request: &QueryRequest,
        filtered_docs: &[u32],
    ) -> Result<Vec<u32>, SearchIndexError> {
        let plan = plan_query(request);
        match request.kind {
            SearchKind::Literal | SearchKind::Auto => {
                if request.pattern.len() < 3 {
                    return Ok(filtered_docs.to_vec());
                }
                let candidates = fast_candidates_for_seed(fast, &request.pattern)?;
                Ok(sorted_intersect(&candidates, filtered_docs))
            }
            SearchKind::Regex => {
                let seeds: Vec<&str> = plan
                    .literal_seeds
                    .iter()
                    .filter(|seed| seed.len() >= 3)
                    .map(String::as_str)
                    .collect();
                if seeds.is_empty() {
                    return Ok(filtered_docs.to_vec());
                }

                let mut candidates = if regex_has_unescaped_alternation(&request.pattern) {
                    let mut union = Vec::new();
                    for seed in seeds {
                        let seed_candidates = fast_candidates_for_seed(fast, seed)?;
                        union = sorted_union(&union, &seed_candidates);
                    }
                    union
                } else {
                    let mut current: Option<Vec<u32>> = None;
                    for seed in seeds {
                        let seed_candidates = fast_candidates_for_seed(fast, seed)?;
                        current = intersect_sorted(current, seed_candidates);
                    }
                    current.unwrap_or_default()
                };
                candidates = sorted_intersect(&candidates, filtered_docs);
                Ok(candidates)
            }
            SearchKind::Path => Ok(Vec::new()),
        }
    }

    // ---- Legacy index methods ----

    fn legacy_path_filtered_docs(
        &self,
        runtime: &RuntimeIndex,
        request: &QueryRequest,
    ) -> Result<Vec<u32>, SearchIndexError> {
        let mut current: Option<Vec<u32>> = None;

        if !request.exact_paths.is_empty() {
            let mut matched = Vec::new();
            for path in &request.exact_paths {
                if let Some(doc_id) = runtime.path_lookup.get(path) {
                    matched.push(*doc_id);
                }
            }
            matched.sort_unstable();
            matched.dedup();
            current = intersect_sorted(current, matched);
        }

        if !request.exact_names.is_empty() {
            let mut matched = Vec::new();
            for name in &request.exact_names {
                if let Some(doc_ids) = runtime.filename_map.get(&name.to_ascii_lowercase()) {
                    matched.extend(doc_ids.iter().copied());
                }
            }
            matched.sort_unstable();
            matched.dedup();
            current = intersect_sorted(current, matched);
        }

        let extensions = request.normalized_extensions();
        if !extensions.is_empty() {
            let mut matched = Vec::new();
            for extension in extensions {
                if let Some(doc_ids) = runtime.extension_map.get(&extension) {
                    matched.extend(doc_ids.iter().copied());
                }
            }
            matched.sort_unstable();
            matched.dedup();
            current = intersect_sorted(current, matched);
        }

        let mut path_substrings = request.path_substrings.clone();
        if matches!(request.kind, SearchKind::Path) && !request.pattern.is_empty() {
            path_substrings.push(request.pattern.clone());
        }
        if !path_substrings.is_empty() {
            let mut matched = Vec::new();
            for pattern in path_substrings {
                matched.extend(self.legacy_path_candidates_for_substring(runtime, &pattern)?);
            }
            matched.sort_unstable();
            matched.dedup();
            current = intersect_sorted(current, matched);
        }

        let globset = build_globset(&request.globs)?;
        if !request.path_prefixes.is_empty() || globset.is_some() {
            let mut matched = Vec::new();
            for doc in &runtime.docs {
                let prefix_match = request.path_prefixes.is_empty()
                    || request
                        .path_prefixes
                        .iter()
                        .any(|prefix| doc.relative_path.starts_with(prefix));
                let glob_match = globset
                    .as_ref()
                    .map(|set| set.is_match(doc.relative_path.as_str()))
                    .unwrap_or(true);
                if prefix_match && glob_match {
                    matched.push(doc.doc_id);
                }
            }
            matched.sort_unstable();
            current = intersect_sorted(current, matched);
        }

        Ok(current.unwrap_or_else(|| runtime.docs.iter().map(|doc| doc.doc_id).collect()))
    }

    fn legacy_path_candidates_for_substring(
        &self,
        runtime: &RuntimeIndex,
        pattern: &str,
    ) -> Result<Vec<u32>, SearchIndexError> {
        if pattern.is_empty() {
            return Ok(runtime.docs.iter().map(|doc| doc.doc_id).collect());
        }
        let lower = pattern.to_ascii_lowercase();
        if lower.len() < 3 {
            return Ok(runtime
                .docs
                .iter()
                .filter(|doc| doc.relative_path.to_ascii_lowercase().contains(&lower))
                .map(|doc| doc.doc_id)
                .collect());
        }

        let grams = trigrams_from_text(&lower);
        let mut candidate_lists: Vec<&[u32]> = Vec::new();
        for gram in &grams {
            let Some(postings) = runtime.path_postings.get(gram) else {
                return Ok(Vec::new());
            };
            candidate_lists.push(postings);
        }
        candidate_lists.sort_by_key(|list| list.len());

        let mut result = candidate_lists[0].to_vec();
        for list in &candidate_lists[1..] {
            result = sorted_intersect(&result, list);
            if result.is_empty() {
                return Ok(Vec::new());
            }
        }

        Ok(result
            .into_iter()
            .filter(|doc_id| {
                runtime
                    .doc(*doc_id)
                    .map(|doc| doc.relative_path.to_ascii_lowercase().contains(&lower))
                    .unwrap_or(false)
            })
            .collect())
    }

    fn legacy_index_candidates(
        &self,
        runtime: &RuntimeIndex,
        request: &QueryRequest,
        filtered_docs: &[u32],
    ) -> Result<Vec<u32>, SearchIndexError> {
        let plan = plan_query(request);
        match request.kind {
            SearchKind::Literal | SearchKind::Auto => {
                if request.pattern.len() < 3 {
                    return Ok(filtered_docs.to_vec());
                }
                let candidates = legacy_candidates_for_seed(runtime, &request.pattern)?;
                Ok(sorted_intersect(&candidates, filtered_docs))
            }
            SearchKind::Regex => {
                let seeds: Vec<&str> = plan
                    .literal_seeds
                    .iter()
                    .filter(|seed| seed.len() >= 3)
                    .map(String::as_str)
                    .collect();
                if seeds.is_empty() {
                    return Ok(filtered_docs.to_vec());
                }

                let mut candidates = if regex_has_unescaped_alternation(&request.pattern) {
                    let mut union = Vec::new();
                    for seed in seeds {
                        let seed_candidates = legacy_candidates_for_seed(runtime, seed)?;
                        union = sorted_union(&union, &seed_candidates);
                    }
                    union
                } else {
                    let mut current: Option<Vec<u32>> = None;
                    for seed in seeds {
                        let seed_candidates = legacy_candidates_for_seed(runtime, seed)?;
                        current = intersect_sorted(current, seed_candidates);
                    }
                    current.unwrap_or_default()
                };
                candidates = sorted_intersect(&candidates, filtered_docs);
                Ok(candidates)
            }
            SearchKind::Path => Ok(Vec::new()),
        }
    }

    fn collect_path_hits(&self, doc_ids: &[u32]) -> SearchExecution {
        let mut hits = Vec::new();
        for doc_id in doc_ids {
            let path = match &self.backend {
                IndexBackend::Fast(fast) => fast.doc_path(*doc_id),
                IndexBackend::Legacy(runtime) => {
                    runtime.doc(*doc_id).map(|d| d.relative_path.clone())
                }
            };
            if let Some(path) = path {
                hits.push(SearchHit::Path { path });
            }
        }
        SearchExecution {
            summary: SearchSummary {
                files_with_matches: hits.len(),
                total_line_matches: hits.len(),
            },
            hits,
            metrics: SearchMetrics {
                verified_docs: 0,
                bytes_scanned: 0,
                ..SearchMetrics::default()
            },
        }
    }

    fn collect_content_hits_parallel(
        &self,
        request: &QueryRequest,
        doc_ids: &[u32],
    ) -> Result<SearchExecution, SearchIndexError> {
        let matcher = build_matcher(request)?;
        let max_results = request.max_results.unwrap_or(usize::MAX);
        let total_found = AtomicUsize::new(0);
        let done = AtomicBool::new(false);
        let verified_docs = AtomicUsize::new(0);

        // Resolve doc paths upfront to enable parallel file I/O
        let work_items: Vec<(u32, std::path::PathBuf, String)> = doc_ids
            .iter()
            .filter_map(|doc_id| {
                let rel_path = match &self.backend {
                    IndexBackend::Fast(fast) => fast.doc_path(*doc_id),
                    IndexBackend::Legacy(runtime) => {
                        runtime.doc(*doc_id).map(|d| d.relative_path.clone())
                    }
                }?;
                Some((*doc_id, self.repo_root.join(&rel_path), rel_path))
            })
            .collect();

        // Parallel verification using rayon
        let results: Vec<Option<(SearchHit, usize, u64)>> = work_items
            .par_iter()
            .map(
                |(_doc_id, abs_path, rel_path): &(u32, std::path::PathBuf, String)| {
                    if done.load(Ordering::Relaxed) {
                        return None;
                    }

                    let bytes = match read_candidate_mmap(abs_path) {
                        Ok(b) => b,
                        Err(_) => return None,
                    };
                    verified_docs.fetch_add(1, Ordering::Relaxed);
                    let text = String::from_utf8_lossy(&bytes);
                    let remaining = max_results.saturating_sub(total_found.load(Ordering::Relaxed));
                    if remaining == 0 {
                        done.store(true, Ordering::Relaxed);
                        return None;
                    }

                    match match_lines(&text, &matcher, Some(remaining)) {
                        Some(mut lines) => loop {
                            let prev = total_found.load(Ordering::Relaxed);
                            if prev >= max_results {
                                done.store(true, Ordering::Relaxed);
                                return None;
                            }
                            let allowed = max_results - prev;
                            let keep = lines.len().min(allowed);
                            if keep == 0 {
                                done.store(true, Ordering::Relaxed);
                                return None;
                            }
                            if total_found
                                .compare_exchange(
                                    prev,
                                    prev + keep,
                                    Ordering::Relaxed,
                                    Ordering::Relaxed,
                                )
                                .is_ok()
                            {
                                if keep < lines.len() {
                                    lines.truncate(keep);
                                }
                                if prev + keep >= max_results {
                                    done.store(true, Ordering::Relaxed);
                                }
                                let count = lines.len();
                                return Some((
                                    SearchHit::Content {
                                        path: rel_path.to_string(),
                                        lines,
                                    },
                                    count,
                                    bytes.len() as u64,
                                ));
                            }
                        },
                        None => None,
                    }
                },
            )
            .collect();

        let mut hits = Vec::new();
        let mut bytes_scanned = 0_u64;
        let mut files_with_matches = 0_usize;
        let mut total_line_matches = 0_usize;

        for result in results.into_iter().flatten() {
            let (hit, count, size) = result;
            hits.push(hit);
            files_with_matches += 1;
            total_line_matches += count;
            bytes_scanned += size;
        }

        Ok(SearchExecution {
            hits,
            summary: SearchSummary {
                files_with_matches,
                total_line_matches,
            },
            metrics: SearchMetrics {
                verified_docs: verified_docs.load(Ordering::Relaxed),
                bytes_scanned,
                ..SearchMetrics::default()
            },
        })
    }
}

fn fast_candidates_for_seed(fast: &FastIndex, seed: &str) -> Result<Vec<u32>, SearchIndexError> {
    let grams = trigrams_from_text(&seed.to_ascii_lowercase());
    if grams.is_empty() {
        return Ok(Vec::new());
    }

    let mut posting_lists: Vec<Vec<u32>> = Vec::new();
    for gram in &grams {
        let Some(postings) = fast.content_postings(*gram) else {
            return Ok(Vec::new());
        };
        posting_lists.push(postings);
    }
    posting_lists.sort_by_key(|list| list.len());

    let mut candidates = posting_lists[0].clone();
    for list in &posting_lists[1..] {
        candidates = sorted_intersect(&candidates, list);
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
    }
    Ok(candidates)
}

fn legacy_candidates_for_seed(
    runtime: &RuntimeIndex,
    seed: &str,
) -> Result<Vec<u32>, SearchIndexError> {
    let grams = trigrams_from_text(&seed.to_ascii_lowercase());
    if grams.is_empty() {
        return Ok(Vec::new());
    }

    let mut posting_lists: Vec<&[u32]> = Vec::new();
    for gram in &grams {
        let Some(postings) = runtime.content_postings.get(gram) else {
            return Ok(Vec::new());
        };
        posting_lists.push(postings);
    }
    posting_lists.sort_by_key(|list| list.len());

    let mut candidates = posting_lists[0].to_vec();
    for list in &posting_lists[1..] {
        candidates = sorted_intersect(&candidates, list);
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
    }
    Ok(candidates)
}

/// Sorted intersection of two sorted slices — O(n+m) with no hashing.
fn sorted_intersect(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut result = Vec::with_capacity(a.len().min(b.len()));
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                result.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    result
}

fn sorted_union(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut result = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => {
                result.push(a[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                result.push(b[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                result.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    result.extend_from_slice(&a[i..]);
    result.extend_from_slice(&b[j..]);
    result
}

/// Intersect an optional accumulated result with a new sorted vec.
fn intersect_sorted(current: Option<Vec<u32>>, incoming: Vec<u32>) -> Option<Vec<u32>> {
    Some(match current {
        Some(current) => sorted_intersect(&current, &incoming),
        None => incoming,
    })
}

fn regex_has_unescaped_alternation(pattern: &str) -> bool {
    let mut escaped = false;
    let mut in_class = false;
    for ch in pattern.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '[' if !in_class => in_class = true,
            ']' if in_class => in_class = false,
            '|' if !in_class => return true,
            _ => {}
        }
    }
    false
}

fn build_matcher(request: &QueryRequest) -> Result<Regex, SearchIndexError> {
    let pattern = match request.kind {
        SearchKind::Literal | SearchKind::Auto => regex::escape(&request.pattern),
        SearchKind::Regex => request.pattern.clone(),
        SearchKind::Path => {
            return Err(SearchIndexError::InvalidQuery(
                "path query cannot build a content matcher".to_string(),
            ));
        }
    };
    let mut builder = RegexBuilder::new(&pattern);
    builder.case_insensitive(matches!(request.case_mode, CaseMode::Insensitive));
    builder.multi_line(false);
    builder.dot_matches_new_line(false);
    Ok(builder.build()?)
}

fn match_lines(text: &str, matcher: &Regex, limit: Option<usize>) -> Option<Vec<SearchLineMatch>> {
    let mut lines_out = Vec::new();
    let limit = limit.unwrap_or(usize::MAX);
    for (line_number, line) in text.lines().enumerate() {
        if let Some(found) = matcher.find(line) {
            lines_out.push(SearchLineMatch {
                line_number: line_number + 1,
                column: found.start() + 1,
                line_text: line.to_string(),
            });
            if lines_out.len() >= limit {
                break;
            }
        }
    }
    if lines_out.is_empty() {
        None
    } else {
        Some(lines_out)
    }
}

/// Read candidate file using mmap — avoids copying for large files.
fn read_candidate_mmap(path: &Path) -> Result<CandidateBytes, SearchIndexError> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    let len = metadata.len();
    if len == 0 {
        return Ok(CandidateBytes::Owned(Vec::new()));
    }
    if len > 32_768 {
        let mmap = unsafe { Mmap::map(&file)? };
        Ok(CandidateBytes::Mapped(mmap))
    } else {
        Ok(CandidateBytes::Owned(std::fs::read(path)?))
    }
}

enum CandidateBytes {
    Owned(Vec<u8>),
    Mapped(Mmap),
}

impl std::ops::Deref for CandidateBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            CandidateBytes::Owned(v) => v,
            CandidateBytes::Mapped(m) => m,
        }
    }
}

fn build_globset(globs: &[String]) -> Result<Option<GlobSet>, SearchIndexError> {
    if globs.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for glob in globs {
        builder
            .add(Glob::new(glob).map_err(|err| SearchIndexError::InvalidQuery(err.to_string()))?);
    }
    Ok(Some(builder.build().map_err(|err| {
        SearchIndexError::InvalidQuery(err.to_string())
    })?))
}

fn path_matches_filters(
    relative_path: &str,
    file_name: &str,
    extension: Option<&str>,
    request: &QueryRequest,
    globset: Option<&GlobSet>,
) -> bool {
    let relative_lower = relative_path.to_ascii_lowercase();
    let name_lower = file_name.to_ascii_lowercase();
    let extension_lower = extension.map(str::to_ascii_lowercase);

    if !request.exact_paths.is_empty()
        && !request.exact_paths.iter().any(|path| path == relative_path)
    {
        return false;
    }
    if !request.exact_names.is_empty()
        && !request
            .exact_names
            .iter()
            .any(|name| name.to_ascii_lowercase() == name_lower)
    {
        return false;
    }
    if !request.extensions.is_empty()
        && !request.normalized_extensions().iter().any(|ext| {
            extension_lower
                .as_ref()
                .map(|candidate| candidate == ext)
                .unwrap_or(false)
        })
    {
        return false;
    }
    if !request.path_prefixes.is_empty()
        && !request
            .path_prefixes
            .iter()
            .any(|prefix| relative_path.starts_with(prefix))
    {
        return false;
    }
    if !request.path_substrings.is_empty()
        && !request
            .path_substrings
            .iter()
            .any(|needle| relative_lower.contains(&needle.to_ascii_lowercase()))
    {
        return false;
    }
    if matches!(request.kind, SearchKind::Path)
        && !request.pattern.is_empty()
        && !relative_lower.contains(&request.pattern.to_ascii_lowercase())
    {
        return false;
    }
    globset
        .map(|set| set.is_match(relative_path))
        .unwrap_or(true)
}
