# TriSeek

TriSeek is a fast local code search CLI for people who like `rg`, but want repeated searches on medium and large codebases to stay fast too. It also ships a Memo layer for agent clients so they can ask whether a file is still fresh in-session instead of blindly re-reading it.

You can use it like a normal search command:

```sh
triseek "AuthConfig" .
```

And when you want speed across repeated searches, TriSeek keeps per-root index state under `~/.triseek` instead of scattering files into your repo.

Why it feels better than a repo-local search wrapper:

- search works immediately with `triseek "needle" [path]`
- indexing is optional, but speeds up repeated searches
- default state lives under `~/.triseek`, not in the repo root
- one global daemon can serve multiple roots
- MCP support is available for Claude Code, Codex, OpenCode, Pi, and other MCP clients
- Memo can observe file reads/edits passively where hooks exist, or provide active freshness checks through MCP

## Install

### macOS and Linux

```sh
curl -fsSL https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.sh | sh
```

By default this installs `triseek` and `triseek-server` into `~/.local/bin`. It prefers prebuilt GitHub Release archives and falls back to `cargo install` when a matching release is not available but Rust is installed locally. A successful install also ensures the TriSeek daemon is running: fresh installs start it, and reinstalls stop and restart it.

`triseek` is the main CLI. `triseek-server` is the background daemon binary used by `triseek daemon`.

Pin a version:

```sh
curl -fsSL https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.sh | sh -s -- --version v0.3.1
```

Install to a different directory:

```sh
curl -fsSL https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.sh | sh -s -- --install-dir /usr/local/bin
```

### Windows PowerShell

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.ps1 | iex"
```

By default this installs `triseek.exe` and `triseek-server.exe` into `%USERPROFILE%\AppData\Local\Programs\TriSeek\bin` and adds that directory to the user `PATH` if needed. It also falls back to `cargo install` when a matching release is not available but Rust is installed locally. A successful install also ensures the TriSeek daemon is running: fresh installs start it, and reinstalls stop and restart it.

### Cargo Fallback

If you already have Rust installed, you can install directly from GitHub:

```sh
cargo install --git https://github.com/Sagart-cactus/TriSeek.git triseek --locked
cargo install --git https://github.com/Sagart-cactus/TriSeek.git search-server --locked
```

From a local checkout:

```sh
cargo install --path crates/search-cli --locked
cargo install --path crates/search-server --locked
```

## Quick Start

Search immediately:

```sh
triseek "AuthConfig" .
triseek "AuthConfig" ./crates/search-cli
```

`triseek search "AuthConfig" .` remains supported as a compatibility alias.

Build an index when you want repeated searches to stay fast:

```sh
triseek build .
triseek "AuthConfig" .
triseek update .
```

Run the background daemon for repeated searches across active roots:

```sh
triseek daemon start
triseek daemon status .
```

TriSeek stores default state here:

```text
~/.triseek/indexes/<root-key>/
~/.triseek/daemon/
```

Nothing is written into the searched repo root by default.

Check the install or inspect the command surface:

```sh
triseek help
triseek doctor
```

## Use TriSeek from Claude Code, Codex, OpenCode, and Pi

TriSeek ships an **MCP (Model Context Protocol) server** so Claude Code,
Codex, OpenCode, Pi, and any MCP-capable client can use it as their primary
local code-search tool. The search tools run in-process over stdio and preserve
TriSeek's hybrid indexed / ripgrep-fallback routing. Memo uses the local
TriSeek daemon for session state, and `triseek install` wires up the hook or
plugin side where the client supports it. On `triseek mcp serve` startup,
TriSeek now schedules a background index sync for that root: full build if no
index exists yet, incremental refresh if one already exists. The MCP server
starts immediately so clients do not block on index creation. Early queries use
the existing index when one is already present, otherwise they fall back to the
normal direct-scan / ripgrep path until the background sync finishes. If the
local TriSeek daemon is running, `mcp serve` also preloads that root into the
daemon so its background watcher can keep the index warm after startup.

### Claude Code

Install for the current user:

```sh
triseek install claude-code
claude mcp list
```

Or install per-project (shareable, writes `.mcp.json`):

```sh
triseek install claude-code --scope project
```

### Codex

```sh
triseek install codex
codex mcp list
```

If the Codex CLI does not expose `mcp add`, the installer falls back to
merging a `[mcp_servers.triseek]` block into `~/.codex/config.toml` while
preserving any existing entries and comments.

Codex Memo currently runs in active mode because Codex hooks still do not
reliably fire for non-Bash tools. Use `memo_check` before re-reading files
you have already seen in the current session.

### OpenCode

```sh
triseek install opencode
```

This writes the TriSeek MCP entry plus a user-level OpenCode plugin under
`~/.config/opencode/plugins/triseek-memo.ts`.

### Pi

```sh
triseek install pi
```

This writes the TriSeek MCP entry plus a Pi extension under
`~/.pi/agent/extensions/triseek-memo/`.

### Run the MCP server manually

```sh
cd /path/to/repo
triseek mcp serve
```

Or, from anywhere:

```sh
triseek mcp serve --repo /path/to/repo
```

All logs go to stderr; stdout carries framed JSON-RPC messages only. The MCP server is currently scoped to one root per process.

### Verify the install

```sh
triseek doctor
```

`doctor` reports the binary path, detected repo root, availability of the
supported CLIs, existing MCP config locations, Memo hook/plugin health where
applicable, and whether a TriSeek index is present for the current repo.

### Tool reference

| Tool | Purpose |
|------|---------|
| `find_files` | Path/filename substring search |
| `search_content` | Literal or regex content search |
| `search_path_and_content` | Narrow by path glob then search content |
| `index_status` | Report TriSeek index health |
| `reindex` | Rebuild or incrementally update the index |
| `memo_status` | Report freshness of one or more files in the current session |
| `memo_session` | Show Memo session state, tracked files, and token savings |
| `memo_check` | Ask whether a single file should be re-read or skipped |

Full input/output schemas and error codes live in the [published MCP reference](https://sagart-cactus.github.io/TriSeek/mcp.html).

### How routing works

Every search response returned by the MCP server includes a `strategy`, a
`fallback_used` flag, and a `cache` field (`hit`, `miss`, or `bypass`) so
callers know which backend ran and whether the result came from the in-process
query cache:

- `triseek_indexed` — trigram index (fast for medium and large repos)
- `triseek_direct_scan` — in-process file walker (for filter-heavy queries)
- `ripgrep_fallback` — shells out to `rg` (for weak-regex and small repos)

Output is capped: default limit 20, hard cap 100, line previews truncated
to 200 characters, and duplicate matches deduped. This keeps responses
comfortably under Claude Code's 10,000-token MCP output warning.

### Troubleshooting MCP installs

- `claude mcp list` does not show TriSeek → re-run
  `triseek install claude-code --scope <scope>` and reload the Claude Code
  workspace. Use `triseek doctor` to confirm the `claude` CLI is on your
  `PATH`.
- Codex does not see TriSeek → inspect `~/.codex/config.toml` for a
  `[mcp_servers.triseek]` block. Re-run `triseek install codex`.
- Codex Memo seems inactive → this is expected until upstream hook support
  matures. Use `memo_check` in active mode and see `docs/codex-memo-skill.md`.
- Tool calls return `INDEX_UNAVAILABLE` → run `triseek build .` or
  call the `reindex` tool.

## Upgrade and Uninstall

Upgrade by rerunning the installer. To remove TriSeek, delete the installed binary:

- macOS/Linux: `rm -f ~/.local/bin/triseek ~/.local/bin/triseek-server`
- Windows: `Remove-Item "$HOME\\AppData\\Local\\Programs\\TriSeek\\bin\\triseek.exe","$HOME\\AppData\\Local\\Programs\\TriSeek\\bin\\triseek-server.exe"`

TriSeek stores indexes under `~/.triseek` by default. Remove that directory if you also want to delete local index data.

## Docs

- [Docs Home](https://sagart-cactus.github.io/TriSeek/)
- [Installation Guide](https://sagart-cactus.github.io/TriSeek/install.html)
- [MCP Server Reference](https://sagart-cactus.github.io/TriSeek/mcp.html)
- [Memo & Caching](https://sagart-cactus.github.io/TriSeek/memo.html)
- [How TriSeek Works](https://sagart-cactus.github.io/TriSeek/triseek-explained.html)
- [TriSeek Architecture](https://sagart-cactus.github.io/TriSeek/triseek-architecture.html)

## Release Automation

- CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and release-binary smoke builds on Linux, macOS, and Windows.
- Pushing a tag like `v0.3.1` triggers a release workflow that builds TriSeek archives for Linux, macOS (Intel and Apple Silicon), and Windows and uploads them to GitHub Releases.
