# TriSeek

TriSeek is a fast local code search CLI for people who like `rg`, but want repeated searches on medium and large codebases to stay fast too.

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
- MCP support is available for Claude Code, Codex, and other MCP clients

## Install

### macOS and Linux

```sh
curl -fsSL https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.sh | sh
```

By default this installs `triseek` and `triseek-server` into `~/.local/bin`. It prefers prebuilt GitHub Release archives and falls back to `cargo install` when a matching release is not available but Rust is installed locally.

`triseek` is the main CLI. `triseek-server` is the background daemon binary used by `triseek daemon`.

Pin a version:

```sh
curl -fsSL https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.sh | sh -s -- --version v0.2.1
```

Install to a different directory:

```sh
curl -fsSL https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.sh | sh -s -- --install-dir /usr/local/bin
```

### Windows PowerShell

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.ps1 | iex"
```

By default this installs `triseek.exe` and `triseek-server.exe` into `%USERPROFILE%\AppData\Local\Programs\TriSeek\bin` and adds that directory to the user `PATH` if needed. It also falls back to `cargo install` when a matching release is not available but Rust is installed locally.

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

## Use TriSeek from Claude Code and Codex (MCP)

TriSeek ships an **MCP (Model Context Protocol) server** so Claude Code,
Codex, and any MCP-capable client can use it as their primary local code
search tool. The server runs in-process over stdio — no daemon required —
and preserves TriSeek's hybrid indexed / ripgrep-fallback routing.

### Claude Code

Install per-project (shareable, writes `.mcp.json`):

```sh
triseek install claude-code --scope project
claude mcp list
```

Or install for the current user via the Claude CLI:

```sh
triseek install claude-code --scope local
```

### Codex

```sh
triseek install codex
codex mcp list
```

If the Codex CLI does not expose `mcp add`, the installer falls back to
merging a `[mcp_servers.triseek]` block into `~/.codex/config.toml` while
preserving any existing entries and comments.

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
Claude and Codex CLIs, existing MCP config locations, and whether a TriSeek
index is present for the current repo.

### Tool reference

| Tool | Purpose |
|------|---------|
| `find_files` | Path/filename substring search |
| `search_content` | Literal or regex content search |
| `search_path_and_content` | Narrow by path glob then search content |
| `index_status` | Report TriSeek index health |
| `reindex` | Rebuild or incrementally update the index |

Full input/output schemas and error codes live in the [published MCP reference](https://sagart-cactus.github.io/TriSeek/mcp.html).

### How routing works

Every search response returned by the MCP server includes a `strategy` and
a `fallback_used` flag so callers know which backend ran:

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
- [How TriSeek Works](https://sagart-cactus.github.io/TriSeek/triseek-explained.html)
- [TriSeek Architecture](https://sagart-cactus.github.io/TriSeek/triseek-architecture.html)

## Release Automation

- CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and release-binary smoke builds on Linux, macOS, and Windows.
- Pushing a tag like `v0.2.1` triggers a release workflow that builds TriSeek archives for Linux, macOS (Intel and Apple Silicon), and Windows and uploads them to GitHub Releases.
