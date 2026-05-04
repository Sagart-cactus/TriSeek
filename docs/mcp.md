# TriSeek MCP Server Reference

TriSeek exposes a local stdio MCP (Model Context Protocol) server so Claude
Code, Codex, and other MCP-capable clients can use it as their primary
code-search tool. Search runs directly inside the stdio MCP server. Memo uses
the local TriSeek daemon for session state so clients can ask whether files are
still fresh within the current session. The server is:

- **local-first by default** — no network in the core daemon; briefing files are generated only when the user explicitly runs `triseek brief` or `triseek handoff`. No telemetry, no OAuth.
- **stdio transport** — one child process per client session
- **search-first** — search tools plus Memo freshness helpers, but no file-editing or shell-execution tools
- **hybrid-routed** — uses the TriSeek trigram index when it helps and falls
  back to ripgrep or direct scan when it doesn't, transparently
- **bounded** — small, stable JSON envelopes that stay well under Claude
  Code's 10,000-token MCP output warning

## Quickstart

```sh
# Claude Code (user-level by default)
triseek install claude-code

# Codex
triseek install codex

# OpenCode
triseek install opencode

# Pi
triseek install pi

# Verify
triseek doctor
```

Use `triseek install claude-code --scope project` if you want a shareable
repo-local Claude Code install via `.mcp.json`.

Run the server manually for debugging:

```sh
cd /path/to/repo
triseek mcp serve
```

Or override the root explicitly:

```sh
triseek mcp serve --repo /path/to/repo
```

On startup, `triseek mcp serve` schedules a best-effort index sync for that
root in the background: full build if no index exists yet, incremental update
if one already exists. The MCP server starts serving immediately instead of
blocking on the sync step. Early queries use the existing index when one is
already present, otherwise they continue through the normal direct-scan /
ripgrep fallback routing until the background sync finishes. If the sync step
fails, the server still keeps serving. When the local TriSeek daemon is already
running, `mcp serve` also registers that root with the daemon so its watcher
can apply incremental index updates in the background.

All logs go to stderr; stdout carries JSON-RPC frames only. The MCP server is currently scoped to one root per process.

Memo-specific note: `memo_status`, `memo_session`, and `memo_check` require a
running local TriSeek daemon. Hooks/plugins installed by `triseek install ...`
feed that daemon passively where supported.

## Protocol

- Framing: newline-delimited JSON (one JSON-RPC 2.0 message per line)
- Supported methods: `initialize`, `notifications/initialized`, `tools/list`,
  `tools/call`, `ping`, `shutdown`
- `protocolVersion`: `2025-06-18`
- `serverInfo.name`: `triseek`
- `capabilities`: `{ "tools": {} }`

## Tools

All tool responses are returned inside a standard MCP `CallToolResult`. Search
tools include a compact human-readable digest in the first `text` content block
and the machine-readable envelope in `structuredContent`. Tool errors set
`isError: true` and return a structured error envelope (see **Errors**).

Search success envelopes contain:

- `version` — schema version string (currently `"1"`)
- `repo_root` — absolute path of the resolved repository
- `strategy` — which backend ran: `triseek_indexed`, `triseek_direct_scan`,
  or `ripgrep_fallback`
- `fallback_used` — `true` when a non-indexed backend was selected
- `cache` — `hit`, `miss`, or `bypass`; `hit` means a repeated indexed search
  returned a context-reuse envelope instead of duplicate results
- `routing_reason` — short machine-readable reason string from the router
- `results` — the list of results
- `search_id` — identifier for the result set recorded in this session
- `truncated` — `true` when results were capped by `limit`

Repeated indexed searches may return a context-reuse envelope:

```json
{
  "version": "1",
  "repo_root": "/repo",
  "strategy": "triseek_indexed",
  "fallback_used": false,
  "cache": "hit",
  "search_id": "search-0001",
  "prior_search_id": "search-0001",
  "reuse_status": "fresh_duplicate",
  "reuse_reason": "unchanged",
  "generation": 42,
  "context_epoch": 0,
  "files_with_matches": 3,
  "total_line_matches": 8,
  "results": [],
  "results_omitted": true,
  "truncated": false
}
```

When `results_omitted` is `true`, the model should reuse the prior search
output already in conversation context. Set `force_refresh: true` on
`find_files`, `search_content`, or `search_path_and_content` when you explicitly
need TriSeek to execute the search again. Freshness is checked through the
daemon's generation, context epoch, and change journal; if relevant files
changed, TriSeek executes the search and returns fresh results.

### `find_files`

Path/filename substring search backed by TriSeek's path index with an
adaptive direct-scan fallback.

Input:

```json
{
  "query": "router auth",
  "limit": 20,
  "force_refresh": false
}
```

Output (example):

```json
{
  "version": "1",
  "repo_root": "/repo",
  "strategy": "triseek_indexed",
  "fallback_used": false,
  "results": [
    { "path": "src/auth/router.rs", "reason": "path_match" }
  ],
  "truncated": false
}
```

### `search_content`

Literal or regex content search.

Input:

```json
{
  "query": "parse_arguments",
  "mode": "literal",
  "limit": 20,
  "force_refresh": false
}
```

Allowed `mode` values: `literal`, `regex`.

Output (example):

```json
{
  "version": "1",
  "repo_root": "/repo",
  "strategy": "triseek_indexed",
  "fallback_used": false,
  "results": [
    {
      "path": "src/cli/parser.rs",
      "matches": [
        { "line": 1, "column": 8, "preview": "pub fn parse_arguments(args: &[String]) -> Config {" }
      ],
      "reason": "content_match"
    }
  ],
  "truncated": false
}
```

### `search_path_and_content`

Narrow by a path glob first, then search content.

Input:

```json
{
  "path_query": "src/**/*.rs",
  "content_query": "Result<",
  "mode": "literal",
  "limit": 20,
  "force_refresh": false
}
```

### `context_pack`

Builds a tiny, intent-aware starting set for a task. This is a trailhead, not
a broad context dump: it returns ranked paths, clipped snippets, reason tags,
and next-step hints under a hard budget.

Input:

```json
{
  "goal": "fix auth panic for service accounts",
  "intent": "bugfix",
  "budget_tokens": 1200,
  "max_files": 4,
  "changed_files": []
}
```

Allowed `intent` values: `bugfix`, `review`. Defaults are `bugfix`, 1200
estimated tokens, and 4 files. Hard caps are 4000 estimated tokens and 12
files.

Output (example):

```json
{
  "version": "1",
  "repo_root": "/repo",
  "intent": "bugfix",
  "goal": "fix auth panic for service accounts",
  "budget_tokens": 1200,
  "max_files": 4,
  "estimated_tokens": 180,
  "items": [
    {
      "path": "src/auth/router.rs",
      "score": 28.0,
      "reasons": ["content_match", "path_match"],
      "snippets": [
        { "line": 42, "column": 5, "preview": "panic!(\"auth panic\");" }
      ]
    }
  ],
  "suggested_next_steps": [
    "Start with the highest-ranked source file and its snippets."
  ],
  "truncated": false
}
```

### `index_status`

Reports whether the TriSeek index is present and healthy for the current repo.

Input: `{}`.

Output (example):

```json
{
  "version": "1",
  "repo_root": "/repo",
  "index_present": true,
  "index_fresh": true,
  "indexed_files": 28132,
  "index_bytes": 91256489,
  "last_updated": "2025-11-08T12:14:30Z",
  "repo_category": "large",
  "routing_hint": "indexed_default"
}
```

### `reindex`

Rebuild or update the index.

Input:

```json
{ "mode": "incremental" }
```

Allowed `mode` values: `incremental`, `full`.

Output:

```json
{
  "version": "1",
  "repo_root": "/repo",
  "completed": true,
  "mode": "incremental",
  "rebuilt_full": false,
  "elapsed_ms": 412,
  "indexed_files": 28145
}
```

### `memo_status`

Check freshness for one or more files in the current session.

Input:

```json
{
  "files": ["src/auth/router.rs"]
}
```

Output (example):

```json
{
  "session_id": "session-123",
  "results": [
    {
      "path": "src/auth/router.rs",
      "status": "stale",
      "tokens": 120,
      "read_count": 1,
      "message": "Changed since last read (now ~132 tokens); re-read file.",
      "current_tokens": 132
    }
  ]
}
```

### `memo_session`

Show tracked-file state and aggregate token savings for the current session.

### `memo_check`

Single-file freshness check used by any client path that cannot be observed
passively through hooks.

Input:

```json
{
  "path": "src/auth/router.rs"
}
```

Output (example):

```json
{
  "path": "src/auth/router.rs",
  "status": "fresh",
  "recommendation": "skip_reread",
  "tokens_at_last_read": 120,
  "last_read_ago_seconds": 9
}
```

`recommendation` is one of:

- `skip_reread`
- `reread_with_diff`
- `reread`

## Task and snapshot tools (since v0.4.2)

The portability surface tracks a **work session**: a goal, the TriSeek calls made while working on it, and zero or more snapshots. The public MCP tools are named `session_*`; the daemon stores them under `<daemon_dir>/sessions/`.

### `session_open`

Declare a work session and set it as the current MCP session.

Input:

```json
{
  "goal": "Debug retry backoff config",
  "session_id": "optional-stable-id"
}
```

Output:

```json
{
  "session": {
    "schema_version": 1,
    "session_id": "session_1777803396000",
    "goal": "Debug retry backoff config",
    "repo_root": "/repo",
    "status": "open",
    "created_at": 1777803396,
    "updated_at": 1777803396
  }
}
```

If `session_id` is omitted, TriSeek generates one. Reopening an existing id updates the goal when a non-empty goal is supplied.

### `session_status`

Return a session record plus the number of action-log entries captured for it.

Input:

```json
{ "session_id": "session_1777803396000" }
```

Output:

```json
{
  "session": { "session_id": "session_1777803396000", "status": "open" },
  "action_log_size": 8
}
```

`session_id` may be omitted only when the MCP server already has a current session.

### `session_list`

List portable sessions for the current repo.

Input:

```json
{}
```

Output:

```json
{
  "sessions": [
    { "session_id": "session_1777803396000", "goal": "Debug retry backoff config", "status": "open" }
  ]
}
```

The response is sorted by most recently updated session first.

### `session_close`

Mark a session as `resolved` or `abandoned`.

Input:

```json
{
  "session_id": "session_1777803396000",
  "status": "resolved"
}
```

Output:

```json
{
  "session": { "session_id": "session_1777803396000", "status": "resolved" }
}
```

Closing clears the current-session hint in the MCP server.

### `session_snapshot`

Create a persistent snapshot directory under the daemon's snapshot store.

Input:

```json
{
  "session_id": "session_1777803396000",
  "source_harness": "claude_code",
  "source_model": "claude-sonnet-4.5",
  "pinned_snippet_paths": [
    { "path": "src/auth/router.rs", "line_start": 40, "line_end": 80 }
  ]
}
```

Output:

```json
{
  "snapshot_id": "snap_1777803396_session_1777803396000",
  "snapshot_dir": "/home/me/.triseek/daemon/snapshots/snap_1777803396_session_1777803396000",
  "manifest": {
    "schema_version": 1,
    "snapshot_id": "snap_1777803396_session_1777803396000",
    "session_id": "session_1777803396000",
    "repo_root": "/repo",
    "repo_commit": "abc123",
    "repo_dirty_files": [],
    "source_harness": "claude_code",
    "source_model": "claude-sonnet-4.5",
    "generation": 42,
    "context_epoch": 0
  }
}
```

Snapshots include metadata, working set, action log, git state, and pinned snippets. They do not copy raw repo files.

### `session_handoff`

Convenience wrapper for MCP users: create `session_snapshot`, then mark the current session resolved. It returns the same envelope as `session_snapshot`.

Input:

```json
{
  "session_id": "session-123",
  "source_harness": "claude_code",
  "pinned_snippet_paths": [
    {
      "path": "src/auth/router.rs",
      "line_start": 40,
      "line_end": 80
    }
  ]
}
```

Use the CLI `triseek handoff codex` or `triseek handoff claude` when you want target-aware project-file writes and briefing generation.

### `session_snapshot_list`

List snapshot manifests, optionally filtered by session.

Input:

```json
{ "session_id": "session_1777803396000" }
```

Output:

```json
{
  "snapshots": [
    {
      "schema_version": 1,
      "snapshot_id": "snap_1777803396_session_1777803396000",
      "session_id": "session_1777803396000",
      "source_harness": "claude_code"
    }
  ]
}
```

### `session_snapshot_get`

Read the full snapshot envelope.

Input:

```json
{ "snapshot_id": "snap_1777803396_session_1777803396000" }
```

Output:

```json
{
  "snapshot": {
    "manifest": { "schema_version": 1, "snapshot_id": "snap_1777803396_session_1777803396000" },
    "working_set": { "schema_version": 1, "files_read": [], "searches_run": [], "frecency_top_n": [] },
    "action_log": [],
    "pinned_snippets": []
  }
}
```

Use this for debugging or for manually inspecting what a handoff captured.

### `session_snapshot_diff`

Compare two snapshots.

Input:

```json
{
  "snapshot_a": "snap_a",
  "snapshot_b": "snap_b"
}
```

Output:

```json
{
  "diff": {
    "added_files": [],
    "removed_files": [],
    "changed_files": ["src/auth/router.rs"],
    "added_searches": ["search-0009"],
    "removed_searches": []
  }
}
```

### `session_resume`

Hydrate daemon state from a portable snapshot and return a bounded Markdown
payload for the new harness.

Input:

```json
{
  "snapshot_id": "snap_1777803396_demo-handoff-real",
  "budget_tokens": 8000
}
```

Output:

```json
{
  "session_id": "session_1777803396000",
  "payload_markdown": "# TriSeek Hydration Payload\n...",
  "payload_token_estimate": 740,
  "hydration_report": {
    "files_primed": 12,
    "searches_warmed": 6,
    "frecency_entries_restored": 20,
    "stale_files": []
  },
  "searches": []
}
```

`snapshot_id` is required. `budget_tokens` is optional and caps the resume payload size. The CLI `triseek resume <snapshot_id> --write-to AGENTS.md` writes that payload to the target harness file.

### When to use what

| Scenario | Tool |
|---|---|
| Start tracking a work session | `session_open` |
| Check what is currently tracked | `session_status` or `session_list` |
| Capture a checkpoint | `session_snapshot` |
| Finish in this harness and hand off | `session_handoff` in MCP, or `triseek handoff <target>` in CLI |
| Inspect what was captured | `session_snapshot_get` |
| Compare two checkpoints | `session_snapshot_diff` |
| Continue from another harness | `session_resume` |

## Memo modes

- Claude Code, OpenCode, and Pi use passive Memo observation via hooks or generated plugins/extensions.
- Codex uses passive Memo observation for supported Bash and MCP file reads via
  `PreToolUse` / `PostToolUse` hooks. For any read path your installed Codex
  does not expose through hooks, call `memo_check` before re-reading files you
  already saw in-session.

## Errors

Tool execution failures return a successful JSON-RPC response with
`CallToolResult.isError = true`. The error body has this shape:

```json
{
  "version": "1",
  "error": {
    "code": "INDEX_UNAVAILABLE",
    "message": "TriSeek index is unavailable for this repository",
    "retryable": true,
    "suggested_action": "Call the `reindex` tool or run `triseek build <PATH>`"
  }
}
```

Error codes:

| Code | Meaning |
|------|---------|
| `INDEX_UNAVAILABLE` | No index exists for this repo. |
| `INDEX_STALE` | Index is out of date; call `reindex`. |
| `INVALID_QUERY` | Missing/invalid arguments. |
| `REPO_NOT_DETECTED` | Could not resolve a repo root. |
| `BACKEND_FAILURE` | Search backend errored internally. |
| `FALLBACK_FAILURE` | Fallback (ripgrep / direct scan) failed. |
| `CONFIG_WRITE_FAILED` | Install/uninstall could not write config. |
| `CLIENT_NOT_INSTALLED` | Required client CLI is missing from PATH. |

## Output discipline

- Default `limit = 20`, hard cap `100`.
- Line previews truncated to 200 characters.
- Duplicate line matches deduped by `(path, line)`.
- `truncated = true` whenever results were clipped.
- Responses never include raw file bodies.

## Troubleshooting

- **Claude Code doesn't see TriSeek** — run
  `triseek install claude-code --scope <scope>`, reload the Claude Code
  workspace, then `claude mcp list`.
- **Codex doesn't see TriSeek** — run `triseek install codex`, restart
  Codex, then `codex mcp list`. Inspect `~/.codex/config.toml` for a
  `[mcp_servers.triseek]` block if verification fails.
- **`INDEX_UNAVAILABLE`** — run `triseek build <PATH>` or call
  the `reindex` tool.
- **Stdout corruption** — TriSeek routes all logs to stderr. If a client
  reports framing errors, check that nothing in the wrapper script writes
  to stdout in front of `triseek mcp serve`.
