use crate::git_state;
use crate::session_state::{SessionStore, now_secs};
use anyhow::{Context, Result, bail};
use search_core::{
    ActionKind, ActionLogEntry, FileReadRef, FullSnapshot, PORTABILITY_SCHEMA_VERSION,
    PinnedSnippet, PinnedSnippetSpec, SearchRef, SessionSnapshotCreateParams, SnapshotDiff,
    SnapshotManifest, WorkingSet,
};
use search_frecency::FrecencyStore;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub fn create_snapshot(
    session_store: &SessionStore,
    frecency: &FrecencyStore,
    repo_root: &Path,
    daemon_dir: &Path,
    generation: u64,
    context_epoch: u64,
    params: &SessionSnapshotCreateParams,
) -> Result<SnapshotManifest> {
    let session = session_store.session(&params.session_id)?;
    session_store.flush_to_disk()?;
    let entries = session_store.entries_for_session(&params.session_id);
    let git = git_state::capture(repo_root)?;
    let snapshot_id = format!(
        "snap_{}_{}",
        now_secs(),
        params.session_id.replace('/', "_")
    );
    let snapshots_root = daemon_dir.join("snapshots");
    fs::create_dir_all(&snapshots_root)
        .with_context(|| format!("create {}", snapshots_root.display()))?;
    let tmp_dir = snapshots_root.join(format!("{snapshot_id}.tmp"));
    let final_dir = snapshots_root.join(&snapshot_id);
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir)?;
    }
    fs::create_dir_all(tmp_dir.join("pinned_snippets"))?;

    let pinned_snippets = build_pinned_snippets(repo_root, &params.pinned_snippet_paths)?;
    for snippet in &pinned_snippets {
        fs::write(
            tmp_dir
                .join("pinned_snippets")
                .join(format!("{}.txt", snippet.sha)),
            &snippet.content,
        )?;
    }

    let working_set = build_working_set(&entries, frecency, &pinned_snippets);
    let manifest = SnapshotManifest {
        schema_version: PORTABILITY_SCHEMA_VERSION,
        snapshot_id: snapshot_id.clone(),
        session_id: params.session_id.clone(),
        created_at: now_secs(),
        repo_root: session.repo_root,
        repo_commit: git.commit.clone(),
        repo_dirty_files: git.dirty_files.clone(),
        source_harness: params.source_harness.clone(),
        source_model: params.source_model.clone(),
        generation,
        context_epoch,
    };

    fs::write(
        tmp_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    fs::write(
        tmp_dir.join("working_set.json"),
        serde_json::to_vec_pretty(&working_set)?,
    )?;
    let action_log = entries
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .join("\n");
    fs::write(tmp_dir.join("action_log.jsonl"), format!("{action_log}\n"))?;
    fs::write(
        tmp_dir.join("git_state.json"),
        serde_json::to_vec_pretty(&git)?,
    )?;
    fs::write(
        tmp_dir.join("pinned_snippets.json"),
        serde_json::to_vec_pretty(&pinned_snippets)?,
    )?;
    if final_dir.exists() {
        fs::remove_dir_all(&final_dir)?;
    }
    fs::rename(&tmp_dir, &final_dir)?;
    Ok(manifest)
}

pub fn list_snapshots(
    daemon_dir: &Path,
    session_id_filter: Option<&str>,
) -> Result<Vec<SnapshotManifest>> {
    let root = daemon_dir.join("snapshots");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut snapshots = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        let manifest = read_manifest(&manifest_path)?;
        if session_id_filter.is_none_or(|session_id| manifest.session_id == session_id) {
            snapshots.push(manifest);
        }
    }
    snapshots.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.created_at));
    Ok(snapshots)
}

pub fn read_snapshot(daemon_dir: &Path, snapshot_id: &str) -> Result<FullSnapshot> {
    let dir = snapshot_dir(daemon_dir, snapshot_id);
    let manifest = read_manifest(&dir.join("manifest.json"))?;
    let working_set: WorkingSet = read_schema_json(&dir.join("working_set.json"))?;
    let action_log = read_action_log(&dir.join("action_log.jsonl"))?;
    let pinned_snippets = if dir.join("pinned_snippets.json").exists() {
        serde_json::from_slice(&fs::read(dir.join("pinned_snippets.json"))?)?
    } else {
        Vec::new()
    };
    Ok(FullSnapshot {
        manifest,
        working_set,
        action_log,
        pinned_snippets,
    })
}

pub fn diff_snapshots(a: &FullSnapshot, b: &FullSnapshot) -> SnapshotDiff {
    let a_files = a
        .working_set
        .files_read
        .iter()
        .map(|file| (file.path.clone(), file.sha.clone()))
        .collect::<BTreeMap<_, _>>();
    let b_files = b
        .working_set
        .files_read
        .iter()
        .map(|file| (file.path.clone(), file.sha.clone()))
        .collect::<BTreeMap<_, _>>();
    let a_searches = a
        .working_set
        .searches_run
        .iter()
        .map(|search| search.search_id.clone())
        .collect::<BTreeSet<_>>();
    let b_searches = b
        .working_set
        .searches_run
        .iter()
        .map(|search| search.search_id.clone())
        .collect::<BTreeSet<_>>();

    SnapshotDiff {
        added_files: b_files
            .keys()
            .filter(|path| !a_files.contains_key(*path))
            .cloned()
            .collect(),
        removed_files: a_files
            .keys()
            .filter(|path| !b_files.contains_key(*path))
            .cloned()
            .collect(),
        changed_files: b_files
            .iter()
            .filter(|(path, sha)| a_files.get(*path).is_some_and(|old| old != *sha))
            .map(|(path, _sha): (&String, &String)| path.clone())
            .collect(),
        added_searches: b_searches.difference(&a_searches).cloned().collect(),
        removed_searches: a_searches.difference(&b_searches).cloned().collect(),
    }
}

pub fn snapshot_dir(daemon_dir: &Path, snapshot_id: &str) -> PathBuf {
    daemon_dir.join("snapshots").join(snapshot_id)
}

fn read_manifest(path: &Path) -> Result<SnapshotManifest> {
    read_schema_json(path)
}

fn read_schema_json<T>(path: &Path) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let value: Value = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read {}", path.display()))?,
    )?;
    let version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or_default() as u32;
    if version != PORTABILITY_SCHEMA_VERSION {
        bail!("unsupported snapshot schema version {version}");
    }
    Ok(serde_json::from_value(value)?)
}

fn read_action_log(path: &Path) -> Result<Vec<ActionLogEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for line in fs::read_to_string(path)?.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: ActionLogEntry = serde_json::from_str(line)?;
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

fn build_working_set(
    entries: &[ActionLogEntry],
    frecency: &FrecencyStore,
    pinned_snippets: &[PinnedSnippet],
) -> WorkingSet {
    let mut files = BTreeMap::<String, FileReadRef>::new();
    let mut searches = Vec::new();
    for snippet in pinned_snippets {
        files.insert(
            snippet.source_path.clone(),
            FileReadRef {
                path: snippet.source_path.clone(),
                sha: snippet.sha.clone(),
                line_start: Some(snippet.line_start),
                line_end: Some(snippet.line_end),
                last_read_at: now_secs(),
            },
        );
    }
    for entry in entries {
        match &entry.kind {
            ActionKind::Search => {
                let query = entry
                    .payload
                    .get("query")
                    .or_else(|| entry.payload.get("pattern"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let kind = entry
                    .payload
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("search")
                    .to_string();
                let result_paths = entry
                    .payload
                    .get("result_paths")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                searches.push(SearchRef {
                    search_id: format!("action-{}", entry.entry_id),
                    query,
                    kind,
                    result_paths,
                });
            }
            ActionKind::Read | ActionKind::MemoCheck => {
                if let Some(path) = entry.payload.get("path").and_then(Value::as_str) {
                    let path = path.to_string();
                    files.entry(path.clone()).or_insert_with(|| FileReadRef {
                        path: path.clone(),
                        sha: entry
                            .payload
                            .get("sha")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        line_start: None,
                        line_end: None,
                        last_read_at: entry.ts,
                    });
                }
            }
            _ => {}
        }
    }
    WorkingSet {
        schema_version: PORTABILITY_SCHEMA_VERSION,
        files_read: files.into_values().collect(),
        searches_run: searches,
        frecency_top_n: frecency.top_n(50),
    }
}

fn build_pinned_snippets(
    repo_root: &Path,
    specs: &[PinnedSnippetSpec],
) -> Result<Vec<PinnedSnippet>> {
    let mut snippets = Vec::new();
    for spec in specs {
        if spec.line_start == 0 || spec.line_end < spec.line_start {
            bail!("invalid pinned snippet range for {}", spec.path);
        }
        let content = read_line_range(&repo_root.join(&spec.path), spec.line_start, spec.line_end)?;
        let sha = hex_sha256(content.as_bytes());
        snippets.push(PinnedSnippet {
            sha,
            source_path: spec.path.clone(),
            line_start: spec.line_start,
            line_end: spec.line_end,
            content,
        });
    }
    Ok(snippets)
}

fn read_line_range(path: &Path, start: usize, end: usize) -> Result<String> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let lines = text
        .lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let line_no = idx + 1;
            (line_no >= start && line_no <= end).then_some(line)
        })
        .collect::<Vec<_>>();
    Ok(lines.join("\n"))
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
