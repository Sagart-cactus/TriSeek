# TriSeek Benchmark Summary

Generated at: 2026-03-29T21:38:45.180184Z

## Notes

- The timing tables below come from the raw run artifacts in `report.json` and `report.csv`.
- The raw `report.json` correctness flags contain 13 false negatives from a pre-fix path-normalization bug in the benchmark harness. The `Correct` column in this markdown reflects the final post-run revalidation recorded in `correctness-revalidation.md`.
- On this macOS host, `/usr/bin/time -l` emitted a raw `maximum resident set size` value without an explicit unit. The JSON/CSV artifacts preserve that raw value; treat it as byte-scale when interpreting memory.

## Machine

- Host: Sagars-MacBook-Pro.local
- OS: macOS 26.2
- Architecture: x86_64
- Logical cores: 16

## Repo Stats

| Repo | Commit | Searchable Files | Searchable Bytes | Category |
|---|---|---:|---:|---|
| serde-rs_serde | fa7da4a93567 | 339 | 1326149 | Small |
| kubernetes_kubernetes | c6a95ffd4c78 | 28131 | 254117420 | Medium |
| torvalds_linux | cbfffcca2bf0 | 92914 | 1567876090 | Large |

## Query Benchmarks

| Repo | Query | Baseline | Correct | TriSeek p50 ms | Baseline p50 ms | TriSeek p95 ms | Baseline p95 ms |
|---|---|---|---|---:|---:|---:|---:|
| serde-rs_serde | path_all | rg --files | yes | 27.636 | 15.973 | 30.943 | 19.646 |
| serde-rs_serde | path_suffix | fd | yes | 26.797 | 19.097 | 30.826 | 23.041 |
| serde-rs_serde | path_exact_name | fd | yes | 26.074 | 17.435 | 29.262 | 22.080 |
| serde-rs_serde | path_substring | rg --files \| rg | yes | 26.514 | 25.528 | 28.677 | 30.663 |
| serde-rs_serde | literal_selective | rg | yes | 20.810 | 16.874 | 23.249 | 18.604 |
| serde-rs_serde | literal_moderate | rg | yes | 22.235 | 16.518 | 25.858 | 18.243 |
| serde-rs_serde | literal_high | rg | yes | 29.992 | 17.343 | 31.090 | 19.753 |
| serde-rs_serde | regex_anchor | rg | yes | 21.913 | 17.075 | 23.601 | 19.534 |
| serde-rs_serde | regex_weak | rg | yes | 48.226 | 21.330 | 51.036 | 22.888 |
| serde-rs_serde | literal_no_match | rg | yes | 22.264 | 17.806 | 23.501 | 20.310 |
| serde-rs_serde | regex_no_match | rg | yes | 21.506 | 16.295 | 22.991 | 20.033 |
| serde-rs_serde | multi_or | rg | yes | 33.703 | 16.358 | 37.806 | 17.943 |
| serde-rs_serde | path_plus_content | rg | yes | 20.263 | 17.326 | 21.614 | 20.047 |
| kubernetes_kubernetes | path_all | rg --files | yes | 780.319 | 78.060 | 795.816 | 80.910 |
| kubernetes_kubernetes | path_suffix | fd | yes | 753.925 | 73.544 | 767.766 | 77.037 |
| kubernetes_kubernetes | path_exact_name | fd | yes | 746.107 | 71.504 | 772.471 | 76.330 |
| kubernetes_kubernetes | path_substring | rg --files \| rg | yes | 753.146 | 88.735 | 781.298 | 91.224 |
| kubernetes_kubernetes | literal_selective | rg | yes | 795.892 | 371.951 | 802.624 | 385.558 |
| kubernetes_kubernetes | literal_moderate | rg | yes | 755.274 | 377.212 | 774.972 | 380.651 |
| kubernetes_kubernetes | literal_high | rg | yes | 770.489 | 390.921 | 832.230 | 411.013 |
| kubernetes_kubernetes | regex_anchor | rg | yes | 823.839 | 377.172 | 839.145 | 420.813 |
| kubernetes_kubernetes | regex_weak | rg | yes | 5422.507 | 670.583 | 5784.654 | 773.744 |
| kubernetes_kubernetes | literal_no_match | rg | yes | 879.457 | 377.142 | 1038.505 | 396.369 |
| kubernetes_kubernetes | regex_no_match | rg | yes | 442.794 | 474.095 | 468.673 | 492.961 |
| kubernetes_kubernetes | multi_or | rg | yes | 1050.083 | 447.018 | 1136.444 | 476.639 |
| kubernetes_kubernetes | path_plus_content | rg | yes | 880.231 | 210.443 | 1012.633 | 217.327 |
| torvalds_linux | path_all | rg --files | yes | 3899.178 | 108.061 | 4123.471 | 112.747 |
| torvalds_linux | path_suffix | fd | yes | 3871.448 | 99.886 | 3970.914 | 104.773 |
| torvalds_linux | path_exact_name | fd | yes | 3883.937 | 96.355 | 4002.788 | 99.207 |
| torvalds_linux | path_substring | rg --files \| rg | yes | 3830.623 | 117.732 | 3971.112 | 121.547 |
| torvalds_linux | literal_selective | rg | yes | 3880.200 | 1513.532 | 3975.314 | 1674.281 |
| torvalds_linux | literal_moderate | rg | yes | 3986.090 | 1530.265 | 4205.680 | 1909.999 |
| torvalds_linux | literal_high | rg | yes | 3917.485 | 1535.937 | 4254.540 | 1906.712 |
| torvalds_linux | regex_anchor | rg | yes | 3923.482 | 1520.499 | 4173.584 | 1856.199 |
| torvalds_linux | regex_weak | rg | yes | 27619.326 | 3757.776 | 28475.049 | 4003.967 |
| torvalds_linux | literal_no_match | rg | yes | 3981.622 | 1494.385 | 4231.139 | 1825.466 |
| torvalds_linux | regex_no_match | rg | yes | 2077.367 | 1924.674 | 2153.521 | 2000.898 |
| torvalds_linux | multi_or | rg | yes | 2683.349 | 1972.760 | 2818.824 | 2052.449 |
| torvalds_linux | path_plus_content | rg | yes | 3883.089 | 496.484 | 4244.875 | 518.875 |

## Sessions

| Repo | Session | TriSeek p50 ms | Baseline p50 ms |
|---|---|---:|---:|
| serde-rs_serde | session_20 | 136.355 | 80.617 |
| serde-rs_serde | session_100 | 658.822 | 79.153 |
| kubernetes_kubernetes | session_20 | 3736.506 | 1935.319 |
| kubernetes_kubernetes | session_100 | 17575.206 | 2042.451 |
| torvalds_linux | session_20 | 11432.385 | 9895.690 |
| torvalds_linux | session_100 | 50214.472 | 9582.882 |

## Build and Update

| Repo | Build ms | Index Bytes | Update ms |
|---|---:|---:|---:|
| serde-rs_serde | 74 | 386942 | 34.135 |
| kubernetes_kubernetes | 13027 | 91256489 | 2279.239 |
| torvalds_linux | 68309 | 491236884 | 9939.164 |

## Headline Findings

- TriSeek was slower than the baseline in 38 of 39 timed single-query cases. The only p50 win was `regex_no_match` on `kubernetes/kubernetes`, where TriSeek ran at 442.794 ms versus 474.095 ms for `rg`.
- Repeated sessions did not amortize index cost. `session_20` regressed by 1.16x to 1.93x, and `session_100` regressed by 5.24x to 8.60x versus the shell-tool baseline.
- Build and update overhead was acceptable only on the smallest repo. The large-repo build cost reached 68.3 s with a 491 MB on-disk index, and incremental update still took 9.9 s on `torvalds/linux`.
- No candidate repo crossed the plan's `very_large` threshold. `torvalds/linux` and `rust-lang/rust` both measured as `large`, so the recommendation below does not claim a validated `very_large` activation policy.

## Recommendation

- Keep `rg` as the default search path for small, medium, and large repositories.
- Keep filename/path-only lookups on shell tools such as `fd` and `rg --files`, because the current path index is materially slower than the baseline.
- Treat the indexed engine as an experimental prototype behind explicit opt-in, not a production default.
