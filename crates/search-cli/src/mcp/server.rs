//! Synchronous stdio JSON-RPC loop implementing the MCP subset TriSeek
//! supports: `initialize`, `notifications/initialized`, `tools/list`,
//! `tools/call`, `ping`, and `shutdown`.
//!
//! Framing is newline-delimited JSON per the MCP stdio transport spec: each
//! JSON-RPC message is a single line of JSON terminated by `\n`. Logs go to
//! stderr only; stdout is reserved for framed messages.

use anyhow::{Context, Result};
use search_core::{DAEMON_PORT_FILE, DaemonRootParams, RpcRequest, RpcResponse};
use search_index::{
    BuildConfig, SearchEngine, UpdateOutcome, daemon_dir, default_index_dir, index_exists,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, StdinLock, StdoutLock, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use crate::mcp::query_cache::QueryCache;
use crate::mcp::schema::{TOOLS, ToolDescriptor};
use crate::mcp::search_memo::SearchMemo;
use crate::mcp::tools::{ToolOutcome, dispatch};
use crate::output_format;

const PROTOCOL_VERSION: &str = "2025-06-18";
const SERVER_NAME: &str = "triseek";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const DISABLE_STARTUP_SYNC_ENV: &str = "TRISEEK_MCP_DISABLE_STARTUP_SYNC";

pub struct McpState {
    repo_root: PathBuf,
    index_dir: PathBuf,
    cached_engine: Mutex<Option<SearchEngine>>,
    index_sync_in_progress: AtomicBool,
    index_mutation_lock: Mutex<()>,
    pub query_cache: QueryCache,
    pub search_memo: SearchMemo,
}

pub struct IndexMutationGuard<'a> {
    state: &'a McpState,
    _guard: MutexGuard<'a, ()>,
}

impl Drop for IndexMutationGuard<'_> {
    fn drop(&mut self) {
        self.state.set_index_sync_in_progress(false);
    }
}

impl McpState {
    pub fn new(repo_root: PathBuf, index_dir_override: Option<PathBuf>) -> Self {
        let index_dir = index_dir_override.unwrap_or_else(|| default_index_dir(&repo_root));
        let ttl_secs: u64 = std::env::var("TRISEEK_SEARCH_CACHE_TTL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);
        Self {
            repo_root,
            index_dir,
            cached_engine: Mutex::new(None),
            index_sync_in_progress: AtomicBool::new(false),
            index_mutation_lock: Mutex::new(()),
            query_cache: QueryCache::new(Duration::from_secs(ttl_secs), 256),
            search_memo: SearchMemo::new(256),
        }
    }

    pub fn repo_root(&self) -> PathBuf {
        self.repo_root.clone()
    }

    pub fn index_dir(&self) -> PathBuf {
        self.index_dir.clone()
    }

    pub fn with_cached_engine<T>(
        &self,
        f: impl FnOnce(Option<&SearchEngine>) -> Result<T>,
    ) -> Result<T> {
        let mut guard = self
            .cached_engine
            .lock()
            .expect("MCP cached engine mutex poisoned");
        if guard.is_none() && self.index_sync_in_progress.load(Ordering::Relaxed) {
            return f(None);
        }
        if guard.is_none() && index_exists(&self.index_dir) {
            let engine = SearchEngine::open(&self.index_dir)
                .with_context(|| format!("failed to open index at {}", self.index_dir.display()))?;
            *guard = Some(engine);
        }
        f(guard.as_ref())
    }

    pub fn should_bypass_index_for_startup_sync(&self) -> bool {
        if !self.index_sync_in_progress.load(Ordering::Relaxed) {
            return false;
        }
        self.cached_engine
            .lock()
            .expect("MCP cached engine mutex poisoned")
            .is_none()
    }

    pub fn invalidate_cached_engine(&self) {
        if let Ok(mut guard) = self.cached_engine.lock() {
            *guard = None;
        }
        self.query_cache.invalidate_all();
        self.search_memo.invalidate_all();
    }

    pub fn set_index_sync_in_progress(&self, in_progress: bool) {
        self.index_sync_in_progress
            .store(in_progress, Ordering::Relaxed);
    }

    pub fn index_sync_in_progress(&self) -> bool {
        self.index_sync_in_progress.load(Ordering::Relaxed)
    }

    pub fn prime_cached_engine(&self) -> Result<bool> {
        let mut guard = self
            .cached_engine
            .lock()
            .expect("MCP cached engine mutex poisoned");
        if guard.is_some() || !index_exists(&self.index_dir) {
            return Ok(guard.is_some());
        }
        let engine = SearchEngine::open(&self.index_dir)
            .with_context(|| format!("failed to open index at {}", self.index_dir.display()))?;
        *guard = Some(engine);
        Ok(true)
    }

    pub fn start_index_mutation(&self) -> IndexMutationGuard<'_> {
        let guard = self
            .index_mutation_lock
            .lock()
            .expect("MCP index mutation mutex poisoned");
        self.set_index_sync_in_progress(true);
        IndexMutationGuard {
            state: self,
            _guard: guard,
        }
    }
}

/// Run the MCP server over stdin/stdout until EOF or shutdown.
pub fn run(repo_root: &Path, index_dir: Option<&Path>) -> Result<()> {
    let state = Arc::new(McpState::new(
        repo_root.to_path_buf(),
        index_dir.map(Path::to_path_buf),
    ));
    let index_was_present = index_exists(&state.index_dir());
    if index_was_present {
        if let Err(err) = state.prime_cached_engine() {
            eprintln!(
                "triseek mcp: failed to prime existing index at {}: {err}; early queries will fall back until refresh completes",
                state.index_dir().display()
            );
        }
        register_root_with_daemon(state.as_ref());
    }
    if std::env::var_os(DISABLE_STARTUP_SYNC_ENV).is_some() {
        eprintln!(
            "triseek mcp: startup sync disabled by {DISABLE_STARTUP_SYNC_ENV}; repo_root={}",
            state.repo_root().display()
        );
    } else {
        spawn_startup_sync(Arc::clone(&state), index_was_present);
    }
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
        handle_line(state.as_ref(), trimmed, &mut writer, &shutdown)?;
    }
    eprintln!("triseek mcp: server exiting");
    Ok(())
}

fn spawn_startup_sync(state: Arc<McpState>, index_was_present: bool) {
    thread::spawn(move || {
        let _mutation = state.start_index_mutation();
        eprintln!(
            "triseek mcp: scheduling background startup sync for {}",
            state.repo_root().display()
        );
        let sync_succeeded = sync_index_on_startup(state.as_ref());
        if sync_succeeded {
            if index_was_present {
                reload_root_with_daemon(state.as_ref());
            } else {
                register_root_with_daemon(state.as_ref());
            }
        }
    });
}

fn sync_index_on_startup(state: &McpState) -> bool {
    let repo_root = state.repo_root();
    let index_dir = state.index_dir();
    let config = BuildConfig::default();

    if index_exists(&index_dir) {
        eprintln!("triseek mcp: refreshing index for {}", repo_root.display());
        match SearchEngine::update(&repo_root, Some(&index_dir), &config) {
            Ok(outcome) => {
                if startup_update_changed_index(&outcome) {
                    state.invalidate_cached_engine();
                }
                eprintln!(
                    "triseek mcp: index refresh complete (rebuilt_full={} indexed_files={})",
                    outcome.rebuilt_full, outcome.metadata.build_stats.docs_indexed
                );
                true
            }
            Err(err) => {
                eprintln!(
                    "triseek mcp: index refresh failed for {}: {err}; continuing with fallback search",
                    repo_root.display()
                );
                false
            }
        }
    } else {
        eprintln!(
            "triseek mcp: no index found for {}; building initial index",
            repo_root.display()
        );
        match SearchEngine::build(&repo_root, Some(&index_dir), &config) {
            Ok(metadata) => {
                state.invalidate_cached_engine();
                eprintln!(
                    "triseek mcp: initial index build complete (indexed_files={})",
                    metadata.build_stats.docs_indexed
                );
                true
            }
            Err(err) => {
                eprintln!(
                    "triseek mcp: initial index build failed for {}: {err}; continuing with fallback search",
                    repo_root.display()
                );
                false
            }
        }
    }
}

fn startup_update_changed_index(outcome: &UpdateOutcome) -> bool {
    outcome.rebuilt_full
        || outcome.metadata.delta_docs > 0
        || outcome.metadata.delta_removed_paths > 0
}

fn register_root_with_daemon(state: &McpState) {
    let repo_root = state.repo_root();
    let Some(mut stream) = connect_to_daemon() else {
        eprintln!(
            "triseek mcp: daemon not running; skipping background watcher preload for {}",
            repo_root.display()
        );
        return;
    };

    match rpc_call(
        &mut stream,
        "preload_root",
        json!(DaemonRootParams {
            target_root: repo_root.display().to_string(),
        }),
    ) {
        Ok(_) => eprintln!(
            "triseek mcp: registered {} with daemon watcher",
            repo_root.display()
        ),
        Err(err) => eprintln!(
            "triseek mcp: failed to register {} with daemon watcher: {err}",
            repo_root.display()
        ),
    }
}

fn reload_root_with_daemon(state: &McpState) {
    let repo_root = state.repo_root();
    let Some(mut stream) = connect_to_daemon() else {
        eprintln!(
            "triseek mcp: daemon not running; skipping daemon reload for {}",
            repo_root.display()
        );
        return;
    };

    match rpc_call(
        &mut stream,
        "reload",
        json!(search_core::DaemonStatusParams {
            target_root: Some(repo_root.display().to_string()),
        }),
    ) {
        Ok(_) => eprintln!(
            "triseek mcp: reloaded daemon index for {}",
            repo_root.display()
        ),
        Err(err) => eprintln!(
            "triseek mcp: failed to reload daemon index for {}: {err}",
            repo_root.display()
        ),
    }
}

fn daemon_port_path() -> PathBuf {
    daemon_dir().join(DAEMON_PORT_FILE)
}

fn read_daemon_port() -> Option<u16> {
    let port_path = daemon_port_path();
    if !port_path.exists() {
        return None;
    }
    std::fs::read_to_string(&port_path)
        .ok()
        .and_then(|port| port.trim().parse::<u16>().ok())
}

fn connect_to_daemon() -> Option<TcpStream> {
    let port = read_daemon_port()?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(250)).ok()
}

fn rpc_call(stream: &mut TcpStream, method: &str, params: Value) -> Result<Value> {
    let req = RpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: method.to_string(),
        params,
    };
    writeln!(stream, "{}", serde_json::to_string(&req)?)?;
    let reader = BufReader::new(stream.try_clone()?);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let resp: RpcResponse = serde_json::from_str(&line)?;
        if let Some(err) = resp.error {
            anyhow::bail!("RPC error {}: {}", err.code, err.message);
        }
        return Ok(resp.result.unwrap_or(Value::Null));
    }
    anyhow::bail!("daemon closed connection without a response")
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
            let session_id = extract_session_id(&params);
            let outcome = dispatch(state, &name, &arguments, session_id.as_deref());
            let result = tool_result_envelope(&name, &arguments, outcome);
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

fn extract_session_id(params: &Value) -> Option<String> {
    params
        .pointer("/_meta/session_id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            params
                .pointer("/_meta/sessionId")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
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
        "instructions": "TriSeek exposes fast local code search tools for this repository. Use `context_pack` when you need a tiny, intent-aware starting set for a bugfix or review task. Prefer `find_files`, `search_content`, and `search_path_and_content` over shell `rg`, `grep`, `sed`, `find`, `ls`, or file globbing for file discovery and exact code search. When a repeated search result says to reuse a prior result from context, rely on the earlier search output unless you need `force_refresh`. On Codex, before re-reading a file you already saw in this session, call `memo_check`. If it returns `skip_reread`, do not read the file again and rely on the content already in conversation context."
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

fn tool_result_envelope(tool_name: &str, arguments: &Value, outcome: ToolOutcome) -> Value {
    match outcome {
        ToolOutcome::Success(value) => {
            let query_hint = extract_query_hint(arguments);
            let digest = output_format::render_digest(tool_name, &value, query_hint.as_deref());
            json!({
                "content": [
                    { "type": "text", "text": digest }
                ],
                "structuredContent": value,
                "isError": false,
            })
        }
        ToolOutcome::Error(err) => {
            // Keep the structured error body available for machine clients,
            // and surface a readable one-line prose message to the LLM.
            let err_value = serde_json::to_value(&err)
                .unwrap_or_else(|e| json!({ "error": format!("serialize_failure: {e}") }));
            let digest = output_format::render_error_digest(tool_name, &err_value);
            json!({
                "content": [
                    { "type": "text", "text": digest }
                ],
                "structuredContent": err_value,
                "isError": true,
            })
        }
    }
}

/// Pull a short, display-only query string out of the tool arguments. Used
/// to prefix the digest (e.g. `search_content: "McpState"`). Only the common
/// tool parameter names are honored; anything else falls through as None.
fn extract_query_hint(arguments: &Value) -> Option<String> {
    let candidate = arguments
        .get("query")
        .or_else(|| arguments.get("content_query"))
        .or_else(|| arguments.get("path_query"))
        .and_then(Value::as_str)?;
    // Clip very long hints so the first line of the digest stays readable.
    const MAX: usize = 80;
    if candidate.chars().count() <= MAX {
        Some(candidate.to_string())
    } else {
        let head: String = candidate.chars().take(MAX.saturating_sub(1)).collect();
        Some(format!("{head}…"))
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
