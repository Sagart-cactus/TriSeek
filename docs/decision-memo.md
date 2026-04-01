# Decision Memo (Updated)

## Status: Use `triseek --engine auto` by default on Medium and Large repos

This memo uses the fresh full rerun in `bench/results/rerun-2026-04-02-all/`. The older `bench/results/final-run/` artifacts remain useful as a pre-optimization baseline only; they are not the current recommendation source.

## Inputs

- Current full rerun: `bench/results/rerun-2026-04-02-all/summary.md`
- Current correctness note: `bench/results/rerun-2026-04-02-all/correctness-revalidation.md`
- Optimization log: `docs/optimization-log.md`
- Historical baseline: `bench/results/final-run/summary.md`

## Recommendation Table (Updated)

| Repo Category | Cold Start Winner | Repeated Session Winner | Recommended Default |
|---|---|---|---|
| Small (<5K files) | `rg` / `fd` | Mixed: TriSeek wins `session_20`, loses `session_100` | Keep shell tools for cold start; use TriSeek once the index is warm or a real session is underway |
| Medium (5K-50K) | **TriSeek auto** (13 / 13 wins in rerun) | **TriSeek auto** (2 / 2 wins) | **`triseek --engine auto`** |
| Large (50K-500K) | **TriSeek auto** (25 / 26 wins across Linux + Rust) | **TriSeek auto** (4 / 4 wins) | **`triseek --engine auto`** |
| Very Large | not benchmarked | not benchmarked | build index in background, use rg until ready |

## Key Results

### Single-Query Performance
- **Won 38 of 65 single-query cases overall** in the fresh full rerun
- **Won 38 of 39 medium+ single-query cases**
- `kubernetes` won all 13 single-query workloads
- `torvalds/linux` won all 13 single-query workloads
- `rust-lang/rust` won 12 of 13; the only non-win was an effective tie on `path_suffix` at `74.309 ms` vs `74.308 ms`

### Session Performance (Agent Workloads)
- **Won 8 of 10 session benchmarks overall**
- **Won 6 of 6 session benchmarks on medium+ repos**
- kubernetes 20-query session: **12.2x faster** than rg
- linux 20-query session: **16.9x faster** than rg
- rust 20-query session: **15.9x faster** than rg

### Remaining Weaknesses
1. **Small repos**: shell tools still win all 26 cold single-query workloads across `serde` and `ripgrep`
2. **Small long sessions**: both small repos still lose `session_100`
3. **One correctness caveat remains**: `kubernetes / regex_weak` disagrees with `rg` on a vendored protobuf file that TriSeek treats as binary-like
4. **Build time is still real**: `13.971 s` on Kubernetes and `150.390 s` on Linux
5. **Index size is still large**: `1.05 GB` on Linux

## Updated Decision

- **Medium+ repos**: Default to `triseek --engine auto` after the first build
- **Small repos**: Keep `rg` / `fd` for cold start, switch to TriSeek after a session is clearly underway
- **Build strategy**: Build index in background on first use, serve from rg until ready
- **Incremental updates**: `35 ms` to `10.1 s` depending on repo size; run before longer sessions
- **Very large repos**: Keep fallback behavior until a real `very_large` corpus is measured

## What Changed

The performance picture flipped because the optimized engine was rerun on the full 5-repo set and reproduced the medium+ win story. The critical changes were:
1. **Parallel verification with rayon**: candidate files are read and matched across cores instead of serially.
2. **Fast binary index format**: the mmap-friendly `fast.idx` path removed the old deserialization bottleneck and made query startup cheap enough for medium and large repos.
3. **Summary-only fast path and correctness hardening**: the final cleanup pass preserved the performance gains while fixing earlier result-comparison issues.
