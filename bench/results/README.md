# Raw Benchmark Artifacts

This directory stores committed benchmark outputs and repo-measurement snapshots.

Current artifacts:

- `prepared_repos.json`: the timed benchmark repo set used by the final run
- `prepared_repos_full.json`: measurements for the wider candidate set, including repos that were cloned and measured but not fully timed
- `final-run/report.json`: raw benchmark report emitted by `search-bench run`
- `final-run/report.csv`: flattened case table for spreadsheet analysis
- `final-run/summary.md`: human-readable timing summary plus recommendation
- `final-run/correctness-revalidation.md`: post-run revalidation of the 13 cases that were false negatives in the raw report because of a path-normalization bug in the harness
- `smoke-run/*`: earlier smoke-test artifacts kept for debugging the harness

Benchmark repositories are intentionally not committed here. They live under the external cache root configured in `bench/manifest/repositories.yaml`.
