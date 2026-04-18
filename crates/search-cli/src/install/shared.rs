//! Helpers shared between installer entrypoints.
//!
//! Provides atomic writes, merge-preserving edits to JSON/TOML configs, and
//! small utilities for writing harness-specific plugin/extension files.

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::install::Scope;

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

/// Merge a `triseek` MCP entry into a JSON config containing `mcpServers`.
pub fn upsert_mcp_json(existing: Option<&str>, command: &str, args: &[&str]) -> Result<String> {
    let mut root: Value = parse_json_root(existing, "failed to parse existing mcp config")?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("existing mcp config is not a JSON object"))?;
    let servers_entry = obj
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !servers_entry.is_object() {
        bail!("existing mcp config `mcpServers` is not an object");
    }
    let servers = servers_entry.as_object_mut().unwrap();
    servers.insert(
        "triseek".to_string(),
        json!({
            "command": command,
            "args": args,
            "env": {}
        }),
    );
    Ok(serde_json::to_string_pretty(&root)? + "\n")
}

/// Remove the `triseek` entry from a JSON config containing `mcpServers`.
pub fn remove_mcp_json(existing: &str) -> Result<Option<String>> {
    let mut root: Value =
        serde_json::from_str(existing).context("failed to parse existing mcp config")?;
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

pub fn mcp_json_has_triseek(existing: &str) -> Result<bool> {
    let root: Value =
        serde_json::from_str(existing).context("failed to parse existing mcp config")?;
    Ok(root
        .get("mcpServers")
        .and_then(Value::as_object)
        .map(|servers| servers.contains_key("triseek"))
        .unwrap_or(false))
}

/// Merge a `[mcp_servers.triseek]` block into a Codex `config.toml`.
pub fn upsert_codex_config(existing: Option<&str>, command: &str, args: &[&str]) -> Result<String> {
    use toml_edit::{Array, DocumentMut, Item, Table, value};

    let mut doc: DocumentMut = match existing {
        Some(text) if !text.trim().is_empty() => text
            .parse()
            .context("failed to parse existing Codex config.toml")?,
        _ => DocumentMut::new(),
    };

    if !doc.contains_key("mcp_servers") {
        doc["mcp_servers"] = Item::Table(Table::new());
    }
    let mcp_servers = doc["mcp_servers"]
        .as_table_mut()
        .context("`mcp_servers` is not a table in Codex config")?;
    mcp_servers.set_implicit(true);

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

pub fn codex_config_has_triseek(existing: &str) -> Result<bool> {
    use toml_edit::DocumentMut;
    let doc: DocumentMut = existing
        .parse()
        .context("failed to parse existing Codex config.toml")?;
    Ok(doc
        .get("mcp_servers")
        .and_then(|item| item.as_table())
        .map(|table| table.contains_key("triseek"))
        .unwrap_or(false))
}

pub fn ensure_codex_hooks_enabled(existing: Option<&str>) -> Result<String> {
    use toml_edit::{DocumentMut, Item, Table, value};
    let mut doc: DocumentMut = match existing {
        Some(text) if !text.trim().is_empty() => text
            .parse()
            .context("failed to parse existing Codex config.toml")?,
        _ => DocumentMut::new(),
    };
    if !doc.contains_key("features") {
        doc["features"] = Item::Table(Table::new());
    }
    let features = doc["features"]
        .as_table_mut()
        .context("`features` is not a table in Codex config")?;
    features["codex_hooks"] = value(true);
    Ok(doc.to_string())
}

pub fn codex_hooks_enabled(existing: &str) -> Result<bool> {
    use toml_edit::DocumentMut;
    let doc: DocumentMut = existing
        .parse()
        .context("failed to parse existing Codex config.toml")?;
    Ok(doc
        .get("features")
        .and_then(|item| item.as_table())
        .and_then(|table| table.get("codex_hooks"))
        .and_then(|item| item.as_bool())
        .unwrap_or(false))
}

pub fn upsert_claude_hooks(existing: Option<&str>, binary: &str) -> Result<String> {
    let mut root = parse_json_root(existing, "failed to parse existing Claude settings.json")?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("existing Claude settings root is not an object"))?;
    let hooks = obj
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("existing Claude `hooks` is not an object"))?;

    let command = quote_for_shell(binary);
    upsert_claude_hook_event(
        hooks,
        "PreToolUse",
        "Read",
        &format!("{command} memo-observe --event pre-tool-use"),
        false,
    )?;
    upsert_claude_hook_event(
        hooks,
        "PostToolUse",
        "Read|Edit|Write|NotebookEdit",
        &format!("{command} memo-observe --event post-tool-use"),
        true,
    )?;
    upsert_claude_hook_event(
        hooks,
        "SessionStart",
        "",
        &format!("{command} memo-observe --event session-start"),
        true,
    )?;
    upsert_claude_hook_event(
        hooks,
        "PreCompact",
        "",
        &format!("{command} memo-observe --event pre-compact"),
        true,
    )?;

    Ok(serde_json::to_string_pretty(&root)? + "\n")
}

pub fn remove_claude_hooks(existing: &str) -> Result<Option<String>> {
    let mut root: Value =
        serde_json::from_str(existing).context("failed to parse existing Claude settings.json")?;
    let Some(obj) = root.as_object_mut() else {
        return Ok(None);
    };
    let Some(hooks) = obj.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    let mut changed = false;
    for event in ["PreToolUse", "PostToolUse", "SessionStart", "PreCompact"] {
        if let Some(groups) = hooks.get_mut(event).and_then(Value::as_array_mut) {
            let before = groups.len();
            groups.retain(|group| !is_triseek_hook_group(group));
            if groups.len() != before {
                changed = true;
            }
        }
    }
    if !changed {
        return Ok(None);
    }
    Ok(Some(serde_json::to_string_pretty(&root)? + "\n"))
}

pub fn claude_hooks_status(existing: &str) -> Result<(bool, bool, bool, bool)> {
    let root: Value =
        serde_json::from_str(existing).context("failed to parse existing Claude settings.json")?;
    let Some(hooks) = root.get("hooks").and_then(Value::as_object) else {
        return Ok((false, false, false, false));
    };
    let pre_tool = has_triseek_hook_in_event(hooks, "PreToolUse");
    let post_tool = has_triseek_hook_in_event(hooks, "PostToolUse");
    let session_start = has_triseek_hook_in_event(hooks, "SessionStart");
    let pre_compact = has_triseek_hook_in_event(hooks, "PreCompact");
    Ok((pre_tool, post_tool, session_start, pre_compact))
}

pub fn upsert_codex_hooks(existing: Option<&str>, binary: &str) -> Result<String> {
    let mut root = parse_json_root(existing, "failed to parse existing Codex hooks.json")?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("existing Codex hooks root is not an object"))?;
    let hooks = obj
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("existing Codex `hooks` is not an object"))?;

    upsert_codex_hook_event(
        hooks,
        "PostToolUse",
        "Bash|Read|Edit|Write",
        &[binary, "memo-observe", "--event", "post-tool-use"],
    )?;
    upsert_codex_hook_event(
        hooks,
        "SessionStart",
        "startup|resume",
        &[binary, "memo-observe", "--event", "session-start"],
    )?;

    Ok(serde_json::to_string_pretty(&root)? + "\n")
}

pub fn remove_codex_hooks(existing: &str) -> Result<Option<String>> {
    let mut root: Value =
        serde_json::from_str(existing).context("failed to parse existing Codex hooks.json")?;
    let Some(obj) = root.as_object_mut() else {
        return Ok(None);
    };
    let Some(hooks) = obj.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    let mut changed = false;
    for event in ["PostToolUse", "SessionStart"] {
        if let Some(groups) = hooks.get_mut(event).and_then(Value::as_array_mut) {
            let before = groups.len();
            groups.retain(|group| !is_triseek_hook_group(group));
            if groups.len() != before {
                changed = true;
            }
        }
    }
    if !changed {
        return Ok(None);
    }
    Ok(Some(serde_json::to_string_pretty(&root)? + "\n"))
}

pub fn codex_hooks_status(existing: &str) -> Result<(bool, bool)> {
    let root: Value =
        serde_json::from_str(existing).context("failed to parse existing Codex hooks.json")?;
    let Some(hooks) = root.get("hooks").and_then(Value::as_object) else {
        return Ok((false, false));
    };
    let post_tool = has_triseek_hook_in_event(hooks, "PostToolUse");
    let session_start = has_triseek_hook_in_event(hooks, "SessionStart");
    Ok((post_tool, session_start))
}

pub fn write_opencode_plugin(plugin_dir: &Path, binary: &str) -> Result<()> {
    fs::create_dir_all(plugin_dir)
        .with_context(|| format!("failed to create {}", plugin_dir.display()))?;
    let escaped_bin = escape_js_string(binary);
    let contents = format!(
        "import type {{ Plugin }} from \"@opencode/plugin\";\n\
         import {{ execFileSync }} from \"child_process\";\n\n\
         const TRISEEK_BIN = '{escaped_bin}';\n\n\
         export const TriseekMemo: Plugin = async (ctx) => {{\n\
           return {{\n\
             \"tool.execute.after\": async (input, output) => {{\n\
               const tool = String(input?.tool ?? '').toLowerCase();\n\
               if (!tool || !['read', 'edit', 'write', 'apply_patch'].includes(tool)) return;\n\
               try {{\n\
                 const payload = JSON.stringify({{\n\
                   session_id: String(process.pid),\n\
                   hook_event_name: 'PostToolUse',\n\
                   tool_name: input.tool,\n\
                   tool_input: output?.args ?? input?.args ?? {{}},\n\
                   cwd: ctx.directory,\n\
                 }});\n\
                 execFileSync(TRISEEK_BIN, ['memo-observe', '--event', 'post-tool-use'], {{\n\
                   input: payload,\n\
                   timeout: 5000,\n\
                   stdio: ['pipe', 'ignore', 'ignore'],\n\
                 }});\n\
               }} catch {{\n\
                 // best-effort\n\
               }}\n\
             }},\n\
           }};\n\
         }};\n"
    );
    atomic_write(&plugin_dir.join("triseek-memo.ts"), &contents)
}

pub fn write_pi_extension(extension_dir: &Path, binary: &str) -> Result<()> {
    fs::create_dir_all(extension_dir)
        .with_context(|| format!("failed to create {}", extension_dir.display()))?;
    let escaped_bin = escape_js_string(binary);
    let contents = format!(
        "import {{ execFileSync }} from 'child_process';\n\n\
         const TRISEEK_BIN = '{escaped_bin}';\n\n\
         export default function activate(pi: any) {{\n\
           let sessionId: string | undefined;\n\n\
           pi.on('session_start', async (event: any) => {{\n\
             sessionId = event?.sessionId ?? String(process.pid);\n\
             try {{\n\
               const payload = JSON.stringify({{\n\
                 session_id: sessionId,\n\
                 hook_event_name: 'SessionStart',\n\
                 cwd: process.cwd(),\n\
               }});\n\
               execFileSync(TRISEEK_BIN, ['memo-observe', '--event', 'session-start'], {{\n\
                 input: payload,\n\
                 timeout: 5000,\n\
                 stdio: ['pipe', 'ignore', 'ignore'],\n\
               }});\n\
             }} catch {{\n\
               // best-effort\n\
             }}\n\
           }});\n\n\
           pi.on('tool_result', async (event: any) => {{\n\
             const tool = String(event?.toolName ?? '').toLowerCase();\n\
             if (!tool || !['read', 'edit', 'write', 'bash'].includes(tool)) return;\n\
             try {{\n\
               const payload = JSON.stringify({{\n\
                 session_id: sessionId ?? String(process.pid),\n\
                 hook_event_name: 'PostToolUse',\n\
                 tool_name: event.toolName,\n\
                 tool_input: event.input ?? {{}},\n\
                 tool_response: event.result ?? {{}},\n\
                 cwd: process.cwd(),\n\
               }});\n\
               execFileSync(TRISEEK_BIN, ['memo-observe', '--event', 'post-tool-use'], {{\n\
                 input: payload,\n\
                 timeout: 5000,\n\
                 stdio: ['pipe', 'ignore', 'ignore'],\n\
               }});\n\
             }} catch {{\n\
               // best-effort\n\
             }}\n\
           }});\n\n\
           pi.on('session_before_compact', async () => {{\n\
             try {{\n\
               const payload = JSON.stringify({{\n\
                 session_id: sessionId ?? String(process.pid),\n\
                 hook_event_name: 'PreCompact',\n\
                 cwd: process.cwd(),\n\
               }});\n\
               execFileSync(TRISEEK_BIN, ['memo-observe', '--event', 'pre-compact'], {{\n\
                 input: payload,\n\
                 timeout: 5000,\n\
                 stdio: ['pipe', 'ignore', 'ignore'],\n\
               }});\n\
             }} catch {{\n\
               // best-effort\n\
             }}\n\
           }});\n\
         }}\n"
    );
    atomic_write(&extension_dir.join("index.ts"), &contents)
}

pub fn codex_config_path() -> Result<PathBuf> {
    let home = home_dir().context("failed to resolve user home directory")?;
    Ok(home.join(".codex").join("config.toml"))
}

pub fn codex_hooks_json_path() -> Result<PathBuf> {
    let home = home_dir().context("failed to resolve user home directory")?;
    Ok(home.join(".codex").join("hooks.json"))
}

pub fn project_mcp_json_path() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to read current working directory")?;
    Ok(cwd.join(".mcp.json"))
}

pub fn claude_hooks_settings_path(scope: Scope) -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to read current working directory")?;
    let home = home_dir().context("failed to resolve user home directory")?;
    Ok(match scope {
        Scope::Project => cwd.join(".claude").join("settings.json"),
        Scope::Local => cwd.join(".claude").join("settings.local.json"),
        Scope::User => home.join(".claude").join("settings.json"),
    })
}

pub fn opencode_config_path() -> Result<PathBuf> {
    let config_home = config_home_dir()?;
    Ok(config_home.join("opencode").join("opencode.json"))
}

pub fn opencode_plugin_dir() -> Result<PathBuf> {
    let config_home = config_home_dir()?;
    Ok(config_home.join("opencode").join("plugins"))
}

pub fn pi_settings_path() -> Result<PathBuf> {
    let home = home_dir().context("failed to resolve user home directory")?;
    Ok(home.join(".pi").join("agent").join("settings.json"))
}

pub fn pi_extension_dir() -> Result<PathBuf> {
    let home = home_dir().context("failed to resolve user home directory")?;
    Ok(home
        .join(".pi")
        .join("agent")
        .join("extensions")
        .join("triseek-memo"))
}

/// Minimal home-directory resolver without extra dependencies.
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

pub fn config_home_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME")
        && !path.is_empty()
    {
        return Ok(PathBuf::from(path));
    }
    let home = home_dir().context("failed to resolve user home directory")?;
    Ok(home.join(".config"))
}

fn parse_json_root(existing: Option<&str>, parse_context: &str) -> Result<Value> {
    match existing {
        Some(text) if !text.trim().is_empty() => {
            serde_json::from_str(text).with_context(|| parse_context.to_string())
        }
        _ => Ok(Value::Object(Map::new())),
    }
}

fn upsert_claude_hook_event(
    hooks: &mut Map<String, Value>,
    event: &str,
    matcher: &str,
    command: &str,
    is_async: bool,
) -> Result<()> {
    let groups = hooks
        .entry(event.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let groups = groups
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("Claude hooks.{event} is not an array"))?;
    groups.retain(|group| !is_triseek_hook_group(group));
    groups.push(json!({
        "matcher": matcher,
        "hooks": [{
            "type": "command",
            "command": command,
            "timeout": 5,
            "async": is_async
        }]
    }));
    Ok(())
}

fn upsert_codex_hook_event(
    hooks: &mut Map<String, Value>,
    event: &str,
    matcher: &str,
    command: &[&str],
) -> Result<()> {
    let groups = hooks
        .entry(event.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let groups = groups
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("Codex hooks.{event} is not an array"))?;
    groups.retain(|group| !is_triseek_hook_group(group));
    groups.push(json!({
        "matcher": matcher,
        "hooks": [{
            "type": "command",
            "command": command,
            "timeout": 5000
        }]
    }));
    Ok(())
}

fn has_triseek_hook_in_event(hooks: &Map<String, Value>, event: &str) -> bool {
    hooks
        .get(event)
        .and_then(Value::as_array)
        .map(|groups| groups.iter().any(is_triseek_hook_group))
        .unwrap_or(false)
}

fn is_triseek_hook_group(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .map(|hooks| {
            hooks.iter().any(|hook| {
                let Some(command) = hook.get("command") else {
                    return false;
                };
                match command {
                    Value::String(value) => {
                        value.contains("memo-observe") || value.contains("triseek")
                    }
                    Value::Array(values) => values.iter().any(|entry| {
                        entry
                            .as_str()
                            .map(|text| text.contains("memo-observe") || text.contains("triseek"))
                            .unwrap_or(false)
                    }),
                    _ => false,
                }
            })
        })
        .unwrap_or(false)
}

fn quote_for_shell(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\"'\"'"))
}

fn escape_js_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

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
        assert!(out.contains("top comment"));
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

    #[test]
    fn upsert_and_remove_claude_hooks_preserves_unrelated_entries() {
        let existing = r#"{
  "hooks": {
    "PostToolUse": [{
      "matcher": "Other",
      "hooks": [{"type":"command","command":"echo other"}]
    }]
  }
}"#;
        let out = upsert_claude_hooks(Some(existing), "/bin/triseek").unwrap();
        let (pre, post, start, compact) = claude_hooks_status(&out).unwrap();
        assert!(pre && post && start && compact);
        assert!(out.contains("\"PreToolUse\""));
        assert!(out.contains("\"matcher\": \"Read\""));
        assert!(out.contains("\"async\": false"));
        assert!(out.contains("echo other"));

        let removed = remove_claude_hooks(&out).unwrap().unwrap();
        assert!(removed.contains("echo other"));
        let status = claude_hooks_status(&removed).unwrap();
        assert_eq!(status, (false, false, false, false));
    }

    #[test]
    fn upsert_and_remove_codex_hooks_round_trip() {
        let out = upsert_codex_hooks(None, "/bin/triseek").unwrap();
        assert_eq!(codex_hooks_status(&out).unwrap(), (true, true));
        assert!(out.contains("\"matcher\": \"Bash|Read|Edit|Write\""));
        let removed = remove_codex_hooks(&out).unwrap().unwrap();
        assert_eq!(codex_hooks_status(&removed).unwrap(), (false, false));
    }

    #[test]
    fn ensure_codex_hooks_enabled_sets_feature_flag() {
        let out = ensure_codex_hooks_enabled(Some("[features]\nfoo=true\n")).unwrap();
        assert!(codex_hooks_enabled(&out).unwrap());
        assert!(out.contains("foo"));
    }

    #[test]
    fn write_opencode_plugin_emits_expected_contract() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("plugins");
        write_opencode_plugin(&plugin_dir, "/tmp/triseek binary").unwrap();
        let generated = std::fs::read_to_string(plugin_dir.join("triseek-memo.ts")).unwrap();
        assert!(generated.contains("const TRISEEK_BIN = '/tmp/triseek binary';"));
        assert!(generated.contains("\"tool.execute.after\""));
        assert!(generated.contains("['read', 'edit', 'write', 'apply_patch']"));
        assert!(generated.contains("output?.args ?? input?.args ?? {}"));
        assert!(generated.contains("memo-observe', '--event', 'post-tool-use'"));
    }

    #[test]
    fn opencode_paths_use_user_config_home() {
        let _guard = env_lock().lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let original = std::env::var_os("XDG_CONFIG_HOME");
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", tmp.path());
        }

        let config_path = opencode_config_path().unwrap();
        let plugin_dir = opencode_plugin_dir().unwrap();

        if let Some(value) = original {
            unsafe {
                std::env::set_var("XDG_CONFIG_HOME", value);
            }
        } else {
            unsafe {
                std::env::remove_var("XDG_CONFIG_HOME");
            }
        }

        assert_eq!(
            config_path,
            tmp.path().join("opencode").join("opencode.json")
        );
        assert_eq!(plugin_dir, tmp.path().join("opencode").join("plugins"));
    }

    #[test]
    fn write_pi_extension_emits_expected_contract() {
        let tmp = tempfile::tempdir().unwrap();
        let extension_dir = tmp.path().join("triseek-memo");
        write_pi_extension(&extension_dir, "/tmp/triseek binary").unwrap();
        let generated = std::fs::read_to_string(extension_dir.join("index.ts")).unwrap();
        assert!(generated.contains("const TRISEEK_BIN = '/tmp/triseek binary';"));
        assert!(generated.contains("pi.on('session_start'"));
        assert!(generated.contains("pi.on('tool_result'"));
        assert!(generated.contains("pi.on('session_before_compact'"));
        assert!(generated.contains("['read', 'edit', 'write', 'bash']"));
        assert!(generated.contains("memo-observe', '--event', 'session-start'"));
        assert!(generated.contains("memo-observe', '--event', 'post-tool-use'"));
        assert!(generated.contains("memo-observe', '--event', 'pre-compact'"));
    }
}
