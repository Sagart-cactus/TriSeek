pub mod metrics;
pub mod planner;
pub mod protocol;
pub mod query;
pub mod repo;
pub mod result;
pub mod trigram;

pub use metrics::{BenchmarkRunMetrics, ProcessMetrics, SearchMetrics, SessionMetrics};
pub use planner::{
    AdaptiveRoute, AdaptiveRoutingDecision, QueryPlan, QuerySelectivity, QueryShape,
    SearchExecutionStrategy, plan_query, route_query,
};
pub use protocol::{
    ActionKind, ActionLogEntry, DAEMON_HOST, DAEMON_PID_FILE, DAEMON_PORT_FILE, DaemonRootParams,
    DaemonRootStatus, DaemonSearchParams, DaemonStatus, DaemonStatusParams, FileReadRef,
    FrecencySelectParams, FullSnapshot, HydrationReport, MemoBulkStaleParams, MemoCheckParams,
    MemoCheckRecommendation, MemoCheckResponse, MemoDebugStats, MemoEventKind, MemoFileStatus,
    MemoFileStatusKind, MemoFileSummary, MemoObserveParams, MemoObserveResponse,
    MemoSessionLifecycleResponse, MemoSessionParams, MemoSessionResponse, MemoStatusParams,
    MemoStatusResponse, PORTABILITY_SCHEMA_VERSION, PinnedSnippet, PinnedSnippetSpec,
    PortabilitySessionStatus, PortabilitySessionStatusParams, PortabilitySessionStatusResponse,
    RpcError, RpcRequest, RpcResponse, SearchRef, SearchReuseCheckParams, SearchReuseCheckResponse,
    SearchReuseReason, SessionCloseParams, SessionCloseResponse, SessionListParams,
    SessionListResponse, SessionOpenParams, SessionOpenResponse, SessionRecordActionParams,
    SessionRecordActionResponse, SessionResumePrepareParams, SessionResumePrepareResponse,
    SessionSnapshotCreateParams, SessionSnapshotCreateResponse, SessionSnapshotDiffParams,
    SessionSnapshotDiffResponse, SessionSnapshotGetParams, SessionSnapshotGetResponse,
    SessionSnapshotListParams, SessionSnapshotListResponse, SessionStateRecord, SnapshotDiff,
    SnapshotManifest, WorkingSet,
};
pub use query::{CaseMode, QueryRequest, SearchEngineKind, SearchKind, SessionQuery};
pub use repo::{
    BuildStats, FileFingerprint, IndexMetadata, MachineInfo, RepoCategory, RepoStats, classify_repo,
};
pub use result::{
    MatchKind, SearchHit, SearchLineMatch, SearchPathMatch, SearchResponse, SearchSummary,
};
pub use trigram::{
    Trigram, decode_trigram, encode_trigram, normalize_for_index, trigrams_from_bytes,
    trigrams_from_text,
};
