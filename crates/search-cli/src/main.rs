use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use search_core::{
    AdaptiveRoute, AdaptiveRoutingDecision, CaseMode, ProcessMetrics, QueryRequest,
    SearchEngineKind, SearchHit, SearchKind, SearchMetrics, SearchResponse, SearchSummary,
    SessionMetrics, SessionQuery, plan_query, route_query,
};
use search_index::{
    BuildConfig, SearchEngine, SearchExecution, default_index_dir, index_exists,
    measure_repository, read_index_metadata,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use time::OffsetDateTime;

#[derive(Parser)]
#[command(name = "search-cli")]
#[command(about = "TriSeek indexed local code search")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Build(BuildArgs),
    Update(UpdateArgs),
    Measure(MeasureArgs),
    Search(SearchArgs),
    Session(SessionArgs),
    Stats(StatsArgs),
}

#[derive(Args)]
struct CommonIndexArgs {
    #[arg(long)]
    repo: PathBuf,
    #[arg(long)]
    index_dir: Option<PathBuf>,
    #[arg(long)]
    include_hidden: bool,
    #[arg(long)]
    include_binary: bool,
    #[arg(long)]
    max_file_size: Option<u64>,
    #[arg(long, default_value_t = 0.25)]
    merge_threshold_ratio: f32,
}

#[derive(Args)]
struct BuildArgs {
    #[command(flatten)]
    common: CommonIndexArgs,
}

#[derive(Args)]
struct UpdateArgs {
    #[command(flatten)]
    common: CommonIndexArgs,
}

#[derive(Args)]
struct MeasureArgs {
    #[arg(long)]
    repo: PathBuf,
    #[arg(long)]
    include_hidden: bool,
    #[arg(long)]
    include_binary: bool,
    #[arg(long)]
    max_file_size: Option<u64>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliSearchKind {
    Auto,
    Literal,
    Regex,
    Path,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliEngine {
    Auto,
    Index,
    Scan,
    Rg,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliCaseMode {
    Sensitive,
    Insensitive,
}

#[derive(Args)]
struct SearchArgs {
    #[arg(long)]
    repo: Option<PathBuf>,
    #[arg(long)]
    index_dir: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = CliSearchKind::Auto)]
    kind: CliSearchKind,
    #[arg(long, value_enum, default_value_t = CliEngine::Auto)]
    engine: CliEngine,
    #[arg(long, value_enum, default_value_t = CliCaseMode::Sensitive)]
    case_mode: CliCaseMode,
    #[arg(long = "path-substring")]
    path_substrings: Vec<String>,
    #[arg(long = "path-prefix")]
    path_prefixes: Vec<String>,
    #[arg(long = "exact-path")]
    exact_paths: Vec<String>,
    #[arg(long = "exact-name")]
    exact_names: Vec<String>,
    #[arg(long = "ext")]
    extensions: Vec<String>,
    #[arg(long = "glob")]
    globs: Vec<String>,
    #[arg(long)]
    include_hidden: bool,
    #[arg(long)]
    include_binary: bool,
    #[arg(long)]
    max_results: Option<usize>,
    #[arg(long)]
    repeated_session_hint: bool,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    summary_only: bool,
    pattern: Option<String>,
}

#[derive(Args)]
struct SessionArgs {
    #[arg(long)]
    repo: Option<PathBuf>,
    #[arg(long)]
    index_dir: Option<PathBuf>,
    #[arg(long)]
    query_file: PathBuf,
    #[arg(long, value_enum, default_value_t = CliEngine::Auto)]
    engine: CliEngine,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    summary_only: bool,
}

#[derive(Args)]
struct StatsArgs {
    #[arg(long)]
    index_dir: PathBuf,
}

#[derive(Debug, Serialize)]
struct BuildOutput {
    action: &'static str,
    index_dir: String,
    metadata: search_core::IndexMetadata,
    generated_at: String,
}

#[derive(Debug, Serialize)]
struct UpdateOutput {
    action: &'static str,
    index_dir: String,
    rebuilt_full: bool,
    metadata: search_core::IndexMetadata,
    generated_at: String,
}

#[derive(Debug, Serialize)]
struct SessionOutput {
    query_count: usize,
    engine_counts: BTreeMap<String, usize>,
    total_matches: usize,
    results: Vec<NamedSearchResponse>,
    metrics: SessionMetrics,
}

#[derive(Debug, Serialize)]
struct NamedSearchResponse {
    name: String,
    response: SearchResponse,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build(args) => handle_build(args),
        Commands::Update(args) => handle_update(args),
        Commands::Measure(args) => handle_measure(args),
        Commands::Search(args) => handle_search(args).map(|_| ()),
        Commands::Session(args) => handle_session(args).map(|_| ()),
        Commands::Stats(args) => handle_stats(args),
    }
}

fn handle_build(args: BuildArgs) -> Result<()> {
    let config = build_config_from_common(&args.common);
    let index_dir = args
        .common
        .index_dir
        .clone()
        .unwrap_or_else(|| default_index_dir(&args.common.repo));
    let metadata = SearchEngine::build(&args.common.repo, Some(&index_dir), &config)?;
    print_json(&BuildOutput {
        action: "build",
        index_dir: index_dir.display().to_string(),
        metadata,
        generated_at: timestamp_now(),
    })
}

fn handle_update(args: UpdateArgs) -> Result<()> {
    let config = build_config_from_common(&args.common);
    let index_dir = args
        .common
        .index_dir
        .clone()
        .unwrap_or_else(|| default_index_dir(&args.common.repo));
    let outcome = SearchEngine::update(&args.common.repo, Some(&index_dir), &config)?;
    print_json(&UpdateOutput {
        action: "update",
        index_dir: index_dir.display().to_string(),
        rebuilt_full: outcome.rebuilt_full,
        metadata: outcome.metadata,
        generated_at: timestamp_now(),
    })
}

fn handle_measure(args: MeasureArgs) -> Result<()> {
    let config = BuildConfig {
        include_hidden: args.include_hidden,
        include_binary: args.include_binary,
        max_file_size: args.max_file_size,
        merge_threshold_ratio: 0.25,
    };
    let stats = measure_repository(&args.repo, &config)?;
    print_json(&stats)
}

fn handle_search(args: SearchArgs) -> Result<SearchResponse> {
    let request = build_query_request(&args);
    let repo_root = resolve_repo_root(args.repo.as_deref(), args.index_dir.as_deref())?;
    let index_dir = args
        .index_dir
        .clone()
        .unwrap_or_else(|| default_index_dir(&repo_root));
    let index_available = index_exists(&index_dir);
    let index_metadata = if index_available {
        Some(read_index_metadata(&index_dir).with_context(|| {
            format!("failed to read index metadata from {}", index_dir.display())
        })?)
    } else {
        None
    };

    let plan = plan_query(&request);
    let mut routing = route_query(
        &request,
        index_metadata.as_ref().map(|metadata| &metadata.repo_stats),
        &plan,
        index_available,
        args.repeated_session_hint,
    );
    let selected_route = adjust_route_for_filters(routing.selected_engine, &request);
    if selected_route != routing.selected_engine {
        routing = AdaptiveRoutingDecision {
            reason: format!("{};filter_adjustment=true", routing.reason),
            selected_engine: selected_route,
            ..routing
        };
    }

    let execution = match selected_route {
        AdaptiveRoute::Indexed => {
            let engine = SearchEngine::open(&index_dir)
                .with_context(|| format!("failed to open index at {}", index_dir.display()))?;
            engine.search(&request)?
        }
        AdaptiveRoute::DirectScan => {
            SearchEngine::search_direct(&repo_root, &request, &BuildConfig::default())?
        }
        AdaptiveRoute::Ripgrep => run_rg_search(&repo_root, &request, args.summary_only)?,
    };

    let mut response = SearchResponse {
        request,
        effective_kind: effective_search_kind(args.kind),
        engine: route_to_engine(selected_route),
        routing,
        plan,
        hits: execution.hits,
        summary: execution.summary,
        metrics: execution.metrics,
    };
    if args.summary_only {
        response.hits.clear();
    }

    if args.json {
        print_json(&response)?;
    } else {
        print_human_search(&response);
    }
    Ok(response)
}

fn handle_session(args: SessionArgs) -> Result<SessionOutput> {
    let query_bytes = fs::read(&args.query_file)
        .with_context(|| format!("failed to read {}", args.query_file.display()))?;
    let queries: Vec<SessionQuery> = serde_json::from_slice(&query_bytes)
        .context("failed to parse session query file as JSON")?;

    let repo_root = resolve_repo_root(args.repo.as_deref(), args.index_dir.as_deref())?;
    let index_dir = args
        .index_dir
        .clone()
        .unwrap_or_else(|| default_index_dir(&repo_root));
    let index_available = index_exists(&index_dir);
    let index_metadata = if index_available {
        Some(read_index_metadata(&index_dir).with_context(|| {
            format!("failed to read index metadata from {}", index_dir.display())
        })?)
    } else {
        None
    };
    let indexed_engine = if index_available {
        Some(SearchEngine::open(&index_dir)?)
    } else {
        None
    };

    let started = Instant::now();
    let mut results = Vec::with_capacity(queries.len());
    let mut engine_counts = BTreeMap::<String, usize>::new();
    let mut total_matches = 0_usize;

    for session_query in queries {
        let mut request = session_query.request.clone();
        if !matches!(args.engine, CliEngine::Auto) {
            request.engine = cli_engine_to_request(args.engine);
        }

        let plan = plan_query(&request);
        let mut routing = route_query(
            &request,
            index_metadata.as_ref().map(|metadata| &metadata.repo_stats),
            &plan,
            index_available,
            true,
        );
        let selected_route = adjust_route_for_filters(routing.selected_engine, &request);
        if selected_route != routing.selected_engine {
            routing = AdaptiveRoutingDecision {
                reason: format!("{};filter_adjustment=true", routing.reason),
                selected_engine: selected_route,
                ..routing
            };
        }

        let execution = match selected_route {
            AdaptiveRoute::Indexed => indexed_engine
                .as_ref()
                .context("session requested indexed search but no index is available")?
                .search(&request)?,
            AdaptiveRoute::DirectScan => {
                SearchEngine::search_direct(&repo_root, &request, &BuildConfig::default())?
            }
            AdaptiveRoute::Ripgrep => run_rg_search(&repo_root, &request, args.summary_only)?,
        };

        total_matches += execution.summary.total_line_matches;
        *engine_counts
            .entry(format!("{:?}", route_to_engine(selected_route)).to_ascii_lowercase())
            .or_default() += 1;

        let mut response = SearchResponse {
            request,
            effective_kind: effective_search_kind(CliSearchKind::Auto),
            engine: route_to_engine(selected_route),
            routing,
            plan,
            hits: execution.hits,
            summary: execution.summary,
            metrics: execution.metrics,
        };
        if args.summary_only {
            response.hits.clear();
        }

        results.push(NamedSearchResponse {
            name: session_query.name,
            response,
        });
    }

    let total_wall_millis = started.elapsed().as_secs_f64() * 1_000.0;
    let build_millis = indexed_engine
        .as_ref()
        .map(|engine| engine.metadata().build_stats.build_millis as f64)
        .unwrap_or_default();
    let query_count = results.len();
    let output = SessionOutput {
        query_count,
        engine_counts,
        total_matches,
        results,
        metrics: SessionMetrics {
            query_count,
            total_matches,
            process: ProcessMetrics {
                wall_millis: total_wall_millis,
                user_cpu_millis: None,
                system_cpu_millis: None,
                max_rss_kib: None,
            },
            amortized_with_index_build_millis: (query_count > 0)
                .then_some((total_wall_millis + build_millis) / query_count as f64),
            amortized_without_index_build_millis: (query_count > 0)
                .then_some(total_wall_millis / query_count as f64),
        },
    };

    if args.json {
        print_json(&output)?;
    } else {
        println!(
            "queries={} total_matches={} wall_ms={:.2}",
            output.query_count, output.total_matches, output.metrics.process.wall_millis
        );
    }
    Ok(output)
}

fn handle_stats(args: StatsArgs) -> Result<()> {
    let metadata = read_index_metadata(&args.index_dir)?;
    print_json(&metadata)
}

fn build_config_from_common(args: &CommonIndexArgs) -> BuildConfig {
    BuildConfig {
        include_hidden: args.include_hidden,
        include_binary: args.include_binary,
        max_file_size: args.max_file_size,
        merge_threshold_ratio: args.merge_threshold_ratio,
    }
}

fn build_query_request(args: &SearchArgs) -> QueryRequest {
    QueryRequest {
        kind: cli_kind_to_request(args.kind),
        engine: cli_engine_to_request(args.engine),
        pattern: args.pattern.clone().unwrap_or_default(),
        case_mode: cli_case_to_request(args.case_mode),
        path_substrings: args.path_substrings.clone(),
        path_prefixes: args.path_prefixes.clone(),
        exact_paths: args.exact_paths.clone(),
        exact_names: args.exact_names.clone(),
        extensions: args.extensions.clone(),
        globs: args.globs.clone(),
        include_hidden: args.include_hidden,
        include_binary: args.include_binary,
        max_results: args.max_results,
    }
}

fn resolve_repo_root(repo: Option<&Path>, index_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(repo) = repo {
        return Ok(repo.to_path_buf());
    }
    if let Some(index_dir) = index_dir {
        let metadata = read_index_metadata(index_dir)?;
        return Ok(PathBuf::from(metadata.repo_stats.repo_root));
    }
    bail!("repo root is required when index metadata cannot supply it")
}

fn run_rg_search(
    repo_root: &Path,
    request: &QueryRequest,
    summary_only: bool,
) -> Result<SearchExecution> {
    if matches!(request.kind, SearchKind::Path) {
        bail!("path queries should not route to ripgrep execution");
    }
    let started = Instant::now();
    let mut command = Command::new("rg");
    command.current_dir(repo_root);
    command.arg("--color").arg("never");
    command.arg("--no-heading");

    if summary_only {
        command.arg("--count");
    } else {
        command.arg("--json");
        command.arg("--line-number");
    }

    if request.include_hidden {
        command.arg("--hidden");
    }
    if request.include_binary {
        command.arg("--text");
    }
    if matches!(request.case_mode, CaseMode::Insensitive) {
        command.arg("--ignore-case");
    }
    if matches!(request.kind, SearchKind::Literal | SearchKind::Auto) {
        command.arg("--fixed-strings");
    }
    for extension in request.normalized_extensions() {
        command.arg("-g").arg(format!("*.{extension}"));
    }
    for glob in &request.globs {
        command.arg("-g").arg(glob);
    }
    for prefix in &request.path_prefixes {
        let mut normalized = prefix.trim_end_matches('/').to_string();
        normalized.push_str("/**");
        command.arg("-g").arg(normalized);
    }
    for name in &request.exact_names {
        command.arg("-g").arg(format!("**/{name}"));
    }
    for path in &request.exact_paths {
        command.arg("-g").arg(path);
    }
    if let Some(max_results) = request.max_results {
        command.arg("--max-count").arg(max_results.to_string());
    }
    command.arg(&request.pattern);
    command.arg(".");

    let output = command.output().context("failed to execute rg")?;
    if !output.status.success() && output.status.code() != Some(1) {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }

    if summary_only {
        let mut files_with_matches = 0_usize;
        let mut total_line_matches = 0_usize;
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let Some((_, count)) = line.rsplit_once(':') else {
                continue;
            };
            let count = count.trim().parse::<usize>().unwrap_or_default();
            if count > 0 {
                files_with_matches += 1;
                total_line_matches += count;
            }
        }
        return Ok(SearchExecution {
            summary: SearchSummary {
                files_with_matches,
                total_line_matches,
            },
            metrics: SearchMetrics {
                process: ProcessMetrics {
                    wall_millis: started.elapsed().as_secs_f64() * 1_000.0,
                    user_cpu_millis: None,
                    system_cpu_millis: None,
                    max_rss_kib: None,
                },
                candidate_docs: 0,
                verified_docs: 0,
                matches_returned: total_line_matches,
                bytes_scanned: 0,
                index_bytes_read: None,
            },
            ..SearchExecution::default()
        });
    }

    let mut grouped = BTreeMap::<String, Vec<search_core::SearchLineMatch>>::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let value: Value = serde_json::from_str(line)?;
        if value.get("type").and_then(Value::as_str) != Some("match") {
            continue;
        }
        let data = &value["data"];
        let path = data["path"]["text"]
            .as_str()
            .or_else(|| data["path"]["bytes"].as_str())
            .unwrap_or_default()
            .to_string();
        let line_text = data["lines"]["text"]
            .as_str()
            .map(|value| value.trim_end_matches(['\n', '\r']).to_string())
            .unwrap_or_default();
        let line_number = data["line_number"].as_u64().unwrap_or_default() as usize;
        let column = data["submatches"]
            .get(0)
            .and_then(|submatch| submatch.get("start"))
            .and_then(Value::as_u64)
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

    let mut hits = Vec::with_capacity(grouped.len());
    let mut total_line_matches = 0_usize;
    for (path, lines) in grouped {
        total_line_matches += lines.len();
        hits.push(SearchHit::Content { path, lines });
    }

    Ok(SearchExecution {
        summary: SearchSummary {
            files_with_matches: hits.len(),
            total_line_matches,
        },
        metrics: SearchMetrics {
            process: ProcessMetrics {
                wall_millis: started.elapsed().as_secs_f64() * 1_000.0,
                user_cpu_millis: None,
                system_cpu_millis: None,
                max_rss_kib: None,
            },
            candidate_docs: 0,
            verified_docs: 0,
            matches_returned: total_line_matches,
            bytes_scanned: 0,
            index_bytes_read: None,
        },
        hits,
    })
}

fn adjust_route_for_filters(route: AdaptiveRoute, request: &QueryRequest) -> AdaptiveRoute {
    if matches!(request.kind, SearchKind::Path) {
        return if matches!(route, AdaptiveRoute::Indexed) {
            AdaptiveRoute::Indexed
        } else {
            AdaptiveRoute::DirectScan
        };
    }
    if !request.path_substrings.is_empty() || !request.exact_paths.is_empty() {
        return AdaptiveRoute::DirectScan;
    }
    route
}

fn effective_search_kind(kind: CliSearchKind) -> SearchKind {
    match kind {
        CliSearchKind::Auto | CliSearchKind::Literal => SearchKind::Literal,
        CliSearchKind::Regex => SearchKind::Regex,
        CliSearchKind::Path => SearchKind::Path,
    }
}

fn route_to_engine(route: AdaptiveRoute) -> SearchEngineKind {
    match route {
        AdaptiveRoute::Indexed => SearchEngineKind::Indexed,
        AdaptiveRoute::DirectScan => SearchEngineKind::DirectScan,
        AdaptiveRoute::Ripgrep => SearchEngineKind::Ripgrep,
    }
}

fn cli_kind_to_request(kind: CliSearchKind) -> SearchKind {
    match kind {
        CliSearchKind::Auto => SearchKind::Auto,
        CliSearchKind::Literal => SearchKind::Literal,
        CliSearchKind::Regex => SearchKind::Regex,
        CliSearchKind::Path => SearchKind::Path,
    }
}

fn cli_engine_to_request(engine: CliEngine) -> SearchEngineKind {
    match engine {
        CliEngine::Auto => SearchEngineKind::Auto,
        CliEngine::Index => SearchEngineKind::Indexed,
        CliEngine::Scan => SearchEngineKind::DirectScan,
        CliEngine::Rg => SearchEngineKind::Ripgrep,
    }
}

fn cli_case_to_request(case_mode: CliCaseMode) -> CaseMode {
    match case_mode {
        CliCaseMode::Sensitive => CaseMode::Sensitive,
        CliCaseMode::Insensitive => CaseMode::Insensitive,
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn print_human_search(response: &SearchResponse) {
    println!(
        "engine={:?} files={} line_matches={} wall_ms={:.2}",
        response.engine,
        response.summary.files_with_matches,
        response.summary.total_line_matches,
        response.metrics.process.wall_millis
    );
    for hit in &response.hits {
        match hit {
            SearchHit::Content { path, lines } => {
                for line in lines {
                    println!(
                        "{path}:{}:{}:{}",
                        line.line_number, line.column, line.line_text
                    );
                }
            }
            SearchHit::Path { path } => println!("{path}"),
        }
    }
}

fn timestamp_now() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string())
}
