# Demo GIFs

These GIFs are benchmark replays based on the source-of-truth rerun in
`bench/results/rerun-2026-04-02-all/`.

They are intended to show the search backend story used by Claude Code and
Codex on a large repo:

- `torvalds/linux`
- single lookup: `literal_selective` / `AEGIS_BLOCK_SIZE`
- repeated session: `session_20`

Render command:

```sh
./scripts/demo/render_benchmark_replay_gifs.sh
```

Outputs:

- `docs/demo/claude-triseek-vs-grep.gif`
- `docs/demo/codex-triseek-vs-grep.gif`

Live capture command:

```sh
./scripts/demo/render_live_benchmark_gifs.sh
```

Live outputs:

- `docs/demo/claude-triseek-vs-grep-live.gif`
- `docs/demo/codex-triseek-vs-grep-live.gif`

Client-session render command:

```sh
./scripts/demo/render_client_session_gifs.sh
```

Client-session outputs:

- `docs/demo/claude-cli-triseek-vs-no-triseek.gif`
- `docs/demo/codex-cli-triseek-vs-no-triseek.gif`

Real TUI render command:

```sh
./scripts/demo/render_real_tui_gifs.sh
```

Real TUI outputs:

- `docs/demo/claude-real-tui-triseek-vs-no-triseek.gif`
- `docs/demo/codex-real-tui-triseek-vs-no-triseek.gif`
