pub mod metrics;
pub mod planner;
pub mod protocol;
pub mod query;
pub mod repo;
pub mod result;
pub mod trigram;

pub use metrics::{BenchmarkRunMetrics, ProcessMetrics, SearchMetrics, SessionMetrics};
pub use planner::{
    plan_query, route_query, AdaptiveRoute, AdaptiveRoutingDecision, QueryPlan, QuerySelectivity,
    QueryShape, SearchExecutionStrategy,
};
pub use protocol::{
    DaemonStatus, FrecencySelectParams, RpcError, RpcRequest, RpcResponse, DAEMON_HOST,
    DAEMON_PID_FILE, DAEMON_PORT_FILE,
};
pub use query::{CaseMode, QueryRequest, SearchEngineKind, SearchKind, SessionQuery};
pub use repo::{
    classify_repo, BuildStats, FileFingerprint, IndexMetadata, MachineInfo, RepoCategory, RepoStats,
};
pub use result::{
    MatchKind, SearchHit, SearchLineMatch, SearchPathMatch, SearchResponse, SearchSummary,
};
pub use trigram::{
    decode_trigram, encode_trigram, normalize_for_index, trigrams_from_bytes, trigrams_from_text,
    Trigram,
};
