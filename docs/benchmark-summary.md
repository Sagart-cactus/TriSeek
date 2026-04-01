# Benchmark Summary

TriSeek was implemented as a Rust workspace with a persistent trigram content index, a delta overlay, a path/filename index, a query planner, a verification engine, a CLI, and a reproducible benchmark harness. The final timed benchmark artifacts are under `bench/results/final-run/`, while repo clones and indexes are cached outside the Git worktree under `/Users/trivedi/Documents/Projects/TriSeek-bench/`.

## Environment

- Host: `Sagars-MacBook-Pro.local`
- OS: `macOS 26.2`
- Architecture: `x86_64`
- Logical cores: `16`
- Rust toolchain installed for this task: `rustup`, `cargo`, and `rustc`
- Benchmark artifacts: `bench/results/final-run/report.json`, `bench/results/final-run/report.csv`, `bench/results/final-run/summary.md`, and `bench/results/final-run/correctness-revalidation.md`

## Dataset

Measured candidate repositories:

| Repo | Commit | Searchable Files | Searchable Bytes | Final Category | Timed in final run |
|---|---|---:|---:|---|---|
| serde-rs/serde | `fa7da4a93567` | 339 | 1,326,149 | small | yes |
| BurntSushi/ripgrep | `4519153` | 208 | 3,109,685 | small | no |
| kubernetes/kubernetes | `c6a95ffd4c78` | 28,132 | 257,586,895 | medium | yes |
| torvalds/linux | `cbfffcca2bf0` | 92,916 | 1,567,893,871 | large | yes |
| rust-lang/rust | `584d32e3ee7a` | 58,472 | 197,699,311 | large | no |

Notes:

- No cloned candidate crossed the handoff plan's `very_large` threshold of more than 500,000 searchable files or more than 20 GB of searchable text.
- `torvalds/linux` and `rust-lang/rust` were both intended `very_large` candidates, but both measured as `large` under the plan's thresholds.
- The final timed run focused on one repo per covered size band: `small`, `medium`, and `large`.

## Methodology

- Repos were cloned once into the external cache root and then reused across runs.
- Single-query benchmarks used 5 cold iterations and 10 warm iterations.
- Session benchmarks were intentionally reduced to keep runtime tractable after the harness stabilized:
  - `session_20`: 1 cold and 2 warm iterations
  - `session_100`: 1 cold and 1 warm iteration
- Baselines:
  - content search: `rg`
  - suffix/exact-name path search: `fd`
  - path substring/listing: `rg --files` or `rg --files | rg`
- Workloads covered:
  - file listing and path filtering
  - selective, moderate, high-match, and no-match literal search
  - anchored, weak-selectivity, and no-match regex search
  - multi-pattern OR
  - path-plus-content narrowing
  - repeated-session sequences
- The raw `report.json` retains 13 stale correctness failures from a harness bug involving leading `./` path prefixes. Those cases were rerun on the final build, and all 13 passed; see `bench/results/final-run/correctness-revalidation.md`.

## Results

High-level outcome:

- TriSeek lost 38 of 39 timed single-query cases on p50 latency.
- The only p50 win was `regex_no_match` on `kubernetes/kubernetes`, where TriSeek ran at 442.794 ms versus 474.095 ms for `rg`.
- TriSeek also lost every repeated-session benchmark. `session_20` regressed by 1.16x to 1.93x, and `session_100` regressed by 5.24x to 8.60x.

Per-repo summary:

| Repo | Median p50 Ratio (TriSeek / baseline) | `session_20` Ratio | `session_100` Ratio | Build Time | Update Time | Index Size |
|---|---:|---:|---:|---:|---:|---:|
| serde-rs/serde | 1.35x | 1.69x | 8.32x | 74 ms | 34.135 ms | 386,942 B |
| kubernetes/kubernetes | 2.35x | 1.93x | 8.60x | 13.0 s | 2.279 s | 91,256,489 B |
| torvalds/linux | 2.66x | 1.16x | 5.24x | 68.3 s | 9.939 s | 491,236,884 B |

CPU and memory observations:

- CPU time was not the main bottleneck on the larger repos. Median total CPU time was roughly on par with baseline on `torvalds/linux` and slightly lower on `kubernetes/kubernetes`, but wall-clock latency was still worse because the indexed path spent too much time on candidate handling and verification.
- The raw `/usr/bin/time -l` RSS field in the report behaved like byte-scale values on this machine. Interpreted that way, TriSeek peaked around 816 MiB on `kubernetes/kubernetes` and 3.1 GiB on `torvalds/linux`, while the baseline tools stayed near 16 MiB on the same runs.

## Interpretation

The current implementation is functionally usable, but it does not meet the handoff goal of beating repeated `rg` invocations on medium and larger repositories. The path index is especially weak: every path workload was materially slower than the shell-tool baseline even after correctness was fixed. Repeated-session performance also failed to amortize the index build/update cost, which means the adaptive runtime policy should not switch to indexed search by default.

## Recommendation

- Default content search: keep `rg`
- Default file/path discovery: keep `fd` for suffix/exact-name lookups and `rg --files`-based paths for substring/listing flows
- Indexed engine: keep as an opt-in prototype for further optimization, not as a production default
- Very large repos: no activation threshold should be claimed until a real `very_large` corpus is measured
