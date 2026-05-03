use crate::query::QueryRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub const DAEMON_HOST: &str = "127.0.0.1";
pub const DAEMON_PID_FILE: &str = "daemon.pid";
pub const DAEMON_PORT_FILE: &str = "daemon.port";

/// JSON-RPC 2.0 request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: Value,
}

/// JSON-RPC 2.0 response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

impl RpcResponse {
    pub fn ok(id: u64, result: impl Serialize) -> Self {
        RpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(serde_json::to_value(result).unwrap_or(Value::Null)),
            error: None,
        }
    }

    pub fn error(id: u64, code: i32, message: impl Into<String>) -> Self {
        RpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

/// Response for the `status` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub daemon_dir: String,
    pub uptime_secs: u64,
    pub active_roots: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<DaemonRootStatus>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonRootStatus {
    pub target_root: String,
    pub index_dir: String,
    pub index_available: bool,
    pub generation: u64,
    #[serde(default)]
    pub context_epoch: u64,
    pub delta_docs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonSearchParams {
    pub target_root: String,
    pub request: QueryRequest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonStatusParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_root: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonRootParams {
    pub target_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchReuseCheckParams {
    pub target_root: String,
    pub request: QueryRequest,
    pub recorded_generation: u64,
    pub recorded_context_epoch: u64,
    pub matched_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchReuseReason {
    Unchanged,
    ContextInvalidated,
    ChangedMatchedPath,
    ChangedSearchScope,
    JournalOverflow,
    GenerationReset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchReuseCheckResponse {
    pub fresh: bool,
    pub reason: SearchReuseReason,
    pub generation: u64,
    pub context_epoch: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_paths: Vec<String>,
}

/// Parameters for the `frecency_select` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct FrecencySelectParams {
    pub target_root: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoEventKind {
    Read,
    Edit,
    SessionStart,
    SessionEnd,
    PreCompact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoObserveParams {
    pub session_id: String,
    pub repo_root: String,
    pub event: MemoEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoSessionParams {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoStatusParams {
    pub session_id: String,
    pub repo_root: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoFileStatusKind {
    Fresh,
    Stale,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoFileStatus {
    pub path: String,
    pub status: MemoFileStatusKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_count: Option<u32>,
    pub message: String,
    /// Estimated tokens currently on disk. Only set when `status == Stale`;
    /// `None` for `Fresh` and `Unknown`. Lets agents judge whether a re-read
    /// is worth the token cost before issuing it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoStatusResponse {
    pub session_id: String,
    pub results: Vec<MemoFileStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoFileSummary {
    pub path: String,
    pub status: MemoFileStatusKind,
    pub reads: u32,
    pub tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoSessionResponse {
    pub session_id: String,
    pub tracked_files: usize,
    pub total_reads: u64,
    pub redundant_reads_prevented: u64,
    pub tokens_saved: u64,
    pub compaction_count: u32,
    pub files: Vec<MemoFileSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoObserveResponse {
    pub observed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoSessionLifecycleResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoBulkStaleParams {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoDebugStats {
    pub sessions: usize,
    pub tracked_files: usize,
    pub per_session_files: HashMap<String, usize>,
}

/// Parameters for the `memo_check` method — single-file freshness query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoCheckParams {
    pub session_id: String,
    pub repo_root: String,
    pub path: String,
}

/// Agent action recommended by `memo_check`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoCheckRecommendation {
    /// File unchanged — skip re-reading, trust conversation history.
    SkipReread,
    /// File changed by more than 10% — re-read normally.
    Reread,
    /// File changed but by less than 10% — re-read expecting a small diff.
    RereadWithDiff,
}

/// Response for the `memo_check` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoCheckResponse {
    pub path: String,
    pub status: MemoFileStatusKind,
    pub recommendation: MemoCheckRecommendation,
    /// Token count recorded at last read. `None` if file is unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_at_last_read: Option<u32>,
    /// Estimated token count currently on disk. `None` if fresh or unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_tokens: Option<u32>,
    /// Seconds elapsed since the last read observe. `None` if unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_read_ago_seconds: Option<u64>,
}

pub const PORTABILITY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PortabilitySessionStatus {
    Open,
    Resolved,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStateRecord {
    pub schema_version: u32,
    pub session_id: String,
    pub goal: String,
    pub repo_root: String,
    pub status: PortabilitySessionStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Search,
    Read,
    FrecencySelect,
    MemoCheck,
    ContextPack,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionLogEntry {
    pub schema_version: u32,
    pub entry_id: u64,
    pub session_id: String,
    pub ts: i64,
    pub kind: ActionKind,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionOpenParams {
    pub target_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub goal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionOpenResponse {
    pub session: SessionStateRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionListParams {
    pub target_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionListResponse {
    pub sessions: Vec<SessionStateRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortabilitySessionStatusParams {
    pub target_root: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortabilitySessionStatusResponse {
    pub session: SessionStateRecord,
    pub action_log_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCloseParams {
    pub target_root: String,
    pub session_id: String,
    pub status: PortabilitySessionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCloseResponse {
    pub session: SessionStateRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecordActionParams {
    pub target_root: String,
    pub session_id: String,
    pub kind: ActionKind,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecordActionResponse {
    pub entry_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinnedSnippetSpec {
    pub path: String,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub session_id: String,
    pub created_at: i64,
    pub repo_root: String,
    pub repo_commit: Option<String>,
    pub repo_dirty_files: Vec<String>,
    pub source_harness: Option<String>,
    pub source_model: Option<String>,
    pub generation: u64,
    pub context_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReadRef {
    pub path: String,
    pub sha: String,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub last_read_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRef {
    pub search_id: String,
    pub query: String,
    pub kind: String,
    pub result_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingSet {
    pub schema_version: u32,
    pub files_read: Vec<FileReadRef>,
    pub searches_run: Vec<SearchRef>,
    pub frecency_top_n: Vec<(String, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinnedSnippet {
    pub sha: String,
    pub source_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullSnapshot {
    pub manifest: SnapshotManifest,
    pub working_set: WorkingSet,
    pub action_log: Vec<ActionLogEntry>,
    pub pinned_snippets: Vec<PinnedSnippet>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDiff {
    pub added_files: Vec<String>,
    pub removed_files: Vec<String>,
    pub changed_files: Vec<String>,
    pub added_searches: Vec<String>,
    pub removed_searches: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshotCreateParams {
    pub target_root: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_harness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_model: Option<String>,
    #[serde(default)]
    pub pinned_snippet_paths: Vec<PinnedSnippetSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshotCreateResponse {
    pub snapshot_id: String,
    pub snapshot_dir: String,
    pub manifest: SnapshotManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshotListParams {
    pub target_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshotListResponse {
    pub snapshots: Vec<SnapshotManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshotGetParams {
    pub target_root: String,
    pub snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshotGetResponse {
    pub snapshot: FullSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshotDiffParams {
    pub target_root: String,
    pub snapshot_a: String,
    pub snapshot_b: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshotDiffResponse {
    pub diff: SnapshotDiff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResumePrepareParams {
    pub target_root: String,
    pub snapshot_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HydrationReport {
    pub files_primed: usize,
    pub searches_warmed: usize,
    pub frecency_entries_restored: usize,
    pub stale_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResumePrepareResponse {
    pub session_id: String,
    pub payload_markdown: String,
    pub payload_token_estimate: usize,
    pub hydration_report: HydrationReport,
    pub searches: Vec<SearchRef>,
}
