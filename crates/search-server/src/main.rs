mod git_state;
mod hydrate;
mod memo;
mod session_state;
mod snapshot;

use anyhow::{Context, Result};
use clap::Parser;
use memo::MemoState;
use search_core::{
    ActionKind, DAEMON_HOST, DAEMON_PID_FILE, DAEMON_PORT_FILE, DaemonRootParams, DaemonRootStatus,
    DaemonSearchParams, DaemonStatus, DaemonStatusParams, FrecencySelectParams, MemoCheckParams,
    MemoEventKind, MemoObserveParams, MemoSessionParams, MemoStatusParams,
    PortabilitySessionStatusParams, PortabilitySessionStatusResponse, RpcRequest, RpcResponse,
    SearchEngineKind, SearchHit, SearchKind, SearchResponse, SearchReuseCheckParams,
    SearchReuseCheckResponse, SearchReuseReason, SessionCloseParams, SessionListParams,
    SessionListResponse, SessionOpenParams, SessionOpenResponse, SessionRecordActionParams,
    SessionRecordActionResponse, SessionResumePrepareParams, SessionSnapshotCreateParams,
    SessionSnapshotCreateResponse, SessionSnapshotDiffParams, SessionSnapshotDiffResponse,
    SessionSnapshotGetParams, SessionSnapshotGetResponse, SessionSnapshotListParams,
    SessionSnapshotListResponse, plan_query, route_query,
};
use search_frecency::{FrecencyStore, QueryEvent};
use search_index::{
    BuildConfig, SearchEngine, WatcherHandle, daemon_dir, default_index_dir, index_exists,
    query_matches_path_filters, start_watcher,
};
use session_state::SessionStore;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "triseek-server")]
#[command(about = "TriSeek background search daemon")]
struct Args {
    /// Optional root to preload into the daemon.
    root: Option<PathBuf>,
    /// Deprecated alias for `root`.
    #[arg(long, hide = true)]
    repo: Option<PathBuf>,
    /// TCP port for the loopback control plane (defaults to an ephemeral port).
    #[arg(long)]
    port: Option<u16>,
    /// Idle timeout in seconds before auto-exit (0 = never).
    #[arg(long, default_value_t = 1800)]
    idle_timeout: u64,
}

struct RepoService {
    repo_root: PathBuf,
    index_dir: PathBuf,
    engine: RwLock<Option<SearchEngine>>,
    frecency: Mutex<FrecencyStore>,
    watcher: Mutex<Option<WatcherHandle>>,
    last_seen_generation: AtomicU64,
    context_epoch: AtomicU64,
    search_change_journal: Arc<Mutex<VecDeque<SearchChangeBatch>>>,
    memo: Arc<MemoState>,
    session_store: Arc<SessionStore>,
}

#[derive(Debug, Clone)]
struct SearchChangeBatch {
    generation: u64,
    paths: Vec<String>,
}

const SEARCH_CHANGE_JOURNAL_LIMIT: usize = 256;

impl RepoService {
    fn new(
        repo_root: PathBuf,
        memo: Arc<MemoState>,
        session_store: Arc<SessionStore>,
    ) -> Result<Self> {
        let index_dir = default_index_dir(&repo_root);
        let engine = if index_exists(&index_dir) {
            Some(SearchEngine::open(&index_dir).context("failed to open index")?)
        } else {
            None
        };
        let service = Self {
            repo_root,
            index_dir: index_dir.clone(),
            engine: RwLock::new(engine),
            frecency: Mutex::new(FrecencyStore::open(&index_dir)),
            watcher: Mutex::new(None),
            last_seen_generation: AtomicU64::new(0),
            context_epoch: AtomicU64::new(0),
            search_change_journal: Arc::new(Mutex::new(VecDeque::new())),
            memo,
            session_store,
        };
        service.start_watcher_if_needed()?;
        Ok(service)
    }

    fn ensure_ready(&self) -> Result<()> {
        if !index_exists(&self.index_dir) {
            self.stop_watcher();
            let mut guard = self.engine.write().unwrap();
            *guard = None;
            return Ok(());
        }

        if self.engine.read().unwrap().is_none() {
            let new_engine = SearchEngine::open(&self.index_dir)
                .with_context(|| format!("failed to open index at {}", self.index_dir.display()))?;
            let mut guard = self.engine.write().unwrap();
            *guard = Some(new_engine);
        }

        self.start_watcher_if_needed()?;
        self.reload_if_dirty()?;
        Ok(())
    }

    fn status(&self) -> DaemonRootStatus {
        let generation = self.current_generation();
        let (index_available, delta_docs) = self
            .engine
            .read()
            .unwrap()
            .as_ref()
            .map(|engine| (true, engine.metadata().delta_docs))
            .unwrap_or_else(|| (index_exists(&self.index_dir), 0));

        DaemonRootStatus {
            target_root: self.repo_root.display().to_string(),
            index_dir: self.index_dir.display().to_string(),
            index_available,
            generation,
            context_epoch: self.context_epoch.load(Ordering::SeqCst),
            delta_docs,
        }
    }

    fn flush(&self) {
        if let Ok(store) = self.frecency.lock() {
            let _ = store.flush();
        }
        let _ = self.session_store.flush_to_disk();
    }

    fn current_generation(&self) -> u64 {
        self.watcher
            .lock()
            .unwrap()
            .as_ref()
            .map(|watcher| watcher.generation.load(Ordering::SeqCst))
            .unwrap_or_else(|| self.last_seen_generation.load(Ordering::SeqCst))
    }

    fn stop_watcher(&self) {
        if let Ok(mut guard) = self.watcher.lock()
            && let Some(handle) = guard.take()
        {
            handle.stop();
        }
    }

    fn start_watcher_if_needed(&self) -> Result<()> {
        if self.engine.read().unwrap().is_none() {
            return Ok(());
        }
        let mut guard = self.watcher.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }
        let handle = start_watcher(
            self.repo_root.clone(),
            self.index_dir.clone(),
            BuildConfig::default(),
            Some({
                let memo = Arc::clone(&self.memo);
                Arc::new(move |path| {
                    memo.mark_stale_for_all(path);
                })
            }),
            Some({
                let repo_root = self.repo_root.clone();
                let journal = self.search_change_journal.clone();
                Arc::new(move |generation, changed_paths| {
                    let paths: Vec<String> = changed_paths
                        .iter()
                        .filter_map(|path| normalize_relative_path(&repo_root, path))
                        .collect();
                    if paths.is_empty() {
                        return;
                    }
                    let mut guard = journal.lock().unwrap();
                    guard.push_back(SearchChangeBatch { generation, paths });
                    while guard.len() > SEARCH_CHANGE_JOURNAL_LIMIT {
                        guard.pop_front();
                    }
                })
            }),
        )
        .with_context(|| format!("failed to start watcher for {}", self.repo_root.display()))?;
        self.last_seen_generation
            .store(handle.generation.load(Ordering::SeqCst), Ordering::SeqCst);
        *guard = Some(handle);
        Ok(())
    }

    fn reload_if_dirty(&self) -> Result<()> {
        let current_generation = self
            .watcher
            .lock()
            .unwrap()
            .as_ref()
            .map(|watcher| watcher.generation.load(Ordering::SeqCst));
        let Some(current_generation) = current_generation else {
            return Ok(());
        };
        let previous_generation = self.last_seen_generation.load(Ordering::SeqCst);
        if current_generation == previous_generation {
            return Ok(());
        }

        let new_engine = SearchEngine::open(&self.index_dir)
            .with_context(|| format!("failed to reload index at {}", self.index_dir.display()))?;
        let mut guard = self.engine.write().unwrap();
        *guard = Some(new_engine);
        self.last_seen_generation
            .store(current_generation, Ordering::SeqCst);
        Ok(())
    }

    fn invalidate_search_context(&self) {
        self.context_epoch.fetch_add(1, Ordering::SeqCst);
    }

    fn check_search_reuse(&self, params: &SearchReuseCheckParams) -> SearchReuseCheckResponse {
        let current_generation = self.current_generation();
        let current_context_epoch = self.context_epoch.load(Ordering::SeqCst);
        if current_context_epoch != params.recorded_context_epoch {
            return SearchReuseCheckResponse {
                fresh: false,
                reason: SearchReuseReason::ContextInvalidated,
                generation: current_generation,
                context_epoch: current_context_epoch,
                changed_paths: Vec::new(),
            };
        }
        if current_generation < params.recorded_generation {
            return SearchReuseCheckResponse {
                fresh: false,
                reason: SearchReuseReason::GenerationReset,
                generation: current_generation,
                context_epoch: current_context_epoch,
                changed_paths: Vec::new(),
            };
        }
        if current_generation == params.recorded_generation {
            return SearchReuseCheckResponse {
                fresh: true,
                reason: SearchReuseReason::Unchanged,
                generation: current_generation,
                context_epoch: current_context_epoch,
                changed_paths: Vec::new(),
            };
        }

        let guard = self.search_change_journal.lock().unwrap();
        let earliest_generation = guard.front().map(|batch| batch.generation).unwrap_or(0);
        if earliest_generation > params.recorded_generation.saturating_add(1) {
            return SearchReuseCheckResponse {
                fresh: false,
                reason: SearchReuseReason::JournalOverflow,
                generation: current_generation,
                context_epoch: current_context_epoch,
                changed_paths: Vec::new(),
            };
        }

        let matched_paths: HashSet<&str> =
            params.matched_paths.iter().map(String::as_str).collect();
        for batch in guard
            .iter()
            .filter(|batch| batch.generation > params.recorded_generation)
        {
            for changed_path in &batch.paths {
                if matched_paths.contains(changed_path.as_str()) {
                    return SearchReuseCheckResponse {
                        fresh: false,
                        reason: SearchReuseReason::ChangedMatchedPath,
                        generation: current_generation,
                        context_epoch: current_context_epoch,
                        changed_paths: vec![changed_path.clone()],
                    };
                }
                let matches_scope =
                    query_matches_path_filters(changed_path, &params.request).unwrap_or(true);
                if matches_scope {
                    return SearchReuseCheckResponse {
                        fresh: false,
                        reason: SearchReuseReason::ChangedSearchScope,
                        generation: current_generation,
                        context_epoch: current_context_epoch,
                        changed_paths: vec![changed_path.clone()],
                    };
                }
            }
        }

        SearchReuseCheckResponse {
            fresh: true,
            reason: SearchReuseReason::Unchanged,
            generation: current_generation,
            context_epoch: current_context_epoch,
            changed_paths: Vec::new(),
        }
    }

    fn create_snapshot(
        &self,
        params: &SessionSnapshotCreateParams,
        daemon_dir: &Path,
    ) -> Result<SessionSnapshotCreateResponse> {
        let frecency = self.frecency.lock().unwrap();
        let manifest = snapshot::create_snapshot(
            &self.session_store,
            &frecency,
            &self.repo_root,
            daemon_dir,
            self.current_generation(),
            self.context_epoch.load(Ordering::SeqCst),
            params,
        )?;
        Ok(SessionSnapshotCreateResponse {
            snapshot_id: manifest.snapshot_id.clone(),
            snapshot_dir: snapshot::snapshot_dir(daemon_dir, &manifest.snapshot_id)
                .display()
                .to_string(),
            manifest,
        })
    }

    fn resume_prepare(
        &self,
        params: &SessionResumePrepareParams,
        daemon_dir: &Path,
    ) -> Result<search_core::SessionResumePrepareResponse> {
        let snapshot = snapshot::read_snapshot(daemon_dir, &params.snapshot_id)?;
        let mut frecency = self.frecency.lock().unwrap();
        hydrate::prepare_resume(
            &snapshot,
            &self.memo,
            &mut frecency,
            &self.repo_root,
            params.budget_tokens,
        )
    }
}

struct ServerState {
    daemon_dir: PathBuf,
    services: Mutex<HashMap<String, Arc<RepoService>>>,
    memo: Arc<MemoState>,
    session_store: Arc<SessionStore>,
}

impl ServerState {
    fn new(daemon_dir: PathBuf) -> Result<Self> {
        Ok(Self {
            session_store: Arc::new(SessionStore::load_from_disk(&daemon_dir)?),
            daemon_dir,
            services: Mutex::new(HashMap::new()),
            memo: Arc::new(MemoState::new(Duration::from_secs(600))),
        })
    }

    fn preload_root(&self, root: PathBuf) -> Result<()> {
        let service = self.service_for_root(&root)?;
        service.ensure_ready()?;
        Ok(())
    }

    fn service_for_root(&self, root: &Path) -> Result<Arc<RepoService>> {
        let key = root.display().to_string();
        if let Some(existing) = self.services.lock().unwrap().get(&key).cloned() {
            return Ok(existing);
        }

        let service = Arc::new(RepoService::new(
            root.to_path_buf(),
            Arc::clone(&self.memo),
            Arc::clone(&self.session_store),
        )?);
        let mut services = self.services.lock().unwrap();
        Ok(services
            .entry(key)
            .or_insert_with(|| Arc::clone(&service))
            .clone())
    }

    fn active_root_count(&self) -> usize {
        self.services.lock().unwrap().len()
    }

    fn flush_all(&self) {
        let services: Vec<_> = self.services.lock().unwrap().values().cloned().collect();
        for service in services {
            service.flush();
        }
    }

    fn shutdown(&self) {
        let services: Vec<_> = self.services.lock().unwrap().values().cloned().collect();
        for service in services {
            service.flush();
            service.stop_watcher();
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let daemon_root = daemon_dir();
    let port_file = daemon_root.join(DAEMON_PORT_FILE);
    let pid_file = daemon_root.join(DAEMON_PID_FILE);
    let idle_timeout = if args.idle_timeout == 0 {
        None
    } else {
        Some(Duration::from_secs(args.idle_timeout))
    };

    std::fs::create_dir_all(&daemon_root).context("failed to create daemon dir")?;
    let _ = std::fs::remove_file(&port_file);

    let bind_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, args.port.unwrap_or(0)));
    let listener = TcpListener::bind(bind_addr).context("failed to bind loopback TCP listener")?;
    listener
        .set_nonblocking(true)
        .context("set_nonblocking failed")?;
    let listen_port = listener
        .local_addr()
        .context("failed to inspect listener address")?
        .port();

    std::fs::write(&pid_file, std::process::id().to_string())
        .context("failed to write PID file")?;
    std::fs::write(&port_file, listen_port.to_string())
        .context("failed to write daemon port file")?;

    let state = Arc::new(ServerState::new(daemon_root.clone())?);
    if let Some(root) = args.root.or(args.repo) {
        state.preload_root(canonicalize_target_root(&root)?)?;
    }

    install_signal_handlers();

    eprintln!(
        "triseek-server: listening on {}:{}",
        DAEMON_HOST, listen_port
    );

    let started = Instant::now();
    let mut last_request = Instant::now();
    let mut last_flush = Instant::now();

    loop {
        if SHUTDOWN.load(Ordering::SeqCst) {
            break;
        }

        if let Some(timeout) = idle_timeout
            && last_request.elapsed() > timeout
        {
            eprintln!("triseek-server: idle timeout reached, shutting down");
            break;
        }

        if last_flush.elapsed() >= Duration::from_secs(5) {
            state.flush_all();
            last_flush = Instant::now();
        }

        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(error) => {
                eprintln!("triseek-server: accept error: {error}");
                continue;
            }
        };

        last_request = Instant::now();

        let state = Arc::clone(&state);
        let started_clone = started;
        std::thread::spawn(move || {
            if let Err(error) = handle_connection(stream, state, started_clone) {
                eprintln!("triseek-server: connection error: {error}");
            }
        });
    }

    eprintln!("triseek-server: shutting down");
    state.shutdown();
    let _ = std::fs::remove_file(&port_file);
    let _ = std::fs::remove_file(&pid_file);

    Ok(())
}

fn handle_connection(stream: TcpStream, state: Arc<ServerState>, started: Instant) -> Result<()> {
    let mut writer = stream.try_clone().context("clone stream")?;
    let reader = BufReader::new(stream);

    for line in reader.lines() {
        let line = line.context("read line")?;
        if line.trim().is_empty() {
            continue;
        }

        let request: RpcRequest = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                let response = RpcResponse::error(0, -32700, format!("parse error: {error}"));
                writeln!(writer, "{}", serde_json::to_string(&response)?)?;
                continue;
            }
        };

        let response = dispatch(request, &state, started);
        writeln!(writer, "{}", serde_json::to_string(&response)?)?;
    }
    Ok(())
}

fn dispatch(request: RpcRequest, state: &ServerState, started: Instant) -> RpcResponse {
    let id = request.id;
    match request.method.as_str() {
        "search" => {
            let params: DaemonSearchParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            let service = match service_for_target_root(state, &params.target_root) {
                Ok(service) => service,
                Err(error) => return RpcResponse::error(id, -32000, error.to_string()),
            };
            if let Err(error) = service.ensure_ready() {
                return RpcResponse::error(id, -32000, error.to_string());
            }
            let session_id = params.session_id.clone();
            let query = params.request;
            let guard = service.engine.read().unwrap();
            let Some(engine) = guard.as_ref() else {
                return RpcResponse::error(id, -32000, "index not available");
            };
            let plan = plan_query(&query);
            let routing = route_query(
                &query,
                Some(&engine.metadata().repo_stats),
                &plan,
                true,
                false,
            );
            match engine.search(&query) {
                Ok(execution) => {
                    let mut hits = execution.hits;
                    if let Ok(mut store) = service.frecency.lock() {
                        if !store.is_empty() {
                            store.rerank_hits(&mut hits);
                        }
                        store.record_results(&hits);
                        store.record_query(QueryEvent {
                            timestamp_secs: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|duration| duration.as_secs() as i64)
                                .unwrap_or(0),
                            pattern: query.pattern.clone(),
                            kind: format!("{:?}", query.kind).to_ascii_lowercase(),
                            result_paths: hits
                                .iter()
                                .take(20)
                                .map(|hit| match hit {
                                    SearchHit::Content { path, .. } | SearchHit::Path { path } => {
                                        path.clone()
                                    }
                                })
                                .collect(),
                            selected_paths: vec![],
                        });
                    }
                    let response = SearchResponse {
                        request: query.clone(),
                        effective_kind: if matches!(query.kind, SearchKind::Regex) {
                            SearchKind::Regex
                        } else {
                            SearchKind::Literal
                        },
                        engine: SearchEngineKind::Indexed,
                        routing,
                        plan,
                        hits,
                        summary: execution.summary,
                        metrics: execution.metrics,
                    };
                    if let Some(session_id) = session_id {
                        let result_paths = response
                            .hits
                            .iter()
                            .take(20)
                            .map(|hit| match hit {
                                SearchHit::Content { path, .. } | SearchHit::Path { path } => {
                                    path.clone()
                                }
                            })
                            .collect::<Vec<_>>();
                        let _ = service.session_store.record_action(
                            &session_id,
                            ActionKind::Search,
                            serde_json::json!({
                                "method": "search",
                                "query": query.pattern,
                                "kind": format!("{:?}", query.kind).to_ascii_lowercase(),
                                "result_paths": result_paths,
                            }),
                        );
                    }
                    RpcResponse::ok(id, response)
                }
                Err(error) => RpcResponse::error(id, -32000, error.to_string()),
            }
        }
        "status" => {
            let params: DaemonStatusParams = if request.params.is_null() {
                DaemonStatusParams { target_root: None }
            } else {
                match serde_json::from_value(request.params) {
                    Ok(params) => params,
                    Err(error) => {
                        return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                    }
                }
            };
            let root = if let Some(target_root) = params.target_root {
                match service_for_target_root(state, &target_root) {
                    Ok(service) => {
                        let _ = service.ensure_ready();
                        Some(service.status())
                    }
                    Err(error) => return RpcResponse::error(id, -32000, error.to_string()),
                }
            } else {
                None
            };
            RpcResponse::ok(
                id,
                DaemonStatus {
                    daemon_dir: state.daemon_dir.display().to_string(),
                    uptime_secs: started.elapsed().as_secs(),
                    active_roots: state.active_root_count(),
                    root,
                },
            )
        }
        "frecency_select" => {
            let params: FrecencySelectParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            let service = match service_for_target_root(state, &params.target_root) {
                Ok(service) => service,
                Err(error) => return RpcResponse::error(id, -32000, error.to_string()),
            };
            if let Ok(mut store) = service.frecency.lock() {
                store.record_select(&params.paths);
                let _ = store.flush();
            }
            RpcResponse::ok(id, serde_json::json!({"recorded": params.paths.len()}))
        }
        "reload" => {
            let params: DaemonStatusParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            let Some(target_root) = params.target_root else {
                return RpcResponse::error(id, -32602, "reload requires target_root");
            };
            let service = match service_for_target_root(state, &target_root) {
                Ok(service) => service,
                Err(error) => return RpcResponse::error(id, -32000, error.to_string()),
            };
            if let Err(error) = service.ensure_ready() {
                return RpcResponse::error(id, -32000, error.to_string());
            }
            match SearchEngine::open(&service.index_dir) {
                Ok(new_engine) => {
                    let mut guard = service.engine.write().unwrap();
                    *guard = Some(new_engine);
                    RpcResponse::ok(id, serde_json::json!({"reloaded": true}))
                }
                Err(error) => RpcResponse::error(id, -32000, error.to_string()),
            }
        }
        "preload_root" => {
            let params: DaemonRootParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            let root = match canonicalize_target_root(Path::new(&params.target_root)) {
                Ok(root) => root,
                Err(error) => return RpcResponse::error(id, -32000, error.to_string()),
            };
            match state.preload_root(root.clone()) {
                Ok(()) => RpcResponse::ok(
                    id,
                    serde_json::json!({
                        "preloaded": true,
                        "target_root": root.display().to_string(),
                    }),
                ),
                Err(error) => RpcResponse::error(id, -32000, error.to_string()),
            }
        }
        "memo_observe" => {
            let params: MemoObserveParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            if matches!(
                params.event,
                MemoEventKind::SessionStart | MemoEventKind::PreCompact
            ) && let Ok(service) = service_for_target_root(state, &params.repo_root)
            {
                service.invalidate_search_context();
            }
            RpcResponse::ok(id, state.memo.observe(&params))
        }
        "memo_status" => {
            let params: MemoStatusParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            RpcResponse::ok(id, state.memo.status(&params))
        }
        "memo_session_start" => {
            let params: MemoSessionParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            RpcResponse::ok(id, state.memo.session_start(&params))
        }
        "memo_session_end" => {
            let params: MemoSessionParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            RpcResponse::ok(id, state.memo.session_end(&params))
        }
        "memo_session" => {
            let params: MemoSessionParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            RpcResponse::ok(id, state.memo.session(&params))
        }
        "memo_check" => {
            let params: MemoCheckParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            let response = state.memo.check(&params);
            RpcResponse::ok(id, response)
        }
        "search_reuse_check" => {
            let params: SearchReuseCheckParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            let service = match service_for_target_root(state, &params.target_root) {
                Ok(service) => service,
                Err(error) => return RpcResponse::error(id, -32000, error.to_string()),
            };
            if let Err(error) = service.ensure_ready() {
                return RpcResponse::error(id, -32000, error.to_string());
            }
            RpcResponse::ok(id, service.check_search_reuse(&params))
        }
        "session_open" => {
            let params: SessionOpenParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            let root = match canonicalize_target_root(Path::new(&params.target_root)) {
                Ok(root) => root,
                Err(error) => return RpcResponse::error(id, -32000, error.to_string()),
            };
            match state.session_store.open_session(
                params.session_id,
                params.goal,
                root.display().to_string(),
            ) {
                Ok(session) => RpcResponse::ok(id, SessionOpenResponse { session }),
                Err(error) => RpcResponse::error(id, -32000, error.to_string()),
            }
        }
        "session_list" => {
            let params: SessionListParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            let root = match canonicalize_target_root(Path::new(&params.target_root)) {
                Ok(root) => root,
                Err(error) => return RpcResponse::error(id, -32000, error.to_string()),
            };
            let root_string = root.display().to_string();
            RpcResponse::ok(
                id,
                SessionListResponse {
                    sessions: state
                        .session_store
                        .list_sessions(Some(root_string.as_str())),
                },
            )
        }
        "session_status" => {
            let params: PortabilitySessionStatusParams =
                match serde_json::from_value(request.params) {
                    Ok(params) => params,
                    Err(error) => {
                        return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                    }
                };
            match state.session_store.session(&params.session_id) {
                Ok(session) => RpcResponse::ok(
                    id,
                    PortabilitySessionStatusResponse {
                        action_log_size: state
                            .session_store
                            .entries_for_session(&params.session_id)
                            .len(),
                        session,
                    },
                ),
                Err(error) => RpcResponse::error(id, -32000, error.to_string()),
            }
        }
        "session_close" => {
            let params: SessionCloseParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            match state
                .session_store
                .close_session(&params.session_id, params.status)
            {
                Ok(session) => RpcResponse::ok(id, search_core::SessionCloseResponse { session }),
                Err(error) => RpcResponse::error(id, -32000, error.to_string()),
            }
        }
        "session_record_action" => {
            let params: SessionRecordActionParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            let service = match service_for_target_root(state, &params.target_root) {
                Ok(service) => service,
                Err(error) => return RpcResponse::error(id, -32000, error.to_string()),
            };
            match service.session_store.record_action(
                &params.session_id,
                params.kind,
                params.payload,
            ) {
                Ok(entry) => RpcResponse::ok(
                    id,
                    SessionRecordActionResponse {
                        entry_id: entry.entry_id,
                    },
                ),
                Err(error) => RpcResponse::error(id, -32000, error.to_string()),
            }
        }
        "session_snapshot_create" => {
            let params: SessionSnapshotCreateParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            let service = match service_for_target_root(state, &params.target_root) {
                Ok(service) => service,
                Err(error) => return RpcResponse::error(id, -32000, error.to_string()),
            };
            match service.create_snapshot(&params, &state.daemon_dir) {
                Ok(response) => RpcResponse::ok(id, response),
                Err(error) => RpcResponse::error(id, -32000, error.to_string()),
            }
        }
        "session_snapshot_list" => {
            let params: SessionSnapshotListParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            match snapshot::list_snapshots(&state.daemon_dir, params.session_id.as_deref()) {
                Ok(snapshots) => RpcResponse::ok(id, SessionSnapshotListResponse { snapshots }),
                Err(error) => RpcResponse::error(id, -32000, error.to_string()),
            }
        }
        "session_snapshot_get" => {
            let params: SessionSnapshotGetParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            match snapshot::read_snapshot(&state.daemon_dir, &params.snapshot_id) {
                Ok(snapshot) => RpcResponse::ok(id, SessionSnapshotGetResponse { snapshot }),
                Err(error) => RpcResponse::error(id, -32000, error.to_string()),
            }
        }
        "session_snapshot_diff" => {
            let params: SessionSnapshotDiffParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            let a = match snapshot::read_snapshot(&state.daemon_dir, &params.snapshot_a) {
                Ok(snapshot) => snapshot,
                Err(error) => return RpcResponse::error(id, -32000, error.to_string()),
            };
            let b = match snapshot::read_snapshot(&state.daemon_dir, &params.snapshot_b) {
                Ok(snapshot) => snapshot,
                Err(error) => return RpcResponse::error(id, -32000, error.to_string()),
            };
            RpcResponse::ok(
                id,
                SessionSnapshotDiffResponse {
                    diff: snapshot::diff_snapshots(&a, &b),
                },
            )
        }
        "session_resume_prepare" => {
            let params: SessionResumePrepareParams = match serde_json::from_value(request.params) {
                Ok(params) => params,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            let service = match service_for_target_root(state, &params.target_root) {
                Ok(service) => service,
                Err(error) => return RpcResponse::error(id, -32000, error.to_string()),
            };
            match service.resume_prepare(&params, &state.daemon_dir) {
                Ok(response) => RpcResponse::ok(id, response),
                Err(error) => RpcResponse::error(id, -32000, error.to_string()),
            }
        }
        "shutdown" => {
            SHUTDOWN.store(true, Ordering::SeqCst);
            RpcResponse::ok(id, serde_json::json!({"shutdown": true}))
        }
        _ => RpcResponse::error(id, -32601, format!("unknown method: {}", request.method)),
    }
}

fn service_for_target_root(state: &ServerState, target_root: &str) -> Result<Arc<RepoService>> {
    let root = canonicalize_target_root(Path::new(target_root))?;
    state.service_for_root(&root)
}

fn canonicalize_target_root(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("failed to canonicalize root {}", path.display()))
}

fn normalize_relative_path(repo_root: &Path, absolute_path: &Path) -> Option<String> {
    let relative = absolute_path.strip_prefix(repo_root).ok()?;
    let rendered = relative.to_string_lossy().replace('\\', "/");
    if rendered.is_empty() {
        None
    } else {
        Some(rendered)
    }
}

#[cfg(unix)]
fn install_signal_handlers() {
    unsafe {
        libc::signal(
            libc::SIGTERM,
            handle_signal as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGINT,
            handle_signal as *const () as libc::sighandler_t,
        );
    }
}

#[cfg(not(unix))]
fn install_signal_handlers() {}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use search_core::{CaseMode, QueryRequest, SearchEngineKind, SearchKind};

    fn make_service(
        initial_generation: u64,
        context_epoch: u64,
    ) -> (tempfile::TempDir, RepoService) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).expect("create repo root");
        let index_dir = repo_root.join(".triseek-index");
        std::fs::create_dir_all(&index_dir).expect("create index dir");
        let service = RepoService {
            repo_root,
            index_dir: index_dir.clone(),
            engine: RwLock::new(None),
            frecency: Mutex::new(FrecencyStore::open(&index_dir)),
            watcher: Mutex::new(None),
            last_seen_generation: AtomicU64::new(initial_generation),
            context_epoch: AtomicU64::new(context_epoch),
            search_change_journal: Arc::new(Mutex::new(VecDeque::new())),
            memo: Arc::new(MemoState::new(Duration::from_secs(600))),
            session_store: Arc::new(
                SessionStore::load_from_disk(tmp.path()).expect("session store"),
            ),
        };
        (tmp, service)
    }

    fn scoped_request() -> QueryRequest {
        QueryRequest {
            kind: SearchKind::Literal,
            engine: SearchEngineKind::Auto,
            pattern: "route".into(),
            case_mode: CaseMode::Sensitive,
            globs: vec!["src/**/*.rs".into()],
            ..QueryRequest::default()
        }
    }

    #[test]
    fn search_reuse_stays_fresh_for_unrelated_scope_changes() {
        let (_tmp, service) = make_service(11, 1);
        service
            .search_change_journal
            .lock()
            .unwrap()
            .push_back(SearchChangeBatch {
                generation: 11,
                paths: vec!["docs/guide.md".into()],
            });
        let response = service.check_search_reuse(&SearchReuseCheckParams {
            target_root: service.repo_root.display().to_string(),
            request: scoped_request(),
            recorded_generation: 10,
            recorded_context_epoch: 1,
            matched_paths: vec!["src/lib.rs".into()],
        });
        assert!(response.fresh);
        assert!(matches!(response.reason, SearchReuseReason::Unchanged));
    }

    #[test]
    fn search_reuse_invalidates_when_matched_path_changes() {
        let (_tmp, service) = make_service(11, 1);
        service
            .search_change_journal
            .lock()
            .unwrap()
            .push_back(SearchChangeBatch {
                generation: 11,
                paths: vec!["src/lib.rs".into()],
            });
        let response = service.check_search_reuse(&SearchReuseCheckParams {
            target_root: service.repo_root.display().to_string(),
            request: scoped_request(),
            recorded_generation: 10,
            recorded_context_epoch: 1,
            matched_paths: vec!["src/lib.rs".into()],
        });
        assert!(!response.fresh);
        assert!(matches!(
            response.reason,
            SearchReuseReason::ChangedMatchedPath
        ));
    }

    #[test]
    fn search_reuse_invalidates_when_context_epoch_changes() {
        let (_tmp, service) = make_service(10, 2);
        let response = service.check_search_reuse(&SearchReuseCheckParams {
            target_root: service.repo_root.display().to_string(),
            request: scoped_request(),
            recorded_generation: 10,
            recorded_context_epoch: 1,
            matched_paths: vec!["src/lib.rs".into()],
        });
        assert!(!response.fresh);
        assert!(matches!(
            response.reason,
            SearchReuseReason::ContextInvalidated
        ));
    }

    #[test]
    fn search_reuse_invalidates_when_journal_window_is_exhausted() {
        let (_tmp, service) = make_service(25, 1);
        service
            .search_change_journal
            .lock()
            .unwrap()
            .push_back(SearchChangeBatch {
                generation: 20,
                paths: vec!["src/lib.rs".into()],
            });
        let response = service.check_search_reuse(&SearchReuseCheckParams {
            target_root: service.repo_root.display().to_string(),
            request: scoped_request(),
            recorded_generation: 10,
            recorded_context_epoch: 1,
            matched_paths: vec!["src/lib.rs".into()],
        });
        assert!(!response.fresh);
        assert!(matches!(
            response.reason,
            SearchReuseReason::JournalOverflow
        ));
    }

    #[test]
    fn memo_observe_session_start_bumps_search_context_epoch() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).expect("create repo root");
        let repo_root = repo_root.canonicalize().expect("canonicalize repo root");
        let state = ServerState::new(tmp.path().join("daemon")).expect("server state");
        let service = state
            .service_for_root(&repo_root)
            .expect("create repo service");
        assert_eq!(service.context_epoch.load(Ordering::SeqCst), 0);

        let response = dispatch(
            RpcRequest {
                jsonrpc: "2.0".into(),
                id: 1,
                method: "memo_observe".into(),
                params: serde_json::to_value(MemoObserveParams {
                    session_id: "s1".into(),
                    repo_root: repo_root.display().to_string(),
                    event: MemoEventKind::SessionStart,
                    path: None,
                    content_hash: None,
                    tokens: None,
                })
                .expect("serialize params"),
            },
            &state,
            Instant::now(),
        );
        assert!(response.error.is_none());
        assert_eq!(service.context_epoch.load(Ordering::SeqCst), 1);
    }
}

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn handle_signal(_: libc::c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}
