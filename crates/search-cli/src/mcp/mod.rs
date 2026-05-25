//! MCP (Model Context Protocol) server for TriSeek.
//!
//! Exposes TriSeek as a local stdio MCP server that Claude Code, Codex, and
//! any other MCP-capable client can use as their primary code-search tool.

pub mod errors;
pub mod query_cache;
pub mod repo_root;
pub mod schema;
pub mod search_memo;
pub mod server;
pub mod tools;

use anyhow::{Result, bail};
use std::path::Path;

/// Entry point invoked from `triseek mcp serve`.
pub fn serve(repo: Option<&Path>, index_dir: Option<&Path>) -> Result<()> {
    let resolved_repo = repo_root::resolve_startup(repo)?;
    if index_dir.is_some() && resolved_repo.is_none() {
        bail!("--index-dir requires --repo, TRISEEK_REPO_ROOT, or a safe git root");
    }
    server::run(resolved_repo, index_dir)
}
