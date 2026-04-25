# TriSeek Installation Guide

TriSeek has two installation paths:

1. Prebuilt GitHub Release archives for the supported platforms.
2. `cargo install` as the fallback when no matching release archive exists yet.

## Recommended Install

### macOS and Linux

```sh
curl -fsSL https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.sh | sh
```

Default install location:

```text
~/.local/bin/triseek
~/.local/bin/triseek-server
```

The installer also ensures the TriSeek daemon is running. Fresh installs start it, and reinstalls stop and restart it.

Pin a version:

```sh
curl -fsSL https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.sh | sh -s -- --version v0.3.3
```

Install to a custom directory:

```sh
curl -fsSL https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.sh | sh -s -- --install-dir /usr/local/bin
```

### Windows PowerShell

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.ps1 | iex"
```

Default install location:

```text
%USERPROFILE%\AppData\Local\Programs\TriSeek\bin\triseek.exe
%USERPROFILE%\AppData\Local\Programs\TriSeek\bin\triseek-server.exe
```

The PowerShell installer updates the user `PATH` automatically unless you run it with `-SkipPathUpdate`.

It also ensures the TriSeek daemon is running. Fresh installs start it, and reinstalls stop and restart it.

## Cargo Fallback

If no matching GitHub Release archive exists yet, the installers fall back to `cargo install` when Rust is available locally.

You can also install with Cargo directly:

```sh
cargo install --git https://github.com/Sagart-cactus/TriSeek.git triseek --locked
cargo install --git https://github.com/Sagart-cactus/TriSeek.git search-server --locked
```

From a local checkout:

```sh
cargo install --path crates/search-cli --locked
cargo install --path crates/search-server --locked
```

## Verify the Install

```sh
triseek help
triseek doctor
```

## Release-Style Validation

TriSeek includes a Docker real harness for testing release binaries without
touching your real daemon or agent configs:

```sh
scripts/run_real_harness_docker.sh
```

The harness builds `triseek` and `triseek-server`, runs CLI smoke checks, starts
an isolated daemon, drives MCP stdio tools, verifies search reuse invalidation,
and checks install/uninstall config generation for Claude Code, Codex, OpenCode,
and Pi. To include a large-repo smoke test:

```sh
TRISEEK_LARGE_REPO=/path/to/large/repo scripts/run_real_harness_docker.sh
```

## Agent Client Installs

TriSeek can register itself as an MCP server and install Memo helpers for
supported agent clients:

```sh
triseek install claude-code
triseek install codex
triseek install opencode
triseek install pi
```

Claude Code, OpenCode, and Pi get passive Memo observation via hooks or
plugins. Codex gets the same MCP install plus `PreToolUse`, `PostToolUse`, and
`SessionStart` hooks for supported Bash and MCP file-read tools, with
active-mode guidance for any remaining non-hooked reads via `memo_check`.

## Quick Start

Search immediately:

```sh
triseek "AuthConfig" /path/to/repo
```

Build an index when you want repeated searches to stay fast:

```sh
triseek build /path/to/repo
triseek "AuthConfig" /path/to/repo
```

Refresh the index after changes:

```sh
triseek update /path/to/repo
```

Run the background daemon for repeated searches:

```sh
triseek daemon start
```

TriSeek stores default state under `~/.triseek`, not in the searched repo root.

## Upgrade

Rerun the installer command you used originally.

## Uninstall

- macOS/Linux: `rm -f ~/.local/bin/triseek ~/.local/bin/triseek-server`
- Windows: `Remove-Item "$HOME\\AppData\\Local\\Programs\\TriSeek\\bin\\triseek.exe","$HOME\\AppData\\Local\\Programs\\TriSeek\\bin\\triseek-server.exe"`

If you also want to remove local index data, delete `~/.triseek`.
