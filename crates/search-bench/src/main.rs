use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use regex::Regex;
use search_core::{
    MachineInfo, RepoStats, SearchHit, SearchResponse, SearchSummary, SessionQuery, classify_repo,
};
use search_index::{BuildConfig, ScanOptions, ScannedFile, measure_repository, walk_repository};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use tempfile::NamedTempFile;
use time::OffsetDateTime;

#[derive(Parser)]
#[command(name = "search-bench")]
#[command(about = "TriSeek benchmark harness")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Prepare(PrepareArgs),
    Run(RunArgs),
}

#[derive(Args)]
struct PrepareArgs {
    #[arg(long, default_value = "bench/manifest/repositories.yaml")]
    manifest: PathBuf,
    #[arg(long, default_value = "bench/results/prepared_repos.json")]
    output: PathBuf,
    #[arg(long)]
    repo_limit: Option<usize>,
}

#[derive(Args)]
struct RunArgs {
    #[arg(long, default_value = "bench/manifest/repositories.yaml")]
    manifest: PathBuf,
    #[arg(long)]
    output_dir: Option<PathBuf>,
    #[arg(long)]
    search_cli_bin: Option<PathBuf>,
    #[arg(long)]
    repo_limit: Option<usize>,
    #[arg(long, default_value_t = 5)]
    cold_iterations: usize,
    #[arg(long, default_value_t = 10)]
    warm_iterations: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    cache_root: PathBuf,
    repositories: Vec<ManifestRepo>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestRepo {
    name: String,
    slug: String,
    url: String,
    #[serde(default)]
    pinned_commit: Option<String>,
    local_path: PathBuf,
    status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PreparedRepo {
    name: String,
    slug: String,
    url: String,
    local_path: PathBuf,
    commit_sha: String,
    stats: RepoStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BenchKind {
    Literal,
    Regex,
    Path,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchQuery {
    label: String,
    family: String,
    kind: BenchKind,
    pattern: String,
    extensions: Vec<String>,
    exact_names: Vec<String>,
    path_substrings: Vec<String>,
    max_results: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
struct CommandSpec {
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
struct IterationMetrics {
    wall_millis: f64,
    user_cpu_millis: Option<f64>,
    system_cpu_millis: Option<f64>,
    max_rss_kib: Option<u64>,
    exit_code: i32,
}

#[derive(Debug, Clone, Serialize)]
struct AggregateMetrics {
    p50_wall_millis: f64,
    p95_wall_millis: f64,
    p99_wall_millis: f64,
    mean_wall_millis: f64,
    min_wall_millis: f64,
    max_wall_millis: f64,
    mean_user_cpu_millis: Option<f64>,
    mean_system_cpu_millis: Option<f64>,
    max_rss_kib: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct TimedCase {
    cold: Vec<IterationMetrics>,
    warm: Vec<IterationMetrics>,
    aggregate: AggregateMetrics,
    command: CommandSpec,
}

#[derive(Debug, Clone, Serialize)]
struct CorrectnessSummary {
    passed: bool,
    indexed_count: usize,
    baseline_count: usize,
    missing: Vec<String>,
    unexpected: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CaseReport {
    repo_slug: String,
    repo_category: String,
    query: BenchQuery,
    baseline_tool: String,
    correctness: CorrectnessSummary,
    indexed: TimedCase,
    baseline: TimedCase,
}

#[derive(Debug, Clone, Serialize)]
struct BuildReport {
    repo_slug: String,
    build: IterationMetrics,
    index_bytes: u64,
    build_millis: u128,
}

#[derive(Debug, Clone, Serialize)]
struct UpdateReport {
    repo_slug: String,
    file_path: String,
    marker: String,
    update: IterationMetrics,
    delta_docs: u64,
    delta_removed_paths: u64,
    verification_summary: SearchSummary,
}

#[derive(Debug, Clone, Serialize)]
struct SessionReport {
    repo_slug: String,
    label: String,
    indexed: TimedCase,
    baseline: TimedCase,
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    generated_at: String,
    machine: MachineInfo,
    manifest_path: String,
    prepared_repos: Vec<PreparedRepo>,
    build_reports: Vec<BuildReport>,
    case_reports: Vec<CaseReport>,
    session_reports: Vec<SessionReport>,
    update_reports: Vec<UpdateReport>,
}

#[derive(Debug, Clone)]
struct RepoSample {
    dominant_extension: Option<String>,
    exact_name: Option<String>,
    path_substring: Option<String>,
    selective_literal: Option<String>,
    moderate_literal: Option<String>,
    high_literal: Option<String>,
    top_tokens: Vec<String>,
    update_target: Option<(String, String)>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Prepare(args) => {
            let prepared = prepare_repositories(&args.manifest, args.repo_limit)?;
            write_json(&args.output, &prepared)?;
        }
        Commands::Run(args) => run_benchmarks(args)?,
    }
    Ok(())
}

fn prepare_repositories(
    manifest_path: &Path,
    repo_limit: Option<usize>,
) -> Result<Vec<PreparedRepo>> {
    let manifest = load_manifest(manifest_path)?;
    let repos = manifest
        .repositories
        .iter()
        .take(repo_limit.unwrap_or(usize::MAX))
        .cloned()
        .collect::<Vec<_>>();

    let mut prepared = Vec::new();
    for repo in repos {
        ensure_repo_present(&repo)?;
        let commit_sha = if let Some(commit) = repo.pinned_commit.as_deref() {
            checkout_commit(&repo.local_path, commit)?;
            git_head(&repo.local_path)?
        } else {
            git_head(&repo.local_path)?
        };
        let stats =
            measure_repository(&repo.local_path, &BuildConfig::default()).with_context(|| {
                format!("failed to measure repository {}", repo.local_path.display())
            })?;
        prepared.push(PreparedRepo {
            name: repo.name,
            slug: repo.slug,
            url: repo.url,
            local_path: repo.local_path,
            commit_sha,
            stats,
        });
    }
    Ok(prepared)
}

fn run_benchmarks(args: RunArgs) -> Result<()> {
    let prepared = prepare_repositories(&args.manifest, args.repo_limit)?;
    let output_dir = args
        .output_dir
        .unwrap_or_else(|| PathBuf::from(format!("bench/results/run-{}", timestamp_slug())));
    fs::create_dir_all(&output_dir)?;
    let machine = collect_machine_info()?;
    let search_cli_bin = resolve_search_cli_bin(args.search_cli_bin)?;
    let index_root = manifest_index_root(&load_manifest(&args.manifest)?);
    fs::create_dir_all(&index_root)?;

    let mut build_reports = Vec::new();
    let mut case_reports = Vec::new();
    let mut session_reports = Vec::new();
    let mut update_reports = Vec::new();

    for repo in &prepared {
        let sample = sample_repository(repo)?;
        let queries = generate_queries(repo, &sample);
        let repo_index_dir = index_root.join(&repo.slug);
        fs::create_dir_all(&repo_index_dir)?;

        let build_command =
            indexed_build_command(&search_cli_bin, &repo.local_path, &repo_index_dir);
        let build_run = run_timed_command(&build_command)?;
        let build_stdout = run_command_capture(&build_command)?;
        let metadata: Value = serde_json::from_slice(&build_stdout.stdout)?;
        let index_metadata = metadata["metadata"].clone();
        build_reports.push(BuildReport {
            repo_slug: repo.slug.clone(),
            build: build_run,
            index_bytes: index_metadata["build_stats"]["index_bytes"]
                .as_u64()
                .unwrap_or_default(),
            build_millis: index_metadata["build_stats"]["build_millis"]
                .as_u64()
                .unwrap_or_default() as u128,
        });

        for query in &queries {
            let mut correctness_query = query.clone();
            correctness_query.max_results = None;
            let indexed_full = indexed_search_command(
                &search_cli_bin,
                &repo.local_path,
                &repo_index_dir,
                &correctness_query,
                false,
            );
            let indexed_summary = indexed_search_command(
                &search_cli_bin,
                &repo.local_path,
                &repo_index_dir,
                query,
                true,
            );
            let baseline_spec = baseline_command(repo, query);
            let correctness_baseline_command = if matches!(query.kind, BenchKind::Path) {
                scan_search_command(&search_cli_bin, &repo.local_path, &correctness_query)
            } else {
                baseline_command(repo, &correctness_query)
            };

            let indexed_response =
                parse_search_response(&run_command_capture(&indexed_full)?.stdout)?;
            let baseline_result =
                run_baseline_once(&correctness_baseline_command, &correctness_query)?;
            let correctness =
                compare_results(&indexed_response, &baseline_result, &correctness_query);

            let indexed_case =
                benchmark_command(&indexed_summary, args.cold_iterations, args.warm_iterations)?;
            let baseline_case =
                benchmark_command(&baseline_spec, args.cold_iterations, args.warm_iterations)?;
            case_reports.push(CaseReport {
                repo_slug: repo.slug.clone(),
                repo_category: format!(
                    "{:?}",
                    repo.stats.category.unwrap_or_else(|| classify_repo(
                        repo.stats.searchable_files,
                        repo.stats.searchable_bytes
                    ))
                )
                .to_ascii_lowercase(),
                query: query.clone(),
                baseline_tool: baseline_tool_for_query(query).to_string(),
                correctness,
                indexed: indexed_case,
                baseline: baseline_case,
            });
        }

        let session_queries_20 = session_queries(&queries, 20);
        let session_queries_100 = session_queries(&queries, 100);
        session_reports.push(benchmark_session(
            &search_cli_bin,
            repo,
            &repo_index_dir,
            "session_20",
            &session_queries_20,
            args.cold_iterations,
            args.warm_iterations,
        )?);
        session_reports.push(benchmark_session(
            &search_cli_bin,
            repo,
            &repo_index_dir,
            "session_100",
            &session_queries_100,
            args.cold_iterations,
            args.warm_iterations,
        )?);

        if let Some(update_report) =
            benchmark_update(&search_cli_bin, repo, &repo_index_dir, &sample)?
        {
            update_reports.push(update_report);
        }
    }

    let report = BenchmarkReport {
        generated_at: timestamp_now(),
        machine,
        manifest_path: args.manifest.display().to_string(),
        prepared_repos: prepared,
        build_reports,
        case_reports,
        session_reports,
        update_reports,
    };

    write_json(&output_dir.join("report.json"), &report)?;
    write_case_csv(&output_dir.join("report.csv"), &report)?;
    write_summary_markdown(&output_dir.join("summary.md"), &report)?;
    Ok(())
}

fn load_manifest(path: &Path) -> Result<Manifest> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(serde_yaml::from_slice(&bytes)?)
}

fn manifest_index_root(manifest: &Manifest) -> PathBuf {
    manifest
        .cache_root
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("indexes")
}

fn ensure_repo_present(repo: &ManifestRepo) -> Result<()> {
    if repo.local_path.exists() {
        return Ok(());
    }
    if repo.status == "pending" || repo.status == "ready" {
        if let Some(parent) = repo.local_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let status = Command::new("git")
            .arg("clone")
            .arg("--filter=blob:none")
            .arg("--no-tags")
            .arg(&repo.url)
            .arg(&repo.local_path)
            .status()
            .with_context(|| format!("failed to clone {}", repo.url))?;
        if !status.success() {
            bail!("git clone failed for {}", repo.url);
        }
    }
    Ok(())
}

fn checkout_commit(repo_path: &Path, commit: &str) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("checkout")
        .arg("--detach")
        .arg(commit)
        .status()?;
    if !status.success() {
        bail!("failed to checkout {commit} in {}", repo_path.display());
    }
    Ok(())
}

fn git_head(repo_path: &Path) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg("HEAD")
        .output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn sample_repository(repo: &PreparedRepo) -> Result<RepoSample> {
    let token_re = Regex::new(r"[A-Za-z_][A-Za-z0-9_]{3,}")?;
    let generic_dirs: HashSet<&str> = [
        "src", "test", "tests", "cmd", "pkg", "internal", "lib", "include", "docs", "doc",
        "examples", "vendor", "crates",
    ]
    .into_iter()
    .collect();
    let mut token_docs = HashMap::<String, usize>::new();
    let mut exact_name = None;
    let mut update_target = None;
    let mut dir_counts = HashMap::<String, usize>::new();
    let mut sampled_files = 0_usize;

    walk_repository(
        &repo.local_path,
        &ScanOptions::from(&BuildConfig::default()),
        |file: ScannedFile| {
            if exact_name.is_none() {
                exact_name = Some(file.file_name.clone());
            }
            if update_target.is_none() {
                update_target = Some((
                    file.relative_path.clone(),
                    file.extension.clone().unwrap_or_default(),
                ));
            }
            if sampled_files < 250 {
                let text = String::from_utf8_lossy(&file.contents);
                let mut unique = HashSet::new();
                for capture in token_re.find_iter(&text) {
                    let token = capture.as_str();
                    if token.len() <= 64 {
                        unique.insert(token.to_string());
                    }
                }
                for token in unique {
                    *token_docs.entry(token).or_default() += 1;
                }
                for component in file.relative_path.split('/').take(3) {
                    if component.len() > 2 && !generic_dirs.contains(component) {
                        *dir_counts.entry(component.to_string()).or_default() += 1;
                    }
                }
            }
            sampled_files += 1;
            Ok(())
        },
    )?;

    let dominant_extension = repo
        .stats
        .languages
        .iter()
        .find_map(|(language, _)| (language != "<none>").then_some(language.clone()));
    let path_substring = dir_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(token, _)| token);

    let mut tokens: Vec<(String, usize)> = token_docs.into_iter().collect();
    tokens.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let selective_literal = pick_token(&tokens, 1, 3, 6);
    let moderate_literal = pick_token(&tokens, 5, 50, 4).or_else(|| pick_token(&tokens, 2, 100, 4));
    let high_literal = pick_token(&tokens, 100, usize::MAX, 4)
        .or_else(|| tokens.first().map(|(token, _)| token.clone()));
    let top_tokens = tokens
        .iter()
        .take(5)
        .map(|(token, _)| token.clone())
        .collect();

    Ok(RepoSample {
        dominant_extension,
        exact_name,
        path_substring,
        selective_literal,
        moderate_literal,
        high_literal,
        top_tokens,
        update_target,
    })
}

fn pick_token(
    tokens: &[(String, usize)],
    min_df: usize,
    max_df: usize,
    min_len: usize,
) -> Option<String> {
    tokens
        .iter()
        .find(|(token, df)| *df >= min_df && *df <= max_df && token.len() >= min_len)
        .map(|(token, _)| token.clone())
}

fn generate_queries(repo: &PreparedRepo, sample: &RepoSample) -> Vec<BenchQuery> {
    let dominant_ext = sample
        .dominant_extension
        .clone()
        .unwrap_or_else(|| "rs".to_string());
    let selective = sample
        .selective_literal
        .clone()
        .unwrap_or_else(|| "TriSeekSelectiveToken".to_string());
    let moderate = sample
        .moderate_literal
        .clone()
        .unwrap_or_else(|| "search".to_string());
    let high = sample
        .high_literal
        .clone()
        .unwrap_or_else(|| "test".to_string());
    let exact_name = sample
        .exact_name
        .clone()
        .unwrap_or_else(|| "README.md".to_string());
    let path_substring = sample
        .path_substring
        .clone()
        .unwrap_or_else(|| repo.slug.clone());
    let or_tokens = sample
        .top_tokens
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>();
    let or_pattern = if or_tokens.len() >= 3 {
        format!("{}|{}|{}", or_tokens[0], or_tokens[1], or_tokens[2])
    } else {
        "error|test|build".to_string()
    };
    let anchor_pattern = format!(r"\b{}\b", regex::escape(&moderate));

    vec![
        BenchQuery {
            label: "path_all".to_string(),
            family: "file_listing".to_string(),
            kind: BenchKind::Path,
            pattern: String::new(),
            extensions: vec![],
            exact_names: vec![],
            path_substrings: vec![],
            max_results: None,
        },
        BenchQuery {
            label: "path_suffix".to_string(),
            family: "file_listing".to_string(),
            kind: BenchKind::Path,
            pattern: String::new(),
            extensions: vec![dominant_ext.clone()],
            exact_names: vec![],
            path_substrings: vec![],
            max_results: None,
        },
        BenchQuery {
            label: "path_exact_name".to_string(),
            family: "file_listing".to_string(),
            kind: BenchKind::Path,
            pattern: String::new(),
            extensions: vec![],
            exact_names: vec![exact_name],
            path_substrings: vec![],
            max_results: None,
        },
        BenchQuery {
            label: "path_substring".to_string(),
            family: "file_listing".to_string(),
            kind: BenchKind::Path,
            pattern: path_substring,
            extensions: vec![],
            exact_names: vec![],
            path_substrings: vec![],
            max_results: None,
        },
        BenchQuery {
            label: "literal_selective".to_string(),
            family: "literal_search".to_string(),
            kind: BenchKind::Literal,
            pattern: selective.clone(),
            extensions: vec![],
            exact_names: vec![],
            path_substrings: vec![],
            max_results: Some(50),
        },
        BenchQuery {
            label: "literal_moderate".to_string(),
            family: "literal_search".to_string(),
            kind: BenchKind::Literal,
            pattern: moderate.clone(),
            extensions: vec![],
            exact_names: vec![],
            path_substrings: vec![],
            max_results: Some(200),
        },
        BenchQuery {
            label: "literal_high".to_string(),
            family: "literal_search".to_string(),
            kind: BenchKind::Literal,
            pattern: high,
            extensions: vec![],
            exact_names: vec![],
            path_substrings: vec![],
            max_results: Some(500),
        },
        BenchQuery {
            label: "regex_anchor".to_string(),
            family: "regex_search".to_string(),
            kind: BenchKind::Regex,
            pattern: anchor_pattern,
            extensions: vec![],
            exact_names: vec![],
            path_substrings: vec![],
            max_results: Some(200),
        },
        BenchQuery {
            label: "regex_weak".to_string(),
            family: "regex_search".to_string(),
            kind: BenchKind::Regex,
            pattern: "[A-Za-z_][A-Za-z0-9_]{10,}".to_string(),
            extensions: vec![],
            exact_names: vec![],
            path_substrings: vec![],
            max_results: Some(200),
        },
        BenchQuery {
            label: "literal_no_match".to_string(),
            family: "literal_search".to_string(),
            kind: BenchKind::Literal,
            pattern: format!("TRISEEK_NO_MATCH_{}", repo.slug.to_uppercase()),
            extensions: vec![],
            exact_names: vec![],
            path_substrings: vec![],
            max_results: Some(5),
        },
        BenchQuery {
            label: "regex_no_match".to_string(),
            family: "regex_search".to_string(),
            kind: BenchKind::Regex,
            pattern: "TRISEEK_[A-Z]{12,}".to_string(),
            extensions: vec![],
            exact_names: vec![],
            path_substrings: vec![],
            max_results: Some(5),
        },
        BenchQuery {
            label: "multi_or".to_string(),
            family: "mixed_search".to_string(),
            kind: BenchKind::Regex,
            pattern: or_pattern,
            extensions: vec![],
            exact_names: vec![],
            path_substrings: vec![],
            max_results: Some(200),
        },
        BenchQuery {
            label: "path_plus_content".to_string(),
            family: "mixed_search".to_string(),
            kind: BenchKind::Literal,
            pattern: selective,
            extensions: vec![dominant_ext],
            exact_names: vec![],
            path_substrings: vec![],
            max_results: Some(100),
        },
    ]
}

fn session_queries(queries: &[BenchQuery], size: usize) -> Vec<BenchQuery> {
    let content_queries: Vec<_> = queries
        .iter()
        .filter(|query| {
            !matches!(query.kind, BenchKind::Path)
                && !matches!(
                    query.label.as_str(),
                    "regex_weak" | "regex_no_match" | "literal_no_match"
                )
        })
        .cloned()
        .collect();
    content_queries
        .into_iter()
        .cycle()
        .take(size)
        .enumerate()
        .map(|(idx, mut query)| {
            query.label = format!("session_{idx:03}_{}", query.label);
            query
        })
        .collect()
}

fn indexed_build_command(search_cli_bin: &Path, repo_path: &Path, index_dir: &Path) -> CommandSpec {
    CommandSpec {
        program: search_cli_bin.display().to_string(),
        args: vec![
            "build".to_string(),
            "--repo".to_string(),
            repo_path.display().to_string(),
            "--index-dir".to_string(),
            index_dir.display().to_string(),
        ],
        cwd: repo_path.to_path_buf(),
    }
}

fn indexed_search_command(
    search_cli_bin: &Path,
    repo_path: &Path,
    index_dir: &Path,
    query: &BenchQuery,
    summary_only: bool,
) -> CommandSpec {
    let mut args = vec![
        "search".to_string(),
        "--repo".to_string(),
        repo_path.display().to_string(),
        "--index-dir".to_string(),
        index_dir.display().to_string(),
        "--engine".to_string(),
        "auto".to_string(),
        "--kind".to_string(),
        bench_kind_to_cli(&query.kind).to_string(),
        "--json".to_string(),
    ];
    if summary_only {
        args.push("--summary-only".to_string());
    }
    if let Some(max_results) = query.max_results {
        args.push("--max-results".to_string());
        args.push(max_results.to_string());
    }
    for extension in &query.extensions {
        args.push("--ext".to_string());
        args.push(extension.clone());
    }
    for exact_name in &query.exact_names {
        args.push("--exact-name".to_string());
        args.push(exact_name.clone());
    }
    for path_substring in &query.path_substrings {
        args.push("--path-substring".to_string());
        args.push(path_substring.clone());
    }
    if !query.pattern.is_empty() {
        args.push(query.pattern.clone());
    }
    CommandSpec {
        program: search_cli_bin.display().to_string(),
        args,
        cwd: repo_path.to_path_buf(),
    }
}

fn scan_search_command(search_cli_bin: &Path, repo_path: &Path, query: &BenchQuery) -> CommandSpec {
    let mut args = vec![
        "search".to_string(),
        "--repo".to_string(),
        repo_path.display().to_string(),
        "--engine".to_string(),
        "scan".to_string(),
        "--kind".to_string(),
        bench_kind_to_cli(&query.kind).to_string(),
        "--json".to_string(),
    ];
    if let Some(max_results) = query.max_results {
        args.push("--max-results".to_string());
        args.push(max_results.to_string());
    }
    for extension in &query.extensions {
        args.push("--ext".to_string());
        args.push(extension.clone());
    }
    for exact_name in &query.exact_names {
        args.push("--exact-name".to_string());
        args.push(exact_name.clone());
    }
    for path_substring in &query.path_substrings {
        args.push("--path-substring".to_string());
        args.push(path_substring.clone());
    }
    if !query.pattern.is_empty() {
        args.push(query.pattern.clone());
    }
    CommandSpec {
        program: search_cli_bin.display().to_string(),
        args,
        cwd: repo_path.to_path_buf(),
    }
}

fn baseline_command(repo: &PreparedRepo, query: &BenchQuery) -> CommandSpec {
    match query.kind {
        BenchKind::Path => {
            if !query.extensions.is_empty() {
                CommandSpec {
                    program: "fd".to_string(),
                    args: vec![
                        "-tf".to_string(),
                        "-e".to_string(),
                        query.extensions[0].clone(),
                        ".".to_string(),
                    ],
                    cwd: repo.local_path.clone(),
                }
            } else if !query.exact_names.is_empty() {
                CommandSpec {
                    program: "fd".to_string(),
                    args: vec![
                        "-tf".to_string(),
                        "-g".to_string(),
                        query.exact_names[0].clone(),
                        ".".to_string(),
                    ],
                    cwd: repo.local_path.clone(),
                }
            } else if !query.pattern.is_empty() {
                CommandSpec {
                    program: "/bin/zsh".to_string(),
                    args: vec![
                        "-lc".to_string(),
                        format!(
                            "rg --files | rg --fixed-strings {}",
                            shell_escape(&query.pattern)
                        ),
                    ],
                    cwd: repo.local_path.clone(),
                }
            } else {
                CommandSpec {
                    program: "rg".to_string(),
                    args: vec!["--files".to_string()],
                    cwd: repo.local_path.clone(),
                }
            }
        }
        BenchKind::Literal | BenchKind::Regex => {
            let mut args = vec![
                "--json".to_string(),
                "--line-number".to_string(),
                "--color".to_string(),
                "never".to_string(),
                "--no-heading".to_string(),
            ];
            if matches!(query.kind, BenchKind::Literal) {
                args.push("--fixed-strings".to_string());
            }
            for extension in &query.extensions {
                args.push("-g".to_string());
                args.push(format!("*.{extension}"));
            }
            if let Some(max_results) = query.max_results {
                args.push("--max-count".to_string());
                args.push(max_results.to_string());
            }
            args.push(query.pattern.clone());
            args.push(".".to_string());
            CommandSpec {
                program: "rg".to_string(),
                args,
                cwd: repo.local_path.clone(),
            }
        }
    }
}

fn baseline_tool_for_query(query: &BenchQuery) -> &'static str {
    match query.kind {
        BenchKind::Path => {
            if query.pattern.is_empty()
                && query.extensions.is_empty()
                && query.exact_names.is_empty()
            {
                "rg --files"
            } else if !query.pattern.is_empty() {
                "rg --files | rg"
            } else {
                "fd"
            }
        }
        BenchKind::Literal | BenchKind::Regex => "rg",
    }
}

fn benchmark_session(
    search_cli_bin: &Path,
    repo: &PreparedRepo,
    index_dir: &Path,
    label: &str,
    queries: &[BenchQuery],
    cold_iterations: usize,
    warm_iterations: usize,
) -> Result<SessionReport> {
    let indexed_query_file = write_session_query_file(queries)?;
    let indexed = CommandSpec {
        program: search_cli_bin.display().to_string(),
        args: vec![
            "session".to_string(),
            "--repo".to_string(),
            repo.local_path.display().to_string(),
            "--index-dir".to_string(),
            index_dir.display().to_string(),
            "--engine".to_string(),
            "auto".to_string(),
            "--query-file".to_string(),
            indexed_query_file.path().display().to_string(),
            "--json".to_string(),
            "--summary-only".to_string(),
        ],
        cwd: repo.local_path.clone(),
    };
    let baseline_script = write_baseline_session_script(queries)?;
    let baseline = CommandSpec {
        program: "/bin/zsh".to_string(),
        args: vec![baseline_script.path().display().to_string()],
        cwd: repo.local_path.clone(),
    };
    let (session_cold, session_warm) = if queries.len() >= 100 {
        (1, 1)
    } else {
        (cold_iterations.min(1), warm_iterations.min(2))
    };

    Ok(SessionReport {
        repo_slug: repo.slug.clone(),
        label: label.to_string(),
        indexed: benchmark_command(&indexed, session_cold, session_warm)?,
        baseline: benchmark_command(&baseline, session_cold, session_warm)?,
    })
}

fn write_session_query_file(queries: &[BenchQuery]) -> Result<NamedTempFile> {
    let mut file = NamedTempFile::new()?;
    let session_queries = queries
        .iter()
        .map(|query| SessionQuery {
            name: query.label.clone(),
            request: bench_query_to_request(query),
        })
        .collect::<Vec<_>>();
    serde_json::to_writer_pretty(file.as_file_mut(), &session_queries)?;
    Ok(file)
}

fn write_baseline_session_script(queries: &[BenchQuery]) -> Result<NamedTempFile> {
    let file = NamedTempFile::new()?;
    let mut script = String::from("#!/bin/zsh\nset -e\n");
    for query in queries {
        let spec = baseline_command(
            &PreparedRepo {
                name: String::new(),
                slug: String::new(),
                url: String::new(),
                local_path: PathBuf::from("."),
                commit_sha: String::new(),
                stats: RepoStats::default(),
            },
            query,
        );
        let escaped = shell_join(&spec.program, &spec.args);
        script.push_str(&escaped);
        script.push_str(" >/dev/null\n");
    }
    fs::write(file.path(), script)?;
    Ok(file)
}

fn benchmark_update(
    search_cli_bin: &Path,
    repo: &PreparedRepo,
    index_dir: &Path,
    sample: &RepoSample,
) -> Result<Option<UpdateReport>> {
    let Some((path, extension)) = sample.update_target.clone() else {
        return Ok(None);
    };
    let comment_prefix = match extension.as_str() {
        "py" | "sh" | "yaml" | "yml" | "toml" => "#",
        _ => "//",
    };
    let marker = format!("TRISEEK_UPDATE_MARKER_{}", repo.slug.to_uppercase());
    let absolute_path = repo.local_path.join(&path);
    let original = fs::read_to_string(&absolute_path).ok();
    let append = format!("\n{comment_prefix} {marker}\n");
    if let Some(original) = original {
        fs::write(&absolute_path, format!("{original}{append}"))?;
        let update_command = CommandSpec {
            program: search_cli_bin.display().to_string(),
            args: vec![
                "update".to_string(),
                "--repo".to_string(),
                repo.local_path.display().to_string(),
                "--index-dir".to_string(),
                index_dir.display().to_string(),
            ],
            cwd: repo.local_path.clone(),
        };
        let update = run_timed_command(&update_command)?;
        let update_stdout = run_command_capture(&update_command)?;
        let update_json: Value = serde_json::from_slice(&update_stdout.stdout)?;
        let verify_command = indexed_search_command(
            search_cli_bin,
            &repo.local_path,
            index_dir,
            &BenchQuery {
                label: "update_verify".to_string(),
                family: "update".to_string(),
                kind: BenchKind::Literal,
                pattern: marker.clone(),
                extensions: vec![],
                exact_names: vec![],
                path_substrings: vec![],
                max_results: Some(10),
            },
            false,
        );
        let verification = parse_search_response(&run_command_capture(&verify_command)?.stdout)?;
        fs::write(&absolute_path, original)?;
        let _ = run_command_capture(&CommandSpec {
            program: search_cli_bin.display().to_string(),
            args: vec![
                "update".to_string(),
                "--repo".to_string(),
                repo.local_path.display().to_string(),
                "--index-dir".to_string(),
                index_dir.display().to_string(),
            ],
            cwd: repo.local_path.clone(),
        });
        return Ok(Some(UpdateReport {
            repo_slug: repo.slug.clone(),
            file_path: path,
            marker,
            update,
            delta_docs: update_json["metadata"]["delta_docs"]
                .as_u64()
                .unwrap_or_default(),
            delta_removed_paths: update_json["metadata"]["delta_removed_paths"]
                .as_u64()
                .unwrap_or_default(),
            verification_summary: verification.summary,
        }));
    }
    Ok(None)
}

fn benchmark_command(
    command: &CommandSpec,
    cold_iterations: usize,
    warm_iterations: usize,
) -> Result<TimedCase> {
    let mut cold = Vec::with_capacity(cold_iterations);
    for _ in 0..cold_iterations {
        cold.push(run_timed_command(command)?);
    }
    let mut warm = Vec::with_capacity(warm_iterations);
    for _ in 0..warm_iterations {
        warm.push(run_timed_command(command)?);
    }
    let mut combined = cold.clone();
    combined.extend(warm.clone());
    Ok(TimedCase {
        cold,
        warm,
        aggregate: aggregate_metrics(&combined),
        command: command.clone(),
    })
}

fn run_timed_command(command: &CommandSpec) -> Result<IterationMetrics> {
    let started = Instant::now();
    let output = Command::new("/usr/bin/time")
        .arg("-l")
        .arg(&command.program)
        .args(&command.args)
        .current_dir(&command.cwd)
        .output()
        .with_context(|| format!("failed to execute {}", command.program))?;
    let wall_millis = started.elapsed().as_secs_f64() * 1_000.0;
    let (user_cpu_millis, system_cpu_millis, max_rss_kib) =
        parse_time_output(&String::from_utf8_lossy(&output.stderr));
    Ok(IterationMetrics {
        wall_millis,
        user_cpu_millis,
        system_cpu_millis,
        max_rss_kib,
        exit_code: output.status.code().unwrap_or(-1),
    })
}

fn run_command_capture(command: &CommandSpec) -> Result<std::process::Output> {
    let output = Command::new(&command.program)
        .args(&command.args)
        .current_dir(&command.cwd)
        .output()
        .with_context(|| format!("failed to execute {}", command.program))?;
    if !output.status.success() && output.status.code() != Some(1) {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(output)
}

fn parse_time_output(stderr: &str) -> (Option<f64>, Option<f64>, Option<u64>) {
    let mut user_cpu = None;
    let mut system_cpu = None;
    let mut max_rss = None;
    for line in stderr.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with("maximum resident set size") {
            if let Some(value) = trimmed.split_whitespace().next() {
                max_rss = value.parse::<u64>().ok();
            }
        }
        if trimmed.contains(" user ") && trimmed.contains(" sys") && trimmed.contains(" real") {
            let parts = trimmed.split_whitespace().collect::<Vec<_>>();
            if parts.len() >= 6 {
                user_cpu = parts
                    .get(2)
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(|v| v * 1_000.0);
                system_cpu = parts
                    .get(4)
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(|v| v * 1_000.0);
            }
        }
    }
    (user_cpu, system_cpu, max_rss)
}

fn run_baseline_once(command: &CommandSpec, query: &BenchQuery) -> Result<SearchResponse> {
    let output = run_command_capture(command)?;
    if matches!(query.kind, BenchKind::Path) {
        if String::from_utf8_lossy(&output.stdout)
            .trim_start()
            .starts_with('{')
        {
            return parse_search_response(&output.stdout);
        }
        let paths = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();
        Ok(SearchResponse {
            request: bench_query_to_request(query),
            effective_kind: search_core::SearchKind::Path,
            engine: search_core::SearchEngineKind::Ripgrep,
            routing: search_core::AdaptiveRoutingDecision {
                requested_engine: search_core::SearchEngineKind::Ripgrep,
                selected_engine: search_core::AdaptiveRoute::Ripgrep,
                reason: baseline_tool_for_query(query).to_string(),
            },
            plan: search_core::plan_query(&bench_query_to_request(query)),
            hits: paths
                .into_iter()
                .map(|path| SearchHit::Path { path })
                .collect(),
            summary: SearchSummary {
                files_with_matches: 0,
                total_line_matches: 0,
            },
            metrics: search_core::SearchMetrics::default(),
        })
    } else {
        let mut grouped = BTreeMap::<String, Vec<search_core::SearchLineMatch>>::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let value: Value = serde_json::from_str(line)?;
            if value["type"].as_str() != Some("match") {
                continue;
            }
            let data = &value["data"];
            let path = data["path"]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let line_number = data["line_number"].as_u64().unwrap_or_default() as usize;
            let line_text = data["lines"]["text"]
                .as_str()
                .map(|value| value.trim_end_matches('\n').to_string())
                .unwrap_or_default();
            let column = data["submatches"]
                .get(0)
                .and_then(|item| item["start"].as_u64())
                .map(|value| value as usize + 1)
                .unwrap_or(1);
            grouped
                .entry(path)
                .or_default()
                .push(search_core::SearchLineMatch {
                    line_number,
                    column,
                    line_text,
                });
        }
        let mut hits = Vec::new();
        let mut total_line_matches = 0_usize;
        for (path, lines) in grouped {
            total_line_matches += lines.len();
            hits.push(SearchHit::Content { path, lines });
        }
        Ok(SearchResponse {
            request: bench_query_to_request(query),
            effective_kind: match query.kind {
                BenchKind::Literal => search_core::SearchKind::Literal,
                BenchKind::Regex => search_core::SearchKind::Regex,
                BenchKind::Path => search_core::SearchKind::Path,
            },
            engine: search_core::SearchEngineKind::Ripgrep,
            routing: search_core::AdaptiveRoutingDecision {
                requested_engine: search_core::SearchEngineKind::Ripgrep,
                selected_engine: search_core::AdaptiveRoute::Ripgrep,
                reason: baseline_tool_for_query(query).to_string(),
            },
            plan: search_core::plan_query(&bench_query_to_request(query)),
            hits,
            summary: SearchSummary {
                files_with_matches: 0,
                total_line_matches,
            },
            metrics: search_core::SearchMetrics::default(),
        })
    }
}

fn parse_search_response(bytes: &[u8]) -> Result<SearchResponse> {
    Ok(serde_json::from_slice(bytes)?)
}

fn compare_results(
    indexed: &SearchResponse,
    baseline: &SearchResponse,
    _query: &BenchQuery,
) -> CorrectnessSummary {
    let indexed_set = canonicalize_hits(&indexed.hits);
    let baseline_set = canonicalize_hits(&baseline.hits);
    let missing = baseline_set
        .difference(&indexed_set)
        .cloned()
        .collect::<Vec<_>>();
    let unexpected = indexed_set
        .difference(&baseline_set)
        .cloned()
        .collect::<Vec<_>>();
    CorrectnessSummary {
        passed: missing.is_empty() && unexpected.is_empty(),
        indexed_count: indexed_set.len(),
        baseline_count: baseline_set.len(),
        missing,
        unexpected,
    }
}

fn canonicalize_hits(hits: &[SearchHit]) -> HashSet<String> {
    let mut set = HashSet::new();
    for hit in hits {
        match hit {
            SearchHit::Path { path } => {
                set.insert(format!("path:{}", normalize_path(path)));
            }
            SearchHit::Content { path, lines } => {
                for line in lines {
                    set.insert(format!(
                        "content:{}:{}:{}:{}",
                        normalize_path(path),
                        line.line_number,
                        line.column,
                        line.line_text
                    ));
                }
            }
        }
    }
    set
}

fn normalize_path(path: &str) -> String {
    path.trim_start_matches("./").to_string()
}

fn aggregate_metrics(samples: &[IterationMetrics]) -> AggregateMetrics {
    let mut walls = samples
        .iter()
        .map(|sample| sample.wall_millis)
        .collect::<Vec<_>>();
    walls.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    AggregateMetrics {
        p50_wall_millis: percentile(&walls, 50.0),
        p95_wall_millis: percentile(&walls, 95.0),
        p99_wall_millis: percentile(&walls, 99.0),
        mean_wall_millis: walls.iter().sum::<f64>() / walls.len().max(1) as f64,
        min_wall_millis: *walls.first().unwrap_or(&0.0),
        max_wall_millis: *walls.last().unwrap_or(&0.0),
        mean_user_cpu_millis: mean_option(
            samples
                .iter()
                .map(|sample| sample.user_cpu_millis)
                .collect(),
        ),
        mean_system_cpu_millis: mean_option(
            samples
                .iter()
                .map(|sample| sample.system_cpu_millis)
                .collect(),
        ),
        max_rss_kib: samples.iter().filter_map(|sample| sample.max_rss_kib).max(),
    }
}

fn percentile(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let index = ((percentile / 100.0) * (values.len() - 1) as f64).round() as usize;
    values[index]
}

fn mean_option(values: Vec<Option<f64>>) -> Option<f64> {
    let filtered = values.into_iter().flatten().collect::<Vec<_>>();
    (!filtered.is_empty()).then_some(filtered.iter().sum::<f64>() / filtered.len() as f64)
}

fn bench_query_to_request(query: &BenchQuery) -> search_core::QueryRequest {
    search_core::QueryRequest {
        kind: match query.kind {
            BenchKind::Literal => search_core::SearchKind::Literal,
            BenchKind::Regex => search_core::SearchKind::Regex,
            BenchKind::Path => search_core::SearchKind::Path,
        },
        engine: search_core::SearchEngineKind::Auto,
        pattern: query.pattern.clone(),
        case_mode: search_core::CaseMode::Sensitive,
        path_substrings: query.path_substrings.clone(),
        path_prefixes: Vec::new(),
        exact_paths: Vec::new(),
        exact_names: query.exact_names.clone(),
        extensions: query.extensions.clone(),
        globs: Vec::new(),
        include_hidden: false,
        include_binary: false,
        max_results: query.max_results,
    }
}

fn bench_kind_to_cli(kind: &BenchKind) -> &'static str {
    match kind {
        BenchKind::Literal => "literal",
        BenchKind::Regex => "regex",
        BenchKind::Path => "path",
    }
}

fn collect_machine_info() -> Result<MachineInfo> {
    let hostname = String::from_utf8_lossy(&Command::new("hostname").output()?.stdout)
        .trim()
        .to_string();
    let architecture = String::from_utf8_lossy(&Command::new("uname").arg("-m").output()?.stdout)
        .trim()
        .to_string();
    let os_name = "macOS".to_string();
    let os_version = String::from_utf8_lossy(
        &Command::new("sw_vers")
            .arg("-productVersion")
            .output()?
            .stdout,
    )
    .trim()
    .to_string();
    Ok(MachineInfo {
        hostname,
        os_name,
        os_version,
        architecture,
        logical_cores: num_cpus::get(),
        generated_at: timestamp_now(),
    })
}

fn resolve_search_cli_bin(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(fs::canonicalize(path)?);
    }
    let exe = std::env::current_exe()?;
    let sibling = exe.with_file_name("search-cli");
    if sibling.exists() {
        return Ok(fs::canonicalize(sibling)?);
    }
    bail!(
        "search-cli binary not found; build with `cargo build --release --bin search-cli --bin search-bench` or pass --search-cli-bin"
    )
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn write_case_csv(path: &Path, report: &BenchmarkReport) -> Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record([
        "repo_slug",
        "query_label",
        "family",
        "baseline_tool",
        "correct",
        "indexed_p50_ms",
        "baseline_p50_ms",
        "indexed_p95_ms",
        "baseline_p95_ms",
        "indexed_max_rss_kib",
        "baseline_max_rss_kib",
    ])?;
    for case in &report.case_reports {
        writer.write_record([
            case.repo_slug.as_str(),
            case.query.label.as_str(),
            case.query.family.as_str(),
            case.baseline_tool.as_str(),
            if case.correctness.passed {
                "true"
            } else {
                "false"
            },
            &format!("{:.3}", case.indexed.aggregate.p50_wall_millis),
            &format!("{:.3}", case.baseline.aggregate.p50_wall_millis),
            &format!("{:.3}", case.indexed.aggregate.p95_wall_millis),
            &format!("{:.3}", case.baseline.aggregate.p95_wall_millis),
            &case
                .indexed
                .aggregate
                .max_rss_kib
                .unwrap_or_default()
                .to_string(),
            &case
                .baseline
                .aggregate
                .max_rss_kib
                .unwrap_or_default()
                .to_string(),
        ])?;
    }
    writer.flush()?;
    Ok(())
}

fn write_summary_markdown(path: &Path, report: &BenchmarkReport) -> Result<()> {
    let mut markdown = String::new();
    markdown.push_str("# TriSeek Benchmark Summary\n\n");
    markdown.push_str(&format!("Generated at: {}\n\n", report.generated_at));
    markdown.push_str("## Machine\n\n");
    markdown.push_str(&format!(
        "- Host: {}\n- OS: {} {}\n- Architecture: {}\n- Logical cores: {}\n\n",
        report.machine.hostname,
        report.machine.os_name,
        report.machine.os_version,
        report.machine.architecture,
        report.machine.logical_cores
    ));
    markdown.push_str("## Repo Stats\n\n");
    markdown.push_str("| Repo | Commit | Searchable Files | Searchable Bytes | Category |\n");
    markdown.push_str("|---|---|---:|---:|---|\n");
    for repo in &report.prepared_repos {
        markdown.push_str(&format!(
            "| {} | {} | {} | {} | {:?} |\n",
            markdown_table_cell(&repo.slug),
            &repo.commit_sha[..repo.commit_sha.len().min(12)],
            repo.stats.searchable_files,
            repo.stats.searchable_bytes,
            repo.stats.category.unwrap_or_else(|| classify_repo(
                repo.stats.searchable_files,
                repo.stats.searchable_bytes
            ))
        ));
    }
    markdown.push_str("\n## Query Benchmarks\n\n");
    markdown.push_str("| Repo | Query | Baseline | Correct | TriSeek p50 ms | Baseline p50 ms | TriSeek p95 ms | Baseline p95 ms |\n");
    markdown.push_str("|---|---|---|---|---:|---:|---:|---:|\n");
    for case in &report.case_reports {
        markdown.push_str(&format!(
            "| {} | {} | {} | {} | {:.3} | {:.3} | {:.3} | {:.3} |\n",
            markdown_table_cell(&case.repo_slug),
            markdown_table_cell(&case.query.label),
            markdown_table_cell(&case.baseline_tool),
            if case.correctness.passed { "yes" } else { "no" },
            case.indexed.aggregate.p50_wall_millis,
            case.baseline.aggregate.p50_wall_millis,
            case.indexed.aggregate.p95_wall_millis,
            case.baseline.aggregate.p95_wall_millis
        ));
    }
    markdown.push_str("\n## Sessions\n\n");
    markdown.push_str("| Repo | Session | TriSeek p50 ms | Baseline p50 ms |\n");
    markdown.push_str("|---|---|---:|---:|\n");
    for session in &report.session_reports {
        markdown.push_str(&format!(
            "| {} | {} | {:.3} | {:.3} |\n",
            markdown_table_cell(&session.repo_slug),
            markdown_table_cell(&session.label),
            session.indexed.aggregate.p50_wall_millis,
            session.baseline.aggregate.p50_wall_millis
        ));
    }
    markdown.push_str("\n## Build and Update\n\n");
    markdown.push_str("| Repo | Build ms | Index Bytes | Update ms |\n");
    markdown.push_str("|---|---:|---:|---:|\n");
    for build in &report.build_reports {
        let update = report
            .update_reports
            .iter()
            .find(|item| item.repo_slug == build.repo_slug)
            .map(|item| format!("{:.3}", item.update.wall_millis))
            .unwrap_or_else(|| "n/a".to_string());
        markdown.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            markdown_table_cell(&build.repo_slug),
            build.build_millis,
            build.index_bytes,
            update
        ));
    }
    fs::write(path, markdown)?;
    Ok(())
}

fn markdown_table_cell(value: &str) -> String {
    value.replace('|', "\\|")
}

fn shell_join(program: &str, args: &[String]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(shell_escape(program));
    parts.extend(args.iter().map(|arg| shell_escape(arg)));
    parts.join(" ")
}

fn shell_escape(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '*'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn timestamp_now() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string())
}

fn timestamp_slug() -> String {
    OffsetDateTime::now_utc()
        .format(
            &time::format_description::parse("[year][month][day]-[hour][minute][second]")
                .expect("valid time format"),
        )
        .unwrap_or_else(|_| "unknown".to_string())
}
