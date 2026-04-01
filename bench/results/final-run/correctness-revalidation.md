# Final-Run Correctness Revalidation

The raw `report.json` from `final-run/` was produced before the last harness fix for leading `./` path normalization. That left 13 false negatives in the recorded correctness field even though the final engine behavior matches the baseline.

Revalidation method:

- Path workloads were rerun with `triseek search --engine auto` and compared against `triseek search --engine scan`.
- The `torvalds/linux literal_high` workload was rerun with `triseek search --engine auto` and compared against `rg --fixed-strings`.
- Comparisons normalized a leading `./` prefix on paths before diffing the result sets.

Validated cases:

| Repo | Query | Result Count | Status |
|---|---|---:|---|
| serde-rs_serde | path_all | 339 | pass |
| serde-rs_serde | path_suffix | 208 | pass |
| serde-rs_serde | path_exact_name | 7 | pass |
| serde-rs_serde | path_substring | 271 | pass |
| kubernetes_kubernetes | path_all | 28132 | pass |
| kubernetes_kubernetes | path_suffix | 16929 | pass |
| kubernetes_kubernetes | path_exact_name | 1 | pass |
| kubernetes_kubernetes | path_substring | 2349 | pass |
| torvalds_linux | path_all | 92916 | pass |
| torvalds_linux | path_suffix | 36549 | pass |
| torvalds_linux | path_exact_name | 1 | pass |
| torvalds_linux | path_substring | 1489 | pass |
| torvalds_linux | literal_high | 86060 | pass |

Summary:

- 13 of 13 previously failed cases passed on the final build.
- The benchmark recommendation is unchanged: correctness is acceptable for the covered workloads, but performance is still worse than the shell-tool baseline.
