use search_core::{CaseMode, QueryRequest, SearchEngineKind, SearchKind};
use search_index::{BuildConfig, SearchEngine};
use std::fs;
use std::path::Path;
use std::process::Command;
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

struct FixtureRepo {
    root: TempDir,
}

impl FixtureRepo {
    fn new() -> Self {
        let root = TempDir::new().expect("tempdir");
        fs::create_dir_all(root.path().join("src")).expect("src dir");
        fs::write(root.path().join(".gitignore"), "ignored.txt\n").expect("gitignore");
        fs::write(
            root.path().join("src/lib.rs"),
            "pub struct WidgetBuilder;\npub fn new_widget() -> WidgetBuilder { WidgetBuilder }\n",
        )
        .expect("lib");
        fs::write(
            root.path().join("src/main.rs"),
            "fn main() { println!(\"hello\"); }\n",
        )
        .expect("main");
        fs::write(root.path().join("ignored.txt"), "should be ignored\n").expect("ignored");
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
