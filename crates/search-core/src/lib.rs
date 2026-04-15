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
    DAEMON_HOST, DAEMON_PID_FILE, DAEMON_PORT_FILE, DaemonRootStatus, DaemonSearchParams,
    DaemonStatus, DaemonStatusParams, FrecencySelectParams, MemoBulkStaleParams, MemoDebugStats,
    MemoEventKind, MemoFileStatus, MemoFileStatusKind, MemoFileSummary, MemoObserveParams,
    MemoObserveResponse, MemoSessionLifecycleResponse, MemoSessionParams, MemoSessionResponse,
    MemoStatusParams, MemoStatusResponse, RpcError, RpcRequest, RpcResponse,
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
