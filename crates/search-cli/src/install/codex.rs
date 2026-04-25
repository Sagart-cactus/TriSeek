//! Codex installer: registers TriSeek as an MCP server inside the Codex CLI.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::install::{current_triseek_binary, shared};

pub fn install() -> Result<()> {
    let binary = current_triseek_binary()?;
    let binary_str = binary.to_string_lossy().into_owned();
    install_mcp(&binary_str)?;
    install_hooks(&binary_str)?;
    enable_hooks_flag()?;
    Ok(())
}

fn install_mcp(binary_str: &str) -> Result<()> {
    // Try `codex mcp add` first. If it succeeds, continue to hook setup.
    if let Some(codex) = locate_codex() {
        let output = Command::new(&codex)
            .arg("mcp")
            .arg("add")
            .arg("triseek")
            .arg("--")
            .arg(binary_str)
            .arg("mcp")
            .arg("serve")
            .output();
        match output {
            Ok(out) if out.status.success() => {
                println!(
                    "triseek: registered with Codex via `codex mcp add`. Verify with `codex mcp list`."
                );
                return Ok(());
            }
            Ok(out) => {
                eprintln!(
                    "triseek: `codex mcp add` failed ({}); falling back to config.toml edit",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            Err(err) => {
                eprintln!(
                    "triseek: could not invoke `codex mcp add` ({err}); falling back to config.toml edit"
                );
            }
        }
    }

    // Fall back to editing ~/.codex/config.toml for MCP entry.
    let path = shared::codex_config_path()?;
    let existing = fs::read_to_string(&path).ok();
    let merged = shared::upsert_codex_config(existing.as_deref(), binary_str, &["mcp", "serve"])
        .context("failed to merge triseek into Codex config.toml")?;
    shared::atomic_write(&path, &merged)
        .with_context(|| format!("failed to write {}", path.display()))?;
    println!(
        "triseek: wrote [mcp_servers.triseek] to {}. Restart Codex and run `codex mcp list` to verify.",
        path.display()
    );
    Ok(())
}

pub fn uninstall() -> Result<()> {
    uninstall_mcp()?;
    uninstall_hooks()?;
    Ok(())
}

fn uninstall_mcp() -> Result<()> {
    if let Some(codex) = locate_codex() {
        let output = Command::new(&codex)
            .arg("mcp")
            .arg("remove")
            .arg("triseek")
            .output();
        if let Ok(out) = output
            && out.status.success()
        {
            println!("triseek: removed from Codex via `codex mcp remove`.");
            return Ok(());
        }
    }
    let path = shared::codex_config_path()?;
    let Ok(existing) = fs::read_to_string(&path) else {
        println!(
            "triseek: no Codex config at {} (nothing to remove)",
            path.display()
        );
        return Ok(());
    };
    match shared::remove_codex_config(&existing)? {
        Some(updated) => {
            shared::atomic_write(&path, &updated)?;
            println!("triseek: removed triseek entry from {}", path.display());
        }
        None => println!("triseek: no triseek entry in {}", path.display()),
    }
    Ok(())
}

fn install_hooks(binary: &str) -> Result<()> {
    let path = shared::codex_hooks_json_path()?;
    let existing = fs::read_to_string(&path).ok();
    let merged = shared::upsert_codex_hooks(existing.as_deref(), binary)
        .with_context(|| format!("failed to merge memo hooks into {}", path.display()))?;
    shared::atomic_write(&path, &merged)
        .with_context(|| format!("failed to write {}", path.display()))?;
    println!("triseek: memo hooks installed into {}", path.display());
    println!(
        "triseek: Codex hooks are installed for Bash reads and MCP file-read tool calls when supported by your Codex version."
    );
    println!(
        "triseek: Bash-based shell reads are observed automatically when Codex emits parsed command metadata."
    );
    println!(
        "triseek: redundant Bash and MCP file rereads can be blocked via PreToolUse when Memo proves the file is unchanged."
    );
    println!(
        "triseek: use `mcp__triseek__memo_check {{\"path\":\"<file>\"}}` before re-reading files through any non-hooked tool."
    );
    println!("triseek: see docs/codex-memo-skill.md for the full decision table.");
    Ok(())
}

fn uninstall_hooks() -> Result<()> {
    let path = shared::codex_hooks_json_path()?;
    let Ok(existing) = fs::read_to_string(&path) else {
        println!(
            "triseek: no Codex hooks file at {} (nothing to remove)",
            path.display()
        );
        return Ok(());
    };
    match shared::remove_codex_hooks(&existing)? {
        Some(updated) => {
            shared::atomic_write(&path, &updated)?;
            println!("triseek: removed memo hooks from {}", path.display());
        }
        None => println!("triseek: no memo hooks found in {}", path.display()),
    }
    Ok(())
}

fn enable_hooks_flag() -> Result<()> {
    let path = shared::codex_config_path()?;
    let existing = fs::read_to_string(&path).ok();
    let updated = shared::ensure_codex_hooks_enabled(existing.as_deref())
        .context("failed to enable codex_hooks feature flag in Codex config.toml")?;
    shared::atomic_write(&path, &updated)
        .with_context(|| format!("failed to write {}", path.display()))?;
    println!("triseek: enabled Codex feature flag `codex_hooks = true`");
    Ok(())
}

pub fn locate_codex() -> Option<PathBuf> {
    which::which("codex").ok()
}
