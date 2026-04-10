# TriSeek Regression Test Plan

## Goal

Validate that TriSeek works as a standalone local code-search CLI, not just as an MCP helper, across:

- install and upgrade flows
- standalone CLI usage
- index lifecycle
- centralized storage under `~/.triseek`
- single global daemon behavior
- MCP server behavior
- search correctness and routing
- path normalization and UX
- frecency, stats, and session flows
- failure handling and compatibility

This plan is intentionally deeper than the current unit/integration suite. It is designed to catch regressions in behavior, UX, storage layout, and process model.

## Release Gates

A candidate is acceptable only if all of the following pass:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- CLI smoke suite
- daemon smoke suite
- MCP smoke suite
- centralized storage validation
- cross-platform install smoke tests

## Test Matrix

### Platforms

- macOS arm64
- macOS x86_64
- Linux x86_64
- Windows x86_64

### Install Paths

- prebuilt installer path
- `cargo install` path
- local checkout `cargo run -p triseek -- ...`

### Repository Shapes

- small repo: under 100 files
- medium repo: a few thousand files
- large repo: tens of thousands of files
- non-git directory
- repo with hidden directories and ignored content
- repo with binary files
- repo with symlinks

### Invocation Styles

- bare search: `triseek "needle" .`
- search alias: `triseek search "needle" .`
- explicit path kinds and filters
- daemon-backed search
- no-daemon local search
- MCP stdio search

## Fixtures

Prepare a reusable set of fixtures:

1. `fixture-small`
- a tiny Rust repo with `src/`, `README.md`, `.gitignore`, `.github/workflows/ci.yml`, one binary file, and one ignored file

2. `fixture-medium`
- several hundred to a few thousand text files across nested directories

3. `fixture-large`
- a real-world checkout used only for smoke/perf sanity, not exact output assertions

4. `fixture-non-git`
- plain directory tree without `.git`

5. `fixture-path-edge`
- nested directories to validate `.`, `..`, relative, absolute, symlink, and path normalization behavior

6. `fixture-mcp`
- small deterministic fixture for MCP output assertions

## Suite 1: Install and Binary Health

### 1.1 Local build health

- Run `cargo build`
- Run `cargo test`
- Run `cargo run -p triseek -- help`
- Run `cargo run -p triseek -- build --help`
- Run `cargo run -p triseek -- update --help`
- Run `cargo run -p triseek -- search --help`
- Run `cargo run -p triseek -- measure --help`
- Run `cargo run -p triseek -- daemon --help`
- Run `cargo run -p triseek -- mcp serve --help`
- Run `cargo run -p triseek -- doctor`

Expected:

- all commands succeed
- `doctor` prints binary/config/index diagnostics without crashing

### 1.2 Installer smoke: macOS/Linux shell script

- Install into a temp prefix
- Verify `triseek` and `triseek-server` are present
- Verify `triseek help` works
- Verify repeated install upgrades cleanly

Expected:

- binaries are replaced in place
- PATH guidance is correct

### 1.3 Installer smoke: Windows PowerShell

- Install into temp or default user location
- Verify binaries exist and execute
- Verify reinstall is idempotent

### 1.4 Cargo install smoke

- `cargo install --path crates/search-cli --locked`
- `cargo install --path crates/search-server --locked`
- Verify both binaries execute

## Suite 2: Standalone CLI UX

### 2.1 Bare search default command

Run:

```sh
triseek "needle" .
triseek "needle" ..
triseek "needle" ./subdir
triseek "needle" /absolute/path
```

Expected:

- all forms parse correctly
- no `search` keyword required
- results correspond to the exact root passed

### 2.2 Compatibility alias

Run:

```sh
triseek search "needle" .
```

Expected:

- output is equivalent to bare search for the same root and options

### 2.3 Reserved-word handling

Run:

```sh
triseek build .
triseek search "build" .
triseek -- "build" .
```

Expected:

- `triseek build .` runs the subcommand
- searching for `build` still works via alias or `--`

### 2.4 Omitted path behavior

Run from inside fixture root:

```sh
triseek "needle"
triseek build
triseek update
```

Expected:

- all default to current working directory

### 2.5 JSON and summary output contracts

Run:

```sh
triseek search --json "needle" .
triseek "needle" . --summary-only
triseek build .
triseek update .
triseek measure .
triseek session . --query-file queries.json --json
```

Expected:

- JSON output is valid and machine-parseable for commands that emit JSON
- summary output stays bounded and human-readable
- key fields are stable enough for scripts and tests

## Suite 3: Path Resolution and Normalization

### 3.1 Equivalent paths map to same index

For the same target directory, run:

```sh
triseek build .
triseek build ./sub/..
triseek build /absolute/path/to/root
```

Expected:

- all resolve to the same `index_dir`
- no duplicate per-root indexes are created

### 3.2 Exact-root semantics

Run:

```sh
triseek build .
triseek build crates/search-cli/src
```

Expected:

- these create two different indexes
- the nested path is treated as a distinct search root

### 3.3 Invalid path handling

Run against:

- nonexistent path
- regular file path
- unreadable path if supported by environment

Expected:

- clear errors
- no partial index directories created

### 3.4 Symlink behavior

Create a symlink to a fixture root and run build/search through both real path and symlink.

Expected:

- canonical path resolution deduplicates to one index

## Suite 4: Centralized Storage

### 4.1 Default layout

After `triseek build <root>`, verify:

- `~/.triseek/indexes/<root-key>/base.bin`
- `~/.triseek/indexes/<root-key>/fast.idx`
- `~/.triseek/indexes/<root-key>/metadata.json`
- optional `delta.bin`
- `frecency.json` appears after search/frecency events

Expected:

- no `.triseek-index` directory is created in the repo root

### 4.2 Multiple-root isolation

Build two different roots.

Expected:

- each gets a separate `<root-key>` directory
- searches and updates do not cross-contaminate

### 4.3 `--index-dir` override

Run build/search/update with explicit `--index-dir`.

Expected:

- explicit override is honored
- default home layout is bypassed only for that invocation

## Suite 5: Index Lifecycle

### 5.1 Full build

Run:

```sh
triseek build .
```

Expected:

- index directory is created
- `metadata.json` reports indexed file counts and timestamps

### 5.2 Incremental update

- build initial index
- modify, add, and delete files
- run `triseek update .`

Expected:

- update succeeds
- changed content becomes searchable
- removed files disappear from results

### 5.3 Missing-index update failure semantics

Run `triseek update .` on an unindexed root.

Expected:

- current expected behavior should be explicit and documented
- either fail clearly or bootstrap, but behavior must be stable and tested

### 5.4 Repeated builds are stable

- run `triseek build .` twice

Expected:

- second run succeeds cleanly
- index remains usable

## Suite 6: Measure Command

### 6.1 Basic repository scan

Run:

```sh
triseek measure .
triseek measure ./subdir
```

Expected:

- command succeeds for valid roots
- output reflects the exact passed root
- file counts and byte counts are plausible for the fixture

### 6.2 Hidden and binary options

Run:

```sh
triseek measure .
triseek measure . --include-hidden
triseek measure . --include-binary
```

Expected:

- hidden and binary toggles materially change counts when fixtures contain those files
- defaults remain consistent with build/search defaults

## Suite 7: Search Correctness

### 7.1 Literal search

Validate:

- exact token found
- no false positives
- case-sensitive and case-insensitive modes

### 7.2 Regex search

Validate:

- anchored regex
- alternation
- weak regex path that falls back appropriately
- invalid regex returns a clean error

### 7.3 Path search

Validate:

- `--kind path`
- path substring matching
- exact name and exact path filters

### 7.4 Filters

Validate:

- `--path-substring`
- `--path-prefix`
- `--exact-path`
- `--exact-name`
- `--ext`
- `--glob`
- combinations of these filters

### 7.5 Result limits

Validate:

- `--max-results`
- `--summary-only`
- bounded output on files with many matches

### 7.6 Hidden and binary behavior

Validate:

- hidden directories excluded by default
- hidden directories included with `--include-hidden`
- binary files excluded by default
- binary files included with `--include-binary`
- ignored files follow ignore rules during full scan paths

### 7.7 Engine selection

Run with:

- `--engine auto`
- `--engine index`
- `--engine scan`
- `--engine rg`

Expected:

- explicit engine overrides are respected
- auto-routing is stable and explainable

## Suite 8: Daemon

### 8.1 Global daemon lifecycle

Run:

```sh
triseek daemon start
triseek daemon status
triseek daemon stop
```

Expected:

- one global daemon starts
- `daemon.pid` and `daemon.port` live under `~/.triseek/daemon/`

### 8.2 Per-root service inside one daemon

- build two roots
- run searches against both while one daemon is running
- query `triseek daemon status <root>`

Expected:

- one daemon process serves both roots
- status reports the requested root correctly

### 8.3 Search forwarding

- build a root
- start daemon
- run `triseek "needle" . --summary-only`

Expected:

- routed search returns indexed results
- no local-fallback-only regression when daemon is available

### 8.4 Watcher/update behavior

- start daemon for a built root
- edit files
- search again

Expected:

- updated content becomes searchable without restarting daemon

### 8.5 Watcher behavior for hidden paths

- start daemon for a built root that excludes hidden content by default
- modify a file under `.github/` or another hidden directory
- search for the new content without `--include-hidden`
- repeat with `--include-hidden` if supported by the route

Expected:

- hidden-path changes do not leak into default search results
- behavior matches full-build hidden file policy

### 8.6 Missing-index behavior under daemon

- run daemon for unindexed root
- search the root

Expected:

- behavior is defined and stable
- no daemon crash

## Suite 9: MCP Server

### 9.1 Handshake

- spawn `triseek mcp serve --repo <root>`
- run `initialize`
- send `notifications/initialized`
- send `ping`
- send `shutdown`

Expected:

- protocol completes cleanly over stdio

### 9.2 Tool list

Validate presence of:

- `find_files`
- `search_content`
- `search_path_and_content`
- `index_status`
- `reindex`

### 9.3 Tool correctness

Validate:

- `find_files`
- `search_content` literal mode
- `search_content` regex mode
- `search_path_and_content`
- `index_status`
- `reindex` incremental and full

### 9.4 MCP error discipline

Validate:

- malformed request returns JSON-RPC parse error
- invalid tool args return structured tool error
- missing index returns documented structured behavior

### 9.5 Output limits

Validate:

- default limit
- hard cap
- truncation
- line preview length
- dedupe behavior

## Suite 10: Frecency

### 10.1 File creation and persistence

- search a root
- verify `frecency.json` exists
- restart CLI/daemon
- verify frecency loads without corruption

### 10.2 Reranking

- perform repeated searches and explicit selections
- verify boosted files rise in ranking

### 10.3 Manual select path

Run:

```sh
triseek frecency-select . --path src/main.rs --path src/lib.rs
```

Expected:

- data is recorded
- subsequent search order reflects the boost

## Suite 11: Session and Stats Commands

### 11.1 Session command

- run `triseek session <root> --query-file <json> --json`

Expected:

- aggregate metrics are produced
- result count and engine counts are coherent

### 11.2 Stats command

- run `triseek stats --index-dir <resolved-index-dir>`

Expected:

- metadata prints successfully
- numbers match the built index

### 11.3 Session summary output

- run `triseek session <root> --query-file <json> --summary-only`

Expected:

- summary output is stable and bounded
- aggregate counters match the JSON form for the same query file

## Suite 12: Install/Uninstall Integrations

### 12.1 `triseek install claude-code`

Validate:

- project scope
- local scope
- uninstall path
- unrelated config preservation

### 12.2 `triseek install codex`

Validate:

- insert/update/remove behavior in `~/.codex/config.toml`
- unrelated config preservation

### 12.3 `triseek doctor`

Validate:

- binary path
- repo-root detection for MCP flow
- config path checks
- centralized index path reporting

## Suite 13: Backward Compatibility

### 13.1 Legacy `search` alias

- `triseek search "needle" .`

Expected:

- still works

### 13.2 Hidden compatibility flags

If hidden compatibility flags remain, validate they still function and do not break normal usage:

- `--repo`
- `--index-dir`

## Suite 14: Performance Smoke

This is not a benchmark gate. It is a regression smoke check.

Validate:

- cold search with no index on small root
- indexed repeated search on medium root
- daemon-backed repeated search on medium root
- large-root search does not regress catastrophically relative to prior release

Suggested checks:

- compare first search vs repeated search latency
- compare daemon-backed repeated latency vs local indexed latency
- compare output correctness against `rg` for a fixed query set

## Suite 15: Security and Robustness

### 15.1 Exit codes

Validate:

- success paths return zero
- invalid CLI args return non-zero
- invalid regex returns non-zero
- malformed MCP requests do not accidentally return success

### 15.2 No unexpected writes

Validate:

- no writes into searched repo root by default
- no writes into unrelated working directories when searching another root
- `TRISEEK_HOME` override confines all writes to the override path

### 15.3 Protocol and process robustness

Validate:

- no stdout contamination in MCP mode
- daemon handles malformed RPC without crashing
- concurrent searches do not corrupt index/frecency state
- interrupted daemon stop/start does not leave unrecoverable state
- stale `daemon.pid` or `daemon.port` files are recovered cleanly

## Automation Plan

### Automated in CI

- formatting, clippy, tests
- CLI parser/path-resolution unit tests
- index storage path tests
- MCP integration tests
- daemon smoke tests on macOS/Linux/Windows

### Automated nightly or release-candidate only

- installer smoke tests
- multi-root daemon tests
- larger fixture correctness comparisons against `rg`
- performance smoke on medium and large roots

### Manual exploratory before release

- install/uninstall on real machines
- MCP use with real Claude Code
- MCP use with real Codex
- long-running daemon session with multiple roots

## Suggested Execution Order

1. local build/test gates
2. CLI parser and path-resolution checks
3. index lifecycle and centralized storage checks
4. search correctness matrix
5. daemon lifecycle and multi-root checks
6. MCP checks
7. frecency/session/stats checks
8. installer and doctor checks
9. performance smoke

## Exit Criteria

TriSeek is ready for release only when:

- all automated gates pass
- no repo-root writes occur in default operation
- one daemon can serve multiple roots reliably
- standalone CLI UX is stable
- search results match expected correctness baselines
- MCP behavior remains bounded and protocol-safe
