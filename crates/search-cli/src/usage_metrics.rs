use anyhow::{Context, Result};
use search_core::{UsageMetricsReport, read_usage_metrics_events};
use std::path::Path;

pub fn build_report(metrics_dir: &Path) -> Result<UsageMetricsReport> {
    let events = read_usage_metrics_events(metrics_dir)
        .with_context(|| format!("read usage metrics from {}", metrics_dir.display()))?;
    Ok(UsageMetricsReport::from_events(&events))
}

#[cfg(test)]
mod tests {
    use super::*;
    use search_core::{
        USAGE_METRICS_SCHEMA_VERSION, UsageMetricSource, UsageMetricsEvent,
        append_usage_metrics_event,
    };

    #[test]
    fn builds_empty_private_report_when_no_events_exist() {
        let temp = tempfile::tempdir().expect("tempdir");

        let report = build_report(temp.path()).expect("report");

        assert_eq!(report.schema_version, USAGE_METRICS_SCHEMA_VERSION);
        assert_eq!(report.storage.privacy_mode, "private_rollups");
        assert_eq!(report.usage.total_calls, 0);
    }

    #[test]
    fn builds_private_report_from_events_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        append_usage_metrics_event(
            temp.path(),
            &UsageMetricsEvent {
                source: UsageMetricSource::Cli,
                tool: "metrics".to_string(),
                ..UsageMetricsEvent::default()
            },
        )
        .expect("append metric");

        let report = build_report(temp.path()).expect("report");

        assert_eq!(report.usage.total_calls, 1);
        assert_eq!(report.usage.cli_calls, 1);
        assert_eq!(report.usage.tool_counts.get("metrics"), Some(&1));
    }
}
