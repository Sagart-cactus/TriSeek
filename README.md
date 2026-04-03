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
curl -fsSL https://raw.githubusercontent.com/Sagart-cactus/TriSeek/main/scripts/install.sh | sh -s -- --version v0.1.0
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
triseek build --repo /path/to/repo
```

Run a search:

```sh
triseek search --repo /path/to/repo "QueryRequest"
```

Update an existing index after repo changes:

```sh
triseek update --repo /path/to/repo
```

Start the background daemon for repeated searches:

```sh
triseek daemon start --repo /path/to/repo
```

Check the install:

```sh
triseek help
```

## Upgrade and Uninstall

Upgrade by rerunning the installer. To remove TriSeek, delete the installed binary:

- macOS/Linux: `rm -f ~/.local/bin/triseek ~/.local/bin/triseek-server`
- Windows: `Remove-Item "$HOME\\AppData\\Local\\Programs\\TriSeek\\bin\\triseek.exe","$HOME\\AppData\\Local\\Programs\\TriSeek\\bin\\triseek-server.exe"`

TriSeek stores indexes under the repo-specific default index directory. Remove that directory if you also want to delete local index data.

## Docs

- [Installation Guide](docs/install.md)
- [How TriSeek Works](docs/triseek-explained.html)
- [TriSeek Architecture](docs/triseek-architecture.html)

## Release Automation

- CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and release-binary smoke builds on Linux, macOS, and Windows.
- Pushing a tag like `v0.1.0` triggers a release workflow that builds TriSeek archives for Linux, macOS (Intel and Apple Silicon), and Windows and uploads them to GitHub Releases.
