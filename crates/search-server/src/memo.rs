use search_core::{
    MemoCheckParams, MemoCheckRecommendation, MemoCheckResponse, MemoEventKind, MemoFileStatus,
    MemoFileStatusKind, MemoFileSummary, MemoObserveParams, MemoObserveResponse,
    MemoSessionLifecycleResponse, MemoSessionParams, MemoSessionResponse, MemoStatusParams,
    MemoStatusResponse,
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
    last_read_at: Instant,
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
                session.files.clear();
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
                            file.last_read_at = Instant::now();
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
                                last_read_at: Instant::now(),
                            });
                            file.content_hash = observed_hash;
                            file.disk_hash = observed_hash;
                            file.tokens = token_value;
                            file.read_count += 1;
                            file.stale = false;
                            file.last_read_at = Instant::now();
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
                let current_tokens = if file.stale {
                    std::fs::metadata(&absolute)
                        .map(|m| estimate_tokens(m.len()))
                        .ok()
                } else {
                    None
                };
                let (status, message) = if file.stale {
                    let size_hint = current_tokens
                        .map(|ct| format!(" (now ~{ct} tokens)"))
                        .unwrap_or_default();
                    (
                        MemoFileStatusKind::Stale,
                        format!("Changed since last read{size_hint}; re-read file."),
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
                    current_tokens,
                });
            } else {
                results.push(MemoFileStatus {
                    path: path.clone(),
                    status: MemoFileStatusKind::Unknown,
                    tokens: None,
                    read_count: None,
                    message: "File not observed in this session.".to_string(),
                    current_tokens: None,
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

    pub fn check(&self, params: &MemoCheckParams) -> MemoCheckResponse {
        let repo_root = Path::new(&params.repo_root);
        let mut sessions = self.sessions.lock().unwrap();
        prune_idle_sessions(&mut sessions, self.session_idle_timeout);
        let session = sessions
            .entry(params.session_id.clone())
            .or_insert_with(SessionState::new);
        session.touch();

        let absolute = normalize_path(repo_root, &params.path);

        if let Some(file) = session.files.get_mut(&absolute) {
            // Refresh stale flag from disk (same logic as status()).
            let disk_hash = read_disk_hash(&absolute).unwrap_or(0);
            if disk_hash == 0 {
                file.stale = true;
            } else {
                file.disk_hash = disk_hash;
                file.stale = file.disk_hash != file.content_hash;
            }

            let current_tokens = if file.stale {
                std::fs::metadata(&absolute)
                    .map(|m| estimate_tokens(m.len()))
                    .ok()
            } else {
                None
            };

            let status = if file.stale {
                MemoFileStatusKind::Stale
            } else {
                MemoFileStatusKind::Fresh
            };

            let recommendation = if file.stale {
                match (current_tokens, file.tokens) {
                    (Some(ct), prev) if prev > 0 => {
                        let ratio = (ct as f64 - prev as f64).abs() / prev as f64;
                        if ratio < 0.10 {
                            MemoCheckRecommendation::RereadWithDiff
                        } else {
                            MemoCheckRecommendation::Reread
                        }
                    }
                    _ => MemoCheckRecommendation::Reread,
                }
            } else {
                MemoCheckRecommendation::SkipReread
            };

            MemoCheckResponse {
                path: params.path.clone(),
                status,
                recommendation,
                tokens_at_last_read: Some(file.tokens),
                current_tokens,
                last_read_ago_seconds: Some(file.last_read_at.elapsed().as_secs()),
            }
        } else {
            MemoCheckResponse {
                path: params.path.clone(),
                status: MemoFileStatusKind::Unknown,
                recommendation: MemoCheckRecommendation::Reread,
                tokens_at_last_read: None,
                current_tokens: None,
                last_read_ago_seconds: None,
            }
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

/// Estimate token count from byte length. Matches the formula in `memo_shim.rs`.
fn estimate_tokens(bytes: u64) -> u32 {
    let est = (bytes as f64 / 3.5).ceil();
    if est > u32::MAX as f64 {
        u32::MAX
    } else {
        est as u32
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
    use search_core::{
        MemoCheckParams, MemoCheckRecommendation, MemoEventKind, MemoObserveParams,
        MemoSessionParams, MemoStatusParams,
    };
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
        let memo = MemoState::new(Duration::from_millis(50));

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

        std::thread::sleep(Duration::from_millis(75));
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

    #[test]
    fn pre_compact_invalidates_session_file_map() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        let file = repo.join("lib.rs");
        fs::write(&file, "pub fn hello() {}\n").unwrap();
        let hash = read_disk_hash(&file).unwrap();
        let memo = MemoState::new(Duration::from_secs(600));
        let repo_str = repo.display().to_string();
        let session_id = "compact-session".to_string();

        memo.observe(&MemoObserveParams {
            session_id: session_id.clone(),
            repo_root: repo_str.clone(),
            event: MemoEventKind::Read,
            path: Some("lib.rs".to_string()),
            content_hash: Some(hash),
            tokens: Some(5),
        });

        let pre = memo.check(&MemoCheckParams {
            session_id: session_id.clone(),
            repo_root: repo_str.clone(),
            path: "lib.rs".to_string(),
        });
        assert!(matches!(pre.status, MemoFileStatusKind::Fresh));
        assert!(matches!(
            pre.recommendation,
            MemoCheckRecommendation::SkipReread
        ));

        memo.observe(&MemoObserveParams {
            session_id: session_id.clone(),
            repo_root: repo_str.clone(),
            event: MemoEventKind::PreCompact,
            path: None,
            content_hash: None,
            tokens: None,
        });

        let post = memo.check(&MemoCheckParams {
            session_id: session_id.clone(),
            repo_root: repo_str.clone(),
            path: "lib.rs".to_string(),
        });
        assert!(
            matches!(post.status, MemoFileStatusKind::Unknown),
            "after PreCompact, memo must not claim the file is Fresh"
        );
        assert!(
            matches!(post.recommendation, MemoCheckRecommendation::Reread),
            "after PreCompact, recommendation must be Reread"
        );

        let session = memo.session(&MemoSessionParams {
            session_id: session_id.clone(),
            repo_root: None,
        });
        assert_eq!(session.compaction_count, 1);
        assert_eq!(session.tracked_files, 0);
        assert_eq!(session.total_reads, 1);
        assert_eq!(session.redundant_reads_prevented, 0);
        assert_eq!(session.tokens_saved, 0);

        memo.observe(&MemoObserveParams {
            session_id: session_id.clone(),
            repo_root: repo_str.clone(),
            event: MemoEventKind::Read,
            path: Some("lib.rs".to_string()),
            content_hash: Some(hash),
            tokens: Some(5),
        });

        let relearned = memo.check(&MemoCheckParams {
            session_id,
            repo_root: repo_str,
            path: "lib.rs".to_string(),
        });
        assert!(matches!(relearned.status, MemoFileStatusKind::Fresh));
        assert!(matches!(
            relearned.recommendation,
            MemoCheckRecommendation::SkipReread
        ));
    }

    #[test]
    fn pre_compact_only_invalidates_observing_session() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        let file = repo.join("shared.rs");
        fs::write(&file, "pub fn shared() {}\n").unwrap();
        let hash = read_disk_hash(&file).unwrap();
        let memo = MemoState::new(Duration::from_secs(600));
        let repo_str = repo.display().to_string();

        for session_id in ["session-a", "session-b"] {
            memo.observe(&MemoObserveParams {
                session_id: session_id.to_string(),
                repo_root: repo_str.clone(),
                event: MemoEventKind::Read,
                path: Some("shared.rs".to_string()),
                content_hash: Some(hash),
                tokens: Some(6),
            });
        }

        memo.observe(&MemoObserveParams {
            session_id: "session-a".to_string(),
            repo_root: repo_str.clone(),
            event: MemoEventKind::PreCompact,
            path: None,
            content_hash: None,
            tokens: None,
        });

        let session_a = memo.check(&MemoCheckParams {
            session_id: "session-a".to_string(),
            repo_root: repo_str.clone(),
            path: "shared.rs".to_string(),
        });
        assert!(matches!(session_a.status, MemoFileStatusKind::Unknown));
        assert!(matches!(
            session_a.recommendation,
            MemoCheckRecommendation::Reread
        ));

        let session_b = memo.check(&MemoCheckParams {
            session_id: "session-b".to_string(),
            repo_root: repo_str,
            path: "shared.rs".to_string(),
        });
        assert!(matches!(session_b.status, MemoFileStatusKind::Fresh));
        assert!(matches!(
            session_b.recommendation,
            MemoCheckRecommendation::SkipReread
        ));
    }

    // Replay a synthetic multi-file, multi-edit trace through MemoState and verify
    // redundant read detection, tokens_saved accounting, and zero false negatives.
    #[test]
    fn replay_synthetic_trace_through_memo_state() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        let memo = MemoState::new(Duration::from_secs(600));
        let session = "replay-session";
        let repo_str = repo.display().to_string();

        // Create 3 files
        let paths = ["src/a.rs", "src/b.rs", "src/c.rs"];
        let contents = [
            "pub fn a() -> u32 { 1 }\n",
            "pub fn b() -> u32 { 2 }\n",
            "pub fn c() -> u32 { 3 }\n",
        ];
        for (path, content) in paths.iter().zip(contents.iter()) {
            let abs = repo.join(path);
            fs::create_dir_all(abs.parent().unwrap()).unwrap();
            fs::write(&abs, content).unwrap();
        }

        // --- Step 1: First reads of all 3 files (no redundancy yet) ---
        for (path, content) in paths.iter().zip(contents.iter()) {
            let abs = repo.join(path);
            let hash = read_disk_hash(&abs).unwrap();
            let tokens = (content.len() as f64 / 3.5).ceil() as u32;

            // status should be Unknown before first observe
            let st = memo.status(&MemoStatusParams {
                session_id: session.to_string(),
                repo_root: repo_str.clone(),
                files: vec![path.to_string()],
            });
            assert!(
                matches!(st.results[0].status, MemoFileStatusKind::Unknown),
                "Expected Unknown before first read of {}",
                path
            );

            memo.observe(&MemoObserveParams {
                session_id: session.to_string(),
                repo_root: repo_str.clone(),
                event: MemoEventKind::Read,
                path: Some(path.to_string()),
                content_hash: Some(hash),
                tokens: Some(tokens),
            });
        }

        // After first reads: all should be Fresh, session totals correct
        let sess = memo.session(&MemoSessionParams {
            session_id: session.to_string(),
            repo_root: None,
        });
        assert_eq!(sess.total_reads, 3);
        assert_eq!(sess.redundant_reads_prevented, 0);
        assert_eq!(sess.tokens_saved, 0);

        // --- Step 2: Re-read all 3 unchanged files (all should be Fresh / redundant) ---
        for (path, content) in paths.iter().zip(contents.iter()) {
            let abs = repo.join(path);
            let hash = read_disk_hash(&abs).unwrap();
            let tokens = (content.len() as f64 / 3.5).ceil() as u32;

            // status must be Fresh (no false negatives)
            let st = memo.status(&MemoStatusParams {
                session_id: session.to_string(),
                repo_root: repo_str.clone(),
                files: vec![path.to_string()],
            });
            assert!(
                matches!(st.results[0].status, MemoFileStatusKind::Fresh),
                "False negative: expected Fresh for unchanged {} but got Stale/Unknown",
                path
            );

            memo.observe(&MemoObserveParams {
                session_id: session.to_string(),
                repo_root: repo_str.clone(),
                event: MemoEventKind::Read,
                path: Some(path.to_string()),
                content_hash: Some(hash),
                tokens: Some(tokens),
            });
        }

        let sess = memo.session(&MemoSessionParams {
            session_id: session.to_string(),
            repo_root: None,
        });
        assert_eq!(sess.total_reads, 6);
        assert_eq!(sess.redundant_reads_prevented, 3);
        assert!(
            sess.tokens_saved > 0,
            "tokens_saved should be non-zero after redundant reads"
        );

        // --- Step 3: Edit src/a.rs (Edit observe), then verify a.rs is Stale, b.rs still Fresh ---
        let new_content_a = "pub fn a() -> u32 { 42 }\n";
        fs::write(repo.join("src/a.rs"), new_content_a).unwrap();
        memo.observe(&MemoObserveParams {
            session_id: session.to_string(),
            repo_root: repo_str.clone(),
            event: MemoEventKind::Edit,
            path: Some("src/a.rs".to_string()),
            content_hash: None,
            tokens: None,
        });

        let st_a = memo.status(&MemoStatusParams {
            session_id: session.to_string(),
            repo_root: repo_str.clone(),
            files: vec!["src/a.rs".to_string()],
        });
        assert!(
            matches!(st_a.results[0].status, MemoFileStatusKind::Stale),
            "src/a.rs should be Stale after edit"
        );

        let st_b = memo.status(&MemoStatusParams {
            session_id: session.to_string(),
            repo_root: repo_str.clone(),
            files: vec!["src/b.rs".to_string()],
        });
        assert!(
            matches!(st_b.results[0].status, MemoFileStatusKind::Fresh),
            "src/b.rs should still be Fresh after unrelated edit"
        );

        // --- Step 4: Re-read src/a.rs with new content — clears stale ---
        let new_hash_a = read_disk_hash(&repo.join("src/a.rs")).unwrap();
        let new_tokens_a = (new_content_a.len() as f64 / 3.5).ceil() as u32;
        memo.observe(&MemoObserveParams {
            session_id: session.to_string(),
            repo_root: repo_str.clone(),
            event: MemoEventKind::Read,
            path: Some("src/a.rs".to_string()),
            content_hash: Some(new_hash_a),
            tokens: Some(new_tokens_a),
        });

        let st_a_after = memo.status(&MemoStatusParams {
            session_id: session.to_string(),
            repo_root: repo_str.clone(),
            files: vec!["src/a.rs".to_string()],
        });
        assert!(
            matches!(st_a_after.results[0].status, MemoFileStatusKind::Fresh),
            "src/a.rs should be Fresh after re-read with new content"
        );

        // Final session check: redundant_reads_prevented must not have increased for the stale re-read
        let final_sess = memo.session(&MemoSessionParams {
            session_id: session.to_string(),
            repo_root: None,
        });
        assert_eq!(
            final_sess.redundant_reads_prevented, 3,
            "The stale re-read of a.rs should not count as a prevented redundant read"
        );
    }

    // Two sessions observe the same files. Session A edits via mark_stale_for_all.
    // Both sessions must see Stale. Sequential vs concurrent results must match.
    #[test]
    fn parallel_session_isolation_comprehensive() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        let file = repo.join("shared.rs");
        fs::write(&file, "fn shared() {}\n").unwrap();
        let unrelated = repo.join("other.rs");
        fs::write(&unrelated, "fn other() {}\n").unwrap();

        let memo = MemoState::new(Duration::from_secs(600));
        let repo_str = repo.display().to_string();

        // Both sessions read shared.rs
        let hash = read_disk_hash(&file).unwrap();
        for sid in ["session-x", "session-y"] {
            memo.observe(&MemoObserveParams {
                session_id: sid.to_string(),
                repo_root: repo_str.clone(),
                event: MemoEventKind::Read,
                path: Some("shared.rs".to_string()),
                content_hash: Some(hash),
                tokens: Some(10),
            });
        }

        // session-x also reads other.rs
        let other_hash = read_disk_hash(&unrelated).unwrap();
        memo.observe(&MemoObserveParams {
            session_id: "session-x".to_string(),
            repo_root: repo_str.clone(),
            event: MemoEventKind::Read,
            path: Some("other.rs".to_string()),
            content_hash: Some(other_hash),
            tokens: Some(8),
        });

        // Verify isolation: session-y does not see other.rs
        let st_y_other = memo.status(&MemoStatusParams {
            session_id: "session-y".to_string(),
            repo_root: repo_str.clone(),
            files: vec!["other.rs".to_string()],
        });
        assert!(
            matches!(st_y_other.results[0].status, MemoFileStatusKind::Unknown),
            "session-y should not know about other.rs read by session-x"
        );

        // External edit — mark_stale_for_all fires (simulates watcher)
        fs::write(&file, "fn shared_v2() {}\n").unwrap();
        memo.mark_stale_for_all(&file);

        // Both sessions must see shared.rs as Stale
        for sid in ["session-x", "session-y"] {
            let st = memo.status(&MemoStatusParams {
                session_id: sid.to_string(),
                repo_root: repo_str.clone(),
                files: vec!["shared.rs".to_string()],
            });
            assert!(
                matches!(st.results[0].status, MemoFileStatusKind::Stale),
                "{} should see shared.rs as Stale after external edit",
                sid
            );
        }

        // session-x's other.rs must remain Fresh (unrelated to the edit)
        let st_x_other = memo.status(&MemoStatusParams {
            session_id: "session-x".to_string(),
            repo_root: repo_str.clone(),
            files: vec!["other.rs".to_string()],
        });
        assert!(
            matches!(st_x_other.results[0].status, MemoFileStatusKind::Fresh),
            "other.rs should remain Fresh in session-x after unrelated edit"
        );

        // session-x re-reads shared.rs (clears stale), session-y does not
        let new_hash = read_disk_hash(&file).unwrap();
        memo.observe(&MemoObserveParams {
            session_id: "session-x".to_string(),
            repo_root: repo_str.clone(),
            event: MemoEventKind::Read,
            path: Some("shared.rs".to_string()),
            content_hash: Some(new_hash),
            tokens: Some(10),
        });

        // session-x: Fresh (just re-read), session-y: still Stale
        let st_x = memo.status(&MemoStatusParams {
            session_id: "session-x".to_string(),
            repo_root: repo_str.clone(),
            files: vec!["shared.rs".to_string()],
        });
        assert!(
            matches!(st_x.results[0].status, MemoFileStatusKind::Fresh),
            "session-x should see Fresh after re-reading updated shared.rs"
        );
        let st_y = memo.status(&MemoStatusParams {
            session_id: "session-y".to_string(),
            repo_root: repo_str.clone(),
            files: vec!["shared.rs".to_string()],
        });
        assert!(
            matches!(st_y.results[0].status, MemoFileStatusKind::Stale),
            "session-y should still see Stale (hasn't re-read yet)"
        );

        // session-x's redundant reads: only the initial re-reads of unchanged files count
        // shared.rs was read once fresh, then stale re-read (not redundant), so redundant=0
        let sess_x = memo.session(&MemoSessionParams {
            session_id: "session-x".to_string(),
            repo_root: None,
        });
        assert_eq!(
            sess_x.redundant_reads_prevented, 0,
            "session-x: no redundant reads (stale re-read doesn't count)"
        );
    }

    #[test]
    fn status_includes_current_tokens_when_stale() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        let file = repo.join("grow.rs");
        fs::write(&file, "fn a() {}\n").unwrap();
        let hash = read_disk_hash(&file).unwrap();
        let tokens = estimate_tokens(fs::metadata(&file).unwrap().len());
        let memo = MemoState::new(Duration::from_secs(600));
        let repo_str = repo.display().to_string();

        memo.observe(&MemoObserveParams {
            session_id: "s".to_string(),
            repo_root: repo_str.clone(),
            event: MemoEventKind::Read,
            path: Some("grow.rs".to_string()),
            content_hash: Some(hash),
            tokens: Some(tokens),
        });

        // Grow the file on disk
        fs::write(&file, "fn a() {}\nfn b() {}\nfn c() {}\n".repeat(10)).unwrap();

        let status = memo.status(&MemoStatusParams {
            session_id: "s".to_string(),
            repo_root: repo_str.clone(),
            files: vec!["grow.rs".to_string()],
        });
        let result = &status.results[0];
        assert!(matches!(result.status, MemoFileStatusKind::Stale));
        let ct = result
            .current_tokens
            .expect("current_tokens should be Some when stale");
        assert!(
            ct > tokens,
            "current_tokens ({ct}) should exceed original tokens ({tokens})"
        );
    }

    #[test]
    fn status_omits_current_tokens_when_fresh() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        let file = repo.join("stable.rs");
        fs::write(&file, "fn stable() {}\n").unwrap();
        let hash = read_disk_hash(&file).unwrap();
        let memo = MemoState::new(Duration::from_secs(600));
        let repo_str = repo.display().to_string();

        memo.observe(&MemoObserveParams {
            session_id: "s".to_string(),
            repo_root: repo_str.clone(),
            event: MemoEventKind::Read,
            path: Some("stable.rs".to_string()),
            content_hash: Some(hash),
            tokens: Some(5),
        });

        let status = memo.status(&MemoStatusParams {
            session_id: "s".to_string(),
            repo_root: repo_str.clone(),
            files: vec!["stable.rs".to_string()],
        });
        let result = &status.results[0];
        assert!(matches!(result.status, MemoFileStatusKind::Fresh));
        assert!(
            result.current_tokens.is_none(),
            "current_tokens should be None for fresh files"
        );
    }

    #[test]
    fn check_returns_skip_reread_for_fresh_file() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        let file = repo.join("check.rs");
        fs::write(&file, "fn check() {}\n").unwrap();
        let hash = read_disk_hash(&file).unwrap();
        let memo = MemoState::new(Duration::from_secs(600));
        let repo_str = repo.display().to_string();

        memo.observe(&MemoObserveParams {
            session_id: "s".to_string(),
            repo_root: repo_str.clone(),
            event: MemoEventKind::Read,
            path: Some("check.rs".to_string()),
            content_hash: Some(hash),
            tokens: Some(10),
        });

        let resp = memo.check(&MemoCheckParams {
            session_id: "s".to_string(),
            repo_root: repo_str.clone(),
            path: "check.rs".to_string(),
        });
        assert!(matches!(resp.status, MemoFileStatusKind::Fresh));
        assert!(matches!(
            resp.recommendation,
            MemoCheckRecommendation::SkipReread
        ));
        assert_eq!(resp.tokens_at_last_read, Some(10));
        assert!(resp.current_tokens.is_none());
    }

    #[test]
    fn check_returns_reread_with_diff_for_small_delta() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        let file = repo.join("small_delta.rs");
        // Write ~350 bytes so estimate_tokens gives ~100 tokens
        let base = "fn placeholder() {}\n".repeat(17);
        fs::write(&file, &base).unwrap();
        let hash = read_disk_hash(&file).unwrap();
        let base_tokens = estimate_tokens(base.len() as u64);
        let memo = MemoState::new(Duration::from_secs(600));
        let repo_str = repo.display().to_string();

        memo.observe(&MemoObserveParams {
            session_id: "s".to_string(),
            repo_root: repo_str.clone(),
            event: MemoEventKind::Read,
            path: Some("small_delta.rs".to_string()),
            content_hash: Some(hash),
            tokens: Some(base_tokens),
        });

        // Append 2 lines (~5% growth — under 10% threshold)
        let grown = format!("{base}// added line 1\n// added line 2\n");
        fs::write(&file, &grown).unwrap();

        let resp = memo.check(&MemoCheckParams {
            session_id: "s".to_string(),
            repo_root: repo_str.clone(),
            path: "small_delta.rs".to_string(),
        });
        assert!(matches!(resp.status, MemoFileStatusKind::Stale));
        assert!(
            matches!(resp.recommendation, MemoCheckRecommendation::RereadWithDiff),
            "small delta should produce reread_with_diff, got {:?}",
            resp.recommendation
        );
    }

    #[test]
    fn check_returns_reread_for_exact_ten_percent_delta() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        let file = repo.join("exact_delta.rs");
        let base = "x".repeat(350);
        fs::write(&file, &base).unwrap();
        let hash = read_disk_hash(&file).unwrap();
        let base_tokens = estimate_tokens(base.len() as u64);
        assert_eq!(
            base_tokens, 100,
            "base length should estimate to 100 tokens"
        );

        let memo = MemoState::new(Duration::from_secs(600));
        let repo_str = repo.display().to_string();

        memo.observe(&MemoObserveParams {
            session_id: "s".to_string(),
            repo_root: repo_str.clone(),
            event: MemoEventKind::Read,
            path: Some("exact_delta.rs".to_string()),
            content_hash: Some(hash),
            tokens: Some(base_tokens),
        });

        let grown = "y".repeat(385);
        fs::write(&file, &grown).unwrap();
        let resp = memo.check(&MemoCheckParams {
            session_id: "s".to_string(),
            repo_root: repo_str,
            path: "exact_delta.rs".to_string(),
        });
        assert!(matches!(resp.status, MemoFileStatusKind::Stale));
        assert_eq!(resp.current_tokens, Some(110));
        assert!(
            matches!(resp.recommendation, MemoCheckRecommendation::Reread),
            "exact 10% delta should use reread, got {:?}",
            resp.recommendation
        );
    }

    #[test]
    fn check_returns_reread_for_unknown_file() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        let memo = MemoState::new(Duration::from_secs(600));

        let resp = memo.check(&MemoCheckParams {
            session_id: "s".to_string(),
            repo_root: repo.display().to_string(),
            path: "never_read.rs".to_string(),
        });
        assert!(matches!(resp.status, MemoFileStatusKind::Unknown));
        assert!(matches!(
            resp.recommendation,
            MemoCheckRecommendation::Reread
        ));
        assert!(resp.tokens_at_last_read.is_none());
    }
}
