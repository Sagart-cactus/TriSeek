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

pub fn render_handoff_block(
    source_harness: Option<&str>,
    target_harness: &str,
    session_id: &str,
    snapshot_id: &str,
    briefing_path: &Path,
) -> String {
    let from = source_harness.unwrap_or("unknown");
    let target_display = harness_display(target_harness);
    format!(
        "TriSeek handoff ready\n\nFrom: {from}\nTo: {target_harness}\nSession: {session_id}\nSnapshot: {snapshot_id}\nBrief: {}\n\nIn {target_display}, paste:\n  /triseek resume {snapshot_id}",
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
        assert!(block.contains("In Codex, paste:\n  /triseek resume snap_123"));
        assert!(block.contains("Session: session_demo"));
    }
}
