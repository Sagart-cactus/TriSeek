# TriSeek Benchmark Summary

Generated at: 2026-03-30T17:14:05.273375Z

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
| rust-lang_rust | 584d32e3ee7a | 58472 | 197699311 | Large |
| BurntSushi_ripgrep | 4519153e5e46 | 207 | 3109499 | Small |

## Query Benchmarks

| Repo | Query | Baseline | Correct | TriSeek p50 ms | Baseline p50 ms | TriSeek p95 ms | Baseline p95 ms |
|---|---|---|---|---:|---:|---:|---:|
| serde-rs_serde | path_all | rg --files | yes | 39.398 | 14.814 | 41.138 | 18.240 |
| serde-rs_serde | path_suffix | fd | yes | 39.372 | 20.099 | 40.758 | 23.656 |
| serde-rs_serde | path_exact_name | fd | yes | 34.698 | 19.744 | 36.287 | 22.237 |
| serde-rs_serde | path_substring | rg --files \| rg | yes | 38.898 | 31.910 | 40.274 | 34.928 |
| serde-rs_serde | literal_selective | rg | yes | 24.352 | 18.113 | 24.913 | 19.140 |
| serde-rs_serde | literal_moderate | rg | yes | 22.336 | 17.936 | 24.944 | 20.971 |
| serde-rs_serde | literal_high | rg | yes | 27.509 | 21.303 | 36.068 | 23.891 |
| serde-rs_serde | regex_anchor | rg | yes | 24.625 | 18.135 | 25.742 | 20.780 |
| serde-rs_serde | regex_weak | rg | yes | 29.958 | 24.264 | 49.161 | 26.557 |
| serde-rs_serde | literal_no_match | rg | yes | 27.840 | 19.064 | 32.080 | 22.666 |
| serde-rs_serde | regex_no_match | rg | yes | 26.162 | 19.898 | 31.383 | 27.079 |
| serde-rs_serde | multi_or | rg | yes | 28.729 | 19.962 | 32.827 | 24.765 |
| serde-rs_serde | path_plus_content | rg | yes | 25.200 | 17.870 | 30.507 | 23.591 |
| kubernetes_kubernetes | path_all | rg --files | yes | 52.715 | 88.602 | 56.402 | 93.859 |
| kubernetes_kubernetes | path_suffix | fd | yes | 48.043 | 85.258 | 51.041 | 91.112 |
| kubernetes_kubernetes | path_exact_name | fd | yes | 44.661 | 80.063 | 49.055 | 83.834 |
| kubernetes_kubernetes | path_substring | rg --files \| rg | yes | 45.581 | 101.486 | 48.609 | 106.236 |
| kubernetes_kubernetes | literal_selective | rg | yes | 49.498 | 415.420 | 53.189 | 435.331 |
| kubernetes_kubernetes | literal_moderate | rg | no | 50.579 | 458.911 | 55.033 | 624.650 |
| kubernetes_kubernetes | literal_high | rg | no | 63.708 | 464.399 | 71.069 | 476.826 |
| kubernetes_kubernetes | regex_anchor | rg | yes | 60.688 | 500.552 | 66.820 | 781.579 |
| kubernetes_kubernetes | regex_weak | rg | yes | 679.018 | 1248.362 | 851.090 | 1313.196 |
| kubernetes_kubernetes | literal_no_match | rg | yes | 63.729 | 619.927 | 65.090 | 665.555 |
| kubernetes_kubernetes | regex_no_match | rg | yes | 72.246 | 609.554 | 75.274 | 864.671 |
| kubernetes_kubernetes | multi_or | rg | no | 89.860 | 592.636 | 101.185 | 625.781 |
| kubernetes_kubernetes | path_plus_content | rg | yes | 66.375 | 315.550 | 71.096 | 506.447 |
| torvalds_linux | path_all | rg --files | yes | 151.326 | 135.341 | 155.074 | 156.407 |
| torvalds_linux | path_suffix | fd | yes | 146.096 | 115.475 | 149.468 | 121.586 |
| torvalds_linux | path_exact_name | fd | yes | 142.234 | 111.206 | 145.175 | 117.847 |
| torvalds_linux | path_substring | rg --files \| rg | yes | 137.875 | 144.526 | 140.761 | 151.506 |
| torvalds_linux | literal_selective | rg | yes | 143.952 | 2008.644 | 147.461 | 2253.215 |
| torvalds_linux | literal_moderate | rg | yes | 189.437 | 2257.230 | 222.190 | 2326.933 |
| torvalds_linux | literal_high | rg | yes | 230.494 | 2190.630 | 238.629 | 2339.487 |
| torvalds_linux | regex_anchor | rg | yes | 185.694 | 2201.692 | 188.989 | 2290.494 |
| torvalds_linux | regex_weak | rg | yes | 1879.153 | 3516.442 | 2241.775 | 3758.679 |
| torvalds_linux | literal_no_match | rg | yes | 130.185 | 1824.759 | 135.209 | 1954.122 |
| torvalds_linux | regex_no_match | rg | yes | 146.433 | 1881.221 | 154.159 | 2015.519 |
| torvalds_linux | multi_or | rg | yes | 185.551 | 1909.754 | 191.174 | 2602.618 |
| torvalds_linux | path_plus_content | rg | yes | 138.494 | 657.438 | 143.614 | 786.503 |
| rust-lang_rust | path_all | rg --files | yes | 79.712 | 82.315 | 80.154 | 89.916 |
| rust-lang_rust | path_suffix | fd | yes | 75.630 | 78.908 | 76.557 | 82.106 |
| rust-lang_rust | path_exact_name | fd | yes | 95.431 | 79.030 | 109.181 | 89.200 |
| rust-lang_rust | path_substring | rg --files \| rg | yes | 68.016 | 93.337 | 69.176 | 101.807 |
| rust-lang_rust | literal_selective | rg | yes | 75.469 | 998.773 | 76.357 | 1163.326 |
| rust-lang_rust | literal_moderate | rg | no | 138.959 | 1138.174 | 150.750 | 1259.467 |
| rust-lang_rust | literal_high | rg | no | 109.480 | 1047.196 | 113.145 | 1072.123 |
| rust-lang_rust | regex_anchor | rg | no | 89.711 | 1048.373 | 92.086 | 1068.861 |
| rust-lang_rust | regex_weak | rg | yes | 1043.715 | 1350.324 | 1226.727 | 1694.660 |
| rust-lang_rust | literal_no_match | rg | yes | 117.328 | 1126.793 | 121.725 | 1206.704 |
| rust-lang_rust | regex_no_match | rg | yes | 84.813 | 1030.044 | 88.024 | 1059.842 |
| rust-lang_rust | multi_or | rg | no | 109.693 | 1044.195 | 115.740 | 1110.447 |
| rust-lang_rust | path_plus_content | rg | yes | 97.632 | 599.941 | 101.287 | 624.841 |
| BurntSushi_ripgrep | path_all | rg --files | yes | 29.670 | 14.908 | 30.376 | 18.228 |
| BurntSushi_ripgrep | path_suffix | fd | yes | 30.247 | 17.825 | 33.897 | 21.635 |
| BurntSushi_ripgrep | path_exact_name | fd | yes | 30.010 | 16.677 | 30.559 | 17.885 |
| BurntSushi_ripgrep | path_substring | rg --files \| rg | yes | 30.138 | 25.673 | 30.560 | 27.736 |
| BurntSushi_ripgrep | literal_selective | rg | yes | 19.963 | 15.215 | 21.149 | 16.863 |
| BurntSushi_ripgrep | literal_moderate | rg | yes | 20.360 | 15.609 | 21.674 | 16.734 |
| BurntSushi_ripgrep | literal_high | rg | yes | 20.633 | 15.601 | 21.959 | 16.320 |
| BurntSushi_ripgrep | regex_anchor | rg | yes | 19.998 | 15.415 | 21.648 | 16.754 |
| BurntSushi_ripgrep | regex_weak | rg | yes | 24.458 | 20.696 | 26.139 | 22.818 |
| BurntSushi_ripgrep | literal_no_match | rg | yes | 19.840 | 15.254 | 21.290 | 16.600 |
| BurntSushi_ripgrep | regex_no_match | rg | yes | 20.581 | 15.540 | 22.280 | 16.733 |
| BurntSushi_ripgrep | multi_or | rg | yes | 20.655 | 16.034 | 21.713 | 18.059 |
| BurntSushi_ripgrep | path_plus_content | rg | yes | 19.418 | 14.523 | 21.665 | 16.071 |

## Sessions

| Repo | Session | TriSeek p50 ms | Baseline p50 ms |
|---|---|---:|---:|
| serde-rs_serde | session_20 | 43.568 | 102.504 |
| serde-rs_serde | session_100 | 177.450 | 108.165 |
| kubernetes_kubernetes | session_20 | 310.399 | 2725.448 |
| kubernetes_kubernetes | session_100 | 1189.163 | 2960.881 |
| torvalds_linux | session_20 | 563.638 | 9877.916 |
| torvalds_linux | session_100 | 2522.703 | 10087.068 |
| rust-lang_rust | session_20 | 330.043 | 5226.304 |
| rust-lang_rust | session_100 | 1268.744 | 5134.832 |
| BurntSushi_ripgrep | session_20 | 43.477 | 91.308 |
| BurntSushi_ripgrep | session_100 | 173.988 | 91.578 |

## Build and Update

| Repo | Build ms | Index Bytes | Update ms |
|---|---:|---:|---:|
| serde-rs_serde | 80 | 1191237 | 58.485 |
| kubernetes_kubernetes | 18228 | 214122456 | 3119.434 |
| torvalds_linux | 93718 | 1050059157 | 11524.868 |
| rust-lang_rust | 12718 | 231987590 | 3660.496 |
| BurntSushi_ripgrep | 169 | 2191925 | 36.702 |
