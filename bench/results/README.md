# Raw Benchmark Artifacts

This directory stores committed benchmark outputs and repo-measurement snapshots.

Current full-rerun artifacts:

- `rerun-2026-04-02-all/report.json`: raw benchmark report from the fresh full rerun across all 5 manifest repos
- `rerun-2026-04-02-all/report.csv`: flattened case table for spreadsheet analysis
- `rerun-2026-04-02-all/summary.md`: human-readable timing summary
- `rerun-2026-04-02-all/correctness-revalidation.md`: note for the one remaining raw correctness mismatch in the rerun

Historical artifacts:

- `final-run/*`: pre-optimization 3-repo baseline committed before the mmap / parallel-verification optimization rounds; kept for before/after comparison, not as the current result set
- `round1-parallel/*`, `round2-fastindex/*`, `round3-fixed/*`: intermediate optimization snapshots
- `prepared_repos.json`: historical timed repo set used by the pre-optimization baseline
- `prepared_repos_full.json`: wider measured candidate set from that same planning pass
- `smoke-run/*`: earlier smoke-test artifacts kept for debugging the harness

Benchmark repositories are intentionally not committed here. They live under the external cache root configured in `bench/manifest/repositories.yaml`.
