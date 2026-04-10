//! `triseek doctor` — diagnostic checks for the MCP install flow.

use anyhow::Result;
use std::path::PathBuf;

use crate::install::{claude_code, codex, current_triseek_binary, shared};

pub fn run() -> Result<()> {
    println!("triseek doctor\n----");

    // 1. Binary location.
    match current_triseek_binary() {
        Ok(path) => println!("[ok] binary: {}", path.display()),
        Err(err) => println!("[warn] binary: could not resolve ({err})"),
    }

    // 2. Repo root detection.
    match crate::mcp::repo_root::resolve(None) {
        Ok(root) => println!("[ok] repo_root: {}", root.display()),
        Err(err) => println!("[warn] repo_root: {err}"),
    }

    // 3. Claude CLI availability.
    match claude_code::locate_claude() {
        Some(path) => println!("[ok] claude CLI: {}", path.display()),
        None => println!(
            "[info] claude CLI: not found on PATH (use `install claude-code --scope project` instead)"
        ),
    }

    // 4. Codex CLI availability.
    match codex::locate_codex() {
        Some(path) => println!("[ok] codex CLI: {}", path.display()),
        None => println!("[info] codex CLI: not found on PATH"),
    }

    // 5. Project .mcp.json presence.
    if let Ok(path) = shared::project_mcp_json_path() {
        if path.exists() {
            println!("[info] project .mcp.json: {} present", path.display());
        } else {
            println!("[info] project .mcp.json: {} absent", path.display());
        }
    }

    // 6. Codex config.toml presence.
    if let Ok(path) = shared::codex_config_path() {
        if path.exists() {
            println!("[info] codex config: {} present", path.display());
        } else {
            println!("[info] codex config: {} absent", path.display());
        }
    }

    // 7. Index presence for current repo.
    if let Ok(repo) = crate::mcp::repo_root::resolve(None) {
        let index_dir: PathBuf = search_index::default_index_dir(&repo);
        if search_index::index_exists(&index_dir) {
            println!("[ok] triseek index: {} present", index_dir.display());
        } else {
            println!(
                "[warn] triseek index: {} absent (run `triseek build {}`)",
                index_dir.display(),
                repo.display()
            );
        }
    }

    println!("----\ndoctor checks complete.");
    Ok(())
}
