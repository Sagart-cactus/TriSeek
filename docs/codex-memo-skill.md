# Memo Active Mode — Codex Skill

TriSeek Memo tracks which files your agent has read this session and detects when they change on disk.
On **Claude Code**, **OpenCode**, and **Pi**, Memo works passively — hooks fire automatically after every Read and Edit tool call.

On **Codex**, TriSeek installs `PreToolUse`, `PostToolUse`, and `SessionStart` hooks.
Bash-based shell reads are observed automatically when Codex emits parsed command metadata.
Codex upstream also dispatches hooks for MCP tools, so MCP file-read tools such as `mcp__filesystem__read_file` can be observed and redundant rereads can be blocked when Memo proves the file is unchanged.
Any file read that your installed Codex does not expose through hooks still needs **active mode**: call `memo_check` yourself before re-reading a file.

---

## When to use `memo_check`

Call `mcp__triseek__memo_check` before re-reading any file you have already read in the current session.

```json
{ "path": "src/lib.rs" }
```

---

## Decision table

| `recommendation` | Meaning | What to do |
|---|---|---|
| `skip_reread` | File unchanged since you last read it. | Trust conversation history. Skip the re-read. |
| `reread_with_diff` | File changed, but only slightly (<10% size delta). | Re-read expecting a small diff. |
| `reread` | File changed significantly, or was never read this session. | Read it normally. |

---

## Full response fields

| Field | Type | Description |
|---|---|---|
| `path` | string | The path you queried. |
| `status` | `fresh` \| `stale` \| `unknown` | Raw freshness status. |
| `recommendation` | see table above | Suggested action. |
| `tokens_at_last_read` | number \| null | Token count when you last read the file. |
| `current_tokens` | number \| null | Estimated token count on disk now (set when stale). |
| `last_read_ago_seconds` | number \| null | Seconds since the last observed read. |

---

## Example agent prompt addition

Add this to your Codex system prompt or per-session instruction:

> Before re-reading any file you have seen this session, call `mcp__triseek__memo_check { "path": "<file>" }`.
> If `recommendation` is `skip_reread`, do not re-read. If `reread_with_diff` or `reread`, proceed normally.
> This prevents redundant token use on unchanged files.

---

## Full passive mode depends on the installed Codex hook surface

For Codex versions that dispatch MCP tool hooks, MCP file reads are passive. For any non-hooked read path, keep using explicit `memo_check` before re-reading.
