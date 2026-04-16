//! Integration tests for the `triseek mcp serve` stdio server.
//!
//! Spawns the real `triseek` binary as a subprocess with piped stdio,
//! performs the MCP `initialize` handshake, and calls each of the 5 tools
//! against a tempdir fixture repo built with `SearchEngine::build`.
//!
//! The test does NOT rely on `git init` because the sandboxed test
//! environment may reject unsigned commits. `SearchEngine::build` only
//! needs a directory with files to walk.

use search_core::DAEMON_PORT_FILE;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

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
        Self::spawn_with_home(repo, None)
    }

    fn spawn_with_home(repo: &Path, home: Option<&Path>) -> Self {
        let binary = triseek_binary();
        let index_dir = repo.join(".triseek-index");
        assert!(
            binary.exists(),
            "triseek binary not found at {}; cargo test should build it",
            binary.display()
        );
        let mut command = Command::new(&binary);
        command
            .arg("mcp")
            .arg("serve")
            .arg("--repo")
            .arg(repo)
            .arg("--index-dir")
            .arg(&index_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(home) = home {
            command.env("HOME", home);
            command.env("USERPROFILE", home);
        }
        let mut child = command.spawn().expect("spawn triseek mcp serve");
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
    call_tool_with_meta(client, name, arguments, Value::Null)
}

fn call_tool_with_meta(client: &mut McpClient, name: &str, arguments: Value, meta: Value) -> Value {
    let mut params = json!({
        "name": name,
        "arguments": arguments,
    });
    if !meta.is_null() {
        params
            .as_object_mut()
            .expect("tool params object")
            .insert("_meta".to_string(), meta);
    }
    let response = client.call("tools/call", params);
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

struct FakeDaemon {
    requests: Arc<Mutex<Vec<Value>>>,
    handle: thread::JoinHandle<()>,
}

impl FakeDaemon {
    fn start(home: &Path, responses: Vec<Value>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind fake daemon");
        let port = listener.local_addr().expect("daemon addr").port();
        let daemon_dir = home.join(".triseek").join("daemon");
        std::fs::create_dir_all(&daemon_dir).expect("create fake daemon dir");
        std::fs::write(daemon_dir.join(DAEMON_PORT_FILE), port.to_string())
            .expect("write fake daemon port");

        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_thread = Arc::clone(&requests);
        let handle = thread::spawn(move || {
            for response in responses {
                let (mut socket, _) = listener.accept().expect("accept fake daemon connection");
                let mut reader = BufReader::new(socket.try_clone().expect("clone fake socket"));
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .expect("read fake daemon request");
                let request: Value =
                    serde_json::from_str(line.trim()).expect("parse fake daemon request");
                requests_for_thread
                    .lock()
                    .expect("fake daemon requests mutex")
                    .push(request.clone());
                let id = request.get("id").cloned().unwrap_or_else(|| json!(1));
                let rpc_response = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": response,
                });
                writeln!(socket, "{}", serde_json::to_string(&rpc_response).unwrap())
                    .expect("write fake daemon response");
            }
        });
        Self { requests, handle }
    }

    fn finish(self) -> Vec<Value> {
        self.handle.join().expect("fake daemon thread join");
        Arc::try_unwrap(self.requests)
            .expect("fake daemon request ownership")
            .into_inner()
            .expect("fake daemon request mutex")
    }
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
    assert!(names.contains(&"memo_status"));
    assert!(names.contains(&"memo_session"));
    assert!(names.contains(&"memo_check"));

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

// ---------------------------------------------------------------------------
// Query cache integration tests
// ---------------------------------------------------------------------------

fn handshake(client: &mut McpClient) {
    let _ = client.call(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name":"t","version":"0"}, "capabilities": {}}),
    );
    client.notify("notifications/initialized", json!({}));
}

#[test]
fn search_content_cache_miss_then_hit() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    handshake(&mut client);

    let args = json!({ "query": "parse_arguments", "mode": "literal", "limit": 5 });

    let first = call_tool(&mut client, "search_content", args.clone());
    assert_eq!(
        first.get("cache").and_then(Value::as_str),
        Some("miss"),
        "first call must be a cache miss"
    );

    let second = call_tool(&mut client, "search_content", args);
    assert_eq!(
        second.get("cache").and_then(Value::as_str),
        Some("hit"),
        "second identical call must be a cache hit"
    );

    // Results must be identical (modulo the cache field itself).
    assert_eq!(
        first.get("files_with_matches"),
        second.get("files_with_matches")
    );
    assert_eq!(first.get("results"), second.get("results"));

    client.shutdown();
}

#[test]
fn find_files_cache_miss_then_hit() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    handshake(&mut client);

    let args = json!({ "query": "router", "limit": 10 });

    let first = call_tool(&mut client, "find_files", args.clone());
    assert_eq!(first.get("cache").and_then(Value::as_str), Some("miss"));

    let second = call_tool(&mut client, "find_files", args);
    assert_eq!(second.get("cache").and_then(Value::as_str), Some("hit"));
    assert_eq!(first.get("results"), second.get("results"));

    client.shutdown();
}

#[test]
fn different_tool_name_is_different_cache_entry() {
    // find_files and search_content share the same pattern string but must NOT
    // share a cache entry because they produce structurally different envelopes.
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    handshake(&mut client);

    // Both calls use "router" as the pattern.
    let ff = call_tool(&mut client, "find_files", json!({ "query": "router" }));
    let sc = call_tool(
        &mut client,
        "search_content",
        json!({ "query": "router", "mode": "literal" }),
    );

    // Both must be misses (separate keys).
    assert_eq!(ff.get("cache").and_then(Value::as_str), Some("miss"));
    assert_eq!(sc.get("cache").and_then(Value::as_str), Some("miss"));

    // Repeat each — both should now be hits.
    let ff2 = call_tool(&mut client, "find_files", json!({ "query": "router" }));
    let sc2 = call_tool(
        &mut client,
        "search_content",
        json!({ "query": "router", "mode": "literal" }),
    );
    assert_eq!(ff2.get("cache").and_then(Value::as_str), Some("hit"));
    assert_eq!(sc2.get("cache").and_then(Value::as_str), Some("hit"));

    client.shutdown();
}

#[test]
fn reindex_invalidates_search_cache() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    handshake(&mut client);

    let args = json!({ "query": "route_auth", "mode": "literal", "limit": 5 });

    // Prime the cache.
    let miss = call_tool(&mut client, "search_content", args.clone());
    assert_eq!(miss.get("cache").and_then(Value::as_str), Some("miss"));
    let hit = call_tool(&mut client, "search_content", args.clone());
    assert_eq!(hit.get("cache").and_then(Value::as_str), Some("hit"));

    // Reindex flushes the cache.
    let reindex = call_tool(&mut client, "reindex", json!({ "mode": "incremental" }));
    assert_eq!(reindex.get("completed"), Some(&json!(true)));

    // After reindex the same query must be a miss again.
    let after = call_tool(&mut client, "search_content", args);
    assert_eq!(
        after.get("cache").and_then(Value::as_str),
        Some("miss"),
        "cache must be cleared after reindex"
    );

    client.shutdown();
}

#[test]
fn ripgrep_fallback_bypasses_cache() {
    // An unindexed repo forces the ripgrep fallback path.
    // Those results must never be cached (strategy == "ripgrep_fallback").
    let fixture = build_unindexed_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    handshake(&mut client);

    let args = json!({ "query": "fallback", "mode": "literal" });

    let first = call_tool(&mut client, "search_content", args.clone());
    assert_eq!(
        first.get("cache").and_then(Value::as_str),
        Some("bypass"),
        "ripgrep fallback results must not be cached"
    );

    // Running again should still be bypass, not hit.
    let second = call_tool(&mut client, "search_content", args);
    assert_eq!(second.get("cache").and_then(Value::as_str), Some("bypass"));

    client.shutdown();
}

#[test]
fn memo_check_uses_meta_session_id_and_forwards_to_daemon() {
    let fixture = build_fixture_repo();
    let fake_home = tempfile::tempdir().expect("fake home");
    let daemon = FakeDaemon::start(
        fake_home.path(),
        vec![
            json!({"ok": true}),
            json!({
                "path": "src/auth/router.rs",
                "status": "fresh",
                "recommendation": "skip_reread",
                "tokens_at_last_read": 14,
                "last_read_ago_seconds": 2
            }),
        ],
    );

    let mut client = McpClient::spawn_with_home(fixture.path(), Some(fake_home.path()));
    handshake(&mut client);

    let response = call_tool_with_meta(
        &mut client,
        "memo_check",
        json!({ "path": "src/auth/router.rs" }),
        json!({ "sessionId": "codex-meta-session" }),
    );
    assert_eq!(response.get("path"), Some(&json!("src/auth/router.rs")));
    assert_eq!(response.get("status"), Some(&json!("fresh")));
    assert_eq!(response.get("recommendation"), Some(&json!("skip_reread")));
    assert_eq!(response.get("tokens_at_last_read"), Some(&json!(14)));

    client.shutdown();

    let requests = daemon.finish();
    let expected_repo_root = fixture
        .path()
        .canonicalize()
        .unwrap_or_else(|_| fixture.path().to_path_buf());
    assert_eq!(
        requests.len(),
        2,
        "memo_check should make 2 daemon RPC calls"
    );
    assert_eq!(
        requests[0].pointer("/method").and_then(Value::as_str),
        Some("memo_session_start")
    );
    assert_eq!(
        requests[0]
            .pointer("/params/session_id")
            .and_then(Value::as_str),
        Some("codex-meta-session")
    );
    assert_eq!(
        requests[0]
            .pointer("/params/repo_root")
            .and_then(Value::as_str),
        Some(expected_repo_root.to_str().expect("fixture path str"))
    );
    assert_eq!(
        requests[1].pointer("/method").and_then(Value::as_str),
        Some("memo_check")
    );
    assert_eq!(
        requests[1]
            .pointer("/params/session_id")
            .and_then(Value::as_str),
        Some("codex-meta-session")
    );
    assert_eq!(
        requests[1].pointer("/params/path").and_then(Value::as_str),
        Some("src/auth/router.rs")
    );
}

#[test]
fn memo_status_uses_meta_session_id_and_returns_current_tokens() {
    let fixture = build_fixture_repo();
    let fake_home = tempfile::tempdir().expect("fake home");
    let daemon = FakeDaemon::start(
        fake_home.path(),
        vec![
            json!({"ok": true}),
            json!({
                "session_id": "claude-meta-session",
                "results": [{
                    "path": "src/auth/router.rs",
                    "status": "stale",
                    "tokens": 20,
                    "read_count": 1,
                    "message": "Changed since last read (now ~24 tokens); re-read file.",
                    "current_tokens": 24
                }]
            }),
        ],
    );

    let mut client = McpClient::spawn_with_home(fixture.path(), Some(fake_home.path()));
    handshake(&mut client);

    let response = call_tool_with_meta(
        &mut client,
        "memo_status",
        json!({ "files": ["src/auth/router.rs"] }),
        json!({ "session_id": "claude-meta-session" }),
    );
    assert_eq!(
        response.get("session_id"),
        Some(&json!("claude-meta-session"))
    );
    let results = response
        .get("results")
        .and_then(Value::as_array)
        .expect("memo_status results");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].get("status"), Some(&json!("stale")));
    assert_eq!(results[0].get("current_tokens"), Some(&json!(24)));

    client.shutdown();

    let requests = daemon.finish();
    assert_eq!(
        requests[0]
            .pointer("/params/session_id")
            .and_then(Value::as_str),
        Some("claude-meta-session")
    );
    assert_eq!(
        requests[1].pointer("/method").and_then(Value::as_str),
        Some("memo_status")
    );
    assert_eq!(
        requests[1]
            .pointer("/params/files/0")
            .and_then(Value::as_str),
        Some("src/auth/router.rs")
    );
}
