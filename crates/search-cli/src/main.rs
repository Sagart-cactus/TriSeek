mod install;
mod mcp;
mod rg;
mod search_runner;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use search_core::{
    AdaptiveRoute, AdaptiveRoutingDecision, CaseMode, DAEMON_HOST, DAEMON_PID_FILE,
    DAEMON_PORT_FILE, ProcessMetrics, QueryRequest, RpcRequest, RpcResponse, SearchEngineKind,
    SearchHit, SearchKind, SearchResponse, SessionMetrics, SessionQuery, plan_query, route_query,
};
use search_frecency::{FrecencyStore, QueryEvent};
use search_index::{
    BuildConfig, SearchEngine, default_index_dir, index_exists, measure_repository,
    read_index_metadata,
};
use search_runner::adjust_route_for_filters;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

#[derive(Parser)]
#[command(name = "triseek")]
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
    FrecencySelect(FrecencySelectArgs),
    Daemon(DaemonArgs),
    /// MCP (Model Context Protocol) server for Claude Code, Codex, and other agent clients.
    Mcp(McpArgs),
    /// Install TriSeek as an MCP server inside an agent client.
    Install(InstallArgs),
    /// Uninstall TriSeek from an agent client.
    Uninstall(InstallArgs),
    /// Run diagnostic checks for the MCP install flow.
    Doctor,
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
    /// Disable frecency-based result reranking.
    #[arg(long)]
    no_frecency: bool,
    /// Bypass the running daemon and execute search locally.
    #[arg(long)]
    no_daemon: bool,
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

// ---------------------------------------------------------------------------
// Daemon subcommands
// ---------------------------------------------------------------------------

#[derive(Args)]
struct DaemonArgs {
    #[command(subcommand)]
    command: DaemonCommands,
}

#[derive(Subcommand)]
enum DaemonCommands {
    /// Start the background daemon.
    Start(DaemonStartArgs),
    /// Stop the running daemon.
    Stop(DaemonStopArgs),
    /// Show daemon status.
    Status(DaemonStopArgs),
}

#[derive(Args)]
struct DaemonStartArgs {
    #[arg(long)]
    repo: PathBuf,
    #[arg(long)]
    index_dir: Option<PathBuf>,
    /// Idle timeout in seconds (0 = never exit). Default: 1800.
    #[arg(long, default_value_t = 1800)]
    idle_timeout: u64,
}

#[derive(Args)]
struct DaemonStopArgs {
    #[arg(long)]
    repo: Option<PathBuf>,
    #[arg(long)]
    index_dir: Option<PathBuf>,
}

/// Record that files were explicitly opened/selected, boosting their frecency score.
#[derive(Args)]
struct FrecencySelectArgs {
    #[arg(long)]
    repo: Option<PathBuf>,
    #[arg(long)]
    index_dir: Option<PathBuf>,
    /// Paths (relative to repo root) that were opened.
    #[arg(long = "path", required = true)]
    paths: Vec<String>,
}

// ---------------------------------------------------------------------------
// MCP subcommands
// ---------------------------------------------------------------------------

#[derive(Args)]
struct McpArgs {
    #[command(subcommand)]
    command: McpCommands,
}

#[derive(Subcommand)]
enum McpCommands {
    /// Run TriSeek as an MCP server over stdio.
    Serve(McpServeArgs),
}

#[derive(Args)]
struct McpServeArgs {
    /// Repository root to search. Defaults to walking up from CWD for a `.git` marker.
    #[arg(long)]
    repo: Option<PathBuf>,
    /// Index directory. Defaults to the TriSeek default for the resolved repo.
    #[arg(long)]
    index_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Install subcommands
// ---------------------------------------------------------------------------

#[derive(Args)]
struct InstallArgs {
    #[command(subcommand)]
    client: InstallClient,
}

#[derive(Subcommand)]
enum InstallClient {
    /// Register TriSeek with the Claude Code CLI.
    ClaudeCode(ClaudeCodeInstallArgs),
    /// Register TriSeek with the Codex CLI.
    Codex,
}

#[derive(Args)]
struct ClaudeCodeInstallArgs {
    /// Installation scope. `local` is per-user on this machine, `project` edits
    /// `.mcp.json` in the current directory and is intended to be committed,
    /// `user` installs globally for the current Claude Code user profile.
    #[arg(long, value_enum, default_value_t = CliClaudeScope::Local)]
    scope: CliClaudeScope,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CliClaudeScope {
    Local,
    Project,
    User,
}

impl From<CliClaudeScope> for install::Scope {
    fn from(value: CliClaudeScope) -> Self {
        match value {
            CliClaudeScope::Local => install::Scope::Local,
            CliClaudeScope::Project => install::Scope::Project,
            CliClaudeScope::User => install::Scope::User,
        }
    }
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
        Commands::FrecencySelect(args) => handle_frecency_select(args),
        Commands::Daemon(args) => match args.command {
            DaemonCommands::Start(args) => handle_daemon_start(args),
            DaemonCommands::Stop(args) => handle_daemon_stop(args),
            DaemonCommands::Status(args) => handle_daemon_status(args),
        },
        Commands::Mcp(args) => match args.command {
            McpCommands::Serve(args) => mcp::serve(args.repo.as_deref(), args.index_dir.as_deref()),
        },
        Commands::Install(args) => match args.client {
            InstallClient::ClaudeCode(a) => install::claude_code::install(a.scope.into()),
            InstallClient::Codex => install::codex::install(),
        },
        Commands::Uninstall(args) => match args.client {
            InstallClient::ClaudeCode(a) => install::claude_code::uninstall(a.scope.into()),
            InstallClient::Codex => install::codex::uninstall(),
        },
        Commands::Doctor => install::doctor::run(),
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

    // Transparent daemon forwarding: if a daemon is running, route there first.
    if !args.no_daemon
        && let Some(response) = try_daemon_search(&index_dir, &request)
    {
        if args.json {
            print_json(&response)?;
        } else {
            print_human_search(&response);
        }
        return Ok(response);
    }

    let executed = search_runner::execute_search(
        &repo_root,
        &index_dir,
        &request,
        args.repeated_session_hint,
        args.summary_only,
    )?;
    let mut response = executed.response;
    if args.summary_only {
        response.hits.clear();
    }

    // Frecency: rerank hits then record this query's results.
    if !args.no_frecency && !args.summary_only && !response.hits.is_empty() {
        let mut store = FrecencyStore::open(&index_dir);
        if !store.is_empty() {
            store.rerank_hits(&mut response.hits);
        }
        store.record_results(&response.hits);
        store.record_query(QueryEvent {
            timestamp_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            pattern: request.pattern.clone(),
            kind: format!("{:?}", response.effective_kind).to_ascii_lowercase(),
            result_paths: response
                .hits
                .iter()
                .take(20)
                .map(|h| match h {
                    SearchHit::Content { path, .. } | SearchHit::Path { path } => path.clone(),
                })
                .collect(),
            selected_paths: vec![],
        });
        let _ = store.flush(); // best-effort; don't fail the search on flush error
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
            AdaptiveRoute::Ripgrep => {
                crate::rg::run_rg_search(&repo_root, &request, args.summary_only)?
            }
        };

        total_matches += execution.summary.total_line_matches;
        *engine_counts
            .entry(
                format!("{:?}", search_runner::route_to_engine(selected_route))
                    .to_ascii_lowercase(),
            )
            .or_default() += 1;

        let mut response = SearchResponse {
            request,
            effective_kind: effective_search_kind(CliSearchKind::Auto),
            engine: search_runner::route_to_engine(selected_route),
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

fn handle_frecency_select(args: FrecencySelectArgs) -> Result<()> {
    let repo_root = resolve_repo_root(args.repo.as_deref(), args.index_dir.as_deref())?;
    let index_dir = args
        .index_dir
        .unwrap_or_else(|| default_index_dir(&repo_root));
    let mut store = FrecencyStore::open(&index_dir);
    store.record_select(&args.paths);
    store.flush().context("failed to flush frecency store")?;
    eprintln!("frecency: recorded select for {} path(s)", args.paths.len());
    Ok(())
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

fn effective_search_kind(kind: CliSearchKind) -> SearchKind {
    match kind {
        CliSearchKind::Auto | CliSearchKind::Literal => SearchKind::Literal,
        CliSearchKind::Regex => SearchKind::Regex,
        CliSearchKind::Path => SearchKind::Path,
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

// ---------------------------------------------------------------------------
// Daemon helpers
// ---------------------------------------------------------------------------

fn resolve_index_dir_for_daemon(repo: Option<&Path>, index_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(dir) = index_dir {
        return Ok(dir.to_path_buf());
    }
    if let Some(repo) = repo {
        return Ok(default_index_dir(repo));
    }
    // Fall back to cwd.
    let cwd = std::env::current_dir().context("failed to get cwd")?;
    Ok(default_index_dir(&cwd))
}

fn daemon_port_path_for_index(index_dir: &Path) -> PathBuf {
    index_dir.join(DAEMON_PORT_FILE)
}

fn pid_path_for_index(index_dir: &Path) -> PathBuf {
    index_dir.join(DAEMON_PID_FILE)
}

fn read_daemon_port(index_dir: &Path) -> Option<u16> {
    let port_path = daemon_port_path_for_index(index_dir);
    if !port_path.exists() {
        return None;
    }
    fs::read_to_string(&port_path)
        .ok()
        .and_then(|port| port.trim().parse::<u16>().ok())
}

/// Return a connected TcpStream to the running daemon, or None if no daemon is listening.
fn connect_to_daemon(index_dir: &Path) -> Option<TcpStream> {
    let port = read_daemon_port(index_dir)?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(250)).ok()
}

/// Send a single JSON-RPC request and return the response. Synchronous.
fn rpc_call(
    stream: &mut TcpStream,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value> {
    use std::io::{BufRead, BufReader, Write};
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
            bail!("RPC error {}: {}", err.code, err.message);
        }
        return Ok(resp.result.unwrap_or(serde_json::Value::Null));
    }
    bail!("daemon closed connection without a response")
}

fn wait_for_daemon(index_dir: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(mut stream) = connect_to_daemon(index_dir)
            && rpc_call(&mut stream, "status", serde_json::Value::Null).is_ok()
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn handle_daemon_start(args: DaemonStartArgs) -> Result<()> {
    let repo = args
        .repo
        .canonicalize()
        .context("failed to resolve repo path")?;
    let index_dir = args.index_dir.unwrap_or_else(|| default_index_dir(&repo));

    // Check if daemon is already running.
    if let Some(mut stream) = connect_to_daemon(&index_dir)
        && rpc_call(&mut stream, "status", serde_json::Value::Null).is_ok()
    {
        eprintln!("triseek: daemon already running for {}", repo.display());
        return Ok(());
    }

    // Find the triseek-server binary next to the current executable.
    let server_exe_name = format!("triseek-server{}", std::env::consts::EXE_SUFFIX);
    let server_exe = std::env::current_exe()
        .context("cannot determine current exe")?
        .parent()
        .context("current exe has no parent dir")?
        .join(server_exe_name);

    if !server_exe.exists() {
        bail!(
            "triseek-server not found at {}. Build the workspace first.",
            server_exe.display()
        );
    }

    let mut cmd = std::process::Command::new(&server_exe);
    cmd.arg("--repo").arg(&repo);
    cmd.arg("--index-dir").arg(&index_dir);
    cmd.arg("--idle-timeout").arg(args.idle_timeout.to_string());
    // Detach from terminal.
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);

    let mut child = cmd.spawn().context("failed to spawn triseek-server")?;
    if wait_for_daemon(&index_dir, Duration::from_secs(5)) {
        eprintln!(
            "triseek: daemon started (pid {}) for {} via {}",
            child.id(),
            repo.display(),
            DAEMON_HOST
        );
        return Ok(());
    }

    if let Some(status) = child
        .try_wait()
        .context("failed to inspect daemon status")?
    {
        bail!("triseek-server exited before becoming ready: {status}");
    }

    eprintln!(
        "triseek: daemon spawned (pid {}) for {} but readiness was not confirmed yet",
        child.id(),
        repo.display()
    );
    Ok(())
}

fn handle_daemon_stop(args: DaemonStopArgs) -> Result<()> {
    let index_dir = resolve_index_dir_for_daemon(args.repo.as_deref(), args.index_dir.as_deref())?;
    if let Some(mut stream) = connect_to_daemon(&index_dir) {
        let _ = rpc_call(&mut stream, "shutdown", serde_json::Value::Null);
        eprintln!("triseek: shutdown signal sent");
        return Ok(());
    }
    // Fall back to SIGTERM via PID file.
    let pid_file = pid_path_for_index(&index_dir);
    if pid_file.exists() {
        let pid_str = fs::read_to_string(&pid_file).context("read PID file")?;
        let pid: i32 = pid_str.trim().parse().context("parse PID")?;
        terminate_pid(pid)?;
    } else {
        eprintln!("triseek: no daemon found for {}", index_dir.display());
    }
    Ok(())
}

fn handle_daemon_status(args: DaemonStopArgs) -> Result<()> {
    let index_dir = resolve_index_dir_for_daemon(args.repo.as_deref(), args.index_dir.as_deref())?;
    if let Some(mut stream) = connect_to_daemon(&index_dir) {
        let result = rpc_call(&mut stream, "status", serde_json::Value::Null)?;
        print_json(&result)?;
    } else {
        eprintln!("triseek: no daemon running for {}", index_dir.display());
    }
    Ok(())
}

/// Try to forward a search request to the running daemon.
/// Returns None if no daemon is available or forwarding fails (silent fallback to local).
fn try_daemon_search(index_dir: &Path, request: &QueryRequest) -> Option<SearchResponse> {
    let mut stream = connect_to_daemon(index_dir)?;
    let params = serde_json::to_value(request).ok()?;
    let result = rpc_call(&mut stream, "search", params).ok()?;
    serde_json::from_value(result).ok()
}

#[cfg(unix)]
fn terminate_pid(pid: i32) -> Result<()> {
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    eprintln!("triseek: sent SIGTERM to pid {pid}");
    Ok(())
}

#[cfg(windows)]
fn terminate_pid(pid: i32) -> Result<()> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .context("failed to invoke taskkill")?;
    if !status.success() {
        bail!("taskkill failed for pid {pid} with status {status}");
    }
    eprintln!("triseek: terminated pid {pid} via taskkill");
    Ok(())
}
