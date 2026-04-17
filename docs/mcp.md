# TriSeek MCP Server Reference

TriSeek exposes a local stdio MCP (Model Context Protocol) server so Claude
Code, Codex, and other MCP-capable clients can use it as their primary
code-search tool. Search runs directly inside the stdio MCP server. Memo uses
the local TriSeek daemon for session state so clients can ask whether files are
still fresh within the current session. The server is:

- **local-only** — no network, no telemetry, no OAuth
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

All tool responses are returned inside a standard MCP `CallToolResult`. The
tool output payload is encoded as a single `text` content block containing
JSON of the shape below. Tool errors set `isError: true` and return a
structured error envelope (see **Errors**).

Search success envelopes contain:

- `version` — schema version string (currently `"1"`)
- `repo_root` — absolute path of the resolved repository
- `strategy` — which backend ran: `triseek_indexed`, `triseek_direct_scan`,
  or `ripgrep_fallback`
- `fallback_used` — `true` when a non-indexed backend was selected
- `cache` — `hit`, `miss`, or `bypass` for the in-process search query cache
- `routing_reason` — short machine-readable reason string from the router
- `results` — the list of results
- `truncated` — `true` when results were capped by `limit`

### `find_files`

Path/filename substring search backed by TriSeek's path index with an
adaptive direct-scan fallback.

Input:

```json
{
  "query": "router auth",
  "limit": 20
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
  "limit": 20
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
  "limit": 20
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

Single-file freshness check used primarily by Codex active mode.

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

## Memo modes

- Claude Code, OpenCode, and Pi use passive Memo observation via hooks or generated plugins/extensions.
- Codex currently uses active Memo mode because upstream hooks still do not reliably fire for non-Bash tools. Call `memo_check` before re-reading files you already saw in-session.

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
