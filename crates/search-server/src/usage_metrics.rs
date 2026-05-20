use anyhow::{Context, Result};
use search_core::{
    MemoCheckParams, MemoCheckRecommendation, MemoCheckResponse, MemoFileStatusKind,
    MemoObserveParams, MemoObserveResponse, MemoSessionResponse, SearchResponse, SearchUsageMetric,
    UsageMetricSource, UsageMetricsEvent, UsageMetricsReport, append_usage_metrics_event,
    private_repo_hash, read_usage_metrics_events,
};
use std::path::{Path, PathBuf};

pub struct UsageMetricsStore {
    metrics_dir: PathBuf,
}

impl UsageMetricsStore {
    pub fn load_from_disk(daemon_dir: &Path) -> Result<Self> {
        let metrics_dir = metrics_dir_for_daemon(daemon_dir);
        std::fs::create_dir_all(&metrics_dir)
            .with_context(|| format!("create {}", metrics_dir.display()))?;
        Ok(Self { metrics_dir })
    }

    pub fn snapshot(&self) -> UsageMetricsReport {
        read_usage_metrics_events(&self.metrics_dir)
            .map(|events| UsageMetricsReport::from_events(&events))
            .unwrap_or_else(|_| UsageMetricsReport::from_events(&[]))
    }

    pub fn record_search_success(&self, repo_root: &Path, response: &SearchResponse) {
        let _ = self.record(UsageMetricsEvent {
            source: UsageMetricSource::Daemon,
            tool: "search".to_string(),
            repo_hash: Some(private_repo_hash(repo_root.display().to_string())),
            search: Some(SearchUsageMetric {
                indexed: matches!(response.engine, search_core::SearchEngineKind::Indexed),
                fallback_used: !matches!(response.engine, search_core::SearchEngineKind::Indexed),
                reuse_hit: false,
                results_omitted: false,
                estimated_tokens_saved: 0,
                wall_millis: Some(response.metrics.process.wall_millis),
                files_with_matches: response.summary.files_with_matches as u64,
                total_line_matches: response.summary.total_line_matches as u64,
            }),
            ..UsageMetricsEvent::default()
        });
    }

    pub fn record_search_error(&self, repo_root: Option<&Path>) {
        let _ = self.record(UsageMetricsEvent {
            source: UsageMetricSource::Daemon,
            tool: "search_error".to_string(),
            repo_hash: repo_root.map(|root| private_repo_hash(root.display().to_string())),
            reliability: Some(search_core::ReliabilityUsageMetric {
                errors: 1,
                ..search_core::ReliabilityUsageMetric::default()
            }),
            ..UsageMetricsEvent::default()
        });
    }

    pub fn record_tool(&self, name: &str) {
        let _ = self.record(UsageMetricsEvent {
            source: UsageMetricSource::Daemon,
            tool: name.to_string(),
            ..UsageMetricsEvent::default()
        });
    }

    pub fn record_memo_observe(&self, params: &MemoObserveParams, response: &MemoObserveResponse) {
        let _ = self.record(UsageMetricsEvent {
            source: UsageMetricSource::Daemon,
            tool: "memo_observe".to_string(),
            repo_hash: Some(private_repo_hash(&params.repo_root)),
            memo: Some(search_core::MemoUsageMetric {
                redundant_reads_prevented: response.redundant_reads_prevented,
                tokens_saved: response.tokens_saved,
                total_reads_observed: response.total_reads_observed,
                compaction_invalidations: response.compaction_invalidations,
                ..search_core::MemoUsageMetric::default()
            }),
            ..UsageMetricsEvent::default()
        });
    }

    pub fn record_memo_session(&self, response: &MemoSessionResponse) {
        let _ = self.record(UsageMetricsEvent {
            source: UsageMetricSource::Daemon,
            tool: "memo_session".to_string(),
            memo: Some(search_core::MemoUsageMetric {
                redundant_reads_prevented: response.redundant_reads_prevented,
                tokens_saved: response.tokens_saved,
                total_reads_observed: response.total_reads,
                tracked_files: response.tracked_files as u64,
                compaction_invalidations: response.compaction_count as u64,
                ..search_core::MemoUsageMetric::default()
            }),
            ..UsageMetricsEvent::default()
        });
    }

    pub fn record_memo_check(&self, params: &MemoCheckParams, response: &MemoCheckResponse) {
        let _ = self.record(UsageMetricsEvent {
            source: UsageMetricSource::Daemon,
            tool: "memo_check".to_string(),
            repo_hash: Some(private_repo_hash(&params.repo_root)),
            memo: Some(search_core::MemoUsageMetric {
                memo_checks: 1,
                skip_reread: u64::from(matches!(
                    response.recommendation,
                    MemoCheckRecommendation::SkipReread
                )),
                stale_detections: u64::from(matches!(response.status, MemoFileStatusKind::Stale)),
                unknown_detections: u64::from(matches!(
                    response.status,
                    MemoFileStatusKind::Unknown
                )),
                tokens_saved: if matches!(
                    response.recommendation,
                    MemoCheckRecommendation::SkipReread
                ) {
                    response.tokens_at_last_read.unwrap_or(0) as u64
                } else {
                    0
                },
                ..search_core::MemoUsageMetric::default()
            }),
            ..UsageMetricsEvent::default()
        });
    }

    pub fn flush_to_disk(&self) -> Result<()> {
        std::fs::create_dir_all(&self.metrics_dir)
            .with_context(|| format!("create {}", self.metrics_dir.display()))
    }

    fn record(&self, event: UsageMetricsEvent) -> Result<()> {
        append_usage_metrics_event(&self.metrics_dir, &event)
            .with_context(|| format!("append usage metrics to {}", self.metrics_dir.display()))
    }
}

fn metrics_dir_for_daemon(daemon_dir: &Path) -> PathBuf {
    if daemon_dir.file_name().is_some_and(|name| name == "daemon")
        && let Some(home) = daemon_dir.parent()
    {
        return home.join("metrics");
    }
    daemon_dir.join("metrics")
}
