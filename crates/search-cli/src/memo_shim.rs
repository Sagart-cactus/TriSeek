use anyhow::{Context, Result};
use search_core::{
    DAEMON_PORT_FILE, MemoEventKind, MemoObserveParams, MemoSessionParams, RpcRequest, RpcResponse,
};
use search_index::daemon_dir;
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;
use xxhash_rust::xxh3::xxh3_64;

const RPC_TIMEOUT: Duration = Duration::from_secs(3);

pub fn run(event: &str, repo_override: Option<&Path>) -> Result<()> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("failed to read hook payload from stdin")?;
    let payload = if input.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&input).context("failed to parse hook payload JSON")?
    };

    let session_id = find_session_id(&payload).unwrap_or_else(|| "default".to_string());
    let repo_root = resolve_repo_root(repo_override, &payload)?;
    let Some(mut stream) = connect_to_daemon() else {
        eprintln!("triseek memo-observe: daemon not running, skipping observe");
        return Ok(());
    };

    match normalize_event(event).as_deref() {
        Some("session-start") => {
            let params = MemoSessionParams {
                session_id,
                repo_root: Some(repo_root.display().to_string()),
            };
            let _ = rpc_call(&mut stream, "memo_session_start", json!(params));
        }
        Some("session-end") => {
            let params = MemoSessionParams {
                session_id,
                repo_root: Some(repo_root.display().to_string()),
            };
            let _ = rpc_call(&mut stream, "memo_session_end", json!(params));
        }
        Some("pre-compact") => {
            let params = MemoObserveParams {
                session_id,
                repo_root: repo_root.display().to_string(),
                event: MemoEventKind::PreCompact,
                path: None,
                content_hash: None,
                tokens: None,
            };
            let _ = rpc_call(&mut stream, "memo_observe", json!(params));
        }
        Some("read") | Some("edit") | Some("write") => {
            let params = build_tool_observe_params(
                &session_id,
                &repo_root,
                match normalize_event(event).as_deref() {
                    Some("read") => MemoEventKind::Read,
                    _ => MemoEventKind::Edit,
                },
                &payload,
            );
            if let Some(params) = params {
                let _ = rpc_call(&mut stream, "memo_observe", json!(params));
            }
        }
        Some("post-tool-use") => {
            let Some(kind) = tool_event_kind(&payload) else {
                return Ok(());
            };
            if let Some(params) = build_tool_observe_params(&session_id, &repo_root, kind, &payload)
            {
                let _ = rpc_call(&mut stream, "memo_observe", json!(params));
            }
        }
        _ => {}
    }
    Ok(())
}

fn build_tool_observe_params(
    session_id: &str,
    repo_root: &Path,
    event: MemoEventKind,
    payload: &Value,
) -> Option<MemoObserveParams> {
    let path = find_path(payload);
    let path_ref = path.as_deref();
    if matches!(event, MemoEventKind::Read) && path_ref.is_none() {
        return None;
    }
    let content_hash = if matches!(event, MemoEventKind::Read) {
        find_content_hash(payload, repo_root, path_ref)
    } else {
        None
    };
    let tokens = if matches!(event, MemoEventKind::Read) {
        find_tokens(payload, repo_root, path_ref)
    } else {
        None
    };

    Some(MemoObserveParams {
        session_id: session_id.to_string(),
        repo_root: repo_root.display().to_string(),
        event,
        path,
        content_hash,
        tokens,
    })
}

fn daemon_port_path() -> PathBuf {
    daemon_dir().join(DAEMON_PORT_FILE)
}

fn read_daemon_port() -> Option<u16> {
    let port_path = daemon_port_path();
    if !port_path.exists() {
        return None;
    }
    fs::read_to_string(&port_path)
        .ok()
        .and_then(|port| port.trim().parse::<u16>().ok())
}

fn connect_to_daemon() -> Option<TcpStream> {
    let port = read_daemon_port()?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let stream = TcpStream::connect_timeout(&addr, Duration::from_millis(300)).ok()?;
    let _ = stream.set_read_timeout(Some(RPC_TIMEOUT));
    let _ = stream.set_write_timeout(Some(RPC_TIMEOUT));
    Some(stream)
}

fn rpc_call(stream: &mut TcpStream, method: &str, params: Value) -> Result<Value> {
    let req = RpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: method.to_string(),
        params,
    };
    writeln!(stream, "{}", serde_json::to_string(&req)?)
        .with_context(|| format!("failed to write RPC request `{method}`"))?;
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
    anyhow::bail!("daemon closed connection without response")
}

fn normalize_event(event: &str) -> Option<String> {
    let normalized = event.trim().to_ascii_lowercase().replace('_', "-");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn tool_event_kind(payload: &Value) -> Option<MemoEventKind> {
    let tool_name = find_string_path(payload, &["tool_name"])
        .or_else(|| find_string_pointer(payload, "/tool/name"))
        .or_else(|| find_string_pointer(payload, "/tool"))
        .or_else(|| find_string_pointer(payload, "/name"))?;
    match tool_name.to_ascii_lowercase().as_str() {
        "read" | "view" => Some(MemoEventKind::Read),
        "edit" | "write" | "multiedit" | "notebookedit" | "apply_patch" => {
            Some(MemoEventKind::Edit)
        }
        _ => None,
    }
}

fn resolve_repo_root(repo_override: Option<&Path>, payload: &Value) -> Result<PathBuf> {
    if let Some(path) = repo_override {
        return Ok(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
    }
    if let Some(candidate) = find_string_path(payload, &["repo_root"]) {
        let path = PathBuf::from(candidate);
        return Ok(path.canonicalize().unwrap_or(path));
    }
    if let Some(candidate) = find_string_pointer(payload, "/cwd") {
        let path = PathBuf::from(candidate);
        return Ok(path.canonicalize().unwrap_or(path));
    }
    let cwd = std::env::current_dir().context("failed to detect cwd for memo-observe")?;
    Ok(cwd.canonicalize().unwrap_or(cwd))
}

fn find_session_id(payload: &Value) -> Option<String> {
    find_string_path(payload, &["session_id"])
        .or_else(|| find_string_pointer(payload, "/session/id"))
        .or_else(|| find_string_pointer(payload, "/_meta/session_id"))
        .or_else(|| find_string_pointer(payload, "/_meta/sessionId"))
        .or_else(|| std::env::var("TRISEEK_SESSION_ID").ok())
}

fn find_path(payload: &Value) -> Option<String> {
    find_string_path(payload, &["path"])
        .or_else(|| find_string_path(payload, &["file_path"]))
        .or_else(|| find_string_path(payload, &["filePath"]))
        .or_else(|| find_string_pointer(payload, "/tool_input/file_path"))
        .or_else(|| find_string_pointer(payload, "/tool_input/path"))
        .or_else(|| find_string_pointer(payload, "/tool_input/filePath"))
        .or_else(|| find_string_pointer(payload, "/tool_response/file_path"))
        .or_else(|| find_string_pointer(payload, "/tool_response/filePath"))
        .or_else(|| find_string_pointer(payload, "/input/file_path"))
        .or_else(|| find_string_pointer(payload, "/input/path"))
}

fn find_content_hash(payload: &Value, repo_root: &Path, path: Option<&str>) -> Option<u64> {
    if let Some(raw) = payload.get("content_hash").and_then(Value::as_u64) {
        return Some(raw);
    }
    if let Some(content) = find_content(payload) {
        return Some(xxh3_64(content.as_bytes()));
    }
    let path = path?;
    let absolute = resolve_data_path(repo_root, path);
    let bytes = fs::read(absolute).ok()?;
    Some(xxh3_64(&bytes))
}

fn find_tokens(payload: &Value, repo_root: &Path, path: Option<&str>) -> Option<u32> {
    if let Some(explicit) = payload
        .get("tokens")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
    {
        return Some(explicit);
    }
    if let Some(explicit) = payload
        .get("token_count")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
    {
        return Some(explicit);
    }
    if let Some(content) = find_content(payload) {
        return Some(estimate_tokens(content.len() as u64));
    }
    let path = path?;
    let absolute = resolve_data_path(repo_root, path);
    let bytes = fs::metadata(absolute).ok()?.len();
    Some(estimate_tokens(bytes))
}

fn estimate_tokens(bytes: u64) -> u32 {
    let est = (bytes as f64 / 3.5).ceil();
    if est > u32::MAX as f64 {
        u32::MAX
    } else {
        est as u32
    }
}

fn resolve_data_path(repo_root: &Path, path: &str) -> PathBuf {
    let raw = Path::new(path);
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        repo_root.join(raw)
    }
}

fn find_content(payload: &Value) -> Option<String> {
    find_string_pointer(payload, "/tool_response/content")
        .or_else(|| find_string_pointer(payload, "/tool_response/text"))
        .or_else(|| find_string_pointer(payload, "/result/content"))
        .or_else(|| find_string_pointer(payload, "/output/content"))
        .or_else(|| {
            payload
                .pointer("/tool_response")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
}

fn find_string_path(payload: &Value, keys: &[&str]) -> Option<String> {
    let mut cursor = payload;
    for key in keys {
        cursor = cursor.get(*key)?;
    }
    cursor.as_str().map(ToString::to_string)
}

fn find_string_pointer(payload: &Value, pointer: &str) -> Option<String> {
    payload
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn infers_post_tool_use_read_event() {
        let payload = json!({"tool_name":"Read"});
        let kind = tool_event_kind(&payload).unwrap();
        assert!(matches!(kind, MemoEventKind::Read));
    }

    #[test]
    fn infers_post_tool_use_edit_event() {
        let payload = json!({"tool_name":"NotebookEdit"});
        let kind = tool_event_kind(&payload).unwrap();
        assert!(matches!(kind, MemoEventKind::Edit));
    }

    #[test]
    fn extracts_codex_session_id_from_meta() {
        let payload = json!({"_meta":{"sessionId":"abc123"}});
        assert_eq!(find_session_id(&payload).as_deref(), Some("abc123"));
    }

    #[test]
    fn hashes_content_when_content_hash_missing() {
        let payload = json!({"tool_response":{"content":"hello"}});
        assert_eq!(
            find_content_hash(&payload, Path::new("/tmp"), Some("x")),
            Some(xxh3_64(b"hello"))
        );
    }

    #[test]
    fn resolves_path_from_file_path_variants() {
        let payload = json!({"tool_input":{"filePath":"src/main.rs"}});
        assert_eq!(find_path(&payload).as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn estimates_tokens_from_content() {
        let payload = json!({"tool_response":{"content":"1234567890"}});
        assert_eq!(
            find_tokens(&payload, Path::new("/tmp"), Some("unused")),
            Some(3)
        );
    }
}
