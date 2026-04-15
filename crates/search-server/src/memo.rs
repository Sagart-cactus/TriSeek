use search_core::{
    MemoEventKind, MemoFileStatus, MemoFileStatusKind, MemoFileSummary, MemoObserveParams,
    MemoObserveResponse, MemoSessionLifecycleResponse, MemoSessionParams, MemoSessionResponse,
    MemoStatusParams, MemoStatusResponse,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use xxhash_rust::xxh3::xxh3_64;

#[derive(Debug, Clone)]
struct FileState {
    content_hash: u64,
    disk_hash: u64,
    tokens: u32,
    read_count: u32,
    redundant_tokens: u64,
    stale: bool,
}

#[derive(Debug)]
struct SessionState {
    files: HashMap<PathBuf, FileState>,
    total_reads: u64,
    redundant_reads_prevented: u64,
    tokens_saved: u64,
    compaction_count: u32,
    last_activity: Instant,
}

impl SessionState {
    fn new() -> Self {
        Self {
            files: HashMap::new(),
            total_reads: 0,
            redundant_reads_prevented: 0,
            tokens_saved: 0,
            compaction_count: 0,
            last_activity: Instant::now(),
        }
    }

    fn touch(&mut self) {
        self.last_activity = Instant::now();
    }
}

pub struct MemoState {
    sessions: Mutex<HashMap<String, SessionState>>,
    session_idle_timeout: Duration,
}

impl MemoState {
    pub fn new(session_idle_timeout: Duration) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            session_idle_timeout,
        }
    }

    pub fn session_start(&self, params: &MemoSessionParams) -> MemoSessionLifecycleResponse {
        let mut sessions = self.sessions.lock().unwrap();
        prune_idle_sessions(&mut sessions, self.session_idle_timeout);
        sessions
            .entry(params.session_id.clone())
            .or_insert_with(SessionState::new)
            .touch();
        MemoSessionLifecycleResponse { ok: true }
    }

    pub fn session_end(&self, params: &MemoSessionParams) -> MemoSessionLifecycleResponse {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.remove(&params.session_id);
        MemoSessionLifecycleResponse { ok: true }
    }

    pub fn observe(&self, params: &MemoObserveParams) -> MemoObserveResponse {
        let mut sessions = self.sessions.lock().unwrap();
        prune_idle_sessions(&mut sessions, self.session_idle_timeout);
        let session = sessions
            .entry(params.session_id.clone())
            .or_insert_with(SessionState::new);
        session.touch();
        match params.event {
            MemoEventKind::SessionStart => {}
            MemoEventKind::SessionEnd => {
                sessions.remove(&params.session_id);
            }
            MemoEventKind::PreCompact => {
                session.compaction_count += 1;
            }
            MemoEventKind::Edit => {
                if let Some(ref path) = params.path {
                    let repo_root = Path::new(&params.repo_root);
                    let absolute = normalize_path(repo_root, path);
                    if let Some(file) = session.files.get_mut(&absolute) {
                        file.stale = true;
                    }
                }
            }
            MemoEventKind::Read => {
                if let Some(ref path) = params.path {
                    let repo_root = Path::new(&params.repo_root);
                    let absolute = normalize_path(repo_root, path);
                    let observed_hash = params
                        .content_hash
                        .or_else(|| read_disk_hash(&absolute))
                        .unwrap_or(0);
                    let observed_tokens = params.tokens.unwrap_or(0);
                    let existing = session.files.get(&absolute).cloned();
                    match existing {
                        Some(previous)
                            if !previous.stale && previous.content_hash == observed_hash =>
                        {
                            let file = session.files.get_mut(&absolute).unwrap();
                            file.read_count += 1;
                            if observed_tokens > 0 {
                                file.tokens = observed_tokens;
                            }
                            let saved = u64::from(file.tokens.max(observed_tokens));
                            file.redundant_tokens += saved;
                            session.redundant_reads_prevented += 1;
                            session.tokens_saved += saved;
                            session.total_reads += 1;
                        }
                        _ => {
                            let token_value = observed_tokens.max(existing.map_or(0, |f| f.tokens));
                            let file = session.files.entry(absolute).or_insert(FileState {
                                content_hash: observed_hash,
                                disk_hash: observed_hash,
                                tokens: token_value,
                                read_count: 0,
                                redundant_tokens: 0,
                                stale: false,
                            });
                            file.content_hash = observed_hash;
                            file.disk_hash = observed_hash;
                            file.tokens = token_value;
                            file.read_count += 1;
                            file.stale = false;
                            session.total_reads += 1;
                        }
                    }
                }
            }
        }
        MemoObserveResponse { observed: true }
    }

    pub fn status(&self, params: &MemoStatusParams) -> MemoStatusResponse {
        let repo_root = Path::new(&params.repo_root);
        let mut sessions = self.sessions.lock().unwrap();
        prune_idle_sessions(&mut sessions, self.session_idle_timeout);
        let session = sessions
            .entry(params.session_id.clone())
            .or_insert_with(SessionState::new);
        session.touch();
        let mut results = Vec::with_capacity(params.files.len());
        for path in &params.files {
            let absolute = normalize_path(repo_root, path);
            if let Some(file) = session.files.get_mut(&absolute) {
                let disk_hash = read_disk_hash(&absolute).unwrap_or(0);
                if disk_hash == 0 {
                    file.stale = true;
                } else {
                    file.disk_hash = disk_hash;
                    file.stale = file.disk_hash != file.content_hash;
                }
                let (status, message) = if file.stale {
                    (
                        MemoFileStatusKind::Stale,
                        "Changed since last read; re-read file.".to_string(),
                    )
                } else {
                    (
                        MemoFileStatusKind::Fresh,
                        format!(
                            "Unchanged since last read. Skip re-read to save {} tokens.",
                            file.tokens
                        ),
                    )
                };
                results.push(MemoFileStatus {
                    path: path.clone(),
                    status,
                    tokens: Some(file.tokens),
                    read_count: Some(file.read_count),
                    message,
                });
            } else {
                results.push(MemoFileStatus {
                    path: path.clone(),
                    status: MemoFileStatusKind::Unknown,
                    tokens: None,
                    read_count: None,
                    message: "File not observed in this session.".to_string(),
                });
            }
        }
        MemoStatusResponse {
            session_id: params.session_id.clone(),
            results,
        }
    }

    pub fn session(&self, params: &MemoSessionParams) -> MemoSessionResponse {
        let mut sessions = self.sessions.lock().unwrap();
        prune_idle_sessions(&mut sessions, self.session_idle_timeout);
        let session = sessions
            .entry(params.session_id.clone())
            .or_insert_with(SessionState::new);
        session.touch();
        let files = session
            .files
            .iter()
            .map(|(path, file)| MemoFileSummary {
                path: path.display().to_string(),
                status: if file.stale {
                    MemoFileStatusKind::Stale
                } else {
                    MemoFileStatusKind::Fresh
                },
                reads: file.read_count,
                tokens: file.tokens,
            })
            .collect::<Vec<_>>();
        MemoSessionResponse {
            session_id: params.session_id.clone(),
            tracked_files: session.files.len(),
            total_reads: session.total_reads,
            redundant_reads_prevented: session.redundant_reads_prevented,
            tokens_saved: session.tokens_saved,
            compaction_count: session.compaction_count,
            files,
        }
    }

    pub fn mark_stale_for_all(&self, changed_path: &Path) {
        let changed_path = changed_path
            .canonicalize()
            .unwrap_or_else(|_| changed_path.to_path_buf());
        let mut sessions = self.sessions.lock().unwrap();
        prune_idle_sessions(&mut sessions, self.session_idle_timeout);
        for session in sessions.values_mut() {
            if let Some(file) = session.files.get_mut(&changed_path) {
                file.stale = true;
            }
        }
    }
}

fn prune_idle_sessions(sessions: &mut HashMap<String, SessionState>, idle_timeout: Duration) {
    if idle_timeout.is_zero() {
        return;
    }
    sessions.retain(|_, state| state.last_activity.elapsed() <= idle_timeout);
}

fn normalize_path(repo_root: &Path, path: &str) -> PathBuf {
    let base = Path::new(path);
    if base.is_absolute() {
        base.canonicalize().unwrap_or_else(|_| base.to_path_buf())
    } else {
        repo_root
            .join(base)
            .canonicalize()
            .unwrap_or_else(|_| repo_root.join(base))
    }
}

fn read_disk_hash(path: &Path) -> Option<u64> {
    let bytes = fs::read(path).ok()?;
    Some(xxh3_64(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use search_core::{MemoEventKind, MemoObserveParams, MemoSessionParams, MemoStatusParams};
    use std::time::Duration;

    #[test]
    fn tracks_unknown_fresh_and_stale_transitions() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        let file = repo.join("src/lib.rs");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "pub fn alpha() {}\n").unwrap();

        let memo = MemoState::new(Duration::from_secs(600));
        let status = memo.status(&MemoStatusParams {
            session_id: "s1".to_string(),
            repo_root: repo.display().to_string(),
            files: vec!["src/lib.rs".to_string()],
        });
        assert!(matches!(
            status.results[0].status,
            MemoFileStatusKind::Unknown
        ));

        let content_hash = read_disk_hash(&file).unwrap();
        memo.observe(&MemoObserveParams {
            session_id: "s1".to_string(),
            repo_root: repo.display().to_string(),
            event: MemoEventKind::Read,
            path: Some("src/lib.rs".to_string()),
            content_hash: Some(content_hash),
            tokens: Some(42),
        });

        let status = memo.status(&MemoStatusParams {
            session_id: "s1".to_string(),
            repo_root: repo.display().to_string(),
            files: vec!["src/lib.rs".to_string()],
        });
        assert!(matches!(
            status.results[0].status,
            MemoFileStatusKind::Fresh
        ));

        fs::write(&file, "pub fn beta() {}\n").unwrap();
        let status = memo.status(&MemoStatusParams {
            session_id: "s1".to_string(),
            repo_root: repo.display().to_string(),
            files: vec!["src/lib.rs".to_string()],
        });
        assert!(matches!(
            status.results[0].status,
            MemoFileStatusKind::Stale
        ));
    }

    #[test]
    fn isolates_sessions_and_prunes_idle() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        fs::write(repo.join("x.rs"), "fn x() {}\n").unwrap();
        let hash = read_disk_hash(&repo.join("x.rs")).unwrap();
        let memo = MemoState::new(Duration::from_millis(1));

        memo.observe(&MemoObserveParams {
            session_id: "a".to_string(),
            repo_root: repo.display().to_string(),
            event: MemoEventKind::Read,
            path: Some("x.rs".to_string()),
            content_hash: Some(hash),
            tokens: Some(10),
        });
        memo.observe(&MemoObserveParams {
            session_id: "b".to_string(),
            repo_root: repo.display().to_string(),
            event: MemoEventKind::Read,
            path: Some("x.rs".to_string()),
            content_hash: Some(hash),
            tokens: Some(10),
        });
        memo.session_end(&MemoSessionParams {
            session_id: "a".to_string(),
            repo_root: None,
        });

        let session_a = memo.session(&MemoSessionParams {
            session_id: "a".to_string(),
            repo_root: None,
        });
        assert_eq!(session_a.tracked_files, 0);
        let session_b = memo.session(&MemoSessionParams {
            session_id: "b".to_string(),
            repo_root: None,
        });
        assert_eq!(session_b.tracked_files, 1);

        std::thread::sleep(Duration::from_millis(5));
        let session_c = memo.session(&MemoSessionParams {
            session_id: "c".to_string(),
            repo_root: None,
        });
        assert_eq!(session_c.tracked_files, 0);
    }

    #[test]
    fn marks_changed_path_stale_across_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        let file = repo.join("src/file.rs");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "fn first() {}\n").unwrap();
        let hash = read_disk_hash(&file).unwrap();
        let memo = MemoState::new(Duration::from_secs(600));

        for session_id in ["a", "b"] {
            memo.observe(&MemoObserveParams {
                session_id: session_id.to_string(),
                repo_root: repo.display().to_string(),
                event: MemoEventKind::Read,
                path: Some("src/file.rs".to_string()),
                content_hash: Some(hash),
                tokens: Some(12),
            });
        }

        fs::write(&file, "fn second() {}\n").unwrap();
        memo.mark_stale_for_all(&file);

        for session_id in ["a", "b"] {
            let status = memo.status(&MemoStatusParams {
                session_id: session_id.to_string(),
                repo_root: repo.display().to_string(),
                files: vec!["src/file.rs".to_string()],
            });
            assert!(matches!(
                status.results[0].status,
                MemoFileStatusKind::Stale
            ));
        }
    }
}
