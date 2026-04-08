//! Helpers shared between the Claude Code and Codex installers.
//!
//! Provides atomic file writes (tempfile → rename) and merge-preserving
//! edits to JSON and TOML config files. Merge-preserving means: unrelated
//! keys and servers are left untouched, only the `triseek` entry is added,
//! updated, or removed.

use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Write `contents` to `path` atomically: write to a sibling tempfile first,
/// then rename into place. Creates parent directories as needed.
pub fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or_else(|| Path::new(".")))
        .context("failed to create temp file")?;
    tmp.write_all(contents.as_bytes())
        .context("failed to write temp file contents")?;
    tmp.flush().context("failed to flush temp file")?;
    tmp.persist(path)
        .map_err(|err| anyhow::anyhow!("failed to persist {}: {err}", path.display()))?;
    Ok(())
}

/// Merge a `triseek` MCP entry into a `.mcp.json`-shaped document.
/// Preserves any existing sibling servers. Returns the serialized result.
pub fn upsert_mcp_json(existing: Option<&str>, command: &str, args: &[&str]) -> Result<String> {
    let mut root: Value = match existing {
        Some(text) if !text.trim().is_empty() => {
            serde_json::from_str(text).context("failed to parse existing .mcp.json")?
        }
        _ => Value::Object(Map::new()),
    };
    if !root.is_object() {
        anyhow::bail!("existing .mcp.json is not a JSON object");
    }
    let obj = root.as_object_mut().unwrap();
    let servers_entry = obj
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !servers_entry.is_object() {
        anyhow::bail!("existing .mcp.json `mcpServers` is not an object");
    }
    let servers = servers_entry.as_object_mut().unwrap();
    servers.insert(
        "triseek".to_string(),
        serde_json::json!({
            "command": command,
            "args": args,
            "env": {}
        }),
    );
    Ok(serde_json::to_string_pretty(&root)? + "\n")
}

/// Remove the `triseek` entry from a `.mcp.json`-shaped document.
/// Returns the serialized result, or `None` if the file did not contain a
/// triseek entry (callers can skip writing).
pub fn remove_mcp_json(existing: &str) -> Result<Option<String>> {
    let mut root: Value =
        serde_json::from_str(existing).context("failed to parse existing .mcp.json")?;
    let Some(obj) = root.as_object_mut() else {
        return Ok(None);
    };
    let Some(servers) = obj.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    if servers.remove("triseek").is_none() {
        return Ok(None);
    }
    Ok(Some(serde_json::to_string_pretty(&root)? + "\n"))
}

/// Merge a `[mcp_servers.triseek]` block into a Codex `config.toml`.
/// Uses `toml_edit` to preserve comments and formatting on unrelated entries.
pub fn upsert_codex_config(existing: Option<&str>, command: &str, args: &[&str]) -> Result<String> {
    use toml_edit::{Array, DocumentMut, Item, Table, value};

    let mut doc: DocumentMut = match existing {
        Some(text) if !text.trim().is_empty() => text
            .parse()
            .context("failed to parse existing Codex config.toml")?,
        _ => DocumentMut::new(),
    };

    // Ensure `[mcp_servers]` table exists.
    if !doc.contains_key("mcp_servers") {
        doc["mcp_servers"] = Item::Table(Table::new());
    }
    let mcp_servers = doc["mcp_servers"]
        .as_table_mut()
        .context("`mcp_servers` is not a table in Codex config")?;
    mcp_servers.set_implicit(true);

    // Replace or insert the triseek subtable.
    let mut triseek_table = Table::new();
    triseek_table["command"] = value(command);
    let mut args_array = Array::new();
    for arg in args {
        args_array.push(*arg);
    }
    triseek_table["args"] = value(args_array);
    mcp_servers.insert("triseek", Item::Table(triseek_table));

    Ok(doc.to_string())
}

/// Remove `[mcp_servers.triseek]` from a Codex `config.toml`. Returns
/// `None` if the entry was absent.
pub fn remove_codex_config(existing: &str) -> Result<Option<String>> {
    use toml_edit::DocumentMut;

    let mut doc: DocumentMut = existing
        .parse()
        .context("failed to parse existing Codex config.toml")?;
    let Some(servers) = doc
        .get_mut("mcp_servers")
        .and_then(|item| item.as_table_mut())
    else {
        return Ok(None);
    };
    if servers.remove("triseek").is_none() {
        return Ok(None);
    }
    Ok(Some(doc.to_string()))
}

/// Default Codex config path: `~/.codex/config.toml`.
pub fn codex_config_path() -> Result<PathBuf> {
    let home = home_dir().context("failed to resolve user home directory")?;
    Ok(home.join(".codex").join("config.toml"))
}

/// Default project `.mcp.json` path: `<cwd>/.mcp.json`.
pub fn project_mcp_json_path() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to read current working directory")?;
    Ok(cwd.join(".mcp.json"))
}

/// Minimal home-directory resolver that works on all tier-1 platforms
/// without pulling in `dirs` just for this.
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| {
                let drive = std::env::var_os("HOMEDRIVE")?;
                let path = std::env::var_os("HOMEPATH")?;
                let mut combined = PathBuf::from(drive);
                combined.push(path);
                Some(combined)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_creates_mcp_servers_object() {
        let out = upsert_mcp_json(None, "/bin/triseek", &["mcp", "serve"]).unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        let servers = parsed.get("mcpServers").unwrap().as_object().unwrap();
        assert!(servers.contains_key("triseek"));
        let triseek = servers.get("triseek").unwrap();
        assert_eq!(triseek.get("command").unwrap(), "/bin/triseek");
        assert_eq!(
            triseek.get("args").unwrap(),
            &serde_json::json!(["mcp", "serve"])
        );
    }

    #[test]
    fn upsert_preserves_other_servers() {
        let existing = r#"{"mcpServers":{"other":{"command":"foo","args":[]}}}"#;
        let out = upsert_mcp_json(Some(existing), "/bin/triseek", &["mcp", "serve"]).unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        let servers = parsed.get("mcpServers").unwrap().as_object().unwrap();
        assert!(servers.contains_key("other"));
        assert!(servers.contains_key("triseek"));
    }

    #[test]
    fn remove_mcp_json_preserves_other_servers() {
        let existing = r#"{"mcpServers":{"other":{"command":"foo","args":[]},"triseek":{"command":"x","args":[]}}}"#;
        let out = remove_mcp_json(existing).unwrap().unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        let servers = parsed.get("mcpServers").unwrap().as_object().unwrap();
        assert!(servers.contains_key("other"));
        assert!(!servers.contains_key("triseek"));
    }

    #[test]
    fn remove_mcp_json_returns_none_when_absent() {
        let existing = r#"{"mcpServers":{"other":{"command":"foo","args":[]}}}"#;
        assert!(remove_mcp_json(existing).unwrap().is_none());
    }

    #[test]
    fn upsert_codex_config_preserves_other_tables() {
        let existing = r#"# top comment
[model]
name = "gpt-5"

[mcp_servers.other]
command = "/bin/other"
args = []
"#;
        let out = upsert_codex_config(Some(existing), "/bin/triseek", &["mcp", "serve"]).unwrap();
        assert!(out.contains("top comment"), "preserves comments");
        assert!(out.contains("[model]"));
        assert!(out.contains("[mcp_servers.other]"));
        assert!(out.contains("[mcp_servers.triseek]"));
        assert!(out.contains("\"/bin/triseek\""));
    }

    #[test]
    fn remove_codex_config_preserves_other_entries() {
        let existing = r#"[mcp_servers.other]
command = "/bin/other"
args = []

[mcp_servers.triseek]
command = "/bin/triseek"
args = ["mcp", "serve"]
"#;
        let out = remove_codex_config(existing).unwrap().unwrap();
        assert!(out.contains("[mcp_servers.other]"));
        assert!(!out.contains("triseek"));
    }
}
