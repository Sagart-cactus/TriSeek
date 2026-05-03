# Codex Handoff Plan: Implement and Benchmark a Faster Local Code Search Engine in Rust

## Goal

Replace or augment Codex's current shell-based file/code search path with a Rust implementation that is faster than repeated `rg` invocations for medium and larger repositories, while remaining no worse than `rg` for small repositories and cold starts.

The implementation must:
1. Build a **production-usable Rust search engine** for local code search.
2. Benchmark it against **the `rg` executable** on representative repositories.
3. Clone benchmark repositories **once**, cache them locally, and reuse them across runs.
4. Measure wall-clock time, CPU time, memory, index build/update cost, and index size.
5. Produce a final recommendation on when Codex should use:
   - the new indexed engine
   - direct `rg`
   - fallback filename search

---

## What to Build

### Recommended architecture

Implement a **hybrid indexed search engine** in Rust with the following design:

1. **Persistent trigram content index**
   - Build an inverted index over file contents using trigrams.
   - Store postings compactly, sorted by document ID.
   - Support memory-mapped read paths where practical.

2. **Incremental delta index**
   - Maintain a small mutable overlay for changed/untracked files.
   - Periodically merge into the main index.
   - Avoid full reindexing on every repository edit.

3. **File metadata / path index**
   - Separate index for path search and glob-like filtering.
   - Support file name, suffix, full path substring, and extension filtering.

4. **Regex/literal query planner**
   - Parse the incoming query and classify into:
     - literal
     - regex with extractable literals/trigrams
     - regex with weak literal selectivity
     - path/file-name search
   - Use trigram candidate pruning when possible.
   - Fall back to direct scan only when pruning is ineffective.

5. **Verification engine**
   - Verify candidate documents using Rust regex libraries.
   - Prefer `regex` / `regex-automata` for linear-time patterns.
   - Only support PCRE-like features if explicitly needed and benchmarked separately.

6. **Adaptive runtime policy**
   - For small repos or cold start: use `rg`.
   - For medium+ repos or repeated searches in the same session: use indexed search.
   - For unsupported/degenerate cases: fall back to `rg`.

### Why this architecture

This gives the best tradeoff of:
- practical engineering complexity
- speedup for repeated agent searches
- lower CPU use per query on medium/large repos
- acceptable memory overhead
- compatibility with existing regex-centric search workflows

---

## Non-Goals for V1

Do **not** build these first:
1. semantic/vector search as the primary search engine
2. AST-only search as the primary search engine
3. suffix-array / FM-index engine as the initial baseline replacement
4. distributed multi-host search
5. full PCRE compatibility at the cost of losing linear-time guarantees

These can be layered later, but they are not the best first implementation for replacing repeated local `rg` scans.

---

## Deliverables

Codex must produce all of the following:

1. **Rust workspace**
   - `search-core`
   - `search-index`
   - `search-cli`
   - `search-bench`
   - optional `search-ffi` if needed later

2. **Search tool**
   - CLI that can:
     - build index
     - update index
     - search literal
     - search regex
     - search paths/files
     - print JSON results

3. **Benchmark harness**
   - reproducible benchmark runner
   - clones repos once
   - records machine info
   - records repo stats
   - compares new engine vs `rg`

4. **Benchmark dataset manifest**
   - repo URLs
   - pinned commits/tags
   - exclusion rules
   - classification thresholds

5. **Benchmark results**
   - raw JSON/CSV
   - summary markdown
   - charts/tables
   - recommendation thresholds

6. **Decision memo**
   - whether to integrate
   - activation policy
   - expected gains by repo size

---

## Repository Selection Strategy

Do not hardcode category labels before measurement. Instead:

1. Clone candidate repositories once.
2. Measure:
   - tracked file count
   - searchable text bytes
   - total on-disk bytes
   - language mix
3. Assign repo category using thresholds below.

### Suggested candidate repositories

Use stable public repositories with broad language diversity and real-world structure.

Primary candidates:
- **Small** candidate: `sharkdp/fd` or `serde-rs/serde`
- **Medium** candidate: `BurntSushi/ripgrep` or `redis/redis`
- **Large** candidate: `kubernetes/kubernetes`
- **Very large** candidate: `torvalds/linux` or `rust-lang/rust`

### Final category thresholds

Classify each cloned repo after measurement:

- **Small**
  - < 5,000 searchable files OR < 200 MB searchable text
- **Medium**
  - 5,000–50,000 searchable files OR 200 MB–2 GB searchable text
- **Large**
  - 50,000–500,000 searchable files OR 2–20 GB searchable text
- **Very large**
  - > 500,000 searchable files OR > 20 GB searchable text

If a chosen candidate does not fit the expected band after measurement, keep it measured but choose another repo until all four bands are covered.

### Clone/cache policy

Create a cache root such as:

`bench/repos/<repo_slug>/`

Rules:
- clone each repo only once
- fetch updates only if explicitly requested
- pin each benchmark run to a commit SHA
- persist measured stats in `repo_manifest.json`

---

## Search Workloads to Benchmark

Each repo must be benchmarked using the same workload families.

### 1. File listing / file discovery
Examples:
- list all tracked searchable files
- find files by suffix: `*.rs`, `*.go`, `*.c`
- find file by exact name
- find file by substring in path

### 2. Selective literal search
Examples:
- rare API name
- config key
- uncommon struct/trait/class name
- rare error string

### 3. Moderate-frequency literal search
Examples:
- common symbol name
- repeated config word
- import/module/package token

### 4. High-match-count literal search
Examples:
- `error`
- `TODO`
- `test`
- `return`

### 5. Regex with strong literal anchors
Examples:
- `fn\s+new`
- `class\s+\w+Controller`
- `k8s\.io/[a-zA-Z0-9_/.-]+`

### 6. Regex with weak literal selectivity
Examples:
- `[A-Za-z_][A-Za-z0-9_]{10,}`
- `[A-Z][A-Za-z0-9]+Error`
- patterns with many possible matches

### 7. No-match searches
Examples:
- improbable literal
- improbable regex

### 8. Multi-pattern OR search
Examples:
- `panic|fatal|abort`
- `TODO|FIXME|XXX`

### 9. Path + content combined narrowing
Examples:
- search only `*.rs`
- search only under `cmd/`
- search only under `pkg/`

### 10. Repeated-session search sequence
Simulate an agent session:
- 20–100 searches in the same repo
- mix of literal, regex, path search
- some repeated queries
- some near-neighbor queries

This repeated-session benchmark is important because the indexed engine is expected to outperform repeated raw scans most clearly here.

---

## Benchmark Metrics

Record for every benchmark:

### Query-time metrics
- wall-clock latency (p50, p95, p99)
- user CPU time
- system CPU time
- max RSS / peak memory
- bytes read (if measurable)
- candidate files examined
- verified files examined
- matches returned

### Index metrics
- full index build time
- incremental update time
- index size on disk
- mmap footprint / resident memory estimate
- postings count
- docs indexed
- files skipped and why

### Session metrics
- total time for 20-query and 100-query sessions
- total CPU
- cumulative peak RSS
- amortized query cost including index build
- amortized query cost excluding index build

---

## Benchmark Methodology

### Baselines

Benchmark against:
1. `rg` executable
2. optional `git grep` for sanity
3. optional `fd`/`find` for path-only searches

### Cold vs warm runs

For each repo and workload:
- **Cold-ish run**: first query after process start / index open
- **Warm run**: repeated queries after caches are hot

Avoid fake precision for OS page cache flushing unless the environment truly supports it. Report cache state clearly.

### Measurement tools

Preferred:
- `hyperfine` for repeated command timing
- `/usr/bin/time -v` for CPU and max RSS
- internal structured metrics emitted by the Rust tool
- optional `perf stat` where available

### Repetition policy

Per benchmark case:
- minimum 10 warm iterations
- minimum 5 cold-ish iterations
- discard obvious outliers only if justified and documented
- always preserve raw data

### Correctness rule

The new engine must match `rg` result sets for supported query classes.
Where semantics differ by design, document the reason explicitly.

---

## Functional Requirements

### File filtering / ignore behavior
Match `rg` defaults as closely as practical:
- respect `.gitignore`
- skip binary files by default
- skip hidden files unless requested
- configurable include/exclude globs
- language/type filters if feasible

### Query support
Support in V1:
- literal substring search
- regex search using Rust regex semantics
- case-sensitive / insensitive modes
- path filters
- extension filters
- JSON output
- context lines optional but not mandatory for first benchmark phase

### Result verification
Every indexed candidate set must be verified against file contents before returning matches.

---

## Proposed Implementation Phases

### Phase 0: Baseline study
1. Confirm current Codex/local search path assumptions.
2. Build a thin benchmark harness for:
   - `rg --files`
   - `rg <pattern>`
   - filename search via `find` or `fd`
3. Clone candidate repos and measure them.
4. Produce initial baseline data.

**Exit criteria**
- all repos cloned once
- repo stats recorded
- baseline `rg` numbers collected

### Phase 1: Minimal index prototype
1. Implement file walker using `ignore` crate.
2. Build trigram extractor for searchable files.
3. Build document table and postings lists.
4. Add literal query support:
   - derive trigrams from query
   - intersect posting lists
   - verify files
5. Add path index.

**Exit criteria**
- indexed literal search works correctly
- measurable win over repeated `rg` on at least medium repos for selective queries

### Phase 2: Regex support
1. Parse regex and extract mandatory literals/trigrams where possible.
2. Use candidate pruning from extracted grams.
3. Verify using `regex` / `regex-automata`.
4. Add fallback direct scan when extraction quality is poor.

**Exit criteria**
- correctness parity with `rg` for supported regex classes
- measurable improvement on regex cases with usable literals/trigrams

### Phase 3: Incremental updates
1. Track file mtime/size/hash or Git object ID.
2. Update changed files into a delta index.
3. Merge delta into main index on threshold.

**Exit criteria**
- update cost is materially cheaper than rebuild
- repeated edit/search loops are fast

### Phase 4: Query planner and adaptive routing
1. Add repo-size aware routing.
2. Add query-shape aware routing.
3. Add session-aware routing:
   - small repo / one-off search -> `rg`
   - medium+ repeated searches -> indexed path

**Exit criteria**
- hybrid path beats naive always-rg and naive always-index on aggregate benchmark score

### Phase 5: Optimization pass
Focus only after correctness and benchmark harness are solid:
- postings compression
- SIMD-accelerated trigram extraction if justified
- mmap layout tuning
- arena reuse / allocation reduction
- parallel candidate verification
- better ranking heuristics

---

## Rust Crates to Consider

Use these only if they help and do not distort the benchmark.

Likely useful:
- `ignore` for recursive traversal and gitignore handling
- `walkdir` only if needed, but prefer `ignore`
- `regex`
- `regex-automata`
- `memmap2`
- `rayon` only if parallel search proves beneficial
- `serde`, `serde_json`
- `clap`
- `anyhow` / `thiserror`
- `criterion` or custom harness for microbenchmarks
- `jiff` or `time` for timestamps
- `ahash`/`hashbrown` if profiling justifies it

Avoid premature dependency bloat.

---

## Data Structures Guidance

Suggested core structures:

1. **Doc table**
   - doc_id -> file path, file size, hash, language/type flags

2. **Path index**
   - filename tokens
   - extension map
   - full-path trigram/substring support if needed

3. **Content trigram postings**
   - trigram -> sorted doc_id list
   - optionally document frequency for query planning

4. **Delta index**
   - small mutable HashMap / BTreeMap based overlay for changed docs

5. **Stats store**
   - repo stats
   - build metadata
   - query counters

Suggested storage layout:
- compact binary files
- one metadata file
- one doc table file
- one postings file
- one lexicon file
- one delta file
- all versioned

---

## Query Planning Rules

### Literal search
- length < 3: usually direct scan or specialized short-pattern mode
- length >= 3: derive trigrams, intersect postings, verify candidates

### Regex search
- if mandatory literals/trigrams exist: use index pruning
- if no useful grams exist: direct scan fallback
- if regex is path-oriented: use path index first

### Path search
- exact filename -> direct metadata lookup
- suffix/extension -> extension index
- substring/glob -> path index or direct path scan depending on selectivity

### High-match queries
If planner predicts huge result volume:
- use fast early signaling
- stream results
- measure separately because throughput dominates latency

---

## Acceptance Criteria

The implementation is a success only if all of the below are met:

1. **Correctness**
   - exact result parity with `rg` for supported semantics

2. **Small repos**
   - no meaningful regression vs `rg`
   - if indexed path is slower, adaptive routing must keep small repos on `rg`

3. **Medium repos**
   - clear win on repeated-session workloads
   - target: >= 2x median session speedup for selective queries

4. **Large repos**
   - strong win on selective literal/regex workloads
   - target: >= 5x median session speedup where query pruning is effective

5. **Very large repos**
   - indexed search should be the default if build/update overhead is amortized
   - target: interactive latency where repeated raw `rg` scans are no longer ideal

6. **Resource usage**
   - index size and RSS must be measured and reported, not assumed acceptable

7. **Operational quality**
   - robust CLI
   - reproducible benchmarks
   - clear fallback paths

---

## Failure Conditions

Stop and write a decision memo if any of these occur:
1. index build/update cost overwhelms query savings on medium repos
2. memory footprint is too high for realistic local Codex sessions
3. regex coverage gaps are too severe for expected use
4. repeated-session speedups are insignificant outside very large repos
5. correctness parity is hard to maintain

If that happens, recommend a narrower scope:
- file/path index only
- session query cache
- batched `rg` execution improvements
- internal `rg` crate embedding instead of full index engine

---

## Final Output Required from Codex

Codex must end with a concise final report containing:

1. **What was implemented**
2. **Which repos were used**
3. **Measured repo stats**
4. **Benchmark tables**
5. **Where it beats `rg`**
6. **Where it does not beat `rg`**
7. **CPU and memory tradeoffs**
8. **Recommended activation thresholds**
9. **Whether Codex should integrate it**
10. **Next improvements if adopted**

---

## Suggested Final Recommendation Format

Use a table like this in the final memo:

| Repo Category | Cold Start Winner | Repeated Session Winner | CPU Winner | Memory Winner | Recommended Default |
|---|---|---|---|---|---|
| Small | rg | rg | rg | rg | rg |
| Medium | rg or hybrid | indexed | indexed | depends | hybrid |
| Large | indexed or hybrid | indexed | indexed | depends | indexed |
| Very Large | indexed | indexed | indexed | depends | indexed |

---

## Implementation Notes for Codex

- Start with correctness and benchmark harness, not micro-optimizations.
- Do not assume a repo is small/medium/large from intuition; measure it.
- Do not assume indexing wins on cold start; prove it.
- Do not compare against `grep`; compare primarily against `rg`, because that is the relevant baseline.
- Keep raw benchmark artifacts.
- Make routing decisions from measured data, not theory alone.

