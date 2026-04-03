use anyhow::{Context, Result};
use clap::Parser;
#[cfg(unix)]
use libc;
use search_core::{
    DAEMON_HOST, DAEMON_PID_FILE, DAEMON_PORT_FILE, DaemonStatus, FrecencySelectParams,
    QueryRequest, RpcRequest, RpcResponse, SearchEngineKind, SearchHit, SearchKind, SearchResponse,
    plan_query, route_query,
};
use search_frecency::{FrecencyStore, QueryEvent};
use search_index::{BuildConfig, SearchEngine, start_watcher};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "triseek-server")]
#[command(about = "TriSeek background search daemon")]
struct Args {
    /// Repository root to watch and index.
    #[arg(long, required = true)]
    repo: PathBuf,
    /// Index directory (defaults to <repo>/.triseek-index).
    #[arg(long)]
    index_dir: Option<PathBuf>,
    /// TCP port for the loopback control plane (defaults to an ephemeral port).
    #[arg(long)]
    port: Option<u16>,
    /// Idle timeout in seconds before auto-exit (0 = never).
    #[arg(long, default_value_t = 1800)]
    idle_timeout: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let repo_root = args
        .repo
        .canonicalize()
        .context("failed to canonicalize repo path")?;
    let index_dir = args
        .index_dir
        .unwrap_or_else(|| search_index::default_index_dir(&repo_root));
    let port_file = index_dir.join(DAEMON_PORT_FILE);
    let pid_file = index_dir.join(DAEMON_PID_FILE);
    let idle_timeout = if args.idle_timeout == 0 {
        None
    } else {
        Some(Duration::from_secs(args.idle_timeout))
    };

    std::fs::create_dir_all(&index_dir).context("failed to create index dir")?;
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

    let engine: Arc<RwLock<Option<SearchEngine>>> = if search_index::index_exists(&index_dir) {
        let eng = SearchEngine::open(&index_dir).context("failed to open index")?;
        Arc::new(RwLock::new(Some(eng)))
    } else {
        Arc::new(RwLock::new(None))
    };

    let frecency: Arc<Mutex<FrecencyStore>> = Arc::new(Mutex::new(FrecencyStore::open(&index_dir)));

    let config = BuildConfig::default();
    let watcher = start_watcher(repo_root.clone(), index_dir.clone(), config)
        .context("failed to start watcher")?;
    let watcher_gen = Arc::clone(&watcher.generation);
    let last_seen_gen = Arc::new(AtomicU64::new(watcher_gen.load(Ordering::SeqCst)));

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

        if let Some(timeout) = idle_timeout {
            if last_request.elapsed() > timeout {
                eprintln!("triseek-server: idle timeout reached, shutting down");
                break;
            }
        }

        let current_gen = watcher_gen.load(Ordering::SeqCst);
        if current_gen != last_seen_gen.load(Ordering::SeqCst) {
            last_seen_gen.store(current_gen, Ordering::SeqCst);
            if let Ok(new_engine) = SearchEngine::open(&index_dir) {
                let mut guard = engine.write().unwrap();
                *guard = Some(new_engine);
            }
        }

        if last_flush.elapsed() >= Duration::from_secs(30) {
            if let Ok(store) = frecency.lock() {
                let _ = store.flush();
            }
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

        let engine = Arc::clone(&engine);
        let frecency = Arc::clone(&frecency);
        let index_dir = index_dir.clone();
        let repo_root = repo_root.clone();
        let watcher_gen_clone = Arc::clone(&watcher_gen);
        let started_clone = started;

        std::thread::spawn(move || {
            if let Err(error) = handle_connection(
                stream,
                engine,
                frecency,
                index_dir,
                repo_root,
                watcher_gen_clone,
                started_clone,
            ) {
                eprintln!("triseek-server: connection error: {error}");
            }
        });
    }

    eprintln!("triseek-server: shutting down");
    watcher.stop();
    if let Ok(store) = frecency.lock() {
        let _ = store.flush();
    }
    let _ = std::fs::remove_file(&port_file);
    let _ = std::fs::remove_file(&pid_file);

    Ok(())
}

fn handle_connection(
    stream: TcpStream,
    engine: Arc<RwLock<Option<SearchEngine>>>,
    frecency: Arc<Mutex<FrecencyStore>>,
    index_dir: PathBuf,
    repo_root: PathBuf,
    generation: Arc<AtomicU64>,
    started: Instant,
) -> Result<()> {
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

        let response = dispatch(
            request,
            &engine,
            &frecency,
            &index_dir,
            &repo_root,
            &generation,
            started,
        );

        writeln!(writer, "{}", serde_json::to_string(&response)?)?;
    }
    Ok(())
}

fn dispatch(
    request: RpcRequest,
    engine: &RwLock<Option<SearchEngine>>,
    frecency: &Mutex<FrecencyStore>,
    index_dir: &std::path::Path,
    repo_root: &std::path::Path,
    generation: &AtomicU64,
    started: Instant,
) -> RpcResponse {
    let id = request.id;
    match request.method.as_str() {
        "search" => {
            let query: QueryRequest = match serde_json::from_value(request.params) {
                Ok(query) => query,
                Err(error) => {
                    return RpcResponse::error(id, -32602, format!("invalid params: {error}"));
                }
            };
            let guard = engine.read().unwrap();
            let Some(eng) = guard.as_ref() else {
                return RpcResponse::error(id, -32000, "index not available");
            };
            let plan = plan_query(&query);
            let routing = route_query(&query, Some(&eng.metadata().repo_stats), &plan, true, false);
            match eng.search(&query) {
                Ok(execution) => {
                    let mut hits = execution.hits;
                    if let Ok(mut store) = frecency.lock() {
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
            let guard = engine.read().unwrap();
            let (delta_docs, _meta_build_secs) = guard
                .as_ref()
                .map(|engine| {
                    let metadata = engine.metadata();
                    (metadata.delta_docs, metadata.build_stats.build_millis)
                })
                .unwrap_or((0, 0));
            drop(guard);
            RpcResponse::ok(
                id,
                DaemonStatus {
                    repo_root: repo_root.display().to_string(),
                    index_dir: index_dir.display().to_string(),
                    uptime_secs: started.elapsed().as_secs(),
                    generation: generation.load(Ordering::SeqCst),
                    delta_docs,
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
            if let Ok(mut store) = frecency.lock() {
                store.record_select(&params.paths);
                let _ = store.flush();
            }
            RpcResponse::ok(id, serde_json::json!({"recorded": params.paths.len()}))
        }
        "reload" => match SearchEngine::open(index_dir) {
            Ok(new_engine) => {
                let mut guard = engine.write().unwrap();
                *guard = Some(new_engine);
                RpcResponse::ok(id, serde_json::json!({"reloaded": true}))
            }
            Err(error) => RpcResponse::error(id, -32000, error.to_string()),
        },
        "shutdown" => {
            SHUTDOWN.store(true, Ordering::SeqCst);
            RpcResponse::ok(id, serde_json::json!({"shutdown": true}))
        }
        _ => RpcResponse::error(id, -32601, format!("unknown method: {}", request.method)),
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

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn handle_signal(_: libc::c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}
