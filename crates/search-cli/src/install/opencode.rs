//! OpenCode installer: registers TriSeek MCP plus Memo plugin.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use crate::install::{current_triseek_binary, shared};

pub fn install() -> Result<()> {
    let binary = current_triseek_binary()?;
    let binary_str = binary.to_string_lossy().into_owned();

    let config_path = shared::opencode_config_path()?;
    let existing = fs::read_to_string(&config_path).ok();
    let merged = shared::upsert_mcp_json(existing.as_deref(), &binary_str, &["mcp", "serve"])
        .with_context(|| {
            format!(
                "failed to merge triseek mcp config into {}",
                config_path.display()
            )
        })?;
    shared::atomic_write(&config_path, &merged)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    let plugin_dir = shared::opencode_plugin_dir()?;
    shared::write_opencode_plugin(&plugin_dir, &binary_str)?;

    println!(
        "triseek: installed MCP server + memo plugin for OpenCode.\nPlugin at: {}/triseek-memo.ts",
        plugin_dir.display()
    );
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let config_path = shared::opencode_config_path()?;
    if let Ok(existing) = fs::read_to_string(&config_path)
        && let Some(updated) = shared::remove_mcp_json(&existing)?
    {
        shared::atomic_write(&config_path, &updated)?;
    }
    let plugin_file = shared::opencode_plugin_dir()?.join("triseek-memo.ts");
    if plugin_file.exists() {
        fs::remove_file(&plugin_file)
            .with_context(|| format!("failed to remove {}", plugin_file.display()))?;
    }
    println!("triseek: removed from OpenCode");
    Ok(())
}

pub fn locate_opencode() -> Option<PathBuf> {
    which::which("opencode").ok()
}
