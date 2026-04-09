//! Claude Code installer: registers TriSeek as an MCP server inside the
//! Claude Code CLI.

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::install::{Scope, current_triseek_binary, shared};

pub fn install(scope: Scope) -> Result<()> {
    let binary = current_triseek_binary()?;
    let binary_str = binary.to_string_lossy().into_owned();

    match scope {
        Scope::Project => install_project(&binary_str),
        Scope::Local | Scope::User => install_via_cli(&binary_str, scope),
    }
}

pub fn uninstall(scope: Scope) -> Result<()> {
    match scope {
        Scope::Project => uninstall_project(),
        Scope::Local | Scope::User => uninstall_via_cli(scope),
    }
}

fn install_project(binary: &str) -> Result<()> {
    let path = shared::project_mcp_json_path()?;
    let existing = fs::read_to_string(&path).ok();
    let merged = shared::upsert_mcp_json(existing.as_deref(), binary, &["mcp", "serve"])
        .context("failed to merge triseek into .mcp.json")?;
    shared::atomic_write(&path, &merged)
        .with_context(|| format!("failed to write {}", path.display()))?;
    println!(
        "triseek: installed into {} (project scope)\nNext: reload Claude Code in this workspace and run `claude mcp list` to verify.",
        path.display()
    );
    Ok(())
}

fn uninstall_project() -> Result<()> {
    let path = shared::project_mcp_json_path()?;
    let Ok(existing) = fs::read_to_string(&path) else {
        println!(
            "triseek: no .mcp.json at {} (nothing to remove)",
            path.display()
        );
        return Ok(());
    };
    match shared::remove_mcp_json(&existing)? {
        Some(updated) => {
            shared::atomic_write(&path, &updated)?;
            println!("triseek: removed triseek entry from {}", path.display());
        }
        None => println!("triseek: no triseek entry found in {}", path.display()),
    }
    Ok(())
}

fn install_via_cli(binary: &str, scope: Scope) -> Result<()> {
    let claude = locate_claude().context("claude CLI not found on PATH")?;
    let status = Command::new(&claude)
        .arg("mcp")
        .arg("add")
        .arg("--scope")
        .arg(scope.as_claude_cli_flag())
        .arg("triseek")
        .arg(binary)
        .arg("mcp")
        .arg("serve")
        .status()
        .context("failed to invoke `claude mcp add`")?;
    if !status.success() {
        bail!("`claude mcp add` exited with {status}");
    }
    println!(
        "triseek: registered with Claude Code ({} scope). Verify with `claude mcp list`.",
        scope.as_claude_cli_flag()
    );
    Ok(())
}

fn uninstall_via_cli(scope: Scope) -> Result<()> {
    let claude = locate_claude().context("claude CLI not found on PATH")?;
    let status = Command::new(&claude)
        .arg("mcp")
        .arg("remove")
        .arg("--scope")
        .arg(scope.as_claude_cli_flag())
        .arg("triseek")
        .status()
        .context("failed to invoke `claude mcp remove`")?;
    if !status.success() {
        bail!("`claude mcp remove` exited with {status}");
    }
    println!(
        "triseek: removed from Claude Code ({} scope)",
        scope.as_claude_cli_flag()
    );
    Ok(())
}

pub fn locate_claude() -> Option<PathBuf> {
    which::which("claude").ok()
}
