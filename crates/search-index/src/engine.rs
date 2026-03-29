use crate::build::{BuildConfig, build_index, update_index};
use crate::error::SearchIndexError;
use crate::model::{RuntimeIndex, SearchExecution};
use crate::storage::{default_index_dir, load_base, load_delta};
use crate::walker::{ScanOptions, scan_repository};
use globset::{Glob, GlobSet, GlobSetBuilder};
use memmap2::Mmap;
use regex::{Regex, RegexBuilder};
use search_core::{
    CaseMode, QueryRequest, SearchHit, SearchKind, SearchLineMatch, SearchMetrics, SearchSummary,
    plan_query, trigrams_from_text,
};
use std::collections::HashSet;
use std::fs::File;
use std::path::Path;
use std::time::Instant;

pub struct SearchEngine {
    runtime: RuntimeIndex,
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
        let base = load_base(index_dir)?;
        let delta = load_delta(index_dir)?;
        Ok(Self {
            runtime: RuntimeIndex::from_snapshots(base, delta),
        })
    }

    pub fn metadata(&self) -> &search_core::IndexMetadata {
        &self.runtime.metadata
    }

    pub fn repo_root(&self) -> &Path {
        &self.runtime.repo_root
    }

    pub fn search(&self, request: &QueryRequest) -> Result<SearchExecution, SearchIndexError> {
        let started = Instant::now();
        let plan = plan_query(request);
        let filtered_docs = self.path_filtered_docs(request)?;
        let candidate_docs = match request.kind {
            SearchKind::Path => filtered_docs.clone(),
            _ if matches!(plan.strategy, search_core::SearchExecutionStrategy::Indexed) => {
                self.index_candidates(request, &filtered_docs)?
            }
            _ => filtered_docs.clone(),
        };

        let execution = if matches!(request.kind, SearchKind::Path) {
            self.collect_path_hits(&candidate_docs)
        } else {
            self.collect_content_hits(request, &candidate_docs)?
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
                index_bytes_read: Some(self.runtime.metadata.build_stats.index_bytes),
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

    fn path_filtered_docs(&self, request: &QueryRequest) -> Result<Vec<u32>, SearchIndexError> {
        let mut current: Option<HashSet<u32>> = None;
        let globset = build_globset(&request.globs)?;

        if !request.exact_paths.is_empty() {
            let mut matched = HashSet::new();
            for path in &request.exact_paths {
                if let Some(doc_id) = self.runtime.path_lookup.get(path) {
                    matched.insert(*doc_id);
                }
            }
            current = intersect_sets(current, matched);
        }

        if !request.exact_names.is_empty() {
            let mut matched = HashSet::new();
            for name in &request.exact_names {
                if let Some(doc_ids) = self.runtime.filename_map.get(&name.to_ascii_lowercase()) {
                    matched.extend(doc_ids.iter().copied());
                }
            }
            current = intersect_sets(current, matched);
        }

        let extensions = request.normalized_extensions();
        if !extensions.is_empty() {
            let mut matched = HashSet::new();
            for extension in extensions {
                if let Some(doc_ids) = self.runtime.extension_map.get(&extension) {
                    matched.extend(doc_ids.iter().copied());
                }
            }
            current = intersect_sets(current, matched);
        }

        let mut path_substrings = request.path_substrings.clone();
        if matches!(request.kind, SearchKind::Path) && !request.pattern.is_empty() {
            path_substrings.push(request.pattern.clone());
        }
        if !path_substrings.is_empty() {
            let mut matched = HashSet::new();
            for pattern in path_substrings {
                matched.extend(self.path_candidates_for_substring(&pattern)?);
            }
            current = intersect_sets(current, matched);
        }

        if !request.path_prefixes.is_empty() || globset.is_some() {
            let mut matched = HashSet::new();
            for doc in &self.runtime.docs {
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
                    matched.insert(doc.doc_id);
                }
            }
            current = intersect_sets(current, matched);
        }

        let mut docs: Vec<u32> = current
            .unwrap_or_else(|| self.runtime.docs.iter().map(|doc| doc.doc_id).collect())
            .into_iter()
            .collect();
        docs.sort_unstable();
        Ok(docs)
    }

    fn path_candidates_for_substring(&self, pattern: &str) -> Result<Vec<u32>, SearchIndexError> {
        if pattern.is_empty() {
            return Ok(self.runtime.docs.iter().map(|doc| doc.doc_id).collect());
        }
        let lower = pattern.to_ascii_lowercase();
        if lower.len() < 3 {
            return Ok(self
                .runtime
                .docs
                .iter()
                .filter(|doc| doc.relative_path.to_ascii_lowercase().contains(&lower))
                .map(|doc| doc.doc_id)
                .collect());
        }

        let grams = trigrams_from_text(&lower);
        let mut current: Option<HashSet<u32>> = None;
        for gram in grams {
            let Some(postings) = self.runtime.path_postings.get(&gram) else {
                return Ok(Vec::new());
            };
            current = intersect_sets(current, postings.iter().copied().collect::<HashSet<u32>>());
        }
        Ok(current
            .unwrap_or_default()
            .into_iter()
            .filter(|doc_id| {
                self.runtime
                    .doc(*doc_id)
                    .map(|doc| doc.relative_path.to_ascii_lowercase().contains(&lower))
                    .unwrap_or(false)
            })
            .collect())
    }

    fn index_candidates(
        &self,
        request: &QueryRequest,
        filtered_docs: &[u32],
    ) -> Result<Vec<u32>, SearchIndexError> {
        let plan = plan_query(request);
        let seed = match request.kind {
            SearchKind::Literal | SearchKind::Auto => request.pattern.as_str(),
            SearchKind::Regex => plan
                .literal_seeds
                .iter()
                .max_by_key(|seed| seed.len())
                .map(String::as_str)
                .unwrap_or_default(),
            SearchKind::Path => "",
        };
        if seed.len() < 3 {
            return Ok(filtered_docs.to_vec());
        }

        let grams = trigrams_from_text(&seed.to_ascii_lowercase());
        let mut current: Option<HashSet<u32>> = None;
        for gram in grams {
            let Some(postings) = self.runtime.content_postings.get(&gram) else {
                return Ok(Vec::new());
            };
            current = intersect_sets(current, postings.iter().copied().collect::<HashSet<u32>>());
        }

        let filtered_set: HashSet<u32> = filtered_docs.iter().copied().collect();
        let mut docs: Vec<u32> = current
            .unwrap_or_default()
            .into_iter()
            .filter(|doc_id| filtered_set.contains(doc_id))
            .collect();
        docs.sort_unstable();
        Ok(docs)
    }

    fn collect_path_hits(&self, doc_ids: &[u32]) -> SearchExecution {
        let mut hits = Vec::new();
        for doc_id in doc_ids {
            if let Some(doc) = self.runtime.doc(*doc_id) {
                hits.push(SearchHit::Path {
                    path: doc.relative_path.clone(),
                });
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

    fn collect_content_hits(
        &self,
        request: &QueryRequest,
        doc_ids: &[u32],
    ) -> Result<SearchExecution, SearchIndexError> {
        let matcher = build_matcher(request)?;
        let mut hits = Vec::new();
        let mut bytes_scanned = 0_u64;
        let mut verified_docs = 0_usize;
        let mut files_with_matches = 0_usize;
        let mut total_line_matches = 0_usize;
        let max_results = request.max_results.unwrap_or(usize::MAX);

        for doc_id in doc_ids {
            let Some(doc) = self.runtime.doc(*doc_id) else {
                continue;
            };
            verified_docs += 1;
            let absolute_path = self.runtime.repo_root.join(&doc.relative_path);
            let bytes = read_candidate_bytes(&absolute_path)?;
            bytes_scanned += bytes.len() as u64;
            let text = String::from_utf8_lossy(&bytes);

            if let Some(lines) = match_lines(
                &text,
                &matcher,
                Some(max_results.saturating_sub(total_line_matches)),
            ) {
                total_line_matches += lines.len();
                files_with_matches += 1;
                hits.push(SearchHit::Content {
                    path: doc.relative_path.clone(),
                    lines,
                });
                if total_line_matches >= max_results {
                    break;
                }
            }
        }

        Ok(SearchExecution {
            hits,
            summary: SearchSummary {
                files_with_matches,
                total_line_matches,
            },
            metrics: SearchMetrics {
                verified_docs,
                bytes_scanned,
                ..SearchMetrics::default()
            },
        })
    }
}

fn intersect_sets(current: Option<HashSet<u32>>, incoming: HashSet<u32>) -> Option<HashSet<u32>> {
    Some(match current {
        Some(current) => current.intersection(&incoming).copied().collect(),
        None => incoming,
    })
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

fn read_candidate_bytes(path: &Path) -> Result<Vec<u8>, SearchIndexError> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if metadata.len() > 1_000_000 {
        let mmap = unsafe { Mmap::map(&file)? };
        Ok(mmap.to_vec())
    } else {
        Ok(std::fs::read(path)?)
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
