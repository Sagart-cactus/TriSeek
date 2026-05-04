# Claude to Codex Handoff Video

This demo records a browser-based terminal using the real PTY-backed wterm
example from `/Users/trivedi/Documents/Projects/wterm/examples/local`.

It shows:

1. The user typing `claude`.
2. A Claude-style terminal TUI doing session work.
3. The user asking Claude to create a Codex handoff, then quitting Claude.
4. The user typing `codex`.
5. A Codex-style terminal TUI restoring the handoff and continuing work.

There are no browser overlays in the recording. The visible product surface is
the terminal and the agent TUI. Before recording starts, the runner builds the
current TriSeek branch and prepares demo-scoped Claude/Codex MCP config that
points at `target/debug/triseek`; it does not mutate the user's real Claude or
Codex settings.

## Run

From the TriSeek repo root:

```sh
docs/demo/handoff-video/run-demo.sh
```

Outputs are written to:

```text
docs/demo/handoff-video/output/
```

Important outputs:

- `handoff-demo.mp4` - final browser recording converted to MP4
- `raw-video.webm` - Playwright viewport recording
- `final-frame.png` - QA screenshot
- `resume-AGENTS.md` - generated resume payload from `triseek resume`

## Requirements

- `pnpm`
- `node`
- `ffmpeg`
- `say`
- Playwright from the wterm workspace
- Rust/Cargo to build the current TriSeek workspace

The runner starts the wterm local PTY app on `http://127.0.0.1:3210` and uses
an isolated `TRISEEK_HOME` under `output/.triseek-home`.
