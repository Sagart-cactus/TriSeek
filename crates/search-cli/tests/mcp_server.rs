//! Integration tests for the `triseek mcp serve` stdio server.
//!
//! Spawns the real `triseek` binary as a subprocess with piped stdio,
//! performs the MCP `initialize` handshake, and calls each of the 5 tools
//! against a tempdir fixture repo built with `SearchEngine::build`.
//!
//! The test does NOT rely on `git init` because the sandboxed test
//! environment may reject unsigned commits. `SearchEngine::build` only
//! needs a directory with files to walk.

use flate2::read::GzDecoder;
use search_core::{DAEMON_PORT_FILE, PORTABILITY_SCHEMA_VERSION};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tar::Archive;

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
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(
        root.join("src/auth/router.rs"),
        "pub fn route_auth() {\n    panic!(\"auth panic\");\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/auth/config.rs"),
        "pub struct AuthConfig {\n    pub service_account: bool,\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tests/auth_test.rs"),
        "#[test]\nfn route_auth_handles_service_accounts() {\n    assert!(true);\n}\n",
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

#[cfg(unix)]
fn build_fake_rg_dir() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().expect("fake rg dir");
    let rg = tmp.path().join("rg");
    std::fs::write(
        &rg,
        r#"#!/bin/sh
case " $* " in
  *" route_auth "*)
    printf '%s\n' '{"type":"match","data":{"path":{"text":"src/auth/router.rs"},"lines":{"text":"pub fn route_auth() {\n"},"line_number":1,"submatches":[{"match":{"text":"route_auth"},"start":7,"end":17}]}}'
    printf '%s\n' '{"type":"match","data":{"path":{"text":"tests/auth_test.rs"},"lines":{"text":"fn route_auth_handles_service_accounts() {\n"},"line_number":2,"submatches":[{"match":{"text":"route_auth"},"start":4,"end":14}]}}'
    ;;
  *" panic "*)
    printf '%s\n' '{"type":"match","data":{"path":{"text":"src/auth/router.rs"},"lines":{"text":"    panic!(\"auth panic\");\n"},"line_number":2,"submatches":[{"match":{"text":"panic"},"start":4,"end":9}]}}'
    ;;
  *)
    exit 1
    ;;
esac
"#,
    )
    .expect("write fake rg");
    let mut permissions = std::fs::metadata(&rg)
        .expect("fake rg metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&rg, permissions).expect("chmod fake rg");
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
        Self::spawn_with_home_and_env(repo, None, &[])
    }

    fn spawn_with_home(repo: &Path, home: Option<&Path>) -> Self {
        Self::spawn_with_home_and_env(repo, home, &[])
    }

    fn spawn_without_startup_sync(repo: &Path) -> Self {
        Self::spawn_with_home_and_env(repo, None, &[("TRISEEK_MCP_DISABLE_STARTUP_SYNC", "1")])
    }

    fn spawn_with_home_and_env(
        repo: &Path,
        home: Option<&Path>,
        extra_env: &[(&str, &str)],
    ) -> Self {
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
        for (key, value) in extra_env {
            command.env(key, value);
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
    // The prose digest must be present for the LLM, but the machine-readable
    // envelope now lives in `structuredContent` (MCP 2025-06-18).
    assert!(
        content[0].get("text").and_then(Value::as_str).is_some(),
        "content[0] must carry a text block"
    );
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let envelope = result
        .get("structuredContent")
        .cloned()
        .expect("structuredContent on tool result");
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

    fn wait_for_requests(&self, expected: usize, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            if self
                .requests
                .lock()
                .expect("fake daemon requests mutex")
                .len()
                >= expected
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {expected} fake-daemon request(s)"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }
}

fn wait_for_index_status(
    client: &mut McpClient,
    predicate: impl Fn(&Value) -> bool,
    timeout: Duration,
) -> Value {
    let deadline = Instant::now() + timeout;
    loop {
        let status = call_tool(client, "index_status", json!({}));
        if predicate(&status) {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for expected index_status, last response: {status}"
        );
        thread::sleep(Duration::from_millis(25));
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
    let instructions = result
        .get("instructions")
        .and_then(Value::as_str)
        .expect("initialize instructions");
    assert!(instructions.contains("memo_check"));
    assert!(instructions.contains("skip_reread"));
    assert!(instructions.contains("rg"));

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
    assert!(names.contains(&"context_pack"));
    let tool_map: std::collections::HashMap<&str, &Value> = tools
        .iter()
        .filter_map(|tool| {
            tool.get("name")
                .and_then(Value::as_str)
                .map(|name| (name, tool))
        })
        .collect();
    let find_files_description = tool_map["find_files"]
        .get("description")
        .and_then(Value::as_str)
        .expect("find_files description");
    assert!(find_files_description.contains("rg --files"));
    let memo_check_description = tool_map["memo_check"]
        .get("description")
        .and_then(Value::as_str)
        .expect("memo_check description");
    assert!(memo_check_description.contains("skip_reread"));
    assert!(memo_check_description.contains("do not read the file again"));

    client.shutdown();
}

#[test]
fn context_pack_returns_bounded_ranked_items_over_mcp() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    handshake(&mut client);

    let envelope = call_tool(
        &mut client,
        "context_pack",
        json!({
            "goal": "fix auth panic for service accounts",
            "intent": "bugfix",
            "budget_tokens": 80,
            "max_files": 3
        }),
    );
    assert_eq!(envelope.get("version"), Some(&json!("1")));
    assert_eq!(envelope.get("intent"), Some(&json!("bugfix")));
    assert_eq!(envelope.get("max_files"), Some(&json!(3)));
    assert!(envelope.get("suggested_next_steps").is_some());
    let items = envelope
        .get("items")
        .and_then(Value::as_array)
        .expect("items array");
    assert!(!items.is_empty(), "expected at least one packed item");
    assert!(items.len() <= 3, "items should respect max_files");
    assert!(
        items.iter().any(|item| item
            .get("path")
            .and_then(Value::as_str)
            .is_some_and(|path| path == "src/auth/router.rs")),
        "expected auth router in context pack, got {items:?}"
    );
    for item in items {
        assert!(item.get("score").and_then(Value::as_f64).is_some());
        assert!(
            item.get("reasons")
                .and_then(Value::as_array)
                .is_some_and(|reasons| !reasons.is_empty())
        );
        assert!(
            item.get("content").is_none(),
            "must not return full file body"
        );
    }

    client.shutdown();
}

#[test]
fn context_pack_clamps_budget_and_max_files_over_mcp() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    handshake(&mut client);

    let envelope = call_tool(
        &mut client,
        "context_pack",
        json!({
            "goal": "review auth config",
            "intent": "review",
            "budget_tokens": 99999,
            "max_files": 99,
            "changed_files": ["src/auth/config.rs"]
        }),
    );
    assert_eq!(envelope.get("intent"), Some(&json!("review")));
    assert_eq!(envelope.get("budget_tokens"), Some(&json!(4000)));
    assert_eq!(envelope.get("max_files"), Some(&json!(12)));
    let items = envelope
        .get("items")
        .and_then(Value::as_array)
        .expect("items array");
    assert!(
        items.iter().any(|item| item
            .get("reasons")
            .and_then(Value::as_array)
            .is_some_and(|reasons| reasons.iter().any(|reason| reason == "changed_file"))),
        "expected changed_file reason, got {items:?}"
    );

    client.shutdown();
}

#[test]
fn context_pack_rejects_empty_goal_over_mcp() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    handshake(&mut client);

    let response = client.call(
        "tools/call",
        json!({
            "name": "context_pack",
            "arguments": { "goal": "   " },
        }),
    );
    let result = response.get("result").expect("result");
    assert_eq!(result.get("isError"), Some(&json!(true)));
    let body = result
        .get("structuredContent")
        .expect("structuredContent on error");
    assert_eq!(
        body.pointer("/error/code").and_then(Value::as_str),
        Some("INVALID_QUERY")
    );

    client.shutdown();
}

#[test]
fn context_pack_cli_outputs_json_and_human_digests() {
    let fixture = build_fixture_repo();
    let binary = triseek_binary();

    let json_output = Command::new(&binary)
        .arg("context-pack")
        .arg("--json")
        .arg("--goal")
        .arg("fix auth panic")
        .arg("--intent")
        .arg("bugfix")
        .arg(fixture.path())
        .output()
        .expect("run context-pack json");
    assert!(
        json_output.status.success(),
        "context-pack json failed: {}",
        String::from_utf8_lossy(&json_output.stderr)
    );
    let envelope: Value =
        serde_json::from_slice(&json_output.stdout).expect("parse context-pack json");
    assert_eq!(envelope.get("version"), Some(&json!("1")));
    assert!(
        envelope
            .get("items")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
    );

    let human_output = Command::new(&binary)
        .arg("context-pack")
        .arg("--goal")
        .arg("fix auth panic")
        .arg(fixture.path())
        .output()
        .expect("run context-pack human");
    assert!(
        human_output.status.success(),
        "context-pack human failed: {}",
        String::from_utf8_lossy(&human_output.stderr)
    );
    let human = String::from_utf8_lossy(&human_output.stdout);
    assert!(human.contains("context_pack"));
    assert!(human.contains("reasons"));
    assert!(human.contains("src/auth/router.rs"));
}

#[test]
fn mcp_serve_registers_root_with_daemon_on_startup() {
    let fixture = build_fixture_repo();
    let fake_home = tempfile::tempdir().expect("fake home");
    let daemon = FakeDaemon::start(
        fake_home.path(),
        vec![json!({"preloaded": true}), json!({"reloaded": true})],
    );

    let mut client = McpClient::spawn_with_home(fixture.path(), Some(fake_home.path()));
    handshake(&mut client);
    daemon.wait_for_requests(2, Duration::from_secs(5));
    client.shutdown();

    let requests = daemon.finish();
    let expected_repo_root = fixture
        .path()
        .canonicalize()
        .unwrap_or_else(|_| fixture.path().to_path_buf());
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].pointer("/method").and_then(Value::as_str),
        Some("preload_root")
    );
    assert_eq!(
        requests[0]
            .pointer("/params/target_root")
            .and_then(Value::as_str),
        Some(expected_repo_root.to_str().expect("fixture path str"))
    );
    assert_eq!(
        requests[1].pointer("/method").and_then(Value::as_str),
        Some("reload")
    );
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
    assert_eq!(envelope.get("indexed_files"), Some(&json!(5)));

    client.shutdown();
}

#[test]
fn mcp_serve_bootstraps_missing_index_on_startup() {
    let fixture = build_unindexed_fixture_repo();
    let mut client = McpClient::spawn(fixture.path());
    handshake(&mut client);

    let envelope = wait_for_index_status(
        &mut client,
        |status| status.get("index_present") == Some(&json!(true)),
        Duration::from_secs(5),
    );
    assert_eq!(envelope.get("version"), Some(&json!("1")));
    assert_eq!(envelope.get("index_present"), Some(&json!(true)));
    assert_eq!(envelope.get("indexed_files"), Some(&json!(1)));

    client.shutdown();
}

#[test]
fn mcp_serve_reindexes_existing_index_on_startup() {
    let fixture = build_fixture_repo();
    std::fs::write(
        fixture.path().join("src/new_feature.rs"),
        "pub fn fresh_startup_symbol() -> bool {\n    true\n}\n",
    )
    .unwrap();

    let mut client = McpClient::spawn(fixture.path());
    handshake(&mut client);

    let status = wait_for_index_status(
        &mut client,
        |status| {
            status.get("index_present") == Some(&json!(true))
                && status.get("indexed_files") == Some(&json!(6))
        },
        Duration::from_secs(5),
    );
    assert_eq!(status.get("index_present"), Some(&json!(true)));
    assert_eq!(status.get("indexed_files"), Some(&json!(6)));

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
    assert_eq!(warm.get("files_with_matches"), Some(&json!(2)));

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
#[cfg(unix)]
fn mcp_search_avoids_large_delta_overlay_indexes() {
    let fixture = build_fixture_repo();
    let fake_rg = build_fake_rg_dir();
    let fake_path = format!(
        "{}:{}",
        fake_rg.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    std::fs::write(
        fixture.path().join(".triseek-index").join("delta.bin"),
        b"non-empty",
    )
    .unwrap();
    std::fs::write(
        fixture.path().join(".triseek-index").join("fast.idx"),
        b"not a valid index",
    )
    .unwrap();

    let mut client = McpClient::spawn_with_home_and_env(
        fixture.path(),
        None,
        &[
            ("TRISEEK_MCP_DISABLE_STARTUP_SYNC", "1"),
            ("TRISEEK_MCP_LEGACY_INDEX_MAX_BYTES", "1"),
            ("PATH", fake_path.as_str()),
        ],
    );
    handshake(&mut client);

    let envelope = call_tool(
        &mut client,
        "search_content",
        json!({ "query": "route_auth", "mode": "literal", "limit": 5 }),
    );
    assert_eq!(envelope.get("fallback_used"), Some(&json!(true)));
    assert_eq!(envelope.get("files_with_matches"), Some(&json!(2)));

    let pack = call_tool(
        &mut client,
        "context_pack",
        json!({
            "goal": "fix route_auth panic",
            "intent": "bugfix",
            "budget_tokens": 1200,
            "max_files": 4
        }),
    );
    assert_eq!(pack.get("version"), Some(&json!("1")));
    assert!(
        pack.get("items")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty()),
        "context_pack should still work through the no-index fallback: {pack}"
    );

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
    assert_eq!(status.get("indexed_files"), Some(&json!(5)));

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
    let prose = content[0].get("text").and_then(Value::as_str).unwrap();
    assert!(
        prose.contains("INVALID_QUERY"),
        "prose digest should mention the error code, got: {prose}"
    );
    let body = result
        .get("structuredContent")
        .expect("structuredContent on error");
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
// Search memo integration tests
// ---------------------------------------------------------------------------

fn handshake(client: &mut McpClient) {
    let _ = client.call(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name":"t","version":"0"}, "capabilities": {}}),
    );
    client.notify("notifications/initialized", json!({}));
}

fn fake_status_response(generation: u64, context_epoch: u64) -> Value {
    json!({
        "daemon_dir": "/tmp/.triseek/daemon",
        "uptime_secs": 1,
        "active_roots": 1,
        "root": {
            "target_root": "/tmp/repo",
            "index_dir": "/tmp/repo/.triseek-index",
            "index_available": true,
            "generation": generation,
            "context_epoch": context_epoch,
            "delta_docs": 0
        }
    })
}

fn fake_preload_response() -> Value {
    json!({
        "preloaded": true,
        "target_root": "/tmp/repo",
    })
}

fn fake_search_reuse_response(
    fresh: bool,
    reason: &str,
    generation: u64,
    context_epoch: u64,
    changed_paths: &[&str],
) -> Value {
    json!({
        "fresh": fresh,
        "reason": reason,
        "generation": generation,
        "context_epoch": context_epoch,
        "changed_paths": changed_paths,
    })
}

fn git_command(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_command_stdout(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn write_snapshot_files(snapshot_dir: &Path, session_id: &str, repo_root: &Path, commit: &str) {
    write_snapshot_files_with_id(
        snapshot_dir,
        "snap_two_clone",
        session_id,
        repo_root,
        Some(commit),
    )
}

fn write_snapshot_files_with_id(
    snapshot_dir: &Path,
    snapshot_id: &str,
    session_id: &str,
    repo_root: &Path,
    commit: Option<&str>,
) {
    std::fs::create_dir_all(snapshot_dir.join("pinned_snippets")).expect("snapshot dir");
    std::fs::write(
        snapshot_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": PORTABILITY_SCHEMA_VERSION,
            "snapshot_id": snapshot_id,
            "session_id": session_id,
            "created_at": 1770000000,
            "repo_root": repo_root.display().to_string(),
            "repo_commit": commit,
            "repo_dirty_files": [],
            "source_harness": "codex",
            "source_model": null,
            "generation": 1,
            "context_epoch": 0
        }))
        .expect("manifest json"),
    )
    .expect("write manifest");
    std::fs::write(
        snapshot_dir.join("working_set.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": PORTABILITY_SCHEMA_VERSION,
            "files_read": [],
            "searches_run": [],
            "frecency_top_n": []
        }))
        .expect("working set json"),
    )
    .expect("write working set");
    std::fs::write(snapshot_dir.join("action_log.jsonl"), "").expect("write action log");
    std::fs::write(
        snapshot_dir.join("pinned_snippets.json"),
        serde_json::to_vec_pretty(&json!([])).expect("pinned json"),
    )
    .expect("write pinned");
    std::fs::write(
        snapshot_dir.join("git_state.json"),
        serde_json::to_vec_pretty(&json!({
            "commit": commit,
            "dirty_files": [],
            "hunk_summary": {}
        }))
        .expect("git state json"),
    )
    .expect("write git state");
}

fn pack_entries(pack_path: &Path) -> Vec<String> {
    let file = std::fs::File::open(pack_path).expect("open pack");
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    let mut entries = archive
        .entries()
        .expect("read archive entries")
        .map(|entry| {
            entry
                .expect("archive entry")
                .path()
                .expect("entry path")
                .to_string_lossy()
                .to_string()
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

#[test]
fn mcp_schema_exposes_tcp_handoff_fields() {
    let fixture = build_fixture_repo();
    let mut client = McpClient::spawn_without_startup_sync(fixture.path());
    handshake(&mut client);

    let listed = client.call("tools/list", json!({}));
    let tools = listed
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .expect("tools array");
    let handoff = tools
        .iter()
        .find(|tool| tool.get("name") == Some(&json!("session_handoff")))
        .expect("session_handoff tool");
    let resume = tools
        .iter()
        .find(|tool| tool.get("name") == Some(&json!("session_resume")))
        .expect("session_resume tool");

    assert!(handoff.pointer("/inputSchema/properties/mode").is_some());
    assert!(
        handoff
            .pointer("/inputSchema/properties/target_harness")
            .is_some()
    );
    assert!(
        handoff
            .pointer("/inputSchema/properties/pack_output_path")
            .is_some()
    );
    assert!(handoff.pointer("/inputSchema/properties/branch").is_some());
    assert!(
        handoff
            .pointer("/inputSchema/properties/continue_existing")
            .is_some()
    );
    assert!(
        resume
            .pointer("/inputSchema/properties/pack_path")
            .is_some()
    );

    client.shutdown();
}

#[test]
fn session_handoff_writes_metadata_tcp_when_pack_output_path_is_provided() {
    let fixture = build_fixture_repo();
    let home = tempfile::tempdir().expect("home tempdir");
    let session_id = "session_mcp_pack";
    let snapshot_id = "snap_mcp_pack";
    let snapshot_dir = home
        .path()
        .join(".triseek")
        .join("daemon")
        .join("snapshots")
        .join(snapshot_id);
    write_snapshot_files_with_id(&snapshot_dir, snapshot_id, session_id, fixture.path(), None);
    let pack_path = home.path().join("handoff.tcp");
    let fake_daemon = FakeDaemon::start(
        home.path(),
        vec![
            fake_preload_response(),
            json!({
                "snapshot_id": snapshot_id,
                "snapshot_dir": snapshot_dir.display().to_string(),
                "manifest": {
                    "schema_version": PORTABILITY_SCHEMA_VERSION,
                    "snapshot_id": snapshot_id,
                    "session_id": session_id,
                    "created_at": 1770000000,
                    "repo_root": fixture.path().display().to_string(),
                    "repo_commit": null,
                    "repo_dirty_files": [],
                    "source_harness": "codex",
                    "source_model": null,
                    "generation": 1,
                    "context_epoch": 0
                }
            }),
            json!({"session": {"session_id": session_id}}),
        ],
    );
    let mut client = McpClient::spawn_with_home_and_env(
        fixture.path(),
        Some(home.path()),
        &[("TRISEEK_MCP_DISABLE_STARTUP_SYNC", "1")],
    );
    handshake(&mut client);

    let envelope = call_tool(
        &mut client,
        "session_handoff",
        json!({
            "session_id": session_id,
            "target_harness": "codex",
            "pack_output_path": pack_path
        }),
    );

    assert_eq!(envelope.get("snapshot_id"), Some(&json!(snapshot_id)));
    assert_eq!(
        envelope.pointer("/handoff/mode").and_then(Value::as_str),
        Some("metadata")
    );
    assert_eq!(
        envelope
            .pointer("/handoff/pack_path")
            .and_then(Value::as_str),
        Some(pack_path.to_str().expect("pack path str"))
    );
    assert!(pack_path.exists(), "expected MCP handoff to write tcp pack");
    assert!(pack_entries(&pack_path).contains(&"handoff.json".to_string()));

    client.shutdown();
    let requests = fake_daemon.finish();
    let methods = requests
        .iter()
        .filter_map(|request| request.get("method").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        vec!["preload_root", "session_snapshot_create", "session_close"]
    );
}

#[test]
fn session_resume_accepts_tcp_pack_path_over_mcp() {
    let fixture = build_fixture_repo();
    let home = tempfile::tempdir().expect("home tempdir");
    let session_id = "session_mcp_resume_pack";
    let snapshot_id = "snap_mcp_resume_pack";
    let source_snapshot_dir = home.path().join("source-snapshot");
    write_snapshot_files_with_id(
        &source_snapshot_dir,
        snapshot_id,
        session_id,
        fixture.path(),
        None,
    );
    let pack_path = home.path().join("resume-pack.tcp");
    let binary = triseek_binary();
    let export = Command::new(&binary)
        .arg("pack")
        .arg("export")
        .arg(snapshot_id)
        .arg("--output")
        .arg(&pack_path)
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .output()
        .expect("run pack export");
    if !export.status.success() {
        std::fs::create_dir_all(
            home.path()
                .join(".triseek")
                .join("daemon")
                .join("snapshots"),
        )
        .expect("snapshots root");
        std::fs::rename(
            &source_snapshot_dir,
            home.path()
                .join(".triseek")
                .join("daemon")
                .join("snapshots")
                .join(snapshot_id),
        )
        .expect("move source snapshot");
        let export = Command::new(&binary)
            .arg("pack")
            .arg("export")
            .arg(snapshot_id)
            .arg("--output")
            .arg(&pack_path)
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .output()
            .expect("rerun pack export");
        assert!(
            export.status.success(),
            "pack export failed: {}",
            String::from_utf8_lossy(&export.stderr)
        );
        std::fs::remove_dir_all(
            home.path()
                .join(".triseek")
                .join("daemon")
                .join("snapshots")
                .join(snapshot_id),
        )
        .expect("remove original snapshot");
    }
    let fake_daemon = FakeDaemon::start(
        home.path(),
        vec![
            fake_preload_response(),
            json!({
                "session_id": session_id,
                "payload_markdown": "# TriSeek Hydration Payload\nfrom pack",
                "payload_token_estimate": 6,
                "hydration_report": {
                    "files_primed": 0,
                    "searches_warmed": 0,
                    "frecency_entries_restored": 0,
                    "stale_files": []
                },
                "searches": []
            }),
        ],
    );
    let mut client = McpClient::spawn_with_home_and_env(
        fixture.path(),
        Some(home.path()),
        &[("TRISEEK_MCP_DISABLE_STARTUP_SYNC", "1")],
    );
    handshake(&mut client);

    let envelope = call_tool(
        &mut client,
        "session_resume",
        json!({ "pack_path": pack_path }),
    );

    assert_eq!(envelope.get("session_id"), Some(&json!(session_id)));
    assert_eq!(
        envelope.get("imported_snapshot_id").and_then(Value::as_str),
        Some(snapshot_id)
    );
    assert_eq!(envelope.get("git_restored"), Some(&json!(false)));
    assert!(
        envelope
            .get("payload_markdown")
            .and_then(Value::as_str)
            .is_some_and(|payload| payload.contains("Hydration Payload"))
    );

    client.shutdown();
    let requests = fake_daemon.finish();
    let methods = requests
        .iter()
        .filter_map(|request| request.get("method").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(methods, vec!["preload_root", "session_resume_prepare"]);
}

#[test]
fn mcp_git_handoff_pack_resumes_in_second_clone() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let remote = tmp.path().join("remote.git");
    let clone_a = tmp.path().join("clone-a");
    let clone_b = tmp.path().join("clone-b");
    git_command(tmp.path(), &["init", "--bare", remote.to_str().unwrap()]);
    git_command(
        tmp.path(),
        &["clone", remote.to_str().unwrap(), clone_a.to_str().unwrap()],
    );
    git_command(&clone_a, &["config", "user.email", "triseek@example.com"]);
    git_command(&clone_a, &["config", "user.name", "TriSeek Test"]);
    std::fs::write(clone_a.join("README.md"), "initial\n").expect("write readme");
    git_command(&clone_a, &["add", "README.md"]);
    git_command(&clone_a, &["commit", "-m", "initial"]);
    git_command(&clone_a, &["push", "-u", "origin", "HEAD:main"]);
    git_command(&clone_a, &["switch", "-c", "feature"]);
    let commit = git_command_stdout(&clone_a, &["rev-parse", "HEAD"]);
    git_command(
        tmp.path(),
        &["clone", remote.to_str().unwrap(), clone_b.to_str().unwrap()],
    );
    git_command(&clone_b, &["switch", "main"]);

    let session_id = "session_mcp_git_two_clone";
    let snapshot_id = "snap_mcp_git_two_clone";
    let home_a = tempfile::tempdir().expect("home a");
    let snapshot_dir = home_a
        .path()
        .join(".triseek")
        .join("daemon")
        .join("snapshots")
        .join(snapshot_id);
    write_snapshot_files_with_id(
        &snapshot_dir,
        snapshot_id,
        session_id,
        &clone_a,
        Some(&commit),
    );
    let daemon_a = FakeDaemon::start(
        home_a.path(),
        vec![
            json!({
                "snapshot_id": snapshot_id,
                "snapshot_dir": snapshot_dir.display().to_string(),
                "manifest": {
                    "schema_version": PORTABILITY_SCHEMA_VERSION,
                    "snapshot_id": snapshot_id,
                    "session_id": session_id,
                    "created_at": 1770000000,
                    "repo_root": clone_a.display().to_string(),
                    "repo_commit": commit,
                    "repo_dirty_files": [],
                    "source_harness": "codex",
                    "source_model": null,
                    "generation": 1,
                    "context_epoch": 0
                }
            }),
            json!({"session": {"session_id": session_id}}),
        ],
    );
    let pack_path = tmp.path().join("mcp-handoff.tcp");
    let mut client_a = McpClient::spawn_with_home_and_env(
        &clone_a,
        Some(home_a.path()),
        &[("TRISEEK_MCP_DISABLE_STARTUP_SYNC", "1")],
    );
    handshake(&mut client_a);

    let handoff = call_tool(
        &mut client_a,
        "session_handoff",
        json!({
            "session_id": session_id,
            "mode": "git",
            "target_harness": "codex",
            "pack_output_path": pack_path
        }),
    );

    assert_eq!(handoff.get("snapshot_id"), Some(&json!(snapshot_id)));
    assert_eq!(
        handoff.pointer("/handoff/mode").and_then(Value::as_str),
        Some("git")
    );
    assert_eq!(
        handoff
            .pointer("/handoff/git/branch")
            .and_then(Value::as_str),
        Some("triseek/handoff/session_mcp_git_two_clone")
    );
    assert!(pack_path.exists(), "MCP git handoff should create tcp pack");
    assert!(pack_entries(&pack_path).contains(&"handoff.json".to_string()));
    assert_eq!(
        git_command_stdout(&clone_a, &["branch", "--show-current"]),
        "triseek/handoff/session_mcp_git_two_clone"
    );

    client_a.shutdown();
    let methods_a = daemon_a
        .finish()
        .iter()
        .filter_map(|request| request.get("method").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(methods_a, vec!["session_snapshot_create", "session_close"]);

    let home_b = tempfile::tempdir().expect("home b");
    let daemon_b = FakeDaemon::start(
        home_b.path(),
        vec![json!({
            "session_id": session_id,
            "payload_markdown": "# TriSeek Hydration Payload\nrestored",
            "payload_token_estimate": 5,
            "hydration_report": {
                "files_primed": 0,
                "searches_warmed": 0,
                "frecency_entries_restored": 0,
                "stale_files": []
            },
            "searches": []
        })],
    );
    let mut client_b = McpClient::spawn_with_home_and_env(
        &clone_b,
        Some(home_b.path()),
        &[("TRISEEK_MCP_DISABLE_STARTUP_SYNC", "1")],
    );
    handshake(&mut client_b);

    let resume = call_tool(
        &mut client_b,
        "session_resume",
        json!({ "pack_path": pack_path }),
    );

    assert_eq!(resume.get("session_id"), Some(&json!(session_id)));
    assert_eq!(
        resume.get("imported_snapshot_id").and_then(Value::as_str),
        Some(snapshot_id)
    );
    assert_eq!(resume.get("git_restored"), Some(&json!(true)));
    assert_eq!(
        git_command_stdout(&clone_b, &["branch", "--show-current"]),
        "triseek/handoff/session_mcp_git_two_clone"
    );
    assert_eq!(git_command_stdout(&clone_b, &["rev-parse", "HEAD"]), commit);

    client_b.shutdown();
    let methods_b = daemon_b
        .finish()
        .iter()
        .filter_map(|request| request.get("method").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(methods_b, vec!["session_resume_prepare"]);
}

#[test]
fn handoff_git_pack_resumes_in_second_clone() {
    let binary = triseek_binary();
    let tmp = tempfile::tempdir().expect("tempdir");
    let remote = tmp.path().join("remote.git");
    let clone_a = tmp.path().join("clone-a");
    let clone_b = tmp.path().join("clone-b");
    git_command(tmp.path(), &["init", "--bare", remote.to_str().unwrap()]);
    git_command(
        tmp.path(),
        &["clone", remote.to_str().unwrap(), clone_a.to_str().unwrap()],
    );
    git_command(&clone_a, &["config", "user.email", "triseek@example.com"]);
    git_command(&clone_a, &["config", "user.name", "TriSeek Test"]);
    std::fs::write(clone_a.join("README.md"), "initial\n").expect("write readme");
    git_command(&clone_a, &["add", "README.md"]);
    git_command(&clone_a, &["commit", "-m", "initial"]);
    git_command(&clone_a, &["push", "-u", "origin", "HEAD:main"]);
    git_command(&clone_a, &["switch", "-c", "feature"]);
    let commit = git_command_stdout(&clone_a, &["rev-parse", "HEAD"]);
    git_command(
        tmp.path(),
        &["clone", remote.to_str().unwrap(), clone_b.to_str().unwrap()],
    );
    git_command(&clone_b, &["switch", "main"]);

    let session_id = "session_two_clone";
    let snapshot_id = "snap_two_clone";
    let home_a = tempfile::tempdir().expect("home a");
    let snapshot_dir = home_a
        .path()
        .join(".triseek")
        .join("daemon")
        .join("snapshots")
        .join(snapshot_id);
    write_snapshot_files(&snapshot_dir, session_id, &clone_a, &commit);
    let daemon_a = FakeDaemon::start(
        home_a.path(),
        vec![
            json!({
                "snapshot_id": snapshot_id,
                "snapshot_dir": snapshot_dir.display().to_string(),
                "manifest": {
                    "schema_version": PORTABILITY_SCHEMA_VERSION,
                    "snapshot_id": snapshot_id,
                    "session_id": session_id,
                    "created_at": 1770000000,
                    "repo_root": clone_a.display().to_string(),
                    "repo_commit": commit,
                    "repo_dirty_files": [],
                    "source_harness": "codex",
                    "source_model": null,
                    "generation": 1,
                    "context_epoch": 0
                }
            }),
            json!({
                "snapshot": {
                    "manifest": {
                        "schema_version": PORTABILITY_SCHEMA_VERSION,
                        "snapshot_id": snapshot_id,
                        "session_id": session_id,
                        "created_at": 1770000000,
                        "repo_root": clone_a.display().to_string(),
                        "repo_commit": commit,
                        "repo_dirty_files": [],
                        "source_harness": "codex",
                        "source_model": null,
                        "generation": 1,
                        "context_epoch": 0
                    },
                    "working_set": {
                        "schema_version": PORTABILITY_SCHEMA_VERSION,
                        "files_read": [],
                        "searches_run": [],
                        "frecency_top_n": []
                    },
                    "action_log": [],
                    "pinned_snippets": []
                }
            }),
        ],
    );
    let pack_path = tmp.path().join("handoff.tcp");
    let handoff = Command::new(&binary)
        .arg("handoff")
        .arg("codex")
        .arg("--mode")
        .arg("git")
        .arg("--session")
        .arg(session_id)
        .arg("--output")
        .arg(&pack_path)
        .current_dir(&clone_a)
        .env("HOME", home_a.path())
        .env("USERPROFILE", home_a.path())
        .output()
        .expect("run triseek handoff");
    assert!(
        handoff.status.success(),
        "handoff failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&handoff.stdout),
        String::from_utf8_lossy(&handoff.stderr)
    );
    assert!(pack_path.exists(), "handoff should create tcp pack");
    assert_eq!(
        git_command_stdout(&clone_a, &["branch", "--show-current"]),
        "triseek/handoff/session_two_clone"
    );
    daemon_a.finish();

    let home_b = tempfile::tempdir().expect("home b");
    let daemon_b = FakeDaemon::start(
        home_b.path(),
        vec![json!({
            "session_id": session_id,
            "payload_markdown": "# TriSeek Hydration Payload\nrestored",
            "payload_token_estimate": 5,
            "hydration_report": {
                "files_primed": 0,
                "searches_warmed": 0,
                "frecency_entries_restored": 0,
                "stale_files": []
            },
            "searches": []
        })],
    );
    let resume = Command::new(&binary)
        .arg("resume")
        .arg(&pack_path)
        .current_dir(&clone_b)
        .env("HOME", home_b.path())
        .env("USERPROFILE", home_b.path())
        .output()
        .expect("run triseek resume");
    assert!(
        resume.status.success(),
        "resume failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resume.stdout),
        String::from_utf8_lossy(&resume.stderr)
    );
    assert!(
        String::from_utf8_lossy(&resume.stdout).contains("TriSeek Hydration Payload"),
        "resume should print hydration payload"
    );
    assert_eq!(
        git_command_stdout(&clone_b, &["branch", "--show-current"]),
        "triseek/handoff/session_two_clone"
    );
    assert_eq!(git_command_stdout(&clone_b, &["rev-parse", "HEAD"]), commit);
    daemon_b.finish();
}

#[test]
fn search_content_reuses_prior_result_when_fresh() {
    let fixture = build_fixture_repo();
    let home = tempfile::tempdir().expect("home tempdir");
    let fake_daemon = FakeDaemon::start(
        home.path(),
        vec![
            fake_preload_response(),
            fake_status_response(10, 1),
            fake_search_reuse_response(true, "unchanged", 10, 1, &[]),
        ],
    );
    let mut client = McpClient::spawn_with_home_and_env(
        fixture.path(),
        Some(home.path()),
        &[("TRISEEK_MCP_DISABLE_STARTUP_SYNC", "1")],
    );
    handshake(&mut client);

    let args = json!({ "query": "parse_arguments", "mode": "literal", "limit": 5 });

    let first = call_tool(&mut client, "search_content", args.clone());
    assert_eq!(
        first.get("cache").and_then(Value::as_str),
        Some("miss"),
        "first call must execute"
    );
    let first_search_id = first
        .get("search_id")
        .and_then(Value::as_str)
        .expect("first search_id")
        .to_string();

    let second = call_tool(&mut client, "search_content", args);
    assert_eq!(
        second.get("cache").and_then(Value::as_str),
        Some("hit"),
        "second identical call must reuse prior context"
    );
    assert_eq!(
        second.get("reuse_status").and_then(Value::as_str),
        Some("fresh_duplicate")
    );
    assert_eq!(second.get("results"), Some(&json!([])));
    assert_eq!(
        second.get("search_id").and_then(Value::as_str),
        Some(first_search_id.as_str())
    );
    fake_daemon.wait_for_requests(3, Duration::from_secs(2));
    let requests = fake_daemon.finish();
    assert_eq!(requests[0]["method"], json!("preload_root"));
    assert_eq!(requests[1]["method"], json!("status"));
    assert_eq!(requests[2]["method"], json!("search_reuse_check"));

    client.shutdown();
}

#[test]
fn search_content_reruns_when_matching_file_changes() {
    let fixture = build_fixture_repo();
    let home = tempfile::tempdir().expect("home tempdir");
    let fake_daemon = FakeDaemon::start(
        home.path(),
        vec![
            fake_preload_response(),
            fake_status_response(10, 1),
            fake_search_reuse_response(
                false,
                "changed_matched_path",
                11,
                1,
                &["src/cli/parser.rs"],
            ),
            fake_status_response(11, 1),
        ],
    );
    let mut client = McpClient::spawn_with_home_and_env(
        fixture.path(),
        Some(home.path()),
        &[("TRISEEK_MCP_DISABLE_STARTUP_SYNC", "1")],
    );
    handshake(&mut client);

    let args = json!({ "query": "parse_arguments", "mode": "literal", "limit": 5 });

    let first = call_tool(&mut client, "search_content", args.clone());
    assert!(
        first
            .get("search_id")
            .and_then(Value::as_str)
            .is_some_and(|search_id| !search_id.is_empty())
    );

    let second = call_tool(&mut client, "search_content", args);
    assert_eq!(second.get("cache").and_then(Value::as_str), Some("miss"));
    assert!(second.get("reuse_status").is_none());
    assert!(
        second
            .get("results")
            .and_then(Value::as_array)
            .is_some_and(|results| !results.is_empty())
    );
    fake_daemon.wait_for_requests(4, Duration::from_secs(2));
    let requests = fake_daemon.finish();
    assert_eq!(requests[0]["method"], json!("preload_root"));
    let methods: Vec<_> = requests
        .iter()
        .filter_map(|request| request.get("method").and_then(Value::as_str))
        .collect();
    assert!(
        methods.contains(&"search_reuse_check"),
        "expected search reuse freshness check in daemon requests: {methods:?}"
    );

    client.shutdown();
}

#[test]
fn different_meta_session_ids_do_not_share_search_memo() {
    let fixture = build_fixture_repo();
    let home = tempfile::tempdir().expect("home tempdir");
    let fake_daemon = FakeDaemon::start(
        home.path(),
        vec![
            fake_preload_response(),
            fake_status_response(10, 1),
            fake_status_response(10, 1),
        ],
    );
    let mut client = McpClient::spawn_with_home_and_env(
        fixture.path(),
        Some(home.path()),
        &[("TRISEEK_MCP_DISABLE_STARTUP_SYNC", "1")],
    );
    handshake(&mut client);

    let first = call_tool_with_meta(
        &mut client,
        "search_content",
        json!({ "query": "router", "mode": "literal" }),
        json!({ "sessionId": "session-a" }),
    );
    let second = call_tool_with_meta(
        &mut client,
        "search_content",
        json!({ "query": "router", "mode": "literal" }),
        json!({ "sessionId": "session-b" }),
    );
    assert_eq!(first.get("cache").and_then(Value::as_str), Some("miss"));
    assert_eq!(second.get("cache").and_then(Value::as_str), Some("miss"));
    fake_daemon.wait_for_requests(3, Duration::from_secs(2));
    let requests = fake_daemon.finish();
    assert_eq!(requests[0]["method"], json!("preload_root"));
    assert_eq!(requests[1]["method"], json!("status"));
    assert_eq!(requests[2]["method"], json!("status"));

    client.shutdown();
}

#[test]
fn reindex_invalidates_search_memo() {
    let fixture = build_fixture_repo();
    let home = tempfile::tempdir().expect("home tempdir");
    let fake_daemon = FakeDaemon::start(
        home.path(),
        vec![
            fake_preload_response(),
            fake_status_response(10, 1),
            fake_search_reuse_response(true, "unchanged", 10, 1, &[]),
            fake_status_response(10, 1),
        ],
    );
    let mut client = McpClient::spawn_with_home_and_env(
        fixture.path(),
        Some(home.path()),
        &[("TRISEEK_MCP_DISABLE_STARTUP_SYNC", "1")],
    );
    handshake(&mut client);

    let args = json!({ "query": "parse_arguments", "mode": "literal", "limit": 5 });

    let miss = call_tool(&mut client, "search_content", args.clone());
    assert_eq!(miss.get("cache").and_then(Value::as_str), Some("miss"));
    let reused = call_tool(&mut client, "search_content", args.clone());
    assert_eq!(reused.get("cache").and_then(Value::as_str), Some("hit"));
    assert_eq!(
        reused.get("reuse_status").and_then(Value::as_str),
        Some("fresh_duplicate")
    );

    let reindex = call_tool(&mut client, "reindex", json!({ "mode": "incremental" }));
    assert_eq!(reindex.get("completed"), Some(&json!(true)));

    let after = call_tool(&mut client, "search_content", args);
    assert_eq!(
        after.get("cache").and_then(Value::as_str),
        Some("miss"),
        "search memo must be cleared after reindex"
    );
    assert!(after.get("reuse_status").is_none());
    fake_daemon.wait_for_requests(4, Duration::from_secs(2));
    let requests = fake_daemon.finish();
    assert_eq!(requests[0]["method"], json!("preload_root"));
    assert_eq!(requests[1]["method"], json!("status"));
    assert_eq!(requests[2]["method"], json!("search_reuse_check"));
    assert_eq!(requests[3]["method"], json!("status"));

    client.shutdown();
}

#[test]
fn non_indexed_fallback_bypasses_cache() {
    // Non-indexed fallback results must never be cached. Disable startup sync
    // so this covers fallback behavior without depending on `rg` being
    // installed on CI or racing the background index build.
    let fixture = build_unindexed_fixture_repo();
    let mut client = McpClient::spawn_without_startup_sync(fixture.path());
    handshake(&mut client);

    let args = json!({ "query": "McpState", "mode": "literal" });

    let first = call_tool(&mut client, "search_content", args.clone());
    assert_eq!(first.get("fallback_used"), Some(&json!(true)));
    assert_eq!(
        first.get("cache").and_then(Value::as_str),
        Some("bypass"),
        "fallback results must not be cached"
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
            json!({"preloaded": true}),
            json!({"reloaded": true}),
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
    daemon.wait_for_requests(2, Duration::from_secs(5));

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
        4,
        "memo_check should make preload_root + reload + 2 daemon RPC calls"
    );
    assert_eq!(
        requests[0].pointer("/method").and_then(Value::as_str),
        Some("preload_root")
    );
    assert_eq!(
        requests[1].pointer("/method").and_then(Value::as_str),
        Some("reload")
    );
    assert_eq!(
        requests[2].pointer("/method").and_then(Value::as_str),
        Some("memo_session_start")
    );
    assert_eq!(
        requests[2]
            .pointer("/params/session_id")
            .and_then(Value::as_str),
        Some("codex-meta-session")
    );
    assert_eq!(
        requests[2]
            .pointer("/params/repo_root")
            .and_then(Value::as_str),
        Some(expected_repo_root.to_str().expect("fixture path str"))
    );
    assert_eq!(
        requests[3].pointer("/method").and_then(Value::as_str),
        Some("memo_check")
    );
    assert_eq!(
        requests[3]
            .pointer("/params/session_id")
            .and_then(Value::as_str),
        Some("codex-meta-session")
    );
    assert_eq!(
        requests[3].pointer("/params/path").and_then(Value::as_str),
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
            json!({"preloaded": true}),
            json!({"reloaded": true}),
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
    daemon.wait_for_requests(2, Duration::from_secs(5));

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
        requests[1].pointer("/method").and_then(Value::as_str),
        Some("reload")
    );
    assert_eq!(
        requests[2]
            .pointer("/params/session_id")
            .and_then(Value::as_str),
        Some("claude-meta-session")
    );
    assert_eq!(
        requests[0].pointer("/method").and_then(Value::as_str),
        Some("preload_root")
    );
    assert_eq!(
        requests[2].pointer("/method").and_then(Value::as_str),
        Some("memo_session_start")
    );
    assert_eq!(
        requests[3].pointer("/method").and_then(Value::as_str),
        Some("memo_status")
    );
    assert_eq!(
        requests[3]
            .pointer("/params/files/0")
            .and_then(Value::as_str),
        Some("src/auth/router.rs")
    );
}
