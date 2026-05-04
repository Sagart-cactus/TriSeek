# TriSeek Handoff — Notes & Follow-ups

## What the handoff command does

`/triseek handoff <target>` snapshots the current session and prints a block
the user pastes into the target harness to resume with full context:

```
TriSeek handoff ready

From: Claude
To: Codex
Session: <id>
Snapshot: <id>
Brief: /path/to/brief.md

In Codex, paste:
  $triseek resume <snapshot_id>
```

The CLI entry point is `crates/search-cli/src/main.rs` (~line 820).  
The rendering logic lives in `crates/search-cli/src/handoff.rs`.

## Recent improvement (2026-05-03)

`render_handoff_block` previously printed raw canonical harness names (`claude_code`,
`codex`) in the `From:` and `To:` fields while already using `harness_display()` for
the "In {target}, paste:" line.  Fixed to pass both fields through `harness_display()`
so the entire block is consistent and human-readable.

The target command must also be harness-specific:

- Codex resumes with `$triseek resume <snapshot_id>`.
- Claude resumes with `/triseek resume <snapshot_id>`.

## Demo storyline

Use a two-step Claude session so the handoff feels like real work, not a
mechanical command demo:

1. Ask Claude to inspect the handoff implementation and identify the files that
   control snapshot creation, brief generation, and resume hydration.
2. Ask Claude to make a small concrete change, such as drafting a short handoff
   checklist in `docs/demo/handoff-demo.md` or adding a focused test expectation.

Then ask Claude to hand off to Codex. Codex should resume the snapshot, summarize
the restored files/searches/goal, and continue with the second step instead of
rediscovering the repo from scratch.

## Follow-ups

- **Machine-readable handoff block**: If any tooling ever needs to parse the block
  (e.g. CI, a shell wrapper), consider adding a `--json` flag to `triseek handoff`
  that emits `{ from, to, session_id, snapshot_id, briefing_path }` instead of the
  prose block.  The current prose format is intentionally human-only.

- **`source_model` field**: `SessionSnapshotCreateParams` accepts `source_model` but
  `main.rs` always passes `None`.  Wiring in the active model name (where available)
  would let the resume side show which model originated the session.

- **`harness_display` fallback**: The catch-all `_ => "target harness"` in
  `harness_display` is a poor display string if an unknown harness leaks into the
  rendered block.  Consider returning the raw value unchanged as a safer default.

---

## Phase 2 — Codex Markdown/docs checklist

*Left for Codex by Claude (Phase 1 completed 2026-05-03).*
*Phase 1 updated the HTML docs site: `docs/context-handoff.html` created, nav updated in all eight HTML pages, card + feature item added to `docs/index.html`.*

The following Markdown docs should be updated by Codex to match the HTML documentation written in Phase 1:

### Files to create or update

- [x] **`docs/mcp.md`** — Add a `session_handoff` and `session_resume` tool entry in the MCP tool reference table. Match the schema style of existing tool entries (`session_open`, `session_snapshot`, etc.). Include the `target` parameter for `session_handoff` and the `snapshot_id` parameter for `session_resume`.

- [x] **`docs/install.md`** (if it documents CLI commands) — Add a brief mention of `triseek handoff <target>` and `triseek resume <snapshot_id>` in the CLI command listing. Two-line entries are sufficient; link to `context-handoff.html`.

- [x] **`README.md`** (root) — Add Context Handoff to the feature list or capability summary. Use the same one-sentence description used in `docs/index.html`: "Snapshot a session and resume it in another harness — goal, files, searches, and git state preserved. Explicit Claude ↔ Codex switching."

### Command behavior to preserve (exact wording)

These are the exact command forms documented in the HTML. Any Markdown docs must match them precisely:

| Harness | Initiate handoff | Resume |
|---|---|---|
| Claude Code | `/triseek handoff codex` | `/triseek resume <snapshot_id>` |
| Codex | `$triseek handoff claude` | `$triseek resume <snapshot_id>` |

- The `From:` / `To:` labels in the printed handoff block use `harness_display()` — display strings are `Claude` and `Codex`, not `claude_code` / `codex`.
- The `In {target}, paste:` line always shows the target-specific command (slash vs dollar prefix).
- `triseek doctor` is the pre-flight check; it should be mentioned before any handoff walkthrough.

### What NOT to change

- Do not alter the snapshot ID format (`snap_<timestamp>_<session_id>`) — it is an implementation detail, not a user-visible API.
- Do not add a `--json` flag to `triseek handoff` — the follow-up item in this file already tracks that as a future consideration.
- Do not document `source_model` — it is always `null` until the follow-up item is implemented.

### Verification

After updating the Markdown docs, run a search to confirm the command strings appear consistently:

```
$triseek search_content "triseek handoff"
$triseek search_content "triseek resume"
```

Both should surface exactly the files you just updated — not stale docs referencing old command names.
