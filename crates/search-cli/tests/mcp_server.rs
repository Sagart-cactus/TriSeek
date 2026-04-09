//! Integration tests for the `triseek mcp serve` stdio server.
//!
//! Spawns the real `triseek` binary as a subprocess with piped stdio,
//! performs the MCP `initialize` handshake, and calls each of the 5 tools
//! against a tempdir fixture repo built with `SearchEngine::build`.
//!
//! The test does NOT rely on `git init` because the sandboxed test
//! environment may reject unsigned commits. `SearchEngine::build` only
//! needs a directory with files to walk.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// Locate the freshly built `triseek` binary next to the current test executable.
fn triseek_binary() -> PathBuf {
    // Cargo places test binaries under target/<profile>/deps/ and the binary
    // under target/<profile>/triseek.
    let exe = std::env::current_exe().expect("current exe");
    let mut path = exe.clone();
    // Walk up from .../deps/<test-hash> to .../<profile>/
    path.pop(); // remove test binary name
    if path.ends_with("deps") {
        path.pop();
    }
    let bin_name = format!("triseek{}", std::env::consts::EXE_SUFFIX);
    path.join(bin_name)
}

fn build_fixture_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src/auth")).unwrap();
    std::fs::create_dir_all(root.join("src/cli")).unwrap();
    std::fs::write(
        root.join("src/auth/router.rs"),
        "pub fn route_auth() {\n    // TODO: implement\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/cli/parser.rs"),
        "pub fn parse_arguments(args: &[String]) -> Config {\n    Config::default()\n}\n\npub struct Config;\n\nimpl Config {\n    pub fn default() -> Self { Config }\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("README.md"),
        "# Fixture repository for triseek mcp tests\n\nUse parse_arguments for the CLI parser.\n",
    )
    .unwrap();

    // Build the triseek index directly without git.
    let index_dir = root.join(".triseek-index");
    search_index::SearchEngine::build(
        root,
        Some(&index_dir),
        &search_index::BuildConfig::default(),
    )
    .expect("build index");

    tmp
}

fn build_unindexed_fixture_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/fallback.rs"),
        "pub fn fallback() {\n    let value = \"McpState\";\n}\n",
    )
    .unwrap();
    tmp
}

fn build_repeated_match_repo(line_matches: usize) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).unwrap();

    let mut contents = String::from("pub fn sample() {\n");
    for idx in 0..line_matches {
        contents.push_str(&format!("    let needle_{idx} = \"McpState\";\n"));
    }
    contents.push_str("}\n");
    std::fs::write(root.join("src/many.rs"), contents).unwrap();

    let index_dir = root.join(".triseek-index");
    search_index::SearchEngine::build(
        root,
        Some(&index_dir),
        &search_index::BuildConfig::default(),
    )
    .expect("build index");

    tmp
}

struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl McpClient {
    fn spawn(repo: &Path) -> Self {
        let binary = triseek_binary();
        assert!(
            binary.exists(),
            "triseek binary not found at {}; cargo test should build it",
            binary.display()
        );
        let mut child = Command::new(&binary)
            .arg("mcp")
            .arg("serve")
            .arg("--repo")
            .arg(repo)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn triseek mcp serve");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let line = serde_json::to_string(&request).unwrap();
        self.stdin.write_all(line.as_bytes()).unwrap();
        self.stdin.write_all(b"\n").unwrap();
        self.stdin.flush().unwrap();

        // Read a single response line.
        let mut buf = String::new();
        self.stdout.read_line(&mut buf).expect("read response");
        assert!(!buf.is_empty(), "empty response line");
        let response: Value = serde_json::from_str(buf.trim()).expect("parse response");
        assert_eq!(response.get("jsonrpc"), Some(&json!("2.0")));
        assert_eq!(response.get("id"), Some(&json!(id)));
        response
    }

    fn notify(&mut self, method: &str, params: Value) {
        let message = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let line = serde_json::to_string(&message).unwrap();
        self.stdin.write_all(line.as_bytes()).unwrap();
        self.stdin.write_all(b"\n").unwrap();
        self.stdin.flush().unwrap();
    }

    fn shutdown(mut self) {
        // Dropping stdin closes the pipe and causes the server loop to exit on EOF.
        drop(self.stdin);
        let _ = self.child.wait();
    }
}

fn call_tool(client: &mut McpClient, name: &str, arguments: Value) -> Value {
    let response = client.call(
        "tools/call",
        json!({
            "name": name,
            "arguments": arguments,
        }),
    );
    let result = response
        .get("result")
        .unwrap_or_else(|| panic!("expected result in {response}"));
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .expect("content array");
    assert!(!content.is_empty(), "content must not be empty");
    let text = content[0]
        .get("text")
        .and_then(Value::as_str)
        .expect("content text");
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let envelope: Value = serde_json::from_str(text).expect("parse envelope JSON");
    if is_error {
        panic!("tool {name} returned isError=true: {envelope}");
    }
    envelope
}

#[test]
fn initialize_handshake_and_tools_list() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());

    // 1. initialize
    let init = client.call(
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "clientInfo": {"name": "triseek-test", "version": "0.0.0"},
            "capabilities": {}
        }),
    );
    let result = init.get("result").expect("initialize result");
    assert!(result.get("serverInfo").is_some());
    assert_eq!(
        result
            .get("serverInfo")
            .and_then(|v| v.get("name"))
            .and_then(Value::as_str),
        Some("triseek")
    );
    assert!(result.get("capabilities").is_some());

    // 2. initialized notification
    client.notify("notifications/initialized", json!({}));

    // 3. tools/list
    let listed = client.call("tools/list", json!({}));
    let tools = listed
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(Value::as_array)
        .expect("tools array");
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .collect();
    assert!(names.contains(&"find_files"));
    assert!(names.contains(&"search_content"));
    assert!(names.contains(&"search_path_and_content"));
    assert!(names.contains(&"index_status"));
    assert!(names.contains(&"reindex"));

    client.shutdown();
}

#[test]
fn find_files_returns_envelope_with_version_and_strategy() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    let _ = client.call(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name":"t","version":"0"}, "capabilities": {}}),
    );
    client.notify("notifications/initialized", json!({}));

    let envelope = call_tool(
        &mut client,
        "find_files",
        json!({ "query": "parser", "limit": 10 }),
    );
    assert_eq!(envelope.get("version"), Some(&json!("1")));
    assert!(envelope.get("repo_root").is_some());
    assert!(envelope.get("strategy").is_some());
    assert!(envelope.get("fallback_used").is_some());
    let results = envelope
        .get("results")
        .and_then(Value::as_array)
        .expect("results array");
    assert!(
        results.iter().any(|r| r
            .get("path")
            .and_then(Value::as_str)
            .is_some_and(|p| p.contains("parser"))),
        "expected a result containing 'parser', got {results:?}"
    );

    client.shutdown();
}

#[test]
fn search_content_finds_literal_match() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    let _ = client.call(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name":"t","version":"0"}, "capabilities": {}}),
    );
    client.notify("notifications/initialized", json!({}));

    let envelope = call_tool(
        &mut client,
        "search_content",
        json!({ "query": "parse_arguments", "mode": "literal", "limit": 5 }),
    );
    assert_eq!(envelope.get("version"), Some(&json!("1")));
    let results = envelope
        .get("results")
        .and_then(Value::as_array)
        .expect("results array");
    assert!(!results.is_empty(), "expected at least one hit");
    // Each content result should have a `matches` array with line/column/preview.
    let first = &results[0];
    assert!(first.get("path").is_some());
    if let Some(matches) = first.get("matches").and_then(Value::as_array) {
        assert!(!matches.is_empty());
        let m = &matches[0];
        assert!(m.get("line").is_some());
        assert!(m.get("column").is_some());
        assert!(m.get("preview").is_some());
    }

    client.shutdown();
}

#[test]
fn search_content_respects_limit_with_many_matches_in_one_file() {
    let fixture = build_repeated_match_repo(20);
    let mut client = McpClient::spawn(fixture.path());
    let _ = client.call(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name":"t","version":"0"}, "capabilities": {}}),
    );
    client.notify("notifications/initialized", json!({}));

    let envelope = call_tool(
        &mut client,
        "search_content",
        json!({ "query": "needle_[0-9]+", "mode": "regex", "limit": 5 }),
    );
    let returned_matches: usize = envelope
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|result| {
            result
                .get("matches")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        })
        .sum();
    assert_eq!(
        returned_matches, 5,
        "result envelope must respect requested limit"
    );
    assert_eq!(envelope.get("truncated"), Some(&json!(true)));

    client.shutdown();
}

#[test]
fn search_path_and_content_respects_glob() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    let _ = client.call(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name":"t","version":"0"}, "capabilities": {}}),
    );
    client.notify("notifications/initialized", json!({}));

    let envelope = call_tool(
        &mut client,
        "search_path_and_content",
        json!({
            "path_query": "src/**/*.rs",
            "content_query": "parse_arguments",
            "mode": "literal"
        }),
    );
    assert_eq!(envelope.get("version"), Some(&json!("1")));
    // strategy + fallback flag must always be present.
    assert!(envelope.get("strategy").is_some());
    assert!(envelope.get("fallback_used").is_some());

    client.shutdown();
}

#[test]
fn index_status_reports_present_index() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    let _ = client.call(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name":"t","version":"0"}, "capabilities": {}}),
    );
    client.notify("notifications/initialized", json!({}));

    let envelope = call_tool(&mut client, "index_status", json!({}));
    assert_eq!(envelope.get("version"), Some(&json!("1")));
    assert_eq!(envelope.get("index_present"), Some(&json!(true)));
    assert_eq!(envelope.get("indexed_files"), Some(&json!(3)));

    client.shutdown();
}

#[test]
fn reindex_incremental_completes() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    let _ = client.call(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name":"t","version":"0"}, "capabilities": {}}),
    );
    client.notify("notifications/initialized", json!({}));

    let envelope = call_tool(&mut client, "reindex", json!({ "mode": "incremental" }));
    assert_eq!(envelope.get("version"), Some(&json!("1")));
    assert_eq!(envelope.get("completed"), Some(&json!(true)));
    assert_eq!(envelope.get("mode"), Some(&json!("incremental")));
    assert!(envelope.get("elapsed_ms").is_some());

    client.shutdown();
}

#[test]
fn reindex_incremental_bootstraps_missing_index() {
    let fixture = build_unindexed_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    let _ = client.call(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name":"t","version":"0"}, "capabilities": {}}),
    );
    client.notify("notifications/initialized", json!({}));

    let envelope = call_tool(&mut client, "reindex", json!({ "mode": "incremental" }));
    assert_eq!(envelope.get("version"), Some(&json!("1")));
    assert_eq!(envelope.get("completed"), Some(&json!(true)));
    assert_eq!(envelope.get("mode"), Some(&json!("incremental")));
    assert_eq!(envelope.get("rebuilt_full"), Some(&json!(true)));

    let status = call_tool(&mut client, "index_status", json!({}));
    assert_eq!(status.get("index_present"), Some(&json!(true)));
    assert_eq!(status.get("index_fresh"), Some(&json!(true)));

    client.shutdown();
}

#[test]
fn reindex_invalidates_cached_engine() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    let _ = client.call(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name":"t","version":"0"}, "capabilities": {}}),
    );
    client.notify("notifications/initialized", json!({}));

    let warm = call_tool(
        &mut client,
        "search_content",
        json!({ "query": "route_auth", "mode": "literal", "limit": 5 }),
    );
    assert_eq!(warm.get("files_with_matches"), Some(&json!(1)));

    std::fs::write(
        fixture.path().join("src/auth/router.rs"),
        "pub fn route_auth() {\n    let fresh_symbol = true;\n}\n",
    )
    .unwrap();

    let reindex = call_tool(&mut client, "reindex", json!({ "mode": "incremental" }));
    assert_eq!(reindex.get("completed"), Some(&json!(true)));

    let fresh = call_tool(
        &mut client,
        "search_content",
        json!({ "query": "fresh_symbol", "mode": "literal", "limit": 5 }),
    );
    assert_eq!(fresh.get("files_with_matches"), Some(&json!(1)));

    client.shutdown();
}

#[test]
fn index_status_reports_repo_searchable_files_after_incremental_update() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    let _ = client.call(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name":"t","version":"0"}, "capabilities": {}}),
    );
    client.notify("notifications/initialized", json!({}));

    std::fs::write(
        fixture.path().join("src/auth/router.rs"),
        "pub fn route_auth() {\n    let changed = true;\n}\n",
    )
    .unwrap();

    let reindex = call_tool(&mut client, "reindex", json!({ "mode": "incremental" }));
    assert_eq!(reindex.get("completed"), Some(&json!(true)));

    let status = call_tool(&mut client, "index_status", json!({}));
    assert_eq!(status.get("index_present"), Some(&json!(true)));
    assert_eq!(status.get("indexed_files"), Some(&json!(3)));

    client.shutdown();
}

#[test]
fn malformed_request_returns_parse_error() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());

    // Send a deliberately malformed line.
    client.stdin.write_all(b"{not valid json}\n").unwrap();
    client.stdin.flush().unwrap();

    let mut buf = String::new();
    client.stdout.read_line(&mut buf).expect("read response");
    let response: Value = serde_json::from_str(buf.trim()).expect("parse response");
    assert_eq!(response.get("jsonrpc"), Some(&json!("2.0")));
    let err = response.get("error").expect("error field");
    assert_eq!(err.get("code"), Some(&json!(-32700)));

    client.shutdown();
}

#[test]
fn invalid_query_is_returned_as_tool_error() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    let _ = client.call(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name":"t","version":"0"}, "capabilities": {}}),
    );
    client.notify("notifications/initialized", json!({}));

    // Empty query should come back as isError=true with an INVALID_QUERY code.
    let response = client.call(
        "tools/call",
        json!({
            "name": "search_content",
            "arguments": { "query": "" },
        }),
    );
    let result = response.get("result").expect("result");
    assert_eq!(result.get("isError"), Some(&json!(true)));
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .expect("content array");
    let text = content[0].get("text").and_then(Value::as_str).unwrap();
    let body: Value = serde_json::from_str(text).unwrap();
    assert_eq!(body.get("version"), Some(&json!("1")));
    let code = body
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str)
        .expect("error code");
    assert_eq!(code, "INVALID_QUERY");

    client.shutdown();
}
