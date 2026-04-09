//! MCP tool handlers.
//!
//! Each tool translates its MCP input into a [`QueryRequest`] (or a metadata
//! read / reindex call), runs it through the shared [`execute_search`]
//! pipeline, and builds a compact JSON envelope. Output discipline is
//! enforced here: default limit, hard cap, preview truncation, dedupe, and
//! `truncated` flag.

use crate::mcp::errors::McpToolError;
use crate::mcp::server::McpState;
use crate::search_runner::{self, ExecutedSearch};
use search_core::{CaseMode, QueryRequest, SearchEngineKind, SearchHit, SearchKind};
use search_index::{BuildConfig, SearchEngine, index_exists, read_index_metadata};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::path::Path;

const DEFAULT_LIMIT: usize = 20;
const HARD_LIMIT: usize = 100;
const PREVIEW_MAX_CHARS: usize = 200;
const ENVELOPE_VERSION: &str = "1";

/// Outcome of a tool invocation. `Success` is serialized as the tool's
/// JSON envelope; `Error` is serialized as an MCP `CallToolResult` with
/// `isError: true`.
pub enum ToolOutcome {
    Success(Value),
    Error(McpToolError),
}

pub fn dispatch(state: &McpState, name: &str, arguments: &Value) -> ToolOutcome {
    match name {
        "find_files" => find_files(state, arguments),
        "search_content" => search_content(state, arguments),
        "search_path_and_content" => search_path_and_content(state, arguments),
        "index_status" => index_status(state, arguments),
        "reindex" => reindex(state, arguments),
        other => ToolOutcome::Error(McpToolError::invalid_query(format!(
            "unknown tool `{other}`"
        ))),
    }
}

// ---------------------------------------------------------------------------
// find_files
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct FindFilesArgs {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
}

fn find_files(state: &McpState, arguments: &Value) -> ToolOutcome {
    let args: FindFilesArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    if args.query.trim().is_empty() {
        return ToolOutcome::Error(McpToolError::invalid_query(
            "`query` must not be empty for find_files",
        ));
    }
    let limit = clamp_limit(args.limit);

    let request = QueryRequest {
        kind: SearchKind::Path,
        engine: SearchEngineKind::Auto,
        pattern: args.query.clone(),
        case_mode: CaseMode::Insensitive,
        max_results: Some(limit + 1),
        ..QueryRequest::default()
    };

    run_and_envelope(state, &request, limit, path_result_mapper)
}

// ---------------------------------------------------------------------------
// search_content
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SearchContentArgs {
    query: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

fn search_content(state: &McpState, arguments: &Value) -> ToolOutcome {
    let args: SearchContentArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    if args.query.trim().is_empty() {
        return ToolOutcome::Error(McpToolError::invalid_query(
            "`query` must not be empty for search_content",
        ));
    }
    let kind = match parse_mode(args.mode.as_deref()) {
        Ok(kind) => kind,
        Err(err) => return ToolOutcome::Error(err),
    };
    let limit = clamp_limit(args.limit);

    let request = QueryRequest {
        kind,
        engine: SearchEngineKind::Auto,
        pattern: args.query.clone(),
        case_mode: CaseMode::Sensitive,
        max_results: Some(limit + 1),
        ..QueryRequest::default()
    };

    run_and_envelope(state, &request, limit, content_result_mapper)
}

// ---------------------------------------------------------------------------
// search_path_and_content
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SearchPathContentArgs {
    path_query: String,
    content_query: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

fn search_path_and_content(state: &McpState, arguments: &Value) -> ToolOutcome {
    let args: SearchPathContentArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    if args.path_query.trim().is_empty() {
        return ToolOutcome::Error(McpToolError::invalid_query(
            "`path_query` must not be empty",
        ));
    }
    if args.content_query.trim().is_empty() {
        return ToolOutcome::Error(McpToolError::invalid_query(
            "`content_query` must not be empty",
        ));
    }
    let kind = match parse_mode(args.mode.as_deref()) {
        Ok(kind) => kind,
        Err(err) => return ToolOutcome::Error(err),
    };
    let limit = clamp_limit(args.limit);

    let request = QueryRequest {
        kind,
        engine: SearchEngineKind::Auto,
        pattern: args.content_query.clone(),
        case_mode: CaseMode::Sensitive,
        globs: vec![args.path_query.clone()],
        max_results: Some(limit + 1),
        ..QueryRequest::default()
    };

    run_and_envelope(state, &request, limit, content_result_mapper)
}

// ---------------------------------------------------------------------------
// index_status
// ---------------------------------------------------------------------------

fn index_status(state: &McpState, _arguments: &Value) -> ToolOutcome {
    let index_dir = state.index_dir();
    let present = index_exists(&index_dir);
    let mut payload = Map::new();
    payload.insert("version".into(), json!(ENVELOPE_VERSION));
    payload.insert(
        "repo_root".into(),
        json!(state.repo_root().display().to_string()),
    );
    payload.insert("index_present".into(), json!(present));

    if present {
        match read_index_metadata(&index_dir) {
            Ok(meta) => {
                payload.insert("index_fresh".into(), json!(meta.delta_docs == 0));
                payload.insert(
                    "indexed_files".into(),
                    json!(meta.repo_stats.searchable_files),
                );
                payload.insert("index_bytes".into(), json!(meta.build_stats.index_bytes));
                payload.insert("last_updated".into(), json!(meta.build_stats.completed_at));
                payload.insert(
                    "repo_category".into(),
                    json!(format!("{:?}", meta.repo_stats.category).to_ascii_lowercase()),
                );
                payload.insert("routing_hint".into(), json!("indexed_default"));
            }
            Err(err) => {
                return ToolOutcome::Error(McpToolError::backend_failure(format!(
                    "failed to read index metadata: {err}"
                )));
            }
        }
    } else {
        payload.insert("index_fresh".into(), json!(false));
        payload.insert("routing_hint".into(), json!("ripgrep_fallback"));
    }

    ToolOutcome::Success(Value::Object(payload))
}

// ---------------------------------------------------------------------------
// reindex
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ReindexArgs {
    #[serde(default)]
    mode: Option<String>,
}

fn reindex(state: &McpState, arguments: &Value) -> ToolOutcome {
    let args: ReindexArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    let mode = args.mode.as_deref().unwrap_or("incremental");
    if mode != "incremental" && mode != "full" {
        return ToolOutcome::Error(McpToolError::invalid_query(format!(
            "invalid reindex mode `{mode}`; expected `incremental` or `full`"
        )));
    }

    let repo_root = state.repo_root();
    let index_dir = state.index_dir();
    let config = BuildConfig::default();
    let index_present = index_exists(&index_dir);

    let started = std::time::Instant::now();
    let (metadata, rebuilt_full) = match mode {
        "full" => match SearchEngine::build(&repo_root, Some(&index_dir), &config) {
            Ok(meta) => (meta, true),
            Err(err) => {
                return ToolOutcome::Error(McpToolError::backend_failure(format!(
                    "full build failed: {err}"
                )));
            }
        },
        _ => {
            if !index_present {
                match SearchEngine::build(&repo_root, Some(&index_dir), &config) {
                    Ok(meta) => (meta, true),
                    Err(err) => {
                        return ToolOutcome::Error(McpToolError::backend_failure(format!(
                            "incremental reindex fallback build failed: {err}"
                        )));
                    }
                }
            } else {
                match SearchEngine::update(&repo_root, Some(&index_dir), &config) {
                    Ok(outcome) => (outcome.metadata, outcome.rebuilt_full),
                    Err(err) => {
                        return ToolOutcome::Error(McpToolError::backend_failure(format!(
                            "incremental update failed: {err}"
                        )));
                    }
                }
            }
        }
    };

    state.invalidate_cached_engine();

    ToolOutcome::Success(json!({
        "version": ENVELOPE_VERSION,
        "repo_root": repo_root.display().to_string(),
        "completed": true,
        "mode": mode,
        "rebuilt_full": rebuilt_full,
        "elapsed_ms": started.elapsed().as_millis() as u64,
        "indexed_files": metadata.build_stats.docs_indexed,
    }))
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn deserialize_args<T: for<'de> Deserialize<'de>>(arguments: &Value) -> Result<T, McpToolError> {
    let value = if arguments.is_null() {
        Value::Object(Map::new())
    } else {
        arguments.clone()
    };
    serde_json::from_value(value)
        .map_err(|err| McpToolError::invalid_query(format!("invalid arguments: {err}")))
}

fn clamp_limit(requested: Option<usize>) -> usize {
    let lim = requested.unwrap_or(DEFAULT_LIMIT);
    lim.clamp(1, HARD_LIMIT)
}

fn parse_mode(mode: Option<&str>) -> Result<SearchKind, McpToolError> {
    match mode.unwrap_or("literal") {
        "literal" => Ok(SearchKind::Literal),
        "regex" => Ok(SearchKind::Regex),
        other => Err(McpToolError::invalid_query(format!(
            "invalid mode `{other}`; expected `literal` or `regex`"
        ))),
    }
}

type ResultMapper = fn(&SearchHit, &mut usize, usize) -> Option<Value>;

fn run_and_envelope(
    state: &McpState,
    request: &QueryRequest,
    limit: usize,
    mapper: ResultMapper,
) -> ToolOutcome {
    let repo_root = state.repo_root();
    let index_dir = state.index_dir();

    let executed = match state.with_cached_engine(|indexed_engine| {
        search_runner::execute_search_with_engine(
            &repo_root,
            &index_dir,
            request,
            /* repeated_session_hint */ true,
            /* summary_only */ false,
            indexed_engine,
        )
    }) {
        Ok(v) => v,
        Err(err) => {
            return ToolOutcome::Error(McpToolError::backend_failure(format!(
                "search backend failed: {err}"
            )));
        }
    };

    ToolOutcome::Success(build_envelope(&repo_root, limit, executed, mapper))
}

fn build_envelope(
    repo_root: &Path,
    limit: usize,
    executed: ExecutedSearch,
    mapper: ResultMapper,
) -> Value {
    let ExecutedSearch {
        response,
        fallback_used,
    } = executed;

    let strategy = strategy_label(response.engine);
    let mut results = Vec::with_capacity(limit);
    let mut produced = 0_usize;

    'outer: for hit in response.hits.iter() {
        if produced >= limit {
            break;
        }
        if let Some(mapped) = mapper(hit, &mut produced, limit) {
            results.push(mapped);
            if produced >= limit {
                break 'outer;
            }
        }
    }

    let truncated = response.hits.len() > limit
        || response.summary.files_with_matches > results.len()
        || produced >= limit;

    json!({
        "version": ENVELOPE_VERSION,
        "repo_root": repo_root.display().to_string(),
        "strategy": strategy,
        "fallback_used": fallback_used,
        "routing_reason": response.routing.reason,
        "files_with_matches": response.summary.files_with_matches,
        "total_line_matches": response.summary.total_line_matches,
        "results": results,
        "truncated": truncated,
    })
}

fn strategy_label(engine: SearchEngineKind) -> &'static str {
    match engine {
        SearchEngineKind::Indexed => "triseek_indexed",
        SearchEngineKind::DirectScan => "triseek_direct_scan",
        SearchEngineKind::Ripgrep => "ripgrep_fallback",
        SearchEngineKind::Auto => "triseek_indexed",
    }
}

fn path_result_mapper(hit: &SearchHit, produced: &mut usize, _limit: usize) -> Option<Value> {
    let path = match hit {
        SearchHit::Path { path } => path.clone(),
        SearchHit::Content { path, .. } => path.clone(),
    };
    *produced += 1;
    Some(json!({
        "path": path,
        "reason": "path_match",
    }))
}

fn content_result_mapper(hit: &SearchHit, produced: &mut usize, limit: usize) -> Option<Value> {
    match hit {
        SearchHit::Content { path, lines } => {
            // Dedupe by (path, line number) and truncate previews.
            let mut seen = HashSet::<usize>::new();
            let mut out_lines = Vec::new();
            for line in lines {
                if *produced >= limit {
                    break;
                }
                if !seen.insert(line.line_number) {
                    continue;
                }
                out_lines.push(json!({
                    "line": line.line_number,
                    "column": line.column,
                    "preview": truncate_preview(&line.line_text),
                }));
                *produced += 1;
            }
            if out_lines.is_empty() {
                return None;
            }
            Some(json!({
                "path": path,
                "matches": out_lines,
                "reason": "content_match",
            }))
        }
        SearchHit::Path { path } => {
            *produced += 1;
            Some(json!({
                "path": path,
                "reason": "path_only",
            }))
        }
    }
}

fn truncate_preview(text: &str) -> String {
    if text.chars().count() <= PREVIEW_MAX_CHARS {
        return text.to_string();
    }
    let truncated: String = text.chars().take(PREVIEW_MAX_CHARS).collect();
    format!("{truncated}…")
}
