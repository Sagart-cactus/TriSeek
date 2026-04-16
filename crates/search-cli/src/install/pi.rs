//! Pi installer: registers TriSeek MCP plus Memo extension.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use crate::install::{current_triseek_binary, shared};

pub fn install() -> Result<()> {
    let binary = current_triseek_binary()?;
    let binary_str = binary.to_string_lossy().into_owned();

    let settings_path = shared::pi_settings_path()?;
    let existing = fs::read_to_string(&settings_path).ok();
    let merged = shared::upsert_mcp_json(existing.as_deref(), &binary_str, &["mcp", "serve"])
        .with_context(|| {
            format!(
                "failed to merge triseek mcp config into {}",
                settings_path.display()
            )
        })?;
    shared::atomic_write(&settings_path, &merged)
        .with_context(|| format!("failed to write {}", settings_path.display()))?;

    let extension_dir = shared::pi_extension_dir()?;
    shared::write_pi_extension(&extension_dir, &binary_str)?;
    println!(
        "triseek: installed MCP server + memo extension for Pi.\nExtension at: {}/index.ts",
        extension_dir.display()
    );
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let settings_path = shared::pi_settings_path()?;
    if let Ok(existing) = fs::read_to_string(&settings_path)
        && let Some(updated) = shared::remove_mcp_json(&existing)?
    {
        shared::atomic_write(&settings_path, &updated)?;
    }
    let extension_dir = shared::pi_extension_dir()?;
    if extension_dir.exists() {
        fs::remove_dir_all(&extension_dir)
            .with_context(|| format!("failed to remove {}", extension_dir.display()))?;
    }
    println!("triseek: removed from Pi");
    Ok(())
}

pub fn locate_pi() -> Option<PathBuf> {
    which::which("pi").ok()
}
