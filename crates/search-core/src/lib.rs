pub mod metrics;
pub mod planner;
pub mod query;
pub mod repo;
pub mod result;
pub mod trigram;

pub use metrics::{BenchmarkRunMetrics, ProcessMetrics, SearchMetrics, SessionMetrics};
pub use planner::{
    AdaptiveRoute, AdaptiveRoutingDecision, QueryPlan, QuerySelectivity, QueryShape,
    SearchExecutionStrategy, plan_query, route_query,
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
