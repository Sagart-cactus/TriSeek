# TriSeek
High-performance Rust code search engine for Codex, combining indexed search and rg fallback to speed up repeated code discovery in medium to very large repositories.

## Automation
- CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and release-binary smoke builds on Linux, macOS, and Windows.
- Pushing a tag like `v0.1.0` triggers a release workflow that builds `triseek` archives for Linux, macOS (Intel and Apple Silicon), and Windows and uploads them to GitHub Releases.
