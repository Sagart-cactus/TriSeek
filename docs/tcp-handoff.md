# TriSeek TCP Handoff

A TriSeek `.tcp` handoff is a portable session pack for moving context from one harness to another. It captures a TriSeek snapshot, including session metadata, working set, searches, action log, git state, and optional pinned snippets. The pack is local until you move it.

Use a `.tcp` handoff when you want another harness, such as Claude or Codex, to continue with warmed TriSeek state instead of rediscovering files and searches from scratch.

## Transport Modes

- `metadata`: writes snapshot metadata only. The target checkout must already match the snapshot commit and dirty-file list.
- `git`: writes snapshot metadata and records a Git handoff branch. On resume, TriSeek can fetch and check out the recorded branch, then validate the restored commit.

## Create A Handoff From A Harness

Inside the source harness, use TriSeek MCP. Do not shell out to `triseek handoff` when MCP is available.

1. Open or reuse a portable session:

```json
{
  "tool": "session_open",
  "arguments": {
    "goal": "Document TCP handoff, then hand off website docs work"
  }
}
```

2. Work normally with TriSeek file discovery and search tools.

3. Create the pack and close the session as resolved:

```json
{
  "tool": "session_handoff",
  "arguments": {
    "mode": "git",
    "target_harness": "claude",
    "pack_output_path": "/path/to/handoff.tcp"
  }
}
```

Useful optional arguments:

- `branch`: choose the Git handoff branch name.
- `remote`: choose the Git remote.
- `message`: choose the handoff commit message.
- `continue_existing`: allow reuse of an existing handoff branch.
- `pinned_snippet_paths`: include exact file ranges the next harness should see first.

## Restore A Handoff In Another Harness

Inside the target harness, use TriSeek MCP `session_resume` with the pack path:

```json
{
  "tool": "session_resume",
  "arguments": {
    "pack_path": "/path/to/handoff.tcp",
    "budget_tokens": 8000
  }
}
```

TriSeek imports the pack, warms memo and search state, and returns a Markdown hydration payload for the target harness. If the pack was created with `mode: git`, TriSeek also uses the recorded Git handoff metadata to restore and validate the working tree.

After resume, read the hydration payload first, then continue with TriSeek MCP tools for file discovery and exact code search.
