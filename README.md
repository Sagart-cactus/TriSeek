# TriSeek

TriSeek is a high-performance local code search CLI written in Rust. It combines a trigram index with shell-tool fallback so repeated searches stay fast on medium and large repositories without giving up flexible query behavior.

## Install

### macOS and Linux

```sh
curl -fsSL https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.sh | sh
```

By default this installs `triseek` and `triseek-server` into `~/.local/bin`. It prefers prebuilt GitHub Release archives and falls back to `cargo install` when a matching release is not available but Rust is installed locally.

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

Build an index for a repository:

```sh
triseek build /path/to/repo
```

Run a search:

```sh
triseek "QueryRequest" /path/to/repo
```

`triseek search "QueryRequest" /path/to/repo` remains supported as a compatibility alias.

Update an existing index after repo changes:

```sh
triseek update /path/to/repo
```

Start the background daemon for repeated searches:

```sh
triseek daemon start
```

Check the install:

```sh
triseek help
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
triseek mcp serve --repo /path/to/repo
```

All logs go to stderr; stdout carries framed JSON-RPC messages only.

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

Full input/output schemas and error codes live in [docs/mcp.md](docs/mcp.md).

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

- [Installation Guide](docs/install.md)
- [How TriSeek Works](docs/triseek-explained.html)
- [TriSeek Architecture](docs/triseek-architecture.html)

## Release Automation

- CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and release-binary smoke builds on Linux, macOS, and Windows.
- Pushing a tag like `v0.2.1` triggers a release workflow that builds TriSeek archives for Linux, macOS (Intel and Apple Silicon), and Windows and uploads them to GitHub Releases.
