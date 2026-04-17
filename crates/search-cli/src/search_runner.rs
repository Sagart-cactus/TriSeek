//! Shared search execution pipeline used by both the `search` CLI subcommand
//! and the MCP server tool handlers.
//!
//! This module owns the full hybrid routing pipeline: build the query plan,
//! consult [`route_query`], adjust the route for filter hints, dispatch to the
//! correct backend (indexed, direct scan, or ripgrep), and assemble a uniform
//! [`SearchResponse`]. Frecency rerank and recording is handled separately by
//! the CLI command because it prints to stdout; MCP callers skip it.

use anyhow::{Context, Result};
use search_core::{
    AdaptiveRoute, AdaptiveRoutingDecision, QueryRequest, SearchEngineKind, SearchKind,
    SearchResponse, plan_query, route_query,
};
use search_index::{BuildConfig, SearchEngine, SearchExecution, index_exists, read_index_metadata};
use std::path::Path;

/// Result of running a search request through the full pipeline.
pub struct ExecutedSearch {
    pub response: SearchResponse,
    /// True if the adaptive router chose a non-indexed backend (DirectScan or Ripgrep).
    pub fallback_used: bool,
}

/// Execute a [`QueryRequest`] end-to-end against the given repo/index.
///
/// This function is intentionally pure and side-effect-free beyond the search
/// itself — it does not touch frecency, does not forward to the daemon, and
/// does not print. Callers that need those behaviors add them on top.
///
/// `summary_only` is only consulted by the ripgrep backend, where it selects
/// `rg --count` over full match parsing. MCP callers pass `false`.
pub fn execute_search(
    repo_root: &Path,
    index_dir: &Path,
    request: &QueryRequest,
    repeated_session_hint: bool,
    summary_only: bool,
) -> Result<ExecutedSearch> {
    execute_search_with_engine(
        repo_root,
        index_dir,
        request,
        repeated_session_hint,
        summary_only,
        None,
    )
}

pub fn execute_search_with_engine(
    repo_root: &Path,
    index_dir: &Path,
    request: &QueryRequest,
    repeated_session_hint: bool,
    summary_only: bool,
    indexed_engine: Option<&SearchEngine>,
) -> Result<ExecutedSearch> {
    let index_available = index_exists(index_dir);
    let index_metadata = if index_available {
        Some(read_index_metadata(index_dir).with_context(|| {
            format!("failed to read index metadata from {}", index_dir.display())
        })?)
    } else {
        None
    };

    let plan = plan_query(request);
    let mut routing = route_query(
        request,
        index_metadata.as_ref().map(|metadata| &metadata.repo_stats),
        &plan,
        index_available,
        repeated_session_hint,
    );
    let selected_route = adjust_route_for_filters(routing.selected_engine, request);
    if selected_route != routing.selected_engine {
        routing = AdaptiveRoutingDecision {
            reason: format!("{};filter_adjustment=true", routing.reason),
            selected_engine: selected_route,
            ..routing
        };
    }

    let execution = dispatch(
        selected_route,
        repo_root,
        index_dir,
        request,
        summary_only,
        indexed_engine,
    )?;

    let response = SearchResponse {
        request: request.clone(),
        effective_kind: effective_search_kind(request.kind),
        engine: route_to_engine(selected_route),
        routing,
        plan,
        hits: execution.hits,
        summary: execution.summary,
        metrics: execution.metrics,
    };

    let fallback_used = !matches!(selected_route, AdaptiveRoute::Indexed);
    Ok(ExecutedSearch {
        response,
        fallback_used,
    })
}

fn dispatch(
    selected_route: AdaptiveRoute,
    repo_root: &Path,
    index_dir: &Path,
    request: &QueryRequest,
    summary_only: bool,
    indexed_engine: Option<&SearchEngine>,
) -> Result<SearchExecution> {
    match selected_route {
        AdaptiveRoute::Indexed => {
            if let Some(engine) = indexed_engine {
                Ok(engine.search(request)?)
            } else {
                let engine = SearchEngine::open(index_dir)
                    .with_context(|| format!("failed to open index at {}", index_dir.display()))?;
                Ok(engine.search(request)?)
            }
        }
        AdaptiveRoute::DirectScan => Ok(SearchEngine::search_direct(
            repo_root,
            request,
            &direct_scan_config(request),
        )?),
        AdaptiveRoute::Ripgrep => crate::rg::run_rg_search(repo_root, request, summary_only),
    }
}

/// Adjust the selected route based on filters that are incompatible with
/// certain backends. Path-kind queries and path-substring filters force a
/// direct-scan path.
pub fn adjust_route_for_filters(route: AdaptiveRoute, request: &QueryRequest) -> AdaptiveRoute {
    if matches!(request.kind, SearchKind::Path) {
        return if matches!(route, AdaptiveRoute::Indexed) {
            AdaptiveRoute::Indexed
        } else {
            AdaptiveRoute::DirectScan
        };
    }
    if !request.path_substrings.is_empty() || !request.exact_paths.is_empty() {
        return AdaptiveRoute::DirectScan;
    }
    route
}

pub fn route_to_engine(route: AdaptiveRoute) -> SearchEngineKind {
    match route {
        AdaptiveRoute::Indexed => SearchEngineKind::Indexed,
        AdaptiveRoute::DirectScan => SearchEngineKind::DirectScan,
        AdaptiveRoute::Ripgrep => SearchEngineKind::Ripgrep,
    }
}

fn effective_search_kind(kind: SearchKind) -> SearchKind {
    match kind {
        SearchKind::Auto => SearchKind::Literal,
        other => other,
    }
}

fn direct_scan_config(request: &QueryRequest) -> BuildConfig {
    BuildConfig {
        include_hidden: request.include_hidden,
        include_binary: request.include_binary,
        max_file_size: None,
        merge_threshold_ratio: BuildConfig::default().merge_threshold_ratio,
    }
}
