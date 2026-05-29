use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

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

pub const USAGE_METRICS_SCHEMA_VERSION: u32 = 1;
pub const USAGE_METRICS_EVENTS_FILE: &str = "events.jsonl";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UsageMetricSource {
    Cli,
    #[default]
    Mcp,
    Daemon,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageMetricsEvent {
    pub schema_version: u32,
    pub ts: i64,
    pub source: UsageMetricSource,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchUsageMetric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<MemoUsageMetric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_pack: Option<ContextPackUsageMetric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portability: Option<PortabilityUsageMetric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reliability: Option<ReliabilityUsageMetric>,
}

impl Default for UsageMetricsEvent {
    fn default() -> Self {
        Self {
            schema_version: USAGE_METRICS_SCHEMA_VERSION,
            ts: now_secs(),
            source: UsageMetricSource::Mcp,
            tool: String::new(),
            repo_hash: None,
            search: None,
            memo: None,
            context_pack: None,
            portability: None,
            reliability: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SearchUsageMetric {
    pub indexed: bool,
    pub fallback_used: bool,
    pub reuse_hit: bool,
    pub results_omitted: bool,
    pub estimated_tokens_saved: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_millis: Option<f64>,
    pub files_with_matches: u64,
    pub total_line_matches: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MemoUsageMetric {
    pub redundant_reads_prevented: u64,
    pub tokens_saved: u64,
    pub total_reads_observed: u64,
    pub memo_checks: u64,
    pub skip_reread: u64,
    pub stale_detections: u64,
    pub unknown_detections: u64,
    pub tracked_files: u64,
    pub compaction_invalidations: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ContextPackUsageMetric {
    pub calls: u64,
    pub items_returned: u64,
    pub estimated_tokens: u64,
    pub budget_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PortabilityUsageMetric {
    pub sessions_opened: u64,
    pub snapshots_created: u64,
    pub resumes: u64,
    pub handoffs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ReliabilityUsageMetric {
    pub daemon_unavailable: u64,
    pub index_available: Option<bool>,
    pub reindex_count: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageMetricsReport {
    pub schema_version: u32,
    pub version: String,
    pub storage: UsageMetricsStorageReport,
    pub usage: UsageRollup,
    pub memo: MemoRollup,
    pub search: SearchRollup,
    pub context: ContextPackRollup,
    pub portability: PortabilityRollup,
    pub reliability: ReliabilityRollup,
    pub repo_impact: Vec<RepoImpactRollup>,
    pub help_score: u64,
}

impl UsageMetricsReport {
    pub fn from_events(events: &[UsageMetricsEvent]) -> Self {
        let mut usage = UsageRollup::default();
        let mut memo = MemoRollup::default();
        let mut search = SearchRollup::default();
        let mut context = ContextPackRollup::default();
        let mut portability = PortabilityRollup::default();
        let mut reliability = ReliabilityRollup::default();
        let mut active_days = BTreeSet::new();
        let mut repos = BTreeSet::new();
        let mut latencies = Vec::new();
        let mut repo_impact = BTreeMap::<String, RepoImpactRollup>::new();

        for event in events
            .iter()
            .filter(|event| event.schema_version == USAGE_METRICS_SCHEMA_VERSION)
        {
            usage.total_calls += 1;
            if event.ts > 0 {
                active_days.insert(event.ts.div_euclid(86_400));
            }
            if let Some(repo_hash) = &event.repo_hash {
                repos.insert(repo_hash.clone());
                repo_impact
                    .entry(repo_hash.clone())
                    .or_insert_with(|| RepoImpactRollup {
                        repo_hash: repo_hash.clone(),
                        ..RepoImpactRollup::default()
                    })
                    .calls += 1;
            }
            match event.source {
                UsageMetricSource::Cli => usage.cli_calls += 1,
                UsageMetricSource::Mcp => usage.mcp_calls += 1,
                UsageMetricSource::Daemon => usage.daemon_events += 1,
            }
            *usage.tool_counts.entry(event.tool.clone()).or_default() += 1;

            if let Some(metric) = &event.search {
                search.total_searches += 1;
                if metric.indexed {
                    search.indexed_searches += 1;
                }
                if metric.fallback_used {
                    search.fallback_searches += 1;
                }
                if metric.reuse_hit {
                    search.reuse_hits += 1;
                }
                if metric.results_omitted {
                    search.result_omissions += 1;
                }
                search.estimated_result_tokens_saved += metric.estimated_tokens_saved;
                search.files_with_matches += metric.files_with_matches;
                search.total_line_matches += metric.total_line_matches;
                if let Some(wall_millis) = metric.wall_millis {
                    latencies.push(wall_millis);
                }
                if let Some(repo_hash) = &event.repo_hash
                    && let Some(repo) = repo_impact.get_mut(repo_hash)
                {
                    repo.searches += 1;
                    repo.estimated_tokens_saved += metric.estimated_tokens_saved;
                }
            }

            if let Some(metric) = &event.memo {
                memo.redundant_rereads_prevented += metric.redundant_reads_prevented;
                memo.tokens_saved += metric.tokens_saved;
                memo.total_reads_observed += metric.total_reads_observed;
                memo.memo_checks += metric.memo_checks;
                memo.skip_reread += metric.skip_reread;
                memo.stale_detections += metric.stale_detections;
                memo.unknown_detections += metric.unknown_detections;
                memo.tracked_files = memo.tracked_files.max(metric.tracked_files);
                memo.compaction_invalidations += metric.compaction_invalidations;
                if let Some(repo_hash) = &event.repo_hash
                    && let Some(repo) = repo_impact.get_mut(repo_hash)
                {
                    repo.redundant_rereads_prevented += metric.redundant_reads_prevented;
                    repo.estimated_tokens_saved += metric.tokens_saved;
                }
            }

            if let Some(metric) = &event.context_pack {
                context.context_pack_calls += metric.calls.max(1);
                context.items_returned += metric.items_returned;
                context.estimated_tokens += metric.estimated_tokens;
                context.budget_tokens += metric.budget_tokens;
                if let Some(intent) = &metric.intent {
                    *context.intent_counts.entry(intent.clone()).or_default() += 1;
                }
            }

            if let Some(metric) = &event.portability {
                portability.sessions_opened += metric.sessions_opened;
                portability.snapshots_created += metric.snapshots_created;
                portability.resumes += metric.resumes;
                portability.handoffs += metric.handoffs;
            }

            if let Some(metric) = &event.reliability {
                reliability.daemon_unavailable += metric.daemon_unavailable;
                reliability.reindex_count += metric.reindex_count;
                reliability.errors += metric.errors;
                match metric.index_available {
                    Some(true) => reliability.index_available_true += 1,
                    Some(false) => reliability.index_available_false += 1,
                    None => {}
                }
            }
        }

        usage.active_days = active_days.len() as u64;
        usage.unique_repos = repos.len() as u64;
        latencies.sort_by(f64::total_cmp);
        search.p50_wall_millis = percentile(&latencies, 0.50);
        search.p95_wall_millis = percentile(&latencies, 0.95);

        let help_score = memo.tokens_saved
            + search.estimated_result_tokens_saved
            + memo.redundant_rereads_prevented * 100
            + search.reuse_hits * 50
            + context.context_pack_calls * 10
            + portability.handoffs * 25
            + portability.resumes * 25;

        let mut repo_impact = repo_impact.into_values().collect::<Vec<_>>();
        repo_impact.sort_by(|a, b| {
            b.estimated_tokens_saved
                .cmp(&a.estimated_tokens_saved)
                .then_with(|| b.calls.cmp(&a.calls))
                .then_with(|| a.repo_hash.cmp(&b.repo_hash))
        });

        Self {
            schema_version: USAGE_METRICS_SCHEMA_VERSION,
            version: "1".to_string(),
            storage: UsageMetricsStorageReport {
                privacy_mode: "private_rollups".to_string(),
                event_count: events.len() as u64,
                raw_queries_included: false,
                raw_paths_included: false,
                raw_file_contents_included: false,
                per_machine_only: true,
            },
            usage,
            memo,
            search,
            context,
            portability,
            reliability,
            repo_impact,
            help_score,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageMetricsStorageReport {
    pub privacy_mode: String,
    pub event_count: u64,
    pub raw_queries_included: bool,
    pub raw_paths_included: bool,
    pub raw_file_contents_included: bool,
    pub per_machine_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UsageRollup {
    pub total_calls: u64,
    pub mcp_calls: u64,
    pub cli_calls: u64,
    pub daemon_events: u64,
    pub active_days: u64,
    pub unique_repos: u64,
    pub tool_counts: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MemoRollup {
    pub redundant_rereads_prevented: u64,
    pub tokens_saved: u64,
    pub total_reads_observed: u64,
    pub memo_checks: u64,
    pub skip_reread: u64,
    pub stale_detections: u64,
    pub unknown_detections: u64,
    pub tracked_files: u64,
    pub compaction_invalidations: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SearchRollup {
    pub total_searches: u64,
    pub indexed_searches: u64,
    pub fallback_searches: u64,
    pub reuse_hits: u64,
    pub result_omissions: u64,
    pub estimated_result_tokens_saved: u64,
    pub files_with_matches: u64,
    pub total_line_matches: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p50_wall_millis: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p95_wall_millis: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ContextPackRollup {
    pub context_pack_calls: u64,
    pub items_returned: u64,
    pub estimated_tokens: u64,
    pub budget_tokens: u64,
    pub intent_counts: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PortabilityRollup {
    pub sessions_opened: u64,
    pub snapshots_created: u64,
    pub resumes: u64,
    pub handoffs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReliabilityRollup {
    pub daemon_unavailable: u64,
    pub index_available_true: u64,
    pub index_available_false: u64,
    pub reindex_count: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RepoImpactRollup {
    pub repo_hash: String,
    pub calls: u64,
    pub searches: u64,
    pub redundant_rereads_prevented: u64,
    pub estimated_tokens_saved: u64,
}

pub fn private_repo_hash(repo_root: impl AsRef<str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(repo_root.as_ref().as_bytes());
    let digest = hasher.finalize();
    format!("repo_{:02x?}", &digest[..8]).replace(['[', ']', ',', ' '], "")
}

pub fn default_usage_metrics_dir() -> PathBuf {
    triseek_home_dir().join("metrics")
}

pub fn usage_metrics_events_path(metrics_dir: &Path) -> PathBuf {
    metrics_dir.join(USAGE_METRICS_EVENTS_FILE)
}

pub fn append_usage_metrics_event(metrics_dir: &Path, event: &UsageMetricsEvent) -> io::Result<()> {
    fs::create_dir_all(metrics_dir)?;
    let path = usage_metrics_events_path(metrics_dir);
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut line =
        serde_json::to_string(event).map_err(|err| io::Error::new(ErrorKind::InvalidData, err))?;
    line.push('\n');
    file.write_all(line.as_bytes())?;
    Ok(())
}

pub fn read_usage_metrics_events(metrics_dir: &Path) -> io::Result<Vec<UsageMetricsEvent>> {
    let path = usage_metrics_events_path(metrics_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path)?;
    let mut events = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let stream = serde_json::Deserializer::from_str(line).into_iter::<UsageMetricsEvent>();
        for event in stream {
            events.push(event.map_err(|err| {
                io::Error::new(
                    ErrorKind::InvalidData,
                    format!("invalid metrics event line {}: {err}", idx + 1),
                )
            })?);
        }
    }
    Ok(events)
}

fn triseek_home_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("TRISEEK_HOME") {
        return PathBuf::from(path);
    }
    #[cfg(windows)]
    {
        if let Some(path) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(path).join("TriSeek");
        }
        if let Some(path) = std::env::var_os("USERPROFILE") {
            return PathBuf::from(path).join(".triseek");
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(path) = std::env::var_os("HOME") {
            return PathBuf::from(path).join(".triseek");
        }
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".triseek")
}

fn percentile(values: &[f64], percentile: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let idx = ((values.len() as f64 * percentile).ceil() as usize)
        .saturating_sub(1)
        .min(values.len() - 1);
    Some(values[idx])
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_metrics_reader_accepts_concatenated_jsonl_events() {
        let metrics_dir = std::env::temp_dir().join(format!(
            "triseek-metrics-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        fs::create_dir_all(&metrics_dir).expect("create metrics dir");
        let events_path = usage_metrics_events_path(&metrics_dir);
        let first = UsageMetricsEvent {
            schema_version: USAGE_METRICS_SCHEMA_VERSION,
            ts: 1,
            source: UsageMetricSource::Mcp,
            tool: "index_status".to_string(),
            ..UsageMetricsEvent::default()
        };
        let second = UsageMetricsEvent {
            schema_version: USAGE_METRICS_SCHEMA_VERSION,
            ts: 2,
            source: UsageMetricSource::Cli,
            tool: "context_pack".to_string(),
            ..UsageMetricsEvent::default()
        };
        let third = UsageMetricsEvent {
            schema_version: USAGE_METRICS_SCHEMA_VERSION,
            ts: 3,
            source: UsageMetricSource::Daemon,
            tool: "memo_check".to_string(),
            ..UsageMetricsEvent::default()
        };
        let text = format!(
            "{}{}\n\n{}\n",
            serde_json::to_string(&first).expect("serialize first"),
            serde_json::to_string(&second).expect("serialize second"),
            serde_json::to_string(&third).expect("serialize third")
        );
        fs::write(&events_path, text).expect("write metrics events");

        let events = read_usage_metrics_events(&metrics_dir).expect("read metrics events");

        assert_eq!(events, vec![first, second, third]);
        fs::remove_dir_all(metrics_dir).expect("remove metrics dir");
    }

    #[test]
    fn usage_metrics_aggregate_private_rollups() {
        let repo_hash = private_repo_hash("/Users/alice/work/secret-product");
        let events = vec![
            UsageMetricsEvent {
                schema_version: USAGE_METRICS_SCHEMA_VERSION,
                ts: 1_772_880_000,
                source: UsageMetricSource::Mcp,
                tool: "search_content".to_string(),
                repo_hash: Some(repo_hash.clone()),
                search: Some(SearchUsageMetric {
                    indexed: true,
                    fallback_used: false,
                    reuse_hit: true,
                    results_omitted: true,
                    estimated_tokens_saved: 96,
                    wall_millis: Some(12.5),
                    files_with_matches: 2,
                    total_line_matches: 4,
                }),
                ..UsageMetricsEvent::default()
            },
            UsageMetricsEvent {
                schema_version: USAGE_METRICS_SCHEMA_VERSION,
                ts: 1_772_880_030,
                source: UsageMetricSource::Daemon,
                tool: "memo_observe".to_string(),
                repo_hash: Some(repo_hash.clone()),
                memo: Some(MemoUsageMetric {
                    redundant_reads_prevented: 1,
                    tokens_saved: 240,
                    total_reads_observed: 2,
                    ..MemoUsageMetric::default()
                }),
                ..UsageMetricsEvent::default()
            },
            UsageMetricsEvent {
                schema_version: USAGE_METRICS_SCHEMA_VERSION,
                ts: 1_772_966_400,
                source: UsageMetricSource::Cli,
                tool: "context_pack".to_string(),
                repo_hash: Some(repo_hash),
                context_pack: Some(ContextPackUsageMetric {
                    calls: 1,
                    items_returned: 3,
                    estimated_tokens: 700,
                    budget_tokens: 1_200,
                    intent: Some("bugfix".to_string()),
                }),
                ..UsageMetricsEvent::default()
            },
        ];

        let report = UsageMetricsReport::from_events(&events);

        assert_eq!(report.schema_version, USAGE_METRICS_SCHEMA_VERSION);
        assert_eq!(report.storage.privacy_mode, "private_rollups");
        assert_eq!(report.storage.event_count, 3);
        assert_eq!(report.usage.total_calls, 3);
        assert_eq!(report.usage.active_days, 2);
        assert_eq!(report.usage.unique_repos, 1);
        assert_eq!(report.usage.mcp_calls, 1);
        assert_eq!(report.usage.cli_calls, 1);
        assert_eq!(report.usage.daemon_events, 1);
        assert_eq!(report.search.indexed_searches, 1);
        assert_eq!(report.search.reuse_hits, 1);
        assert_eq!(report.search.result_omissions, 1);
        assert_eq!(report.search.estimated_result_tokens_saved, 96);
        assert_eq!(report.memo.redundant_rereads_prevented, 1);
        assert_eq!(report.memo.tokens_saved, 240);
        assert_eq!(report.context.context_pack_calls, 1);
        assert!(report.help_score > 0);

        let serialized = serde_json::to_string(&events).expect("serialize events");
        assert!(!serialized.contains("/Users/alice"));
        assert!(!serialized.contains("secret-product"));
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UsageMetrics {
    pub schema_version: u32,
    pub started_at: i64,
    pub updated_at: i64,
    pub total_searches: u64,
    pub search_cache_hits: u64,
    pub search_cache_misses: u64,
    pub search_errors: u64,
    pub memo_checks: u64,
    pub context_packs: u64,
    pub session_actions: u64,
    pub total_matches_returned: u64,
    pub total_bytes_scanned: u64,
    pub total_search_wall_millis: f64,
    pub tool_counts: BTreeMap<String, u64>,
}
