//! `triseek doctor` — diagnostic checks for the MCP install flow.

use anyhow::Result;
use std::path::PathBuf;

use crate::install::{Scope, claude_code, codex, current_triseek_binary, opencode, pi, shared};

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

    // 5. OpenCode / Pi availability.
    match opencode::locate_opencode() {
        Some(path) => println!("[ok] opencode CLI: {}", path.display()),
        None => println!("[info] opencode CLI: not found on PATH"),
    }
    match pi::locate_pi() {
        Some(path) => println!("[ok] pi CLI: {}", path.display()),
        None => println!("[info] pi CLI: not found on PATH"),
    }

    // 6. Project .mcp.json presence.
    if let Ok(path) = shared::project_mcp_json_path() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            match shared::mcp_json_has_triseek(&text) {
                Ok(true) => println!("[ok] project MCP: triseek registered ({})", path.display()),
                Ok(false) => println!("[info] project MCP: triseek absent ({})", path.display()),
                Err(err) => println!(
                    "[warn] project MCP: parse failed ({}): {err}",
                    path.display()
                ),
            }
        } else {
            println!("[info] project .mcp.json: {} absent", path.display());
        }
    }

    // 7. Claude hook files.
    for (scope_name, scope) in [
        ("project", Scope::Project),
        ("local", Scope::Local),
        ("user", Scope::User),
    ] {
        if let Ok(path) = shared::claude_hooks_settings_path(scope) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                match shared::claude_hooks_status(&text) {
                    Ok((post_tool, session_start, pre_compact)) => {
                        let mut missing = Vec::new();
                        if !post_tool {
                            missing.push("PostToolUse");
                        }
                        if !session_start {
                            missing.push("SessionStart");
                        }
                        if !pre_compact {
                            missing.push("PreCompact");
                        }
                        if missing.is_empty() {
                            println!(
                                "[ok] Claude hooks ({scope_name}): PostToolUse, SessionStart, PreCompact"
                            );
                        } else {
                            println!(
                                "[warn] Claude hooks ({scope_name}): missing {} ({})",
                                missing.join(", "),
                                path.display()
                            );
                        }
                    }
                    Err(err) => println!(
                        "[warn] Claude hooks ({scope_name}): parse failed ({}): {err}",
                        path.display()
                    ),
                }
            } else {
                println!(
                    "[info] Claude hooks ({scope_name}): settings file not found ({})",
                    path.display()
                );
            }
        }
    }

    // 8. Codex config + hooks.
    if let Ok(path) = shared::codex_config_path() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            match shared::codex_config_has_triseek(&text) {
                Ok(true) => println!("[ok] Codex MCP: triseek registered"),
                Ok(false) => println!("[info] Codex MCP: triseek not registered"),
                Err(err) => println!("[warn] Codex MCP: parse failed: {err}"),
            }
            match shared::codex_hooks_enabled(&text) {
                Ok(true) => println!("[ok] Codex hooks enabled: codex_hooks = true"),
                Ok(false) => println!("[warn] Codex hooks enabled: codex_hooks missing/false"),
                Err(err) => println!("[warn] Codex hooks enabled: parse failed: {err}"),
            }
        } else {
            println!("[info] codex config: {} absent", path.display());
        }
    }
    if let Ok(path) = shared::codex_hooks_json_path() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            match shared::codex_hooks_status(&text) {
                Ok((post_tool, session_start)) => {
                    let mut missing = Vec::new();
                    if !post_tool {
                        missing.push("PostToolUse");
                    }
                    if !session_start {
                        missing.push("SessionStart");
                    }
                    if missing.is_empty() {
                        println!("[ok] Codex hooks: PostToolUse, SessionStart");
                    } else {
                        println!(
                            "[warn] Codex hooks: missing {} ({})",
                            missing.join(", "),
                            path.display()
                        );
                    }
                }
                Err(err) => println!(
                    "[warn] Codex hooks: parse failed ({}): {err}",
                    path.display()
                ),
            }
        } else {
            println!("[info] Codex hooks file: {} absent", path.display());
        }
    }

    // 9. OpenCode / Pi config and plugin files.
    if let Ok(path) = shared::opencode_config_path() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            match shared::mcp_json_has_triseek(&text) {
                Ok(true) => println!("[ok] OpenCode MCP: triseek registered"),
                Ok(false) => println!("[info] OpenCode MCP: triseek not registered"),
                Err(err) => println!("[warn] OpenCode MCP: parse failed: {err}"),
            }
        } else {
            println!("[info] OpenCode config: {} absent", path.display());
        }
    }
    if let Ok(path) = shared::opencode_plugin_dir().map(|dir| dir.join("triseek-memo.ts")) {
        if path.exists() {
            println!("[ok] OpenCode plugin: {}", path.display());
        } else {
            println!("[info] OpenCode plugin: {} absent", path.display());
        }
    }
    if let Ok(path) = shared::pi_settings_path() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            match shared::mcp_json_has_triseek(&text) {
                Ok(true) => println!("[ok] Pi MCP: triseek registered"),
                Ok(false) => println!("[info] Pi MCP: triseek not registered"),
                Err(err) => println!("[warn] Pi MCP: parse failed: {err}"),
            }
        } else {
            println!("[info] Pi settings: {} absent", path.display());
        }
    }
    if let Ok(path) = shared::pi_extension_dir().map(|dir| dir.join("index.ts")) {
        if path.exists() {
            println!("[ok] Pi extension: {}", path.display());
        } else {
            println!("[info] Pi extension: {} absent", path.display());
        }
    }

    // 10. Index presence for current repo.
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
