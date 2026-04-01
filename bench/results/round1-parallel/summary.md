# TriSeek Benchmark Summary

Generated at: 2026-03-30T13:52:01.077777Z

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
| serde-rs_serde | path_all | rg --files | yes | 32.745 | 14.491 | 33.325 | 17.674 |
| serde-rs_serde | path_suffix | fd | yes | 32.984 | 19.020 | 33.372 | 22.510 |
| serde-rs_serde | path_exact_name | fd | yes | 31.167 | 17.810 | 36.088 | 20.785 |
| serde-rs_serde | path_substring | rg --files \| rg | yes | 32.902 | 25.393 | 33.353 | 27.820 |
| serde-rs_serde | literal_selective | rg | yes | 21.462 | 16.417 | 23.902 | 17.797 |
| serde-rs_serde | literal_moderate | rg | yes | 22.638 | 16.711 | 24.550 | 17.596 |
| serde-rs_serde | literal_high | rg | yes | 30.587 | 16.368 | 32.060 | 17.368 |
| serde-rs_serde | regex_anchor | rg | yes | 22.505 | 16.666 | 23.955 | 18.245 |
| serde-rs_serde | regex_weak | rg | yes | 51.779 | 20.456 | 53.668 | 21.742 |
| serde-rs_serde | literal_no_match | rg | yes | 21.257 | 16.866 | 22.791 | 18.878 |
| serde-rs_serde | regex_no_match | rg | yes | 21.350 | 16.794 | 22.758 | 18.776 |
| serde-rs_serde | multi_or | rg | yes | 34.916 | 17.273 | 36.227 | 21.311 |
| serde-rs_serde | path_plus_content | rg | yes | 20.470 | 16.055 | 22.326 | 17.216 |
| kubernetes_kubernetes | path_all | rg --files | yes | 877.609 | 78.938 | 896.385 | 84.881 |
| kubernetes_kubernetes | path_suffix | fd | yes | 862.273 | 75.182 | 880.594 | 79.334 |
| kubernetes_kubernetes | path_exact_name | fd | yes | 878.068 | 72.427 | 891.206 | 74.823 |
| kubernetes_kubernetes | path_substring | rg --files \| rg | yes | 872.783 | 89.977 | 887.543 | 93.833 |
| kubernetes_kubernetes | literal_selective | rg | yes | 868.531 | 386.242 | 880.489 | 394.548 |
| kubernetes_kubernetes | literal_moderate | rg | no | 875.935 | 391.195 | 898.294 | 402.882 |
| kubernetes_kubernetes | literal_high | rg | no | 891.765 | 403.337 | 907.649 | 415.058 |
| kubernetes_kubernetes | regex_anchor | rg | yes | 874.548 | 386.894 | 896.165 | 396.865 |
| kubernetes_kubernetes | regex_weak | rg | yes | 5677.962 | 864.659 | 5931.812 | 951.448 |
| kubernetes_kubernetes | literal_no_match | rg | yes | 879.387 | 383.229 | 906.736 | 403.652 |
| kubernetes_kubernetes | regex_no_match | rg | yes | 881.239 | 394.831 | 916.269 | 401.069 |
| kubernetes_kubernetes | multi_or | rg | no | 890.497 | 418.663 | 914.650 | 426.005 |
| kubernetes_kubernetes | path_plus_content | rg | yes | 876.484 | 212.956 | 935.090 | 216.504 |
| torvalds_linux | path_all | rg --files | yes | 4205.711 | 108.898 | 4361.626 | 111.567 |
| torvalds_linux | path_suffix | fd | yes | 4210.556 | 105.296 | 4364.209 | 131.085 |
| torvalds_linux | path_exact_name | fd | yes | 4182.189 | 98.220 | 4285.707 | 101.539 |
| torvalds_linux | path_substring | rg --files \| rg | yes | 4260.259 | 121.329 | 4416.670 | 131.420 |
| torvalds_linux | literal_selective | rg | yes | 4173.223 | 1532.755 | 4297.346 | 1580.455 |
| torvalds_linux | literal_moderate | rg | yes | 4178.098 | 1532.614 | 4371.264 | 1573.571 |
| torvalds_linux | literal_high | rg | yes | 4256.241 | 1552.297 | 4336.629 | 1611.852 |
| torvalds_linux | regex_anchor | rg | yes | 4245.412 | 1540.570 | 4495.249 | 1557.240 |
| torvalds_linux | regex_weak | rg | yes | 28187.387 | 2835.879 | 28504.236 | 3133.563 |
| torvalds_linux | literal_no_match | rg | yes | 3979.252 | 1505.774 | 4160.871 | 1541.260 |
| torvalds_linux | regex_no_match | rg | yes | 3989.836 | 1498.106 | 4262.855 | 1551.141 |
| torvalds_linux | multi_or | rg | no | 4013.118 | 1533.357 | 4250.026 | 1554.432 |
| torvalds_linux | path_plus_content | rg | yes | 4008.764 | 508.743 | 4265.447 | 525.347 |

## Sessions

| Repo | Session | TriSeek p50 ms | Baseline p50 ms |
|---|---|---:|---:|
| serde-rs_serde | session_20 | 44.701 | 83.825 |
| serde-rs_serde | session_100 | 152.906 | 95.038 |
| kubernetes_kubernetes | session_20 | 984.510 | 1967.818 |
| kubernetes_kubernetes | session_100 | 1434.472 | 2673.118 |
| torvalds_linux | session_20 | 4326.052 | 7557.152 |
| torvalds_linux | session_100 | 5719.013 | 7516.578 |

## Build and Update

| Repo | Build ms | Index Bytes | Update ms |
|---|---:|---:|---:|
| serde-rs_serde | 76 | 329280 | 40.645 |
| kubernetes_kubernetes | 13837 | 91199589 | 2042.452 |
| torvalds_linux | 76331 | 483761344 | 7908.591 |
