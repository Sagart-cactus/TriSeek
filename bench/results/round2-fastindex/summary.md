# TriSeek Benchmark Summary

Generated at: 2026-03-30T15:50:29.167005Z

## Machine

- Host: Sagars-MacBook-Pro.local
- OS: macOS 26.2
- Architecture: x86_64
- Logical cores: 16

## Repo Stats

| Repo | Commit | Searchable Files | Searchable Bytes | Category |
|---|---|---:|---:|---|
| serde-rs_serde | fa7da4a93567 | 339 | 1326149 | Small |
| kubernetes_kubernetes | c6a95ffd4c78 | 28132 | 257586895 | Medium |
| torvalds_linux | cbfffcca2bf0 | 92916 | 1567893871 | Large |

## Query Benchmarks

| Repo | Query | Baseline | Correct | TriSeek p50 ms | Baseline p50 ms | TriSeek p95 ms | Baseline p95 ms |
|---|---|---|---|---:|---:|---:|---:|
| serde-rs_serde | path_all | rg --files | yes | 33.518 | 14.950 | 34.054 | 17.288 |
| serde-rs_serde | path_suffix | fd | yes | 33.508 | 19.993 | 34.019 | 27.985 |
| serde-rs_serde | path_exact_name | fd | yes | 33.866 | 18.643 | 38.903 | 19.943 |
| serde-rs_serde | path_substring | rg --files \| rg | yes | 34.211 | 26.346 | 39.050 | 28.492 |
| serde-rs_serde | literal_selective | rg | yes | 21.387 | 16.990 | 22.745 | 18.256 |
| serde-rs_serde | literal_moderate | rg | yes | 23.094 | 17.131 | 24.286 | 18.682 |
| serde-rs_serde | literal_high | rg | yes | 31.330 | 17.127 | 32.772 | 18.099 |
| serde-rs_serde | regex_anchor | rg | yes | 22.775 | 17.351 | 24.136 | 18.715 |
| serde-rs_serde | regex_weak | rg | yes | 52.309 | 21.617 | 53.377 | 23.319 |
| serde-rs_serde | literal_no_match | rg | yes | 21.902 | 17.139 | 24.562 | 18.138 |
| serde-rs_serde | regex_no_match | rg | yes | 22.198 | 17.521 | 23.039 | 18.799 |
| serde-rs_serde | multi_or | rg | yes | 35.837 | 18.325 | 36.793 | 19.774 |
| serde-rs_serde | path_plus_content | rg | yes | 21.776 | 28.749 | 24.158 | 47.440 |
| kubernetes_kubernetes | path_all | rg --files | yes | 39.017 | 78.049 | 41.057 | 79.927 |
| kubernetes_kubernetes | path_suffix | fd | yes | 41.821 | 79.021 | 71.728 | 90.191 |
| kubernetes_kubernetes | path_exact_name | fd | yes | 42.506 | 75.376 | 45.302 | 98.463 |
| kubernetes_kubernetes | path_substring | rg --files \| rg | yes | 37.255 | 91.439 | 39.634 | 99.680 |
| kubernetes_kubernetes | literal_selective | rg | yes | 42.320 | 371.252 | 43.480 | 391.088 |
| kubernetes_kubernetes | literal_moderate | rg | no | 45.544 | 386.434 | 48.365 | 396.840 |
| kubernetes_kubernetes | literal_high | rg | no | 52.009 | 392.171 | 53.954 | 396.905 |
| kubernetes_kubernetes | regex_anchor | rg | yes | 51.358 | 382.949 | 65.085 | 414.429 |
| kubernetes_kubernetes | regex_weak | rg | yes | 6075.255 | 905.849 | 6167.521 | 1229.910 |
| kubernetes_kubernetes | literal_no_match | rg | yes | 38.322 | 387.261 | 39.269 | 394.822 |
| kubernetes_kubernetes | regex_no_match | rg | yes | 40.893 | 387.583 | 42.037 | 418.750 |
| kubernetes_kubernetes | multi_or | rg | no | 49.090 | 414.451 | 50.011 | 429.212 |
| kubernetes_kubernetes | path_plus_content | rg | yes | 41.899 | 222.321 | 42.486 | 238.598 |
| torvalds_linux | path_all | rg --files | yes | 123.990 | 118.323 | 129.533 | 122.264 |
| torvalds_linux | path_suffix | fd | yes | 129.832 | 119.518 | 143.815 | 160.772 |
| torvalds_linux | path_exact_name | fd | yes | 116.381 | 109.050 | 169.992 | 139.190 |
| torvalds_linux | path_substring | rg --files \| rg | yes | 111.391 | 131.537 | 114.815 | 153.978 |
| torvalds_linux | literal_selective | rg | yes | 120.417 | 1632.855 | 137.437 | 1670.619 |
| torvalds_linux | literal_moderate | rg | yes | 123.891 | 1560.158 | 127.529 | 1650.300 |
| torvalds_linux | literal_high | rg | yes | 163.674 | 1624.712 | 166.501 | 1778.788 |
| torvalds_linux | regex_anchor | rg | yes | 131.155 | 1650.441 | 145.538 | 1765.925 |
| torvalds_linux | regex_weak | rg | yes | 31584.922 | 3076.134 | 35722.577 | 3265.085 |
| torvalds_linux | literal_no_match | rg | yes | 115.844 | 1557.376 | 126.043 | 1600.792 |
| torvalds_linux | regex_no_match | rg | yes | 122.644 | 1659.421 | 128.515 | 1708.790 |
| torvalds_linux | multi_or | rg | no | 161.255 | 1708.267 | 165.764 | 1843.855 |
| torvalds_linux | path_plus_content | rg | yes | 121.710 | 560.942 | 125.192 | 599.007 |

## Sessions

| Repo | Session | TriSeek p50 ms | Baseline p50 ms |
|---|---|---:|---:|
| serde-rs_serde | session_20 | 38.189 | 92.008 |
| serde-rs_serde | session_100 | 182.659 | 97.386 |
| kubernetes_kubernetes | session_20 | 222.967 | 2086.332 |
| kubernetes_kubernetes | session_100 | 879.852 | 2097.942 |
| torvalds_linux | session_20 | 523.527 | 8530.615 |
| torvalds_linux | session_100 | 2087.362 | 8618.894 |

## Build and Update

| Repo | Build ms | Index Bytes | Update ms |
|---|---:|---:|---:|
| serde-rs_serde | 76 | 1196035 | 40.562 |
| kubernetes_kubernetes | 13177 | 214173890 | 2320.090 |
| torvalds_linux | 74532 | 1051511933 | 10916.180 |
