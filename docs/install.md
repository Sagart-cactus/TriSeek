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

Pin a version:

```sh
curl -fsSL https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.sh | sh -s -- --version v0.2.1
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
```

## Quick Start

Build an index:

```sh
triseek build /path/to/repo
```

Search a repository:

```sh
triseek "QueryRequest" /path/to/repo
```

Refresh the index after changes:

```sh
triseek update /path/to/repo
```

Run the background daemon for repeated searches:

```sh
triseek daemon start
```

## Upgrade

Rerun the installer command you used originally.

## Uninstall

- macOS/Linux: `rm -f ~/.local/bin/triseek ~/.local/bin/triseek-server`
- Windows: `Remove-Item "$HOME\\AppData\\Local\\Programs\\TriSeek\\bin\\triseek.exe","$HOME\\AppData\\Local\\Programs\\TriSeek\\bin\\triseek-server.exe"`

If you also want to remove local index data, delete `~/.triseek`.
