//! Ergonomic handoff UX layered on top of snapshot/brief/resume primitives.

use anyhow::{Result, bail};
use std::path::Path;

pub fn normalize_harness(value: &str) -> Result<String> {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    match normalized.as_str() {
        "claude" | "claude_code" | "claudecode" => Ok("claude_code".to_string()),
        "codex" | "codex_cli" => Ok("codex".to_string()),
        _ => bail!("unsupported harness `{value}`; expected `claude` or `codex`"),
    }
}

pub fn harness_display(canonical: &str) -> &'static str {
    match canonical {
        "claude_code" => "Claude",
        "codex" => "Codex",
        _ => "target harness",
    }
}

pub fn resume_command_for_harness(canonical: &str, snapshot_id: &str) -> String {
    match canonical {
        "codex" => format!("$triseek resume {snapshot_id}"),
        "claude_code" => format!("/triseek resume {snapshot_id}"),
        _ => format!("triseek resume {snapshot_id}"),
    }
}

#[cfg(test)]
pub fn render_handoff_block(
    source_harness: Option<&str>,
    target_harness: &str,
    session_id: &str,
    snapshot_id: &str,
    briefing_path: &Path,
) -> String {
    render_handoff_block_with_resume_arg(
        source_harness,
        target_harness,
        session_id,
        snapshot_id,
        briefing_path,
        snapshot_id,
    )
}

pub fn render_handoff_block_with_resume_arg(
    source_harness: Option<&str>,
    target_harness: &str,
    session_id: &str,
    snapshot_id: &str,
    briefing_path: &Path,
    resume_arg: &str,
) -> String {
    let from_display = source_harness.map(harness_display).unwrap_or("unknown");
    let target_display = harness_display(target_harness);
    let resume_command = resume_command_for_harness(target_harness, resume_arg);
    format!(
        "TriSeek handoff ready\n\nFrom: {from_display}\nTo: {target_display}\nSession: {session_id}\nSnapshot: {snapshot_id}\nBrief: {}\n\nIn {target_display}, paste:\n  {resume_command}",
        briefing_path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn normalizes_supported_harness_aliases() {
        assert_eq!(normalize_harness("claude").unwrap(), "claude_code");
        assert_eq!(normalize_harness("claude-code").unwrap(), "claude_code");
        assert_eq!(normalize_harness("codex").unwrap(), "codex");
    }

    #[test]
    fn handoff_block_includes_target_paste_command() {
        let block = render_handoff_block(
            Some("claude_code"),
            "codex",
            "session_demo",
            "snap_123",
            &PathBuf::from("/tmp/briefing.md"),
        );
        assert!(block.contains("TriSeek handoff ready"));
        assert!(block.contains("In Codex, paste:\n  $triseek resume snap_123"));
        assert!(block.contains("Session: session_demo"));
    }

    #[test]
    fn handoff_block_uses_slash_command_for_claude_target() {
        let block = render_handoff_block(
            Some("codex"),
            "claude_code",
            "session_demo",
            "snap_456",
            &PathBuf::from("/tmp/briefing.md"),
        );
        assert!(block.contains("In Claude, paste:\n  /triseek resume snap_456"));
    }

    #[test]
    fn handoff_metadata_round_trips_git_restore_fields() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let metadata = crate::handoff_metadata::HandoffMetadata::git(
            "codex",
            crate::handoff_metadata::GitHandoffMetadata {
                remote: "origin".to_string(),
                remote_url: "git@example.com:acme/project.git".to_string(),
                branch: "triseek/handoff/session_demo".to_string(),
                commit: "abc123".to_string(),
                base_branch: Some("main".to_string()),
                base_commit: Some("def456".to_string()),
                dirty_commit_created: true,
                pushed_at: 1770000000,
            },
        );

        crate::handoff_metadata::write(tmp.path(), &metadata).expect("write metadata");
        let restored = crate::handoff_metadata::read(tmp.path()).expect("read metadata");

        assert_eq!(restored.mode, crate::handoff_metadata::HandoffMode::Git);
        assert_eq!(restored.target_harness, "codex");
        let git = restored.git.expect("git metadata");
        assert_eq!(git.branch, "triseek/handoff/session_demo");
        assert_eq!(git.commit, "abc123");
        assert!(git.dirty_commit_created);
    }
}
