# TriSeek

<p align="center">
  <img src="demo/triseek.gif" alt="TriSeek demo — install, search, repeat search from index, memo layer skipping a re-read in Claude Code" width="760" />
</p>

<p align="center">
  <em>Code search for AI coding agents — and the humans behind them.</em>
</p>

<p align="center">
  <a href="https://crates.io/crates/triseek"><img src="https://img.shields.io/crates/v/triseek.svg" alt="crates.io" /></a>
  <a href="https://github.com/Sagart-cactus/TriSeek/actions"><img src="https://github.com/Sagart-cactus/TriSeek/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT license" /></a>
  <img src="https://img.shields.io/badge/rust-stable-orange.svg" alt="Rust stable" />
</p>

**TriSeek** is a local code-search daemon with an MCP server for Claude Code, Codex, OpenCode, and Pi. It keeps a trigram index of your repos, falls back to ripgrep when the index is cold, and — most importantly for agents — tracks which files and search results are already in the current session so your agent stops re-reading the same file or reprinting the same search output three times per conversation.

On the Linux kernel, a 20-query agent session takes **0.6 s with TriSeek vs 10.3 s with ripgrep** — 16.9× faster, measured. The memo layer on top catches redundant re-reads before they hit disk, with zero false negatives across 12 replayed Claude Code sessions.

```sh
curl -fsSL https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.sh | sh
triseek "AuthConfig" .
triseek install claude-code
```

That's the whole onboarding. Search works immediately. The index and daemon warm up in the background.

---

## Why TriSeek

- **Indexed where it matters, raw where it doesn't.** Trigram index for repeated queries; ripgrep fallback for weak regex and small repos. You never wait for an index to build before the first search.
- **Built for AI agents from day one.** First-class MCP server (8 tools), installs into Claude Code / Codex / OpenCode / Pi with one command, and exposes a session-aware memo layer that prevents agents from re-reading files they just saw.
- **State stays out of your repo.** Indexes, daemon, and session data live under `~/.triseek`, not in repo-local dotfiles. One global daemon serves many roots.
- **Honest about what ran.** Every MCP response tells you which backend answered (`triseek_indexed`, `triseek_direct_scan`, `ripgrep_fallback`) and whether a repeated indexed search reused earlier context (`cache: hit`) or executed again (`miss` / `bypass`).

---

## Install

### macOS and Linux

```sh
curl -fsSL https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.sh | sh
```

Installs `triseek` and `triseek-server` into `~/.local/bin`. Prefers prebuilt GitHub Releases and falls back to `cargo install` if Rust is local. Fresh installs start the daemon; reinstalls stop and restart it.

Pin a version or install elsewhere:

```sh
curl -fsSL .../install.sh | sh -s -- --version v0.4.0
curl -fsSL .../install.sh | sh -s -- --install-dir /usr/local/bin
```

### Windows PowerShell

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.ps1 | iex"
```

Installs into `%USERPROFILE%\AppData\Local\Programs\TriSeek\bin` and adds it to the user `PATH`.

### Cargo

```sh
cargo install --git https://github.com/Sagart-cactus/TriSeek.git triseek --locked
cargo install --git https://github.com/Sagart-cactus/TriSeek.git search-server --locked
```

---

## Quick Start

```sh
# search immediately — no setup required
triseek "AuthConfig" .

# build an index once; repeat searches get much faster
triseek build .
triseek update .

# run the daemon so indexes stay warm across sessions
triseek daemon start
triseek daemon status .

# install into your AI coding agent
triseek install claude-code     # or codex, opencode, pi

# check everything is wired up
triseek doctor
```

TriSeek writes nothing into the searched repo. Default state lives at `~/.triseek/indexes/<root-key>/` and `~/.triseek/daemon/`.

---

## Benchmarks

On medium-and-up repos, TriSeek is dramatically faster than running ripgrep per query — the workload agents actually produce. **On the Linux kernel, a 20-query agent session takes 0.6 s with TriSeek vs 10.3 s with ripgrep — a 16.9× speedup.**

**20-query agent session** (the primary workload — averaged over a mix of literal, regex, path, and no-match queries):

| Repo | Size | TriSeek p50 | ripgrep p50 | Speedup |
|---|---|---:|---:|---:|
| kubernetes | 28k files · 254 MB | 292 ms | 3,575 ms | **12.2×** |
| rust-lang/rust | 58k files · 198 MB | 375 ms | 5,954 ms | **15.9×** |
| torvalds/linux | 93k files · 1.5 GB | 613 ms | 10,342 ms | **16.9×** |

**Single literal search** (content search, p50):

| Repo | TriSeek | ripgrep | Speedup |
|---|---:|---:|---:|
| kubernetes | 48 ms | 493 ms | **10.2×** |
| rust-lang/rust | 74 ms | 1,003 ms | **13.6×** |
| torvalds/linux | 197 ms | 3,972 ms | **20.1×** |

One-time index build cost: 14 s (kubernetes), 10 s (rust-lang/rust), 2.5 min (linux). During the build, searches transparently fall through to ripgrep — you're never blocked.

Hardware: MacBook Pro, macOS 26.2, Intel x86_64, 16 cores. ripgrep 14.x pinned in results. Full methodology, all 13 query families, and raw JSON + CSV: [bench/results/README.md](bench/results/README.md) and [`bench/results/rerun-2026-04-02-all/`](bench/results/rerun-2026-04-02-all/).

Reproduce:

```sh
cargo run -p search-bench -- run \
  --manifest bench/manifest/repositories.yaml \
  --cold-iterations 5 --warm-iterations 10
```

### When TriSeek is not the right tool

On **small repos (under ~1,000 files)**, TriSeek is slightly slower than cold ripgrep — the index and daemon overhead doesn't pay off. Measured: serde (339 files) 21 ms vs rg 15 ms; the ripgrep repo itself (207 files) 23 ms vs rg 18 ms. If your repos are small, just use ripgrep. TriSeek's sweet spot starts at a few thousand files.

### Honest correctness note

In the 2026-04-02 full rerun, 64 of 65 benchmark cases matched ripgrep exactly. The one mismatch (kubernetes · `regex_weak`) turned out to be a *binary-vs-text classification* difference: ripgrep included 2,547 matches from a vendored `.pb` (protobuf) blob; TriSeek correctly treated it as binary and skipped. Full writeup: [`bench/results/rerun-2026-04-02-all/correctness-revalidation.md`](bench/results/rerun-2026-04-02-all/correctness-revalidation.md).

---

## The memo layer — keeping re-reads honest

Agents routinely re-read the same file within a single conversation: once to understand it, once to plan the edit, once to verify. The memo layer catches those redundant re-reads before they hit disk.

TriSeek's daemon tracks, per session, which files an agent has read, their content hash (`xxh3_64`), estimated token size, and elapsed time since the read. Before a re-read, the agent calls `memo_check`; if the file is unchanged, TriSeek returns a skip decision, and the agent proceeds with what it already has in context.

Observable on the wire (abridged):

```
triseek.memo_check
  → { "status": "fresh",
      "recommendation": "skip_reread",
      "path": "crates/search-cli/src/memo_shim.rs",
      "tokens_at_last_read": 312,
      "last_read_ago_seconds": 142 }
```

**Validation.** We replayed 12 traced Claude Code sessions (ripgrep and serde tasks, six explicit-scope, six cold-start) through the live daemon. 100% of Memo-eligible redundant re-reads were prevented; **0 false negatives** across all 12 runs. Methodology and raw reports: [`memo_validation/`](memo_validation/). The absolute file-read token numbers are small (tens to low hundreds per task on the traces we ran) — treat memo as a correctness-and-hygiene feature, not a headline cost-cutter. If your sessions involve many repeated full-file reads of large files, savings scale accordingly.

Repeated search reuse is separate from file-read memo. When the same MCP search runs again in the same session and the daemon proves relevant files are unchanged, TriSeek returns a compact `fresh_duplicate` response with `results_omitted: true` and tells the model to reuse the earlier output already in context. The v0.3.2 validation run saved 628 of 679 repeat-search result outputs, for 1124 of 1175 combined file-read plus search opportunities.

**Compaction.** When a harness compacts its conversation context, the model loses the file bodies it previously saw. The memo daemon handles this honestly: on a `PreCompact` event from the harness, it invalidates the session's file map so post-compaction `memo_check` calls return `Reread` rather than a stale `SkipReread`. This matters because a "skip" recommendation is only safe when the model still has the content. The daemon never pretends otherwise.

Hook support varies by client:

| Client | Read observation | PreCompact observation |
|---|---|---|
| Claude Code | via `PreToolUse` / `PostToolUse` hooks | **Yes** — memo invalidates correctly |
| Pi | via `tool_result` extension event | **Yes** — memo invalidates correctly |
| OpenCode | via companion plugin using `tool.execute.after` | **Yes** — via companion plugin using `experimental.session.compacting` |
| Codex | explicit `memo_check` for non-hooked file reads; Bash and MCP file reads via `PreToolUse` / `PostToolUse` when supported by the installed Codex version | No (harness doesn't expose one) |

OpenCode support works through the plugin sidecar installed by `triseek install opencode`, not through the MCP transport itself. TriSeek installs both the MCP server entry and a companion OpenCode plugin; that plugin listens for `session.created`, `tool.execute.after`, and `experimental.session.compacting`, then forwards those events to `triseek memo-observe`. That gives OpenCode the same compaction-aware memo behavior as Claude Code and Pi for this use case, even though the integration surface is a plugin API rather than Claude Code's JSON hook config. Codex upstream now has hook dispatch for MCP tools, so TriSeek installs Codex matchers for Bash and MCP file-read tools; Codex still lacks an equivalent pre-compact hook surface, so memo correctness is bounded to the current uncompacted segment.

---

## Use with AI coding agents

One command per client. All four land TriSeek as a first-class MCP server; the memo integration varies by what each client's hook surface supports.

| Client | Install command | Memo mode | Compaction-aware | Config touched |
|---|---|---|---|---|
| Claude Code | `triseek install claude-code [--scope user\|project\|local]` | passive (via hooks) | Yes | `~/.claude/settings.json` · `.mcp.json` · `.claude/settings.local.json` |
| Pi | `triseek install pi` | passive (via extension) | Yes | MCP config + `~/.pi/agent/extensions/triseek-memo/` |
| OpenCode | `triseek install opencode` | passive (via companion plugin events) | Yes | MCP config + `~/.config/opencode/plugins/triseek-memo.ts` |
| Codex | `triseek install codex` | active for non-hooked file reads; passive for supported Bash and MCP file reads | No (harness doesn't expose a hook) | `~/.codex/config.toml` · `~/.codex/hooks.json` |

Verify any install with `triseek doctor`. Run the server manually (for debugging or CI) with `triseek mcp serve --repo /path/to/repo` — stdout carries framed JSON-RPC, stderr carries logs.

### MCP tool reference

| Tool | Purpose |
|---|---|
| `context_pack` | Tiny intent-aware starting set for bugfix/review tasks |
| `find_files` | Path / filename substring search |
| `search_content` | Literal or regex content search |
| `search_path_and_content` | Narrow by path glob, then search content |
| `index_status` | Report TriSeek index health |
| `reindex` | Rebuild or incrementally update the index |
| `memo_session` | Show session state, tracked files, and token savings |
| `memo_status` | Batch freshness check on a set of files |
| `memo_check` | Single-file skip-or-reread decision |

Full schemas and error codes: [MCP reference](https://sagart-cactus.github.io/TriSeek/mcp.html).

### Context packs

`context_pack` and `triseek context-pack` provide a small task trailhead for
agents before they chain broad searches. The first version is heuristic-only:
exact search plus path, test, config, fixture, and changed-file signals. It
returns ranked paths, clipped snippets, reason tags, and follow-up hints; it
does not return full file bodies.

```sh
triseek context-pack --goal "fix auth panic for service accounts" --intent bugfix --json
```

Defaults are intentionally small: `bugfix`, 1200 estimated tokens, and 4 files.
Use `--intent review --changed-file <path>` to bias the pack around a review
surface.

### How routing works

Every search response includes `strategy`, `fallback_used`, and `cache` (`hit` / `miss` / `bypass`):

- `triseek_indexed` — trigram index, fast on medium-to-large repos.
- `triseek_direct_scan` — in-process walker, used when path filters dominate.
- `ripgrep_fallback` — shells out to `rg` for weak regex or very small repos.

For indexed repeated searches, `cache: hit` means TriSeek skipped returning duplicate results and emitted a context-reuse envelope. Use `force_refresh: true` on a search tool call when you explicitly need a fresh execution.

Default result cap: 20 results, 200-char previews, dedup on. Keeps responses comfortably under Claude Code's 10,000-token MCP output warning.

---

## Why not X?

| Alternative | When it's the right choice | Why TriSeek instead |
|---|---|---|
| **ripgrep** | One-off searches from your shell, pipelines, scripts. | ripgrep is incredible and TriSeek uses it for cold queries. But agents repeat the same searches dozens of times per session — that's where a persistent index pays off. |
| **Zoekt** | Server-side search across many repos for a team. | Zoekt is built for a search backend that many people query. TriSeek is built for a single developer and their AI agent on local repos, with MCP-native integration and a memo layer Zoekt doesn't have. |
| **Sourcegraph** | Enterprise code intelligence, team-wide. | Sourcegraph is a hosted platform with many concerns beyond search. TriSeek is one binary, local state, zero auth. |
| **ast-grep** | Structural, AST-aware refactoring. | Different job. ast-grep answers "where does this *pattern* occur"; TriSeek answers "where does this *string or regex* occur, fast, across sessions" and exposes that to agents. |
| **VS Code / JetBrains global search** | Interactive search inside your editor. | Agent tool calls don't route through your editor. TriSeek gives the agent its own fast path. |

---

## Troubleshooting

- **`claude mcp list` doesn't show TriSeek** — re-run `triseek install claude-code --scope <scope>` and reload your Claude Code workspace. `triseek doctor` confirms the `claude` CLI is on `PATH`.
- **Codex doesn't see TriSeek** — check `~/.codex/config.toml` for `[mcp_servers.triseek]`. Re-run `triseek install codex`.
- **Codex memo seems inactive** — Bash and MCP file reads are hook-observed when your Codex version emits those hooks; other file reads still need explicit `memo_check`; see `docs/codex-memo-skill.md`.
- **`INDEX_UNAVAILABLE` on MCP calls** — run `triseek build .` or call the `reindex` tool.
- **Daemon crashed / stuck** — `triseek daemon stop` then `triseek daemon start`. Logs: `~/.triseek/daemon/*.log`.

---

## Upgrade and Uninstall

Upgrade by rerunning the installer. To remove:

```sh
# macOS / Linux
rm -f ~/.local/bin/triseek ~/.local/bin/triseek-server

# Windows (PowerShell)
Remove-Item "$HOME\AppData\Local\Programs\TriSeek\bin\triseek.exe","$HOME\AppData\Local\Programs\TriSeek\bin\triseek-server.exe"
```

Local index and session data live under `~/.triseek`; remove that directory if you want a fully clean uninstall.

---

## Docs

- [Docs home](https://sagart-cactus.github.io/TriSeek/)
- [Installation guide](https://sagart-cactus.github.io/TriSeek/install.html)
- [MCP server reference](https://sagart-cactus.github.io/TriSeek/mcp.html)
- [Memo & search reuse](https://sagart-cactus.github.io/TriSeek/memo.html)
- [How TriSeek works](https://sagart-cactus.github.io/TriSeek/triseek-explained.html)
- [Architecture](https://sagart-cactus.github.io/TriSeek/triseek-architecture.html)

## Contributing

Issues and PRs welcome. Start with [good-first-issue](https://github.com/Sagart-cactus/TriSeek/labels/good%20first%20issue). CI runs formatting, clippy, workspace tests, and release-binary smoke builds across Linux, macOS, and Windows. For release-style local validation, `scripts/run_real_harness_docker.sh` builds the binaries in Docker, starts an isolated daemon, exercises CLI and MCP behavior, and verifies install config generation for Claude Code, Codex, OpenCode, and Pi.

## License

MIT. See [LICENSE](LICENSE).
