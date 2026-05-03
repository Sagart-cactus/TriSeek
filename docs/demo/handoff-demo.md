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
