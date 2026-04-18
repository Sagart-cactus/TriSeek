mod memo;

use anyhow::{Context, Result};
use clap::Parser;
use memo::MemoState;
use search_core::{
    DAEMON_HOST, DAEMON_PID_FILE, DAEMON_PORT_FILE, DaemonRootParams, DaemonRootStatus,
    DaemonSearchParams, DaemonStatus, DaemonStatusParams, FrecencySelectParams, MemoCheckParams,
    MemoObserveParams, MemoSessionParams, MemoStatusParams, RpcRequest, RpcResponse,
    SearchEngineKind, SearchHit, SearchKind, SearchResponse, plan_query, route_query,
};
use search_frecency::{FrecencyStore, QueryEvent};
use search_index::{
    BuildConfig, SearchEngine, WatcherHandle, daemon_dir, default_index_dir, index_exists,
    start_watcher,
};
use std::collections::HashMap;
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
    memo: Arc<MemoState>,
}

impl RepoService {
    fn new(repo_root: PathBuf, memo: Arc<MemoState>) -> Result<Self> {
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
            memo,
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
        let generation = self
            .watcher
            .lock()
            .unwrap()
            .as_ref()
            .map(|watcher| watcher.generation.load(Ordering::SeqCst))
            .unwrap_or(0);
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
            delta_docs,
        }
    }

    fn flush(&self) {
        if let Ok(store) = self.frecency.lock() {
            let _ = store.flush();
        }
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
}

struct ServerState {
    daemon_dir: PathBuf,
    services: Mutex<HashMap<String, Arc<RepoService>>>,
    memo: Arc<MemoState>,
}

impl ServerState {
    fn new(daemon_dir: PathBuf) -> Self {
        Self {
            daemon_dir,
            services: Mutex::new(HashMap::new()),
            memo: Arc::new(MemoState::new(Duration::from_secs(600))),
        }
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

    let state = Arc::new(ServerState::new(daemon_root.clone()));
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

        if last_flush.elapsed() >= Duration::from_secs(30) {
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
            RpcResponse::ok(id, state.memo.check(&params))
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

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn handle_signal(_: libc::c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}
