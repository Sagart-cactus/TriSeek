//! Agent-client installer module.
//!
//! Registers TriSeek as an MCP server inside Claude Code, Codex, OpenCode,
//! and Pi by
//! shelling out to their CLIs when available and falling back to
//! merge-preserving edits to their config files.

pub mod claude_code;
pub mod codex;
pub mod doctor;
pub mod opencode;
pub mod pi;
pub mod shared;

use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Local,
    Project,
    User,
}

impl Scope {
    pub fn as_claude_cli_flag(self) -> &'static str {
        match self {
            Scope::Local => "local",
            Scope::Project => "project",
            Scope::User => "user",
        }
    }
}

/// Absolute path of the currently-running `triseek` binary, used as the
/// command registered into MCP client configs.
pub fn current_triseek_binary() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(exe.canonicalize().unwrap_or(exe))
}
