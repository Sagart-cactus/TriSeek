# Memo Active Mode — Codex Skill

TriSeek Memo tracks which files your agent has read this session and detects when they change on disk.
On **Claude Code**, **OpenCode**, and **Pi**, Memo works passively — hooks fire automatically after every Read and Edit tool call.

On **Codex**, hooks currently fire only for the Bash tool (upstream issue [#16732](https://github.com/openai/codex/issues/16732)).
TriSeek observes Bash-based shell reads automatically when Codex emits parsed command metadata, but built-in Codex `Read`
tool calls still need **active mode**: call `memo_check` yourself before re-reading a file.

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

## Full passive mode will be available once Codex hooks mature

Once Codex fires hooks for non-Bash tools (issue #16732), `triseek install codex` will automatically
upgrade to passive mode and this explicit `memo_check` step will no longer be needed.
