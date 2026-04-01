# Benchmark Summary

The current source of truth is the fresh full rerun under `bench/results/rerun-2026-04-02-all/`. The older `bench/results/final-run/` directory is a historical pre-optimization baseline that predates the mmap / parallel-verification work and is kept only for before/after comparison.

## Environment

- Host: `Sagars-MacBook-Pro.local`
- OS: `macOS 26.2`
- Architecture: `x86_64`
- Logical cores: `16`
- Rust toolchain installed for this task: `rustup`, `cargo`, and `rustc`
- Current artifacts: `bench/results/rerun-2026-04-02-all/report.json`, `bench/results/rerun-2026-04-02-all/report.csv`, `bench/results/rerun-2026-04-02-all/summary.md`, and `bench/results/rerun-2026-04-02-all/correctness-revalidation.md`
- Historical baseline: `bench/results/final-run/*`

## Dataset

All 5 `ready` repositories from `bench/manifest/repositories.yaml` were timed in the fresh rerun:

| Repo | Commit | Searchable Files | Searchable Bytes | Measured Category |
|---|---|---:|---:|---|
| serde-rs/serde | `fa7da4a93567` | 339 | 1,326,149 | small |
| BurntSushi/ripgrep | `4519153e5e46` | 207 | 3,109,499 | small |
| kubernetes/kubernetes | `c6a95ffd4c78` | 28,131 | 254,117,420 | medium |
| torvalds/linux | `cbfffcca2bf0` | 92,916 | 1,567,893,871 | large |
| rust-lang/rust | `584d32e3ee7a` | 58,472 | 197,699,311 | large |

Notes:

- No cloned candidate crossed the handoff plan's `very_large` threshold of more than 500,000 searchable files or more than 20 GB of searchable text.
- `torvalds/linux` and `rust-lang/rust` were both intended `very_large` candidates, but both measured as `large` under the plan's thresholds.

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
- The TriSeek side used `triseek search --engine auto` and `triseek session --engine auto`; the `indexed_*` column names in the CSV are historical harness names.
- The raw rerun report records one remaining mismatch on `kubernetes_kubernetes / regex_weak`; see `bench/results/rerun-2026-04-02-all/correctness-revalidation.md`.

## Results

High-level outcome:

- TriSeek won `38 / 65` single-query workloads across the full 5-repo rerun.
- TriSeek won `38 / 39` single-query workloads on medium+ repos (`kubernetes`, `linux`, `rust`).
- TriSeek won `8 / 10` repeated-session benchmarks overall, and `6 / 6` on medium+ repos.
- All `26` single-query losses came from the two small repos (`serde`, `ripgrep`), which still favor shell tools for cold-start latency.
- Peak measured speedup was `20.1x` on `torvalds/linux literal_selective`: `197.129 ms` versus `3971.990 ms`.

Per-repo summary:

| Repo | Single-query wins | `session_20` | `session_100` | Build Time | Update Time | Index Size |
|---|---:|---|---|---:|---:|---:|
| serde-rs/serde | 0 / 13 | `34.172 ms` vs `82.135 ms` (2.4x faster) | `131.653 ms` vs `86.027 ms` (1.53x slower) | `78 ms` | `35.940 ms` | `1.24 MB` |
| BurntSushi/ripgrep | 0 / 13 | `39.426 ms` vs `92.137 ms` (2.3x faster) | `155.990 ms` vs `101.079 ms` (1.54x slower) | `166 ms` | `36.586 ms` | `2.19 MB` |
| kubernetes/kubernetes | 13 / 13 | `292.270 ms` vs `3575.138 ms` (12.2x faster) | `1255.185 ms` vs `3839.544 ms` (3.1x faster) | `13.971 s` | `4.018 s` | `213.5 MB` |
| torvalds/linux | 13 / 13 | `612.810 ms` vs `10342.155 ms` (16.9x faster) | `2468.112 ms` vs `10408.510 ms` (4.2x faster) | `150.390 s` | `10.061 s` | `1.05 GB` |
| rust-lang/rust | 12 / 13 | `374.870 ms` vs `5954.215 ms` (15.9x faster) | `1500.637 ms` vs `6062.653 ms` (4.0x faster) | `9.633 s` | `4.037 s` | `231.9 MB` |

Notable details:

- `kubernetes` won all 13 single-query workloads in the rerun.
- `torvalds/linux` won all 13 single-query workloads in the rerun.
- `rust-lang/rust` won 12 of 13 single-query workloads; the only non-win was `path_suffix`, effectively tied at `74.309 ms` versus `74.308 ms`.
- The rerun did not reproduce the extreme RSS spikes from the historical pre-optimization baseline. The large remaining cost on big repos is index build time, not query-time collapse.

## Interpretation

The current implementation now clears the original benchmark goal on medium and large repos when run in `auto` mode against shell-tool baselines. Query-time behavior is strong once the repo is large enough, but the build/update costs are still meaningful on the largest corpus. Small repos remain a bad fit for indexed cold-start queries, and the Kubernetes `regex_weak` protobuf mismatch should stay on the follow-up list.

## Recommendation

- Medium and large repos: use `triseek --engine auto` as the default search path once an index exists.
- Small repos: keep `rg` / `fd` for cold single queries; switch to TriSeek once a real session is underway or the index is already warm.
- Build strategy: build in the background on first use and refresh incrementally before longer sessions.
- Very large repos: do not claim an activation threshold until a real `very_large` corpus is benchmarked.
