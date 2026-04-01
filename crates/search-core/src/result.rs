use crate::metrics::SearchMetrics;
use crate::planner::{AdaptiveRoutingDecision, QueryPlan};
use crate::query::{QueryRequest, SearchEngineKind, SearchKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    Content,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchLineMatch {
    pub line_number: usize,
    pub column: usize,
    pub line_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPathMatch {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchHit {
    Content {
        path: String,
        lines: Vec<SearchLineMatch>,
    },
    Path {
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SearchSummary {
    pub files_with_matches: usize,
    pub total_line_matches: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub request: QueryRequest,
    pub effective_kind: SearchKind,
    pub engine: SearchEngineKind,
    pub routing: AdaptiveRoutingDecision,
    pub plan: QueryPlan,
    pub hits: Vec<SearchHit>,
    pub summary: SearchSummary,
    pub metrics: SearchMetrics,
}
