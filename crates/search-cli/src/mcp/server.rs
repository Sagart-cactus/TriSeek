//! Synchronous stdio JSON-RPC loop implementing the MCP subset TriSeek
//! supports: `initialize`, `notifications/initialized`, `tools/list`,
//! `tools/call`, `ping`, and `shutdown`.
//!
//! Framing is newline-delimited JSON per the MCP stdio transport spec: each
//! JSON-RPC message is a single line of JSON terminated by `\n`. Logs go to
//! stderr only; stdout is reserved for framed messages.

use anyhow::{Context, Result};
use search_index::default_index_dir;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, StdinLock, StdoutLock, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::mcp::schema::{TOOLS, ToolDescriptor};
use crate::mcp::tools::{ToolOutcome, dispatch};

const PROTOCOL_VERSION: &str = "2025-06-18";
const SERVER_NAME: &str = "triseek";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct McpState {
    repo_root: PathBuf,
    index_dir: PathBuf,
}

impl McpState {
    pub fn new(repo_root: PathBuf, index_dir_override: Option<PathBuf>) -> Self {
        let index_dir = index_dir_override.unwrap_or_else(|| default_index_dir(&repo_root));
        Self {
            repo_root,
            index_dir,
        }
    }

    pub fn repo_root(&self) -> PathBuf {
        self.repo_root.clone()
    }

    pub fn index_dir(&self) -> PathBuf {
        self.index_dir.clone()
    }
}

/// Run the MCP server over stdin/stdout until EOF or shutdown.
pub fn run(repo_root: &Path, index_dir: Option<&Path>) -> Result<()> {
    let state = McpState::new(repo_root.to_path_buf(), index_dir.map(Path::to_path_buf));
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();

    eprintln!(
        "triseek mcp: server up; repo_root={} index_dir={}",
        state.repo_root().display(),
        state.index_dir().display()
    );

    let shutdown = AtomicBool::new(false);
    let mut line = String::new();
    while !shutdown.load(Ordering::Relaxed) {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(err) => {
                eprintln!("triseek mcp: stdin read error: {err}");
                break;
            }
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        handle_line(&state, trimmed, &mut writer, &shutdown)?;
    }
    eprintln!("triseek mcp: server exiting");
    Ok(())
}

fn handle_line(
    state: &McpState,
    raw: &str,
    writer: &mut StdoutLock<'_>,
    shutdown: &AtomicBool,
) -> Result<()> {
    // Parse as generic JSON-RPC. Notifications (no `id`) must not get a
    // response; requests (with `id`) always get one.
    let value: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(err) => {
            write_error(writer, Value::Null, -32700, format!("parse error: {err}"))?;
            return Ok(());
        }
    };
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    let is_notification = value.get("id").is_none();

    match method.as_str() {
        "initialize" => {
            let result = initialize_result();
            write_result(writer, id, result)?;
        }
        "notifications/initialized" | "initialized" => {
            // Client acknowledgement of initialize; no response expected.
        }
        "ping" => {
            if !is_notification {
                write_result(writer, id, json!({}))?;
            }
        }
        "tools/list" => {
            let result = tools_list_result();
            write_result(writer, id, result)?;
        }
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::new()));
            let outcome = dispatch(state, &name, &arguments);
            let result = tool_result_envelope(outcome);
            write_result(writer, id, result)?;
        }
        "shutdown" => {
            if !is_notification {
                write_result(writer, id, Value::Null)?;
            }
            shutdown.store(true, Ordering::Relaxed);
        }
        "exit" => {
            shutdown.store(true, Ordering::Relaxed);
        }
        _ if is_notification => {
            // Unknown notifications are ignored per JSON-RPC 2.0.
            eprintln!("triseek mcp: ignoring unknown notification `{method}`");
        }
        _ => {
            write_error(writer, id, -32601, format!("method not found: `{method}`"))?;
        }
    }
    Ok(())
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": SERVER_VERSION,
        },
        "instructions": "TriSeek exposes fast local code search tools (find_files, search_content, search_path_and_content, index_status, reindex). Prefer these over shell grep or file globbing for exact code search in the current repository."
    })
}

fn tools_list_result() -> Value {
    let tools: Vec<Value> = TOOLS.iter().map(tool_descriptor_to_value).collect();
    json!({ "tools": tools })
}

fn tool_descriptor_to_value(descriptor: &ToolDescriptor) -> Value {
    json!({
        "name": descriptor.name,
        "title": descriptor.title,
        "description": descriptor.description,
        "inputSchema": (descriptor.input_schema)(),
    })
}

fn tool_result_envelope(outcome: ToolOutcome) -> Value {
    match outcome {
        ToolOutcome::Success(value) => {
            let text = serde_json::to_string(&value)
                .unwrap_or_else(|err| format!(r#"{{"error":"serialize_failure: {err}"}}"#));
            json!({
                "content": [
                    { "type": "text", "text": text }
                ],
                "isError": false,
            })
        }
        ToolOutcome::Error(err) => {
            let text = serde_json::to_string(&err)
                .unwrap_or_else(|e| format!(r#"{{"error":"serialize_failure: {e}"}}"#));
            json!({
                "content": [
                    { "type": "text", "text": text }
                ],
                "isError": true,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC write helpers
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct JsonRpcEnvelope<'a> {
    jsonrpc: &'a str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

fn write_result(writer: &mut StdoutLock<'_>, id: Value, result: Value) -> Result<()> {
    let envelope = JsonRpcEnvelope {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    };
    write_envelope(writer, &envelope)
}

fn write_error(writer: &mut StdoutLock<'_>, id: Value, code: i32, message: String) -> Result<()> {
    let envelope = JsonRpcEnvelope {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(JsonRpcError { code, message }),
    };
    write_envelope(writer, &envelope)
}

fn write_envelope(writer: &mut StdoutLock<'_>, envelope: &JsonRpcEnvelope<'_>) -> Result<()> {
    let line = serde_json::to_string(envelope).context("serialize JSON-RPC envelope")?;
    writer
        .write_all(line.as_bytes())
        .context("write envelope bytes")?;
    writer.write_all(b"\n").context("write envelope newline")?;
    writer.flush().context("flush stdout")?;
    Ok(())
}

// `StdinLock` is imported to document the reader type; silence the unused
// warning if the type is not directly referenced after construction.
#[allow(dead_code)]
fn _type_assertion(_: BufReader<StdinLock<'_>>) {}
