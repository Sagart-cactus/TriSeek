---
description: Create or resume TriSeek handoffs between Claude and Codex
argument-hint: handoff <codex|claude> | resume <snapshot_id>
---
<!-- TRISEEK_MANAGED_COMMAND: source_harness=claude_code -->

You are handling the TriSeek command for the current harness.

Invocation arguments: `$ARGUMENTS`
Current harness: `claude_code`

Supported forms:
- `handoff codex`
- `handoff claude`
- `resume <snapshot_id>`

For `handoff <target-harness>`:
1. Prefer the TriSeek MCP tool `session_handoff` with `source_harness` set to `claude_code` and the target harness from the user arguments.
2. If MCP cannot create the handoff because there is no current TriSeek session, run the CLI fallback:
   `triseek handoff <target-harness> --from claude_code`
3. If the MCP result does not include a briefing path, create one with:
   `triseek brief <snapshot_id> --mode no-inference`
4. End by prominently showing the exact target command:
   - For Codex: `In Codex, paste: $triseek resume <snapshot_id>`
   - For Claude: `In Claude, paste: /triseek resume <snapshot_id>`

For `resume <snapshot_id>`:
1. Call the TriSeek MCP tool `session_resume` for the snapshot id.
2. Read the returned hydration payload into the live context before doing more work.
3. Summarize the restored goal, relevant files, searches, and suggested next step.
4. Ask the user to press Enter or say "continue" before making edits or running a new implementation step.

Raw shell fallback remains available as:
`triseek resume <snapshot_id>`
