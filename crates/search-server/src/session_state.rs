use anyhow::{Context, Result, bail};
use search_core::{
    ActionKind, ActionLogEntry, PORTABILITY_SCHEMA_VERSION, PortabilitySessionStatus,
    SessionStateRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Serialize, Deserialize)]
struct SessionIndex {
    schema_version: u32,
    sessions: Vec<SessionStateRecord>,
    next_entry_id: u64,
}

pub struct SessionStore {
    root: PathBuf,
    sessions: Mutex<HashMap<String, SessionStateRecord>>,
    action_log: Mutex<Vec<ActionLogEntry>>,
    next_entry_id: AtomicU64,
}

impl SessionStore {
    pub fn load_from_disk(daemon_dir: &Path) -> Result<Self> {
        let root = daemon_dir.join("sessions");
        fs::create_dir_all(&root).with_context(|| format!("create {}", root.display()))?;
        let index_path = root.join("sessions.json");
        let mut sessions = HashMap::new();
        let mut action_log = Vec::new();
        let mut next_entry_id = 1;

        if index_path.exists() {
            let index: SessionIndex = serde_json::from_slice(
                &fs::read(&index_path)
                    .with_context(|| format!("read session index {}", index_path.display()))?,
            )
            .with_context(|| format!("parse session index {}", index_path.display()))?;
            if index.schema_version != PORTABILITY_SCHEMA_VERSION {
                bail!(
                    "unsupported session index schema version {}",
                    index.schema_version
                );
            }
            next_entry_id = index.next_entry_id.max(1);
            for session in index.sessions {
                let session_id = session.session_id.clone();
                action_log.extend(read_action_log(&root, &session_id)?);
                sessions.insert(session_id, session);
            }
        }

        Ok(Self {
            root,
            sessions: Mutex::new(sessions),
            action_log: Mutex::new(action_log),
            next_entry_id: AtomicU64::new(next_entry_id),
        })
    }

    pub fn open_session(
        &self,
        session_id: Option<String>,
        goal: String,
        repo_root: String,
    ) -> Result<SessionStateRecord> {
        let now = now_secs();
        let session_id = session_id.unwrap_or_else(|| format!("session_{}", now_millis()));
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .entry(session_id.clone())
            .and_modify(|session| {
                if !goal.trim().is_empty() {
                    session.goal = goal.clone();
                }
                session.status = PortabilitySessionStatus::Open;
                session.updated_at = now;
            })
            .or_insert_with(|| SessionStateRecord {
                schema_version: PORTABILITY_SCHEMA_VERSION,
                session_id,
                goal,
                repo_root,
                status: PortabilitySessionStatus::Open,
                created_at: now,
                updated_at: now,
            })
            .clone();
        drop(sessions);
        self.flush_to_disk()?;
        Ok(session)
    }

    pub fn close_session(
        &self,
        session_id: &str,
        status: PortabilitySessionStatus,
    ) -> Result<SessionStateRecord> {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(session) = sessions.get_mut(session_id) else {
            bail!("unknown session_id `{session_id}`");
        };
        session.status = status;
        session.updated_at = now_secs();
        let session = session.clone();
        drop(sessions);
        self.flush_to_disk()?;
        Ok(session)
    }

    pub fn session(&self, session_id: &str) -> Result<SessionStateRecord> {
        self.sessions
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .with_context(|| format!("unknown session_id `{session_id}`"))
    }

    pub fn list_sessions(&self, repo_root: Option<&str>) -> Vec<SessionStateRecord> {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|session| repo_root.is_none_or(|root| session.repo_root == root))
            .cloned()
            .collect::<Vec<_>>();
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sessions
    }

    pub fn record_action(
        &self,
        session_id: &str,
        kind: ActionKind,
        payload: Value,
    ) -> Result<ActionLogEntry> {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(session) = sessions.get_mut(session_id) else {
            bail!("unknown session_id `{session_id}`");
        };
        session.updated_at = now_secs();
        drop(sessions);

        let entry = ActionLogEntry {
            schema_version: PORTABILITY_SCHEMA_VERSION,
            entry_id: self.next_entry_id.fetch_add(1, Ordering::SeqCst),
            session_id: session_id.to_string(),
            ts: now_secs(),
            kind,
            payload,
        };
        self.action_log.lock().unwrap().push(entry.clone());
        self.append_action(&entry)?;
        Ok(entry)
    }

    pub fn entries_for_session(&self, session_id: &str) -> Vec<ActionLogEntry> {
        self.action_log
            .lock()
            .unwrap()
            .iter()
            .filter(|entry| entry.session_id == session_id)
            .cloned()
            .collect()
    }

    pub fn flush_to_disk(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("create {}", self.root.display()))?;
        let mut sessions = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        sessions.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        let index = SessionIndex {
            schema_version: PORTABILITY_SCHEMA_VERSION,
            sessions,
            next_entry_id: self.next_entry_id.load(Ordering::SeqCst),
        };
        let tmp = self.root.join("sessions.json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&index)?)
            .with_context(|| format!("write {}", tmp.display()))?;
        fs::rename(&tmp, self.root.join("sessions.json")).context("replace session index")?;
        Ok(())
    }

    fn append_action(&self, entry: &ActionLogEntry) -> Result<()> {
        let dir = self.root.join(&entry.session_id);
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let path = dir.join("action_log.jsonl");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open {}", path.display()))?;
        writeln!(file, "{}", serde_json::to_string(entry)?)?;
        Ok(())
    }
}

fn read_action_log(root: &Path, session_id: &str) -> Result<Vec<ActionLogEntry>> {
    let path = root.join(session_id).join("action_log.jsonl");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut entries = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: ActionLogEntry = serde_json::from_str(line)
            .with_context(|| format!("parse {} line {}", path.display(), idx + 1))?;
        if entry.schema_version != PORTABILITY_SCHEMA_VERSION {
            bail!(
                "unsupported action log schema version {}",
                entry.schema_version
            );
        }
        entries.push(entry);
    }
    Ok(entries)
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
