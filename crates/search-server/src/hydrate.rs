use crate::memo::MemoState;
use anyhow::Result;
use search_core::{
    FullSnapshot, HydrationReport, MemoEventKind, MemoObserveParams, SessionResumePrepareResponse,
};
use search_frecency::FrecencyStore;
use std::path::Path;

pub fn prepare_resume(
    snapshot: &FullSnapshot,
    memo: &MemoState,
    frecency: &mut FrecencyStore,
    repo_root: &Path,
    budget_tokens: Option<usize>,
) -> Result<SessionResumePrepareResponse> {
    let session_id = format!("resume-{}", snapshot.manifest.snapshot_id);
    let mut stale_files = Vec::new();
    for file in &snapshot.working_set.files_read {
        if !file.path.is_empty() && repo_root.join(&file.path).exists() {
            memo.observe(&MemoObserveParams {
                session_id: session_id.clone(),
                repo_root: repo_root.display().to_string(),
                event: MemoEventKind::Read,
                path: Some(file.path.clone()),
                content_hash: None,
                tokens: None,
            });
        } else if !file.path.is_empty() {
            stale_files.push(file.path.clone());
        }
    }
    frecency.restore_scores(&snapshot.working_set.frecency_top_n);
    let _ = frecency.flush();
    let payload_markdown = build_hydration_payload(snapshot, budget_tokens.unwrap_or(3000));
    let payload_token_estimate = payload_markdown.len().div_ceil(4);
    Ok(SessionResumePrepareResponse {
        session_id: snapshot.manifest.session_id.clone(),
        payload_markdown,
        payload_token_estimate,
        hydration_report: HydrationReport {
            files_primed: snapshot
                .working_set
                .files_read
                .len()
                .saturating_sub(stale_files.len()),
            searches_warmed: snapshot.working_set.searches_run.len(),
            frecency_entries_restored: snapshot.working_set.frecency_top_n.len(),
            stale_files,
        },
        searches: snapshot.working_set.searches_run.clone(),
    })
}

fn build_hydration_payload(snapshot: &FullSnapshot, budget_tokens: usize) -> String {
    let target_bytes = budget_tokens.saturating_mul(4).max(512);
    let mut out = String::new();
    out.push_str("# TriSeek Hydration Payload\n\n");
    out.push_str(&format!(
        "- snapshot_id: {}\n- session_id: {}\n- repo_root: {}\n- created_at: {}\n\n",
        snapshot.manifest.snapshot_id,
        snapshot.manifest.session_id,
        snapshot.manifest.repo_root,
        snapshot.manifest.created_at
    ));
    out.push_str("## Working Set\n");
    for file in snapshot.working_set.files_read.iter().take(40) {
        out.push_str(&format!("- {} ({})\n", file.path, file.sha));
    }
    out.push_str("\n## Searches\n");
    for search in snapshot.working_set.searches_run.iter().take(40) {
        out.push_str(&format!(
            "- [{}] {}: {} -> {}\n",
            search.search_id,
            search.kind,
            search.query,
            search.result_paths.join(", ")
        ));
    }
    if !snapshot.pinned_snippets.is_empty() {
        out.push_str("\n## Pinned Snippets\n");
        for snippet in &snapshot.pinned_snippets {
            out.push_str(&format!(
                "\n### {}:{}-{}\n```text\n{}\n```\n",
                snippet.source_path, snippet.line_start, snippet.line_end, snippet.content
            ));
            if out.len() >= target_bytes {
                break;
            }
        }
    }
    if out.len() > target_bytes {
        out.truncate(target_bytes);
        out.push_str("\n\n[truncated to hydration budget]\n");
    }
    out
}
