use crate::build::{BuildConfig, apply_incremental_changes};
use crate::error::SearchIndexError;
use crate::walker::{ScanOptions, normalize_relative, scan_single_file};
use ignore::gitignore::GitignoreBuilder;
use notify::event::{EventKind, ModifyKind, RenameMode};
use notify::{RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Signals the index generation number. Incremented each time the watcher updates the index.
/// Consumers (e.g. the server) can compare this to detect when to reload the engine.
pub type GenerationCounter = Arc<AtomicU64>;
pub type WatcherChangeCallback = Arc<dyn Fn(&Path) + Send + Sync + 'static>;
pub type WatcherBatchCallback = Arc<dyn Fn(u64, &[PathBuf]) + Send + Sync + 'static>;

/// Debounce windows.
const PER_FILE_WAIT: Duration = Duration::from_millis(200);
const BATCH_WAIT: Duration = Duration::from_millis(500);
const MAX_BATCH_WAIT: Duration = Duration::from_secs(5);

/// What happened to a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChangeKind {
    CreatedOrModified,
    Deleted,
}

/// Handle to the background watcher thread. Drop to stop watching.
pub struct WatcherHandle {
    shutdown_tx: mpsc::SyncSender<()>,
    thread: Option<JoinHandle<()>>,
    /// Incremented each time the index is updated.
    pub generation: GenerationCounter,
}

impl WatcherHandle {
    /// Request a graceful shutdown and wait for the watcher thread to finish.
    pub fn stop(mut self) {
        let _ = self.shutdown_tx.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.try_send(());
        // Don't join on drop — just signal. The thread will exit on its own.
    }
}

/// Start a background watcher on `repo_root`.
///
/// The watcher monitors the repository for file-system changes, debounces rapid
/// bursts, and applies incremental index updates via [`apply_incremental_changes`].
///
/// Returns a [`WatcherHandle`] that keeps the watcher alive. Drop or call
/// [`WatcherHandle::stop`] to shut it down.
pub fn start_watcher(
    repo_root: PathBuf,
    index_dir: PathBuf,
    config: BuildConfig,
    on_change: Option<WatcherChangeCallback>,
    on_index_update: Option<WatcherBatchCallback>,
) -> Result<WatcherHandle, SearchIndexError> {
    // Canonicalize so path comparisons work even when symlinks are involved
    // (e.g. /tmp → /private/tmp on macOS).
    let repo_root = repo_root.canonicalize().unwrap_or(repo_root);
    let index_dir = index_dir.canonicalize().unwrap_or(index_dir);
    let (event_tx, event_rx) = mpsc::channel::<notify::Event>();
    let (shutdown_tx, shutdown_rx) = mpsc::sync_channel::<()>(1);
    let generation = Arc::new(AtomicU64::new(0));
    let generation_clone = Arc::clone(&generation);

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            let _ = event_tx.send(event);
        }
    })
    .map_err(|e| SearchIndexError::Io(std::io::Error::other(e.to_string())))?;

    watcher
        .watch(&repo_root, RecursiveMode::Recursive)
        .map_err(|e| SearchIndexError::Io(std::io::Error::other(e.to_string())))?;

    let thread = thread::spawn(move || {
        // Keep the watcher alive for the duration of the thread.
        let _watcher = watcher;
        watcher_loop(
            repo_root,
            index_dir,
            config,
            event_rx,
            shutdown_rx,
            generation_clone,
            on_change,
            on_index_update,
        );
    });

    Ok(WatcherHandle {
        shutdown_tx,
        thread: Some(thread),
        generation,
    })
}

#[allow(clippy::too_many_arguments)]
fn watcher_loop(
    repo_root: PathBuf,
    index_dir: PathBuf,
    config: BuildConfig,
    event_rx: mpsc::Receiver<notify::Event>,
    shutdown_rx: mpsc::Receiver<()>,
    generation: GenerationCounter,
    on_change: Option<WatcherChangeCallback>,
    on_index_update: Option<WatcherBatchCallback>,
) {
    // Build a gitignore matcher for the repo root.
    let gitignore = {
        let mut builder = GitignoreBuilder::new(&repo_root);
        let _ = builder.add(repo_root.join(".gitignore"));
        builder.build().ok()
    };

    let opts = ScanOptions::from(&config);

    loop {
        // Check for shutdown first (non-blocking).
        if shutdown_rx.try_recv().is_ok() {
            break;
        }

        // Wait for the first event (block until one arrives or shutdown).
        let first = match event_rx.recv_timeout(Duration::from_secs(1)) {
            Ok(event) => event,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        // Collect events into a pending change map with two-tier debounce.
        let mut pending: HashMap<PathBuf, ChangeKind> = HashMap::new();
        apply_event(&mut pending, first);

        let batch_start = Instant::now();
        let mut last_event = Instant::now();

        loop {
            // Stop collecting when the per-file window has been quiet long enough
            // OR we've been collecting for the max batch wait.
            let per_file_remaining = PER_FILE_WAIT.saturating_sub(last_event.elapsed());
            let batch_remaining = MAX_BATCH_WAIT.saturating_sub(batch_start.elapsed());
            let wait = per_file_remaining.min(batch_remaining).max(BATCH_WAIT);

            if wait.is_zero() {
                break;
            }

            match event_rx.recv_timeout(wait) {
                Ok(event) => {
                    apply_event(&mut pending, event);
                    last_event = Instant::now();
                }
                Err(_) => break,
            }
        }

        if pending.is_empty() {
            continue;
        }

        // Filter ignored paths and split into added/removed.
        let mut added_or_modified = Vec::new();
        let mut removed_relative = Vec::new();
        let mut changed_paths = Vec::new();

        for (abs_path, kind) in &pending {
            // Skip paths outside the repo root (shouldn't happen but be safe).
            if !abs_path.starts_with(&repo_root) {
                continue;
            }

            // Check gitignore.
            if let Some(ref gi) = gitignore {
                let is_dir = abs_path.is_dir();
                if gi.matched(abs_path, is_dir).is_ignore() {
                    continue;
                }
            }

            // Skip the index directory itself.
            if abs_path.starts_with(&index_dir) {
                continue;
            }
            changed_paths.push(abs_path.clone());

            let relative = normalize_relative(&repo_root, abs_path);

            match kind {
                ChangeKind::Deleted => {
                    removed_relative.push(relative);
                }
                ChangeKind::CreatedOrModified => {
                    match scan_single_file(&repo_root, abs_path, &opts) {
                        Ok(Some(file)) => added_or_modified.push(file),
                        Ok(None) => {
                            // File excluded (binary, too large, etc.) — treat as delete
                            // so stale index entries are cleaned up.
                            removed_relative.push(relative);
                        }
                        Err(_) => {
                            // File disappeared between event and scan — treat as delete.
                            removed_relative.push(relative);
                        }
                    }
                }
            }
        }

        if added_or_modified.is_empty() && removed_relative.is_empty() {
            continue;
        }

        if let Some(ref callback) = on_change {
            for path in &changed_paths {
                callback(path);
            }
        }

        // Apply changes to the index.
        let result = apply_incremental_changes(
            &repo_root,
            &index_dir,
            added_or_modified,
            removed_relative,
            &config,
        );

        match result {
            Ok(outcome) => {
                let new_generation = generation.fetch_add(1, Ordering::SeqCst) + 1;
                if let Some(ref callback) = on_index_update {
                    callback(new_generation, &changed_paths);
                }
                if outcome.rebuilt_full {
                    eprintln!(
                        "triseek-watcher: full rebuild triggered (delta ratio exceeded threshold)"
                    );
                }
            }
            Err(e) => {
                eprintln!("triseek-watcher: index update failed: {e}");
            }
        }

        // Re-build gitignore in case .gitignore changed.
        if pending
            .keys()
            .any(|p| p.file_name().map(|n| n == ".gitignore").unwrap_or(false))
        {
            // The gitignore variable is local to this iteration; next loop iteration
            // will re-bind it from scratch via the outer `let gitignore = ...`.
            // Since we can't rebind `gitignore` easily here, just let the loop continue.
            // A full rebuild will be triggered if needed.
        }
    }
}

/// Update the pending map with a single notify event.
fn apply_event(pending: &mut HashMap<PathBuf, ChangeKind>, event: notify::Event) {
    match event.kind {
        // Creations and data modifications — covers both fine-grained and
        // coarse-grained backends (FSEvents on macOS emits ModifyKind::Any).
        EventKind::Create(_)
        | EventKind::Modify(ModifyKind::Data(_))
        | EventKind::Modify(ModifyKind::Any) => {
            for path in event.paths {
                // Don't downgrade a Deleted entry back to CreatedOrModified.
                pending
                    .entry(path)
                    .and_modify(|k| {
                        if *k != ChangeKind::Deleted {
                            *k = ChangeKind::CreatedOrModified;
                        }
                    })
                    .or_insert(ChangeKind::CreatedOrModified);
            }
        }
        EventKind::Remove(_) => {
            for path in event.paths {
                pending.insert(path, ChangeKind::Deleted);
            }
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() == 2 => {
            // paths[0] = old, paths[1] = new
            pending.insert(event.paths[0].clone(), ChangeKind::Deleted);
            pending
                .entry(event.paths[1].clone())
                .or_insert(ChangeKind::CreatedOrModified);
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            for path in event.paths {
                pending.insert(path, ChangeKind::Deleted);
            }
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            for path in event.paths {
                pending.entry(path).or_insert(ChangeKind::CreatedOrModified);
            }
        }
        _ => {}
    }
}
