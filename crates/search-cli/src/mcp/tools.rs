//! MCP tool handlers.
//!
//! Each tool translates its MCP input into a [`QueryRequest`] (or a metadata
//! read / reindex call), runs it through the shared [`execute_search`]
//! pipeline, and builds a compact JSON envelope. Output discipline is
//! enforced here: default limit, hard cap, preview truncation, dedupe, and
//! `truncated` flag.

use crate::context_pack::{self, ContextPackRequest};
use crate::mcp::errors::McpToolError;
use crate::mcp::search_memo::SearchMemoEntry;
use crate::mcp::server::McpState;
use crate::search_runner::{self, ExecutedSearch};
use search_core::{
    ActionKind, CaseMode, DAEMON_PORT_FILE, DaemonStatus, DaemonStatusParams, MemoCheckParams,
    MemoSessionParams, MemoStatusParams, PinnedSnippetSpec, PortabilitySessionStatus,
    PortabilitySessionStatusParams, QueryRequest, RpcRequest, RpcResponse, SearchEngineKind,
    SearchHit, SearchKind, SearchReuseCheckParams, SearchReuseReason, SessionCloseParams,
    SessionListParams, SessionOpenParams, SessionRecordActionParams, SessionResumePrepareParams,
    SessionSnapshotCreateParams, SessionSnapshotDiffParams, SessionSnapshotGetParams,
    SessionSnapshotListParams,
};
use search_index::{BuildConfig, SearchEngine, daemon_dir, index_exists, read_index_metadata};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

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

pub fn dispatch(
    state: &McpState,
    name: &str,
    arguments: &Value,
    session_id_hint: Option<&str>,
) -> ToolOutcome {
    match name {
        "find_files" => find_files(state, arguments, session_id_hint),
        "search_content" => search_content(state, arguments, session_id_hint),
        "search_path_and_content" => search_path_and_content(state, arguments, session_id_hint),
        "context_pack" => context_pack_tool(state, arguments),
        "index_status" => index_status(state, arguments),
        "reindex" => reindex(state, arguments),
        "memo_status" => memo_status(state, arguments, session_id_hint),
        "memo_session" => memo_session(arguments, session_id_hint),
        "memo_check" => memo_check(state, arguments, session_id_hint),
        "session_open" => session_open(state, arguments),
        "session_status" => session_status(state, arguments),
        "session_list" => session_list(state, arguments),
        "session_close" => session_close(state, arguments),
        "session_handoff" => session_handoff(state, arguments),
        "session_snapshot" => session_snapshot(state, arguments),
        "session_snapshot_list" => session_snapshot_list(state, arguments),
        "session_snapshot_get" => session_snapshot_get(state, arguments),
        "session_snapshot_diff" => session_snapshot_diff(state, arguments),
        "session_resume" => session_resume(state, arguments),
        other => ToolOutcome::Error(McpToolError::invalid_query(format!(
            "unknown tool `{other}`"
        ))),
    }
}

// ---------------------------------------------------------------------------
// context_pack
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ContextPackArgs {
    goal: String,
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    budget_tokens: Option<usize>,
    #[serde(default)]
    max_files: Option<usize>,
    #[serde(default)]
    changed_files: Vec<String>,
}

fn context_pack_tool(state: &McpState, arguments: &Value) -> ToolOutcome {
    let args: ContextPackArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    if args.goal.trim().is_empty() {
        return ToolOutcome::Error(McpToolError::invalid_query(
            "`goal` must not be empty for context_pack",
        ));
    }
    let intent = match context_pack::ContextPackIntent::parse(args.intent.as_deref()) {
        Ok(intent) => intent,
        Err(err) => return ToolOutcome::Error(McpToolError::invalid_query(err.to_string())),
    };
    match context_pack::build_context_pack(
        &state.repo_root(),
        &state.index_dir(),
        ContextPackRequest {
            goal: args.goal,
            intent,
            budget_tokens: args.budget_tokens,
            max_files: args.max_files,
            changed_files: args.changed_files,
        },
    ) {
        Ok(envelope) => ToolOutcome::Success(
            serde_json::to_value(envelope).unwrap_or_else(|_| json!({"version": ENVELOPE_VERSION})),
        ),
        Err(err) => ToolOutcome::Error(McpToolError::backend_failure(format!(
            "context pack failed: {err}"
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
    #[serde(default)]
    force_refresh: bool,
}

fn find_files(state: &McpState, arguments: &Value, session_id_hint: Option<&str>) -> ToolOutcome {
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

    run_and_envelope(
        state,
        "find_files",
        &request,
        limit,
        path_result_mapper,
        args.force_refresh,
        session_id_hint,
    )
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
    #[serde(default)]
    force_refresh: bool,
}

fn search_content(
    state: &McpState,
    arguments: &Value,
    session_id_hint: Option<&str>,
) -> ToolOutcome {
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

    run_and_envelope(
        state,
        "search_content",
        &request,
        limit,
        content_result_mapper,
        args.force_refresh,
        session_id_hint,
    )
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
    #[serde(default)]
    force_refresh: bool,
}

fn search_path_and_content(
    state: &McpState,
    arguments: &Value,
    session_id_hint: Option<&str>,
) -> ToolOutcome {
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

    run_and_envelope(
        state,
        "search_path_and_content",
        &request,
        limit,
        content_result_mapper,
        args.force_refresh,
        session_id_hint,
    )
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
                if !state.index_sync_in_progress() {
                    return ToolOutcome::Error(McpToolError::backend_failure(format!(
                        "failed to read index metadata: {err}"
                    )));
                }
                payload.insert("index_present".into(), json!(false));
                payload.insert("index_fresh".into(), json!(false));
                payload.insert("routing_hint".into(), json!("index_syncing"));
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
    let _mutation = state.start_index_mutation();
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
// memo_status
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct MemoStatusArgs {
    files: Vec<String>,
    #[serde(default)]
    session_id: Option<String>,
}

fn memo_status(state: &McpState, arguments: &Value, session_id_hint: Option<&str>) -> ToolOutcome {
    let args: MemoStatusArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    if args.files.is_empty() || args.files.iter().any(|path| path.trim().is_empty()) {
        return ToolOutcome::Error(McpToolError::invalid_query(
            "`files` must contain at least one non-empty path",
        ));
    }
    let session_id = resolve_session_id(args.session_id, session_id_hint);
    if let Err(error) = daemon_rpc(
        "memo_session_start",
        json!(MemoSessionParams {
            session_id: session_id.clone(),
            repo_root: Some(state.repo_root().display().to_string()),
        }),
    ) {
        return ToolOutcome::Error(error);
    }
    let params = MemoStatusParams {
        session_id,
        repo_root: state.repo_root().display().to_string(),
        files: args.files,
    };
    record_action(
        state,
        ActionKind::Read,
        json!({"method": "memo_status", "paths": params.files.clone()}),
    );
    match daemon_rpc("memo_status", json!(params)) {
        Ok(value) => ToolOutcome::Success(value),
        Err(error) => ToolOutcome::Error(error),
    }
}

// ---------------------------------------------------------------------------
// memo_session
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct MemoSessionArgs {
    #[serde(default)]
    session_id: Option<String>,
}

fn memo_session(arguments: &Value, session_id_hint: Option<&str>) -> ToolOutcome {
    let args: MemoSessionArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    let session_id = resolve_session_id(args.session_id, session_id_hint);
    if let Err(error) = daemon_rpc(
        "memo_session_start",
        json!(MemoSessionParams {
            session_id: session_id.clone(),
            repo_root: None,
        }),
    ) {
        return ToolOutcome::Error(error);
    }
    match daemon_rpc(
        "memo_session",
        json!(MemoSessionParams {
            session_id,
            repo_root: None
        }),
    ) {
        Ok(value) => ToolOutcome::Success(value),
        Err(error) => ToolOutcome::Error(error),
    }
}

// ---------------------------------------------------------------------------
// memo_check
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct MemoCheckArgs {
    path: String,
    #[serde(default)]
    session_id: Option<String>,
}

fn memo_check(state: &McpState, arguments: &Value, session_id_hint: Option<&str>) -> ToolOutcome {
    let args: MemoCheckArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    if args.path.trim().is_empty() {
        return ToolOutcome::Error(McpToolError::invalid_query(
            "`path` must not be empty for memo_check",
        ));
    }
    let session_id = resolve_session_id(args.session_id, session_id_hint);
    if let Err(error) = daemon_rpc(
        "memo_session_start",
        json!(MemoSessionParams {
            session_id: session_id.clone(),
            repo_root: Some(state.repo_root().display().to_string()),
        }),
    ) {
        return ToolOutcome::Error(error);
    }
    let params = MemoCheckParams {
        session_id,
        repo_root: state.repo_root().display().to_string(),
        path: args.path,
    };
    record_action(
        state,
        ActionKind::MemoCheck,
        json!({"method": "memo_check", "path": params.path.clone()}),
    );
    match daemon_rpc("memo_check", json!(params)) {
        Ok(value) => ToolOutcome::Success(value),
        Err(error) => ToolOutcome::Error(error),
    }
}

// ---------------------------------------------------------------------------
// session tools
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SessionOpenArgs {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    goal: String,
}

fn session_open(state: &McpState, arguments: &Value) -> ToolOutcome {
    let args: SessionOpenArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    match daemon_rpc(
        "session_open",
        json!(SessionOpenParams {
            target_root: state.repo_root().display().to_string(),
            session_id: args.session_id,
            goal: args.goal,
        }),
    ) {
        Ok(value) => {
            if let Some(session_id) = value
                .get("session")
                .and_then(|session| session.get("session_id"))
                .and_then(Value::as_str)
            {
                state.set_current_session_id(Some(session_id.to_string()));
            }
            ToolOutcome::Success(value)
        }
        Err(error) => ToolOutcome::Error(error),
    }
}

#[derive(Debug, Deserialize)]
struct SessionIdArgs {
    #[serde(default)]
    session_id: Option<String>,
}

fn session_status(state: &McpState, arguments: &Value) -> ToolOutcome {
    let args: SessionIdArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    let Some(session_id) = args.session_id.or_else(|| state.current_session_id()) else {
        return ToolOutcome::Error(McpToolError::invalid_query(
            "`session_id` is required when no current session is open",
        ));
    };
    match daemon_rpc(
        "session_status",
        json!(PortabilitySessionStatusParams {
            target_root: state.repo_root().display().to_string(),
            session_id,
        }),
    ) {
        Ok(value) => ToolOutcome::Success(value),
        Err(error) => ToolOutcome::Error(error),
    }
}

fn session_list(state: &McpState, _arguments: &Value) -> ToolOutcome {
    match daemon_rpc(
        "session_list",
        json!(SessionListParams {
            target_root: state.repo_root().display().to_string(),
        }),
    ) {
        Ok(value) => ToolOutcome::Success(value),
        Err(error) => ToolOutcome::Error(error),
    }
}

#[derive(Debug, Deserialize)]
struct SessionCloseArgs {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default = "default_resolved_status")]
    status: String,
}

fn default_resolved_status() -> String {
    "resolved".to_string()
}

fn parse_session_status(status: &str) -> Result<PortabilitySessionStatus, McpToolError> {
    match status {
        "open" => Ok(PortabilitySessionStatus::Open),
        "resolved" => Ok(PortabilitySessionStatus::Resolved),
        "abandoned" => Ok(PortabilitySessionStatus::Abandoned),
        other => Err(McpToolError::invalid_query(format!(
            "invalid session status `{other}`"
        ))),
    }
}

fn session_close(state: &McpState, arguments: &Value) -> ToolOutcome {
    let args: SessionCloseArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    let Some(session_id) = args.session_id.or_else(|| state.current_session_id()) else {
        return ToolOutcome::Error(McpToolError::invalid_query(
            "`session_id` is required when no current session is open",
        ));
    };
    let status = match parse_session_status(&args.status) {
        Ok(status) => status,
        Err(error) => return ToolOutcome::Error(error),
    };
    match daemon_rpc(
        "session_close",
        json!(SessionCloseParams {
            target_root: state.repo_root().display().to_string(),
            session_id: session_id.clone(),
            status,
        }),
    ) {
        Ok(value) => {
            state.set_current_session_id(None);
            ToolOutcome::Success(value)
        }
        Err(error) => ToolOutcome::Error(error),
    }
}

#[derive(Debug, Deserialize)]
struct SessionSnapshotArgs {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    source_harness: Option<String>,
    #[serde(default)]
    source_model: Option<String>,
    #[serde(default)]
    pinned_snippet_paths: Vec<PinnedSnippetSpec>,
}

fn session_snapshot(state: &McpState, arguments: &Value) -> ToolOutcome {
    let args: SessionSnapshotArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    let Some(session_id) = args.session_id.or_else(|| state.current_session_id()) else {
        return ToolOutcome::Error(McpToolError::invalid_query(
            "`session_id` is required when no current session is open",
        ));
    };
    match daemon_rpc(
        "session_snapshot_create",
        json!(SessionSnapshotCreateParams {
            target_root: state.repo_root().display().to_string(),
            session_id,
            source_harness: args.source_harness,
            source_model: args.source_model,
            pinned_snippet_paths: args.pinned_snippet_paths,
        }),
    ) {
        Ok(value) => ToolOutcome::Success(value),
        Err(error) => ToolOutcome::Error(error),
    }
}

#[derive(Debug, Deserialize)]
struct SnapshotListArgs {
    #[serde(default)]
    session_id: Option<String>,
}

fn session_snapshot_list(state: &McpState, arguments: &Value) -> ToolOutcome {
    let args: SnapshotListArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    match daemon_rpc(
        "session_snapshot_list",
        json!(SessionSnapshotListParams {
            target_root: state.repo_root().display().to_string(),
            session_id: args.session_id,
        }),
    ) {
        Ok(value) => ToolOutcome::Success(value),
        Err(error) => ToolOutcome::Error(error),
    }
}

#[derive(Debug, Deserialize)]
struct SnapshotGetArgs {
    snapshot_id: String,
}

fn session_snapshot_get(state: &McpState, arguments: &Value) -> ToolOutcome {
    let args: SnapshotGetArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    match daemon_rpc(
        "session_snapshot_get",
        json!(SessionSnapshotGetParams {
            target_root: state.repo_root().display().to_string(),
            snapshot_id: args.snapshot_id,
        }),
    ) {
        Ok(value) => ToolOutcome::Success(value),
        Err(error) => ToolOutcome::Error(error),
    }
}

#[derive(Debug, Deserialize)]
struct SnapshotDiffArgs {
    snapshot_a: String,
    snapshot_b: String,
}

fn session_snapshot_diff(state: &McpState, arguments: &Value) -> ToolOutcome {
    let args: SnapshotDiffArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    match daemon_rpc(
        "session_snapshot_diff",
        json!(SessionSnapshotDiffParams {
            target_root: state.repo_root().display().to_string(),
            snapshot_a: args.snapshot_a,
            snapshot_b: args.snapshot_b,
        }),
    ) {
        Ok(value) => ToolOutcome::Success(value),
        Err(error) => ToolOutcome::Error(error),
    }
}

#[derive(Debug, Deserialize)]
struct SessionResumeArgs {
    snapshot_id: String,
    #[serde(default)]
    budget_tokens: Option<usize>,
}

fn session_resume(state: &McpState, arguments: &Value) -> ToolOutcome {
    let args: SessionResumeArgs = match deserialize_args(arguments) {
        Ok(args) => args,
        Err(err) => return ToolOutcome::Error(err),
    };
    match daemon_rpc(
        "session_resume_prepare",
        json!(SessionResumePrepareParams {
            target_root: state.repo_root().display().to_string(),
            snapshot_id: args.snapshot_id,
            budget_tokens: args.budget_tokens,
        }),
    ) {
        Ok(value) => {
            if let Some(session_id) = value.get("session_id").and_then(Value::as_str) {
                state.set_current_session_id(Some(session_id.to_string()));
            }
            if let Some(searches) = value.get("searches").and_then(Value::as_array) {
                let entries = searches
                    .iter()
                    .filter_map(|search| {
                        Some(SearchMemoEntry {
                            search_id: search.get("search_id")?.as_str()?.to_string(),
                            recorded_generation: 0,
                            recorded_context_epoch: 0,
                            matched_paths: search
                                .get("result_paths")
                                .and_then(Value::as_array)
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect(),
                            files_with_matches: search
                                .get("result_paths")
                                .and_then(Value::as_array)
                                .map(|paths| paths.len() as u64)
                                .unwrap_or(0),
                            total_line_matches: 0,
                            strategy: "hydrated_snapshot".to_string(),
                        })
                    })
                    .collect::<Vec<_>>();
                state.search_memo.warm_from_snapshot(entries);
            }
            ToolOutcome::Success(value)
        }
        Err(error) => ToolOutcome::Error(error),
    }
}

fn session_handoff(state: &McpState, arguments: &Value) -> ToolOutcome {
    let snapshot = match session_snapshot(state, arguments) {
        ToolOutcome::Success(value) => value,
        ToolOutcome::Error(error) => return ToolOutcome::Error(error),
    };
    if let Some(session_id) = state.current_session_id() {
        let _ = daemon_rpc(
            "session_close",
            json!(SessionCloseParams {
                target_root: state.repo_root().display().to_string(),
                session_id,
                status: PortabilitySessionStatus::Resolved,
            }),
        );
    }
    ToolOutcome::Success(snapshot)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn resolve_session_id(explicit: Option<String>, hint: Option<&str>) -> String {
    explicit
        .or_else(|| hint.map(ToString::to_string))
        .unwrap_or_else(|| "default".to_string())
}

fn daemon_port_path() -> std::path::PathBuf {
    daemon_dir().join(DAEMON_PORT_FILE)
}

fn read_daemon_port() -> Option<u16> {
    let port_path = daemon_port_path();
    if !port_path.exists() {
        return None;
    }
    fs::read_to_string(&port_path)
        .ok()
        .and_then(|port| port.trim().parse::<u16>().ok())
}

fn connect_to_daemon() -> Option<TcpStream> {
    let port = read_daemon_port()?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(250)).ok()
}

fn try_daemon_rpc(method: &str, params: Value) -> Option<Value> {
    let mut stream = connect_to_daemon()?;
    let req = RpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: method.to_string(),
        params,
    };
    writeln!(stream, "{}", serde_json::to_string(&req).ok()?).ok()?;
    let reader = BufReader::new(stream.try_clone().ok()?);
    for line in reader.lines() {
        let line = line.ok()?;
        if line.trim().is_empty() {
            continue;
        }
        let resp: RpcResponse = serde_json::from_str(&line).ok()?;
        if resp.error.is_some() {
            return None;
        }
        return resp.result;
    }
    None
}

fn daemon_rpc(method: &str, params: Value) -> Result<Value, McpToolError> {
    let mut stream = connect_to_daemon().ok_or_else(|| {
        McpToolError::backend_failure(
            "TriSeek daemon is not running; memo tools require `triseek daemon start`",
        )
    })?;
    let req = RpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: method.to_string(),
        params,
    };
    writeln!(
        stream,
        "{}",
        serde_json::to_string(&req).map_err(|error| {
            McpToolError::backend_failure(format!(
                "failed to serialize daemon RPC request: {error}"
            ))
        })?
    )
    .map_err(|error| {
        McpToolError::backend_failure(format!("failed to write daemon RPC: {error}"))
    })?;
    let reader = BufReader::new(stream.try_clone().map_err(|error| {
        McpToolError::backend_failure(format!("failed to clone socket: {error}"))
    })?);
    for line in reader.lines() {
        let line = line.map_err(|error| {
            McpToolError::backend_failure(format!("failed to read daemon RPC: {error}"))
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let resp: RpcResponse = serde_json::from_str(&line).map_err(|error| {
            McpToolError::backend_failure(format!("failed to parse daemon RPC response: {error}"))
        })?;
        if let Some(error) = resp.error {
            return Err(McpToolError::backend_failure(format!(
                "daemon RPC {method} failed ({}): {}",
                error.code, error.message
            )));
        }
        return Ok(resp.result.unwrap_or(Value::Null));
    }
    Err(McpToolError::backend_failure(format!(
        "daemon RPC {method} returned no response"
    )))
}

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

#[derive(Debug, Clone)]
struct SearchContextStatus {
    generation: u64,
    context_epoch: u64,
}

fn run_and_envelope(
    state: &McpState,
    tool_name: &str,
    request: &QueryRequest,
    limit: usize,
    mapper: ResultMapper,
    force_refresh: bool,
    session_id_hint: Option<&str>,
) -> ToolOutcome {
    let repo_root = state.repo_root();
    let index_dir = state.index_dir();
    let context_key = search_context_key(session_id_hint);
    let cache_key = format!(
        "{}|{}|{}|{}",
        context_key,
        tool_name,
        limit,
        serde_json::to_string(request).unwrap_or_default(),
    );

    if !force_refresh
        && let Some(entry) = state.search_memo.get(&cache_key)
        && let Some(reuse_envelope) = build_context_reuse_envelope(state, request, &entry)
    {
        record_search_action(state, tool_name, request, &reuse_envelope, true);
        return ToolOutcome::Success(reuse_envelope);
    }

    let search_context = search_context_status(state);

    // Execute search.
    let executed_result = if state.should_bypass_index_for_startup_sync() {
        search_runner::execute_search_without_index(
            &repo_root, &index_dir, request, /* repeated_session_hint */ true,
            /* summary_only */ false,
        )
    } else {
        state.with_cached_engine(|indexed_engine| {
            search_runner::execute_search_with_engine(
                &repo_root,
                &index_dir,
                request,
                /* repeated_session_hint */ true,
                /* summary_only */ false,
                indexed_engine,
            )
        })
    };
    let executed = match executed_result {
        Ok(v) => v,
        Err(err) => {
            return ToolOutcome::Error(McpToolError::backend_failure(format!(
                "search backend failed: {err}"
            )));
        }
    };

    let mut envelope = build_envelope(&repo_root, limit, executed, mapper);

    let fallback_used = envelope
        .get("fallback_used")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let cache_status = if fallback_used { "bypass" } else { "miss" };
    if let Some(obj) = envelope.as_object_mut() {
        obj.insert("cache".into(), json!(cache_status));
        if force_refresh {
            obj.insert("force_refreshed".into(), json!(true));
        }
    }

    if fallback_used {
        return ToolOutcome::Success(envelope);
    }

    if let Some(context) = search_context {
        let entry = state.search_memo.put(
            cache_key,
            SearchMemoEntry {
                search_id: String::new(),
                recorded_generation: context.generation,
                recorded_context_epoch: context.context_epoch,
                matched_paths: collect_matched_paths(&envelope),
                files_with_matches: envelope
                    .get("files_with_matches")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                total_line_matches: envelope
                    .get("total_line_matches")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                strategy: envelope
                    .get("strategy")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
            },
        );
        if let Some(obj) = envelope.as_object_mut() {
            obj.insert("search_id".into(), json!(entry.search_id));
        }
    }

    record_search_action(state, tool_name, request, &envelope, false);
    ToolOutcome::Success(envelope)
}

fn search_context_key(session_id_hint: Option<&str>) -> String {
    session_id_hint
        .map(|session| format!("session:{session}"))
        .unwrap_or_else(|| format!("process:{}", std::process::id()))
}

fn search_context_status(state: &McpState) -> Option<SearchContextStatus> {
    let value = try_daemon_rpc(
        "status",
        json!(DaemonStatusParams {
            target_root: Some(state.repo_root().display().to_string()),
        }),
    )?;
    let status: DaemonStatus = serde_json::from_value(value).ok()?;
    let root = status.root?;
    Some(SearchContextStatus {
        generation: root.generation,
        context_epoch: root.context_epoch,
    })
}

fn build_context_reuse_envelope(
    state: &McpState,
    request: &QueryRequest,
    entry: &SearchMemoEntry,
) -> Option<Value> {
    let response = try_daemon_rpc(
        "search_reuse_check",
        json!(SearchReuseCheckParams {
            target_root: state.repo_root().display().to_string(),
            request: request.clone(),
            recorded_generation: entry.recorded_generation,
            recorded_context_epoch: entry.recorded_context_epoch,
            matched_paths: entry.matched_paths.clone(),
        }),
    )?;
    let response: search_core::SearchReuseCheckResponse = serde_json::from_value(response).ok()?;
    if !response.fresh {
        return None;
    }
    Some(json!({
        "version": ENVELOPE_VERSION,
        "repo_root": state.repo_root().display().to_string(),
        "strategy": entry.strategy,
        "fallback_used": false,
        "cache": "hit",
        "search_id": entry.search_id,
        "prior_search_id": entry.search_id,
        "reuse_status": "fresh_duplicate",
        "reuse_reason": serde_json::to_value(response.reason).unwrap_or(json!(SearchReuseReason::Unchanged)),
        "generation": response.generation,
        "context_epoch": response.context_epoch,
        "files_with_matches": entry.files_with_matches,
        "total_line_matches": entry.total_line_matches,
        "results": [],
        "results_omitted": true,
        "truncated": false,
    }))
}

fn record_search_action(
    state: &McpState,
    tool_name: &str,
    request: &QueryRequest,
    envelope: &Value,
    cache_hit: bool,
) {
    record_action(
        state,
        ActionKind::Search,
        json!({
            "method": tool_name,
            "query": request.pattern,
            "kind": format!("{:?}", request.kind).to_ascii_lowercase(),
            "cache_hit": cache_hit,
            "result_paths": collect_matched_paths(envelope),
        }),
    );
}

fn record_action(state: &McpState, kind: ActionKind, payload: Value) {
    let Some(session_id) = state.current_session_id() else {
        return;
    };
    let _ = daemon_rpc(
        "session_record_action",
        json!(SessionRecordActionParams {
            target_root: state.repo_root().display().to_string(),
            session_id,
            kind,
            payload,
        }),
    );
}

fn collect_matched_paths(envelope: &Value) -> Vec<String> {
    let mut paths = HashSet::new();
    let mut ordered = Vec::new();
    for result in envelope
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(path) = result.get("path").and_then(Value::as_str) else {
            continue;
        };
        if paths.insert(path.to_string()) {
            ordered.push(path.to_string());
        }
    }
    ordered
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
                    "preview": crate::output_format::trim_preview(
                        &line.line_text,
                        PREVIEW_MAX_CHARS,
                    ),
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
