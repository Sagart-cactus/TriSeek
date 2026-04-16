# MCP Regression Checklist

Executed on 2026-04-08.

## Purpose

Validate that the MCP branch changes:

- keep existing `triseek search` behavior working
- keep MCP protocol/tool behavior stable
- keep installer/config edits safe
- handle no-index and bounded-output scenarios correctly

## Checklist

### 1. Baseline health

- [x] Run `cargo test -p triseek`
  - Result: passed `18` tests (`8` unit tests, `10` MCP integration tests).
- [x] Run `cargo run -p triseek -- doctor`
  - Result: binary, repo root, Claude CLI, Codex CLI, and local index all detected successfully.

### 2. Existing CLI regression checks

Precondition:

- [x] Refresh the local index with `cargo run -p triseek -- update .`
  - Result: rebuilt full index successfully; `delta_docs = 0`.

Query set executed against `McpState` and `server.rs`:

- [x] `cargo run -p triseek -- search --no-daemon --json --engine index McpState .`
  - Result: `engine=indexed`, `files_with_matches=2`, `total_line_matches=12`.
- [x] `cargo run -p triseek -- search --no-daemon --json --engine scan McpState .`
  - Result: `engine=direct_scan`, `files_with_matches=2`, `total_line_matches=12`.
- [x] `cargo run -p triseek -- search --no-daemon --json --engine rg McpState .`
  - Result: `engine=ripgrep`, `files_with_matches=2`, `total_line_matches=12`.
- [x] `cargo run -p triseek -- search --no-daemon --json --summary-only McpState .`
  - Result: `hits=[]` with `files_with_matches=2` and `total_line_matches=12`.
- [x] `cargo run -p triseek -- search --no-daemon --json --kind path server.rs .`
  - Result: `engine=direct_scan`, hits included `crates/search-cli/src/mcp/server.rs` and `crates/search-cli/tests/mcp_server.rs`.
- [x] `cargo run -p triseek -- search --no-daemon --json --path-substring mcp McpState .`
  - Result: `engine=direct_scan`, routing reason included `filter_adjustment=true`, hits limited to MCP paths.

### 3. MCP protocol and tool checks

- [x] Run `cargo test -p triseek --test mcp_server -- --nocapture`
  - Result: passed `10` end-to-end MCP tests.
- [x] Validate handshake and protocol flow
  - Result: `initialize`, `notifications/initialized`, `ping`, and `shutdown` all completed successfully in a live stdio probe.
- [x] Validate `search_content` regex mode and bounded output
  - Result: regex query returned `is_error=false`, respected `limit=5`, and set `truncated=true`.
- [x] Validate `reindex` full mode in a live stdio probe
  - Result: `completed=true`.
- [x] Validate no-index behavior in a live stdio probe
  - Result: `index_status` reported `index_present=false`; `search_content` and `find_files` still succeeded via fallback/direct-scan behavior.
- [x] Validate incremental reindex from a missing index
  - Result: `reindex incremental` now bootstraps the index successfully with `rebuilt_full=true`, followed by `index_present=true` and `index_fresh=true`.

### 4. Installer/config safety

- [x] Project-scope Claude config smoke test in a temp workspace
  - Command path: `triseek install claude-code --scope project`, then uninstall.
  - Result: `.mcp.json` gained and removed only the `triseek` entry; unrelated `other` server remained intact.
- [x] Codex config smoke test in a temp `HOME`
  - Command path: `triseek install codex`, then uninstall.
  - Result: `~/.codex/config.toml` gained and removed only `[mcp_servers.triseek]`; unrelated config remained intact.

### 5. Issues found during execution

- [x] MCP content result limiting
  - Issue found: a single file with many matches could exceed the requested `limit`.
  - Resolution: fixed in the MCP envelope mapper and covered by integration test `search_content_respects_limit_with_many_matches_in_one_file`.
- [x] Incremental reindex with no existing index
  - Issue found: `reindex` with `mode=incremental` returned an error when no index existed.
  - Resolution: fixed to bootstrap with a full build and covered by integration test `reindex_incremental_bootstraps_missing_index`.

### 6. Real client acceptance

- [x] Install TriSeek into a real Claude Code workspace and run `claude mcp list`.
  - Result: project-scoped `triseek-live-check` connected successfully in `claude mcp list`.
- [ ] Invoke successful `find_files` and `search_content` calls in Claude Code.
  - Blocked: `claude -p` failed before tool execution with `401 Invalid authentication credentials`, so no live Claude MCP tool calls were possible in this environment.
- [x] Install TriSeek into a real Codex environment and run `codex mcp list`.
  - Result: global `triseek-live-check` was added and listed as enabled.
- [x] In Codex, invoke at least one successful `find_files` call and one successful `search_content` call.
  - Result: wrapper logs confirmed `find_files` and `search_content` requests reached the live MCP server from Codex.
- [x] Confirm stdout remains clean JSON-RPC only while logs go to stderr in the traced real-client integration.
  - Result: the wrapper captured JSON-RPC request/response lines on stdio and TriSeek lifecycle logs on stderr during the Codex run.

## Exit Criteria

This branch is validated for CLI regressions, MCP protocol/tool behavior, fallback handling, installer safety, and live Codex client integration. The only remaining gap is live Claude Code tool execution, which is blocked by invalid Claude authentication in this environment.
