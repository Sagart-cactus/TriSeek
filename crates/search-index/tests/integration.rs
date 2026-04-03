use search_core::{CaseMode, QueryRequest, SearchEngineKind, SearchKind};
use search_index::{BuildConfig, SearchEngine, start_watcher};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn build_search_and_update_index() {
    let fixture = FixtureRepo::new();
    let index_dir = fixture.root.path().join(".triseek-index");

    SearchEngine::build(
        fixture.root.path(),
        Some(&index_dir),
        &BuildConfig::default(),
    )
    .expect("build succeeds");
    let engine = SearchEngine::open(&index_dir).expect("index opens");

    let literal = engine
        .search(&QueryRequest {
            kind: SearchKind::Literal,
            engine: SearchEngineKind::Indexed,
            pattern: "WidgetBuilder".to_string(),
            case_mode: CaseMode::Sensitive,
            max_results: Some(20),
            ..QueryRequest::default()
        })
        .expect("literal search succeeds");
    assert_eq!(literal.summary.files_with_matches, 1);

    let regex = engine
        .search(&QueryRequest {
            kind: SearchKind::Regex,
            engine: SearchEngineKind::Indexed,
            pattern: r"\bfn\b".to_string(),
            case_mode: CaseMode::Sensitive,
            max_results: Some(20),
            ..QueryRequest::default()
        })
        .expect("regex search succeeds");
    assert!(regex.summary.files_with_matches >= 1);

    let path = engine
        .search(&QueryRequest {
            kind: SearchKind::Path,
            engine: SearchEngineKind::Indexed,
            pattern: "src".to_string(),
            case_mode: CaseMode::Sensitive,
            ..QueryRequest::default()
        })
        .expect("path search succeeds");
    assert_eq!(path.summary.files_with_matches, 2);

    fs::write(
        fixture.root.path().join("src/lib.rs"),
        "pub fn replacement_token() { println!(\"updated\"); }\n",
    )
    .expect("write update");
    SearchEngine::update(
        fixture.root.path(),
        Some(&index_dir),
        &BuildConfig::default(),
    )
    .expect("update succeeds");
    let updated = SearchEngine::open(&index_dir).expect("updated index opens");
    let updated_result = updated
        .search(&QueryRequest {
            kind: SearchKind::Literal,
            engine: SearchEngineKind::Indexed,
            pattern: "replacement_token".to_string(),
            case_mode: CaseMode::Sensitive,
            max_results: Some(10),
            ..QueryRequest::default()
        })
        .expect("updated search succeeds");
    assert_eq!(updated_result.summary.files_with_matches, 1);
}

#[test]
fn incremental_update_is_visible_when_fast_index_exists() {
    let fixture = FixtureRepo::with_files(
        (1..=10)
            .map(|idx| {
                (
                    format!("src/file_{idx}.rs"),
                    format!("pub fn file_{idx}() {{}}\n"),
                )
            })
            .collect(),
    );
    let index_dir = fixture.root.path().join(".triseek-index");

    SearchEngine::build(
        fixture.root.path(),
        Some(&index_dir),
        &BuildConfig::default(),
    )
    .expect("build succeeds");

    fs::write(
        fixture.root.path().join("src/file_1.rs"),
        "pub fn replacement_token() {}\n",
    )
    .expect("write update");

    let outcome = SearchEngine::update(
        fixture.root.path(),
        Some(&index_dir),
        &BuildConfig::default(),
    )
    .expect("update succeeds");
    assert!(
        !outcome.rebuilt_full,
        "test should exercise delta update path"
    );

    let engine = SearchEngine::open(&index_dir).expect("updated index opens");
    let result = engine
        .search(&QueryRequest {
            kind: SearchKind::Literal,
            engine: SearchEngineKind::Indexed,
            pattern: "replacement_token".to_string(),
            case_mode: CaseMode::Sensitive,
            ..QueryRequest::default()
        })
        .expect("search succeeds");
    assert_eq!(result.summary.files_with_matches, 1);
}

#[test]
fn regex_alternation_returns_all_matching_branches() {
    let fixture = FixtureRepo::with_files(vec![
        ("src/a.txt".to_string(), "panic only\n".to_string()),
        ("src/b.txt".to_string(), "fatal only\n".to_string()),
        ("src/c.txt".to_string(), "abort only\n".to_string()),
    ]);
    let index_dir = fixture.root.path().join(".triseek-index");

    SearchEngine::build(
        fixture.root.path(),
        Some(&index_dir),
        &BuildConfig::default(),
    )
    .expect("build succeeds");
    let engine = SearchEngine::open(&index_dir).expect("index opens");

    let result = engine
        .search(&QueryRequest {
            kind: SearchKind::Regex,
            engine: SearchEngineKind::Indexed,
            pattern: "panic|fatal|abort".to_string(),
            case_mode: CaseMode::Sensitive,
            ..QueryRequest::default()
        })
        .expect("regex search succeeds");
    assert_eq!(result.summary.files_with_matches, 3);
}

#[test]
fn max_results_is_respected_during_parallel_verification() {
    let fixture = FixtureRepo::with_files(
        (1..=20)
            .map(|idx| {
                (
                    format!("src/file_{idx}.rs"),
                    format!("pub fn hit_{idx}() {{ println!(\"needle\"); }}\n"),
                )
            })
            .collect(),
    );
    let index_dir = fixture.root.path().join(".triseek-index");

    SearchEngine::build(
        fixture.root.path(),
        Some(&index_dir),
        &BuildConfig::default(),
    )
    .expect("build succeeds");
    let engine = SearchEngine::open(&index_dir).expect("index opens");

    let result = engine
        .search(&QueryRequest {
            kind: SearchKind::Literal,
            engine: SearchEngineKind::Indexed,
            pattern: "needle".to_string(),
            case_mode: CaseMode::Sensitive,
            max_results: Some(1),
            ..QueryRequest::default()
        })
        .expect("limited search succeeds");
    assert_eq!(result.summary.total_line_matches, 1);
    assert_eq!(result.summary.files_with_matches, 1);
}

#[test]
fn invalid_utf8_control_heavy_files_are_treated_as_binary() {
    let fixture = FixtureRepo::with_files(vec![(
        "src/lib.rs".to_string(),
        "pub fn searchable() { println!(\"needle\"); }\n".to_string(),
    )]);
    let binary_path = fixture.root.path().join("vendor/swagger.pb");
    fs::create_dir_all(binary_path.parent().expect("vendor dir")).expect("vendor dir");
    fs::write(
        &binary_path,
        [
            0x0b, b's', b'i', b'd', b'e', b'E', b'f', b'f', b'e', b'c', b't', b's', 0x12, 0xff,
            0xfe, 0x08, 0x07, b'O', b't', b'h', b'e', b'r',
        ],
    )
    .expect("binary write");
    let index_dir = fixture.root.path().join(".triseek-index");

    SearchEngine::build(
        fixture.root.path(),
        Some(&index_dir),
        &BuildConfig::default(),
    )
    .expect("build succeeds");
    let engine = SearchEngine::open(&index_dir).expect("index opens");

    let result = engine
        .search(&QueryRequest {
            kind: SearchKind::Literal,
            engine: SearchEngineKind::Indexed,
            pattern: "sideEffects".to_string(),
            case_mode: CaseMode::Sensitive,
            ..QueryRequest::default()
        })
        .expect("search succeeds");
    assert_eq!(result.summary.files_with_matches, 0);
}

struct FixtureRepo {
    root: TempDir,
}

impl FixtureRepo {
    fn new() -> Self {
        Self::with_files(vec![
            (
                "src/lib.rs".to_string(),
                "pub struct WidgetBuilder;\npub fn new_widget() -> WidgetBuilder { WidgetBuilder }\n"
                    .to_string(),
            ),
            (
                "src/main.rs".to_string(),
                "fn main() { println!(\"hello\"); }\n".to_string(),
            ),
            ("ignored.txt".to_string(), "should be ignored\n".to_string()),
        ])
    }

    fn with_files(files: Vec<(String, String)>) -> Self {
        let root = TempDir::new().expect("tempdir");
        fs::create_dir_all(root.path().join("src")).expect("src dir");
        fs::write(root.path().join(".gitignore"), "ignored.txt\n").expect("gitignore");
        for (relative_path, contents) in files {
            let path = root.path().join(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent dir");
            }
            fs::write(path, contents).expect("fixture file");
        }
        git_init(root.path());
        Self { root }
    }
}

fn git_init(path: &Path) {
    run(path, &["git", "init"]);
    run(
        path,
        &["git", "config", "user.email", "triseek@example.com"],
    );
    run(path, &["git", "config", "user.name", "TriSeek"]);
    run(path, &["git", "add", "."]);
    run(path, &["git", "commit", "-m", "fixture"]);
}

fn run(path: &Path, args: &[&str]) {
    let status = Command::new(args[0])
        .args(&args[1..])
        .current_dir(path)
        .status()
        .expect("command runs");
    assert!(status.success());
}

/// Verify that the background watcher detects a file modification and increments the generation.
#[test]
fn watcher_detects_file_modification() {
    let fixture = FixtureRepo::new();
    // Canonicalize to resolve macOS /tmp → /private/tmp symlinks.
    let repo_root = fixture.root.path().canonicalize().expect("canonicalize");
    let index_dir = repo_root.join(".triseek-index");
    let config = BuildConfig::default();

    // Build an initial index.
    SearchEngine::build(&repo_root, Some(&index_dir), &config).expect("build");

    // Start the watcher.
    let handle = start_watcher(repo_root.clone(), index_dir.clone(), config.clone())
        .expect("watcher starts");

    let gen_before = handle.generation.load(Ordering::SeqCst);

    // Modify a tracked file.
    std::thread::sleep(Duration::from_millis(100)); // let the watcher settle
    let target = repo_root.join("src/lib.rs");
    let original = fs::read_to_string(&target).expect("read original");
    fs::write(&target, format!("{original}// watcher_test_marker\n")).expect("write");

    // Wait for the watcher to pick up the change (debounce = 500ms + processing time).
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        std::thread::sleep(Duration::from_millis(100));
        if handle.generation.load(Ordering::SeqCst) > gen_before {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("watcher did not update the index within 5s");
        }
    }

    // Reload the engine and verify the new content is searchable.
    let engine = SearchEngine::open(&index_dir).expect("reopen");
    let results = engine
        .search(&QueryRequest {
            kind: SearchKind::Literal,
            engine: SearchEngineKind::Indexed,
            pattern: "watcher_test_marker".to_string(),
            case_mode: CaseMode::Sensitive,
            ..QueryRequest::default()
        })
        .expect("search");

    handle.stop();

    assert!(
        results.summary.files_with_matches > 0,
        "watcher_test_marker should be searchable after watcher update"
    );
}
