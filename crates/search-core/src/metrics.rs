use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProcessMetrics {
    pub wall_millis: f64,
    pub user_cpu_millis: Option<f64>,
    pub system_cpu_millis: Option<f64>,
    pub max_rss_kib: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SearchMetrics {
    pub process: ProcessMetrics,
    pub candidate_docs: usize,
    pub verified_docs: usize,
    pub matches_returned: usize,
    pub bytes_scanned: u64,
    pub index_bytes_read: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SessionMetrics {
    pub query_count: usize,
    pub total_matches: usize,
    pub process: ProcessMetrics,
    pub amortized_with_index_build_millis: Option<f64>,
    pub amortized_without_index_build_millis: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BenchmarkRunMetrics {
    pub cold_runs: Vec<ProcessMetrics>,
    pub warm_runs: Vec<ProcessMetrics>,
}
