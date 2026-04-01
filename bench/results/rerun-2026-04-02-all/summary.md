# TriSeek Benchmark Summary

Generated at: 2026-04-01T18:18:48.081379Z

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
| torvalds_linux | cbfffcca2bf0 | 92916 | 1567893871 | Large |
| rust-lang_rust | 584d32e3ee7a | 58472 | 197699311 | Large |
| BurntSushi_ripgrep | 4519153e5e46 | 207 | 3109499 | Small |

## Query Benchmarks

| Repo | Query | Baseline | Correct | TriSeek p50 ms | Baseline p50 ms | TriSeek p95 ms | Baseline p95 ms |
|---|---|---|---|---:|---:|---:|---:|
| serde-rs_serde | path_all | rg --files | yes | 32.233 | 13.876 | 32.866 | 16.479 |
| serde-rs_serde | path_suffix | fd | yes | 32.386 | 17.678 | 33.944 | 20.719 |
| serde-rs_serde | path_exact_name | fd | yes | 31.753 | 16.751 | 35.695 | 17.845 |
| serde-rs_serde | path_substring | rg --files \| rg | yes | 31.574 | 27.053 | 33.085 | 29.431 |
| serde-rs_serde | literal_selective | rg | yes | 20.966 | 15.341 | 21.845 | 16.488 |
| serde-rs_serde | literal_moderate | rg | yes | 21.247 | 16.022 | 22.572 | 17.081 |
| serde-rs_serde | literal_high | rg | yes | 22.280 | 16.133 | 23.850 | 17.589 |
| serde-rs_serde | regex_anchor | rg | yes | 20.859 | 16.142 | 22.254 | 17.540 |
| serde-rs_serde | regex_weak | rg | yes | 25.422 | 21.223 | 26.729 | 22.225 |
| serde-rs_serde | literal_no_match | rg | yes | 20.528 | 16.149 | 21.766 | 17.123 |
| serde-rs_serde | regex_no_match | rg | yes | 20.895 | 16.478 | 22.585 | 17.802 |
| serde-rs_serde | multi_or | rg | yes | 22.756 | 16.856 | 24.886 | 17.641 |
| serde-rs_serde | path_plus_content | rg | yes | 19.836 | 14.535 | 21.405 | 15.360 |
| kubernetes_kubernetes | path_all | rg --files | yes | 42.359 | 79.749 | 43.036 | 82.356 |
| kubernetes_kubernetes | path_suffix | fd | yes | 40.176 | 76.759 | 42.008 | 82.302 |
| kubernetes_kubernetes | path_exact_name | fd | yes | 36.453 | 74.768 | 38.610 | 77.613 |
| kubernetes_kubernetes | path_substring | rg --files \| rg | yes | 38.280 | 100.896 | 41.592 | 105.039 |
| kubernetes_kubernetes | literal_selective | rg | yes | 48.384 | 492.712 | 49.560 | 546.441 |
| kubernetes_kubernetes | literal_moderate | rg | yes | 58.292 | 598.560 | 60.160 | 643.268 |
| kubernetes_kubernetes | literal_high | rg | yes | 78.410 | 651.514 | 78.890 | 656.755 |
| kubernetes_kubernetes | regex_anchor | rg | yes | 69.372 | 664.654 | 71.106 | 728.676 |
| kubernetes_kubernetes | regex_weak | rg | no | 650.310 | 1213.794 | 789.596 | 1285.686 |
| kubernetes_kubernetes | literal_no_match | rg | yes | 64.290 | 673.482 | 64.861 | 751.741 |
| kubernetes_kubernetes | regex_no_match | rg | yes | 72.320 | 728.455 | 73.684 | 748.250 |
| kubernetes_kubernetes | multi_or | rg | yes | 90.003 | 750.664 | 91.215 | 766.303 |
| kubernetes_kubernetes | path_plus_content | rg | yes | 72.917 | 405.262 | 73.389 | 421.357 |
| torvalds_linux | path_all | rg --files | yes | 256.866 | 257.284 | 263.813 | 270.458 |
| torvalds_linux | path_suffix | fd | yes | 239.699 | 242.953 | 275.042 | 318.104 |
| torvalds_linux | path_exact_name | fd | yes | 207.863 | 218.428 | 209.976 | 221.990 |
| torvalds_linux | path_substring | rg --files \| rg | yes | 187.998 | 242.045 | 189.817 | 248.081 |
| torvalds_linux | literal_selective | rg | yes | 197.129 | 3971.990 | 200.123 | 4441.587 |
| torvalds_linux | literal_moderate | rg | yes | 288.942 | 4406.756 | 292.606 | 5348.296 |
| torvalds_linux | literal_high | rg | yes | 511.847 | 5971.179 | 552.255 | 6192.121 |
| torvalds_linux | regex_anchor | rg | yes | 406.075 | 5858.977 | 409.293 | 5965.303 |
| torvalds_linux | regex_weak | rg | yes | 2760.068 | 4023.162 | 2911.993 | 4166.090 |
| torvalds_linux | literal_no_match | rg | yes | 122.627 | 2024.604 | 125.961 | 2189.955 |
| torvalds_linux | regex_no_match | rg | yes | 152.777 | 2166.471 | 160.326 | 2209.612 |
| torvalds_linux | multi_or | rg | yes | 195.375 | 2129.535 | 202.036 | 2233.919 |
| torvalds_linux | path_plus_content | rg | yes | 140.333 | 732.721 | 143.214 | 750.024 |
| rust-lang_rust | path_all | rg --files | yes | 76.367 | 79.382 | 78.873 | 81.493 |
| rust-lang_rust | path_suffix | fd | yes | 74.309 | 74.308 | 75.466 | 80.484 |
| rust-lang_rust | path_exact_name | fd | yes | 67.099 | 72.964 | 68.425 | 74.322 |
| rust-lang_rust | path_substring | rg --files \| rg | yes | 66.107 | 88.978 | 67.692 | 95.806 |
| rust-lang_rust | literal_selective | rg | yes | 73.944 | 1002.537 | 75.415 | 1186.536 |
| rust-lang_rust | literal_moderate | rg | yes | 97.022 | 1225.084 | 97.875 | 1272.015 |
| rust-lang_rust | literal_high | rg | yes | 123.215 | 1253.578 | 129.834 | 1309.688 |
| rust-lang_rust | regex_anchor | rg | yes | 101.049 | 1203.499 | 108.017 | 1373.853 |
| rust-lang_rust | regex_weak | rg | yes | 1257.682 | 1405.874 | 1418.568 | 1449.082 |
| rust-lang_rust | literal_no_match | rg | yes | 98.816 | 1254.690 | 99.567 | 1328.188 |
| rust-lang_rust | regex_no_match | rg | yes | 95.519 | 1166.351 | 97.018 | 1178.303 |
| rust-lang_rust | multi_or | rg | yes | 121.755 | 1161.950 | 126.817 | 1206.597 |
| rust-lang_rust | path_plus_content | rg | yes | 104.139 | 669.707 | 107.729 | 680.502 |
| BurntSushi_ripgrep | path_all | rg --files | yes | 29.230 | 14.706 | 29.614 | 16.802 |
| BurntSushi_ripgrep | path_suffix | fd | yes | 29.164 | 19.633 | 30.017 | 21.508 |
| BurntSushi_ripgrep | path_exact_name | fd | yes | 29.109 | 20.421 | 29.913 | 23.557 |
| BurntSushi_ripgrep | path_substring | rg --files \| rg | yes | 29.177 | 25.640 | 29.434 | 30.257 |
| BurntSushi_ripgrep | literal_selective | rg | yes | 23.313 | 18.421 | 27.017 | 22.167 |
| BurntSushi_ripgrep | literal_moderate | rg | yes | 22.145 | 16.403 | 24.098 | 19.270 |
| BurntSushi_ripgrep | literal_high | rg | yes | 23.108 | 18.741 | 26.338 | 22.502 |
| BurntSushi_ripgrep | regex_anchor | rg | yes | 22.149 | 19.526 | 24.856 | 23.203 |
| BurntSushi_ripgrep | regex_weak | rg | yes | 25.873 | 22.689 | 29.394 | 24.794 |
| BurntSushi_ripgrep | literal_no_match | rg | yes | 22.120 | 18.708 | 25.814 | 20.356 |
| BurntSushi_ripgrep | regex_no_match | rg | yes | 22.181 | 21.715 | 25.741 | 22.919 |
| BurntSushi_ripgrep | multi_or | rg | yes | 22.231 | 18.082 | 23.963 | 19.668 |
| BurntSushi_ripgrep | path_plus_content | rg | yes | 21.378 | 15.650 | 24.951 | 16.764 |

## Sessions

| Repo | Session | TriSeek p50 ms | Baseline p50 ms |
|---|---|---:|---:|
| serde-rs_serde | session_20 | 34.172 | 82.135 |
| serde-rs_serde | session_100 | 131.653 | 86.027 |
| kubernetes_kubernetes | session_20 | 292.270 | 3575.138 |
| kubernetes_kubernetes | session_100 | 1255.185 | 3839.544 |
| torvalds_linux | session_20 | 612.810 | 10342.155 |
| torvalds_linux | session_100 | 2468.112 | 10408.510 |
| rust-lang_rust | session_20 | 374.870 | 5954.215 |
| rust-lang_rust | session_100 | 1500.637 | 6062.653 |
| BurntSushi_ripgrep | session_20 | 39.426 | 92.137 |
| BurntSushi_ripgrep | session_100 | 155.990 | 101.079 |

## Build and Update

| Repo | Build ms | Index Bytes | Update ms |
|---|---:|---:|---:|
| serde-rs_serde | 78 | 1237417 | 35.940 |
| kubernetes_kubernetes | 13971 | 213548193 | 4018.095 |
| torvalds_linux | 150390 | 1050094607 | 10060.955 |
| rust-lang_rust | 9633 | 231942920 | 4037.005 |
| BurntSushi_ripgrep | 166 | 2191925 | 36.586 |
