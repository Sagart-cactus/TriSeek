# TriSeek Optimization Log

## Baseline (Pre-Optimization)
- TriSeek lost 38/39 single-query cases on p50 latency (1.35x–2.66x slower than rg)
- Lost every session benchmark (1.16x–8.60x slower)
- Peak RSS: 816 MiB (kubernetes), 3.1 GiB (linux) vs 16 MiB for rg
- Build time: 74ms (small), 13s (medium), 68.3s (large)

---

## Round 1: Parallel Verification + Sorted Merge + mmap reads + Regex Extraction
**Changes:**
- Added rayon for parallel candidate file verification in `collect_content_hits`
- Replaced HashSet-based posting list intersection with sorted vec merge (O(n+m))
- Use mmap directly for candidate reading instead of copying to Vec
- Parallel file walking during index build using ignore crate's parallel walker
- Early termination with AtomicBool/AtomicUsize for max_results
- Improved regex literal extraction: handles `|`, `?`, `*`, `{n}`, groups, alternation

**Benchmark Results (Round 1):**

### Session Benchmarks — MAJOR IMPROVEMENT
| Repo | TriSeek p50 ms | Baseline p50 ms | Ratio | vs Pre-Optimization |
|---|---:|---:|---|---|
| serde session_20 | 44.7 | 83.8 | **1.87x FASTER** | was 1.69x slower |
| serde session_100 | 152.9 | 95.0 | 1.61x slower | was 8.32x slower (5.2x improvement) |
| kubernetes session_20 | 984.5 | 1967.8 | **2.0x FASTER** | was 1.93x slower |
| kubernetes session_100 | 1434.5 | 2673.1 | **1.86x FASTER** | was 8.60x slower |
| linux session_20 | 4326.1 | 7557.2 | **1.75x FASTER** | was 1.16x slower |
| linux session_100 | 5719.0 | 7516.6 | **1.31x FASTER** | was 5.24x slower |

### Single-Query — Still Behind (index loading dominates)
| Repo | Median Ratio | vs Pre-Optimization |
|---|---|---|
| serde | ~1.6x slower | was 1.35x slower |
| kubernetes | ~2.2x slower | was 2.35x slower |
| linux | ~2.7x slower | was 2.66x slower |

**Key Takeaway:** Parallel verification fixed session perf, but index loading is the single-query bottleneck.

---

## Round 2: Fast Binary Index Format with mmap
**Changes:**
- Designed new flat binary index format (`fast.idx`) with fixed-size header and sections
- Trigram lookup tables stored as sorted arrays with (trigram, offset, count) entries
- Posting lists stored as raw u32 arrays, readable directly from mmap
- Doc metadata stored as fixed-size records with string pool
- Index opened via mmap — no deserialization, no heap allocation for postings
- Trigram HashMap built on open from the table (fast — just iterating compact entries)
- BufWriter for faster index writing

**Benchmark Results (Round 2):**

### Single-Query — MASSIVE TURNAROUND (Won 30 of 39 cases!)

**Kubernetes (Medium) — TriSeek dominates:**
| Query | TriSeek p50 ms | Baseline p50 ms | Ratio |
|---|---:|---:|---|
| path_all | 39.0 | 78.0 | **2.0x FASTER** |
| path_substring | 37.3 | 91.4 | **2.5x FASTER** |
| literal_selective | 42.3 | 371.3 | **8.8x FASTER** |
| literal_moderate | 45.5 | 386.4 | **8.5x FASTER** |
| literal_high | 52.0 | 392.2 | **7.5x FASTER** |
| regex_anchor | 51.4 | 383.0 | **7.5x FASTER** |
| literal_no_match | 38.3 | 387.3 | **10.1x FASTER** |
| regex_no_match | 40.9 | 387.6 | **9.5x FASTER** |
| multi_or | 49.1 | 414.5 | **8.4x FASTER** |
| path_plus_content | 41.9 | 222.3 | **5.3x FASTER** |
| regex_weak | 6075.3 | 905.8 | 6.7x slower (expected — falls back to full scan) |

**Linux (Large) — TriSeek dominates even harder:**
| Query | TriSeek p50 ms | Baseline p50 ms | Ratio |
|---|---:|---:|---|
| literal_selective | 120.4 | 1632.9 | **13.6x FASTER** |
| literal_moderate | 123.9 | 1560.2 | **12.6x FASTER** |
| literal_high | 163.7 | 1624.7 | **9.9x FASTER** |
| regex_anchor | 131.2 | 1650.4 | **12.6x FASTER** |
| literal_no_match | 115.8 | 1557.4 | **13.4x FASTER** |
| regex_no_match | 122.6 | 1659.4 | **13.5x FASTER** |
| multi_or | 161.3 | 1708.3 | **10.6x FASTER** |
| path_plus_content | 121.7 | 560.9 | **4.6x FASTER** |
| path_all | 124.0 | 118.3 | ~tied |
| path_substring | 111.4 | 131.5 | **1.2x FASTER** |
| regex_weak | 31584.9 | 3076.1 | 10.3x slower (falls back to full scan) |

### Session Benchmarks — Even Better
| Repo | TriSeek p50 ms | Baseline p50 ms | Ratio |
|---|---:|---:|---|
| serde session_20 | 38.2 | 92.0 | **2.4x FASTER** |
| kubernetes session_20 | 223.0 | 2086.3 | **9.4x FASTER** |
| kubernetes session_100 | 879.9 | 2097.9 | **2.4x FASTER** |
| linux session_20 | 523.5 | 8530.6 | **16.3x FASTER** |
| linux session_100 | 2087.4 | 8618.9 | **4.1x FASTER** |

### Summary
| Metric | Before Optimization | After Round 2 |
|---|---|---|
| Single-query wins | 1/39 | **30/39** |
| Session wins | 0/6 | **5/6** |
| Best speedup | 1.0x (one tie) | **16.3x** (linux session_20) |
| Worst case | 8.60x slower | regex_weak still 6-10x slower (expected) |

### Remaining Issues
1. **Serde (small repo)**: TriSeek still ~1.5-2x slower — for repos <500 files, rg is optimal since it fits in page cache
2. **regex_weak**: Patterns with no extractable literals fall back to full file scan (slower than rg's optimized SIMD scanner)
3. **Correctness**: 3 cases marked "no" on kubernetes — need investigation (likely minor result-count differences)

---
