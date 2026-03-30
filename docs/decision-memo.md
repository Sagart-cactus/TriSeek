# Decision Memo (Updated)

## Status: Indexed engine now recommended for Medium and Large repos

After two rounds of optimization, TriSeek's indexed engine now significantly outperforms shell-based tools (rg, fd) on medium and large repositories for both single-query and session workloads.

## Inputs

- Round 2 benchmark: `bench/results/round2-fastindex/summary.md`
- Optimization log: `docs/optimization-log.md`
- Previous decision: the original memo recommended against integration; this update reverses that for medium+ repos.

## Recommendation Table (Updated)

| Repo Category | Cold Start Winner | Repeated Session Winner | Recommended Default |
|---|---|---|---|
| Small (<5K files) | `rg` / shell tools (TriSeek ~1.5x slower) | TriSeek (2.4x faster at 20 queries) | `rg` for cold start; indexed if session detected |
| Medium (5K-50K) | **TriSeek** (7-10x faster on content, 2x on paths) | **TriSeek** (9.4x faster at 20 queries) | **Indexed** (build index eagerly) |
| Large (50K-500K) | **TriSeek** (10-14x faster on content) | **TriSeek** (16.3x faster at 20 queries) | **Indexed** (build index eagerly) |
| Very Large | not benchmarked | not benchmarked | build index in background, use rg until ready |

## Key Results

### Single-Query Performance
- **Won 30 of 39 single-query cases** (was 1/39 before optimization)
- Kubernetes: 7.5x to 10x faster than rg on content search
- Linux: 10x to 14x faster than rg on content search
- Path queries: competitive or faster on medium/large repos

### Session Performance (Agent Workloads)
- kubernetes 20-query session: **9.4x faster** than rg
- linux 20-query session: **16.3x faster** than rg
- Every session benchmark won except serde session_100

### Remaining Weaknesses
1. **Small repos**: rg is faster for cold single queries (but TriSeek wins sessions)
2. **regex_weak patterns**: No extractable literals → falls back to full scan (6-10x slower)
3. **Build time**: 13s (medium), 75s (large) — needs background build strategy
4. **Index size**: Fast index is ~560MB for linux (larger than source) — acceptable for dev machines

## Updated Decision

- **Medium+ repos**: Activate indexed search as default after first build
- **Small repos**: Keep rg for cold start, switch to indexed after 2-3 queries in a session
- **Build strategy**: Build index in background on first use, serve from rg until ready
- **Incremental updates**: 40ms-8s depending on repo size — run before each session
- **regex_weak**: Keep rg fallback for patterns with no extractable literals

## What Changed

The two critical optimizations that flipped the performance picture:
1. **Parallel verification with rayon**: Candidate files are read and matched in parallel across all CPU cores. This eliminated the sequential I/O bottleneck.
2. **Fast binary index format**: Custom mmap-friendly format replaces bincode deserialization. Index opens in <5ms (was 200-800ms) with near-zero heap allocation. Posting lists are read directly from mapped memory.
