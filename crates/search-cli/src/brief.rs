use anyhow::{Result, bail};
use search_core::{ActionKind, FullSnapshot};

pub enum BriefingMode {
    NoInference,
    LocalModel {
        endpoint: String,
        model: String,
    },
    CloudModel {
        provider: String,
        model: String,
        api_key_env: String,
    },
}

pub fn generate_briefing(
    snapshot: &FullSnapshot,
    mode: &BriefingMode,
    _transcript: Option<&str>,
) -> Result<String> {
    match mode {
        BriefingMode::NoInference => Ok(no_inference_brief(snapshot)),
        BriefingMode::LocalModel { endpoint, model } => {
            let _ = (endpoint, model);
            Ok(no_inference_brief(snapshot))
        }
        BriefingMode::CloudModel {
            provider,
            model,
            api_key_env,
        } => {
            let _ = (provider, model, api_key_env);
            Ok(no_inference_brief(snapshot))
        }
    }
}

pub fn validate_briefing(snapshot: &FullSnapshot, markdown: &str) -> Result<()> {
    for header in [
        "## Goal",
        "## Current hypothesis",
        "## Established facts",
        "## Ruled out",
        "## Open questions",
        "## Next planned action",
    ] {
        if !markdown.contains(header) {
            bail!("briefing missing required header {header}");
        }
    }
    if markdown.len() > 6000 {
        bail!("briefing exceeds 6000 byte budget");
    }
    let valid_ids = snapshot
        .action_log
        .iter()
        .map(|entry| entry.entry_id.to_string())
        .collect::<std::collections::HashSet<_>>();
    for line in markdown.lines() {
        if line.starts_with("- ") && line.contains("[action_log:") {
            let Some(id) = line
                .split("[action_log:")
                .nth(1)
                .and_then(|tail| tail.split(']').next())
            else {
                bail!("malformed action log citation");
            };
            if !valid_ids.contains(id) {
                bail!("unknown action log citation {id}");
            }
        }
    }
    Ok(())
}

fn no_inference_brief(snapshot: &FullSnapshot) -> String {
    let goal = snapshot
        .action_log
        .iter()
        .find_map(|entry| {
            entry
                .payload
                .get("goal")
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or("Continue the captured session.");
    let mut facts = Vec::new();
    for entry in snapshot.action_log.iter().take(12) {
        let fact = match entry.kind {
            ActionKind::Search => {
                let query = entry
                    .payload
                    .get("query")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown query");
                format!("- Ran search `{query}` [action_log:{}]", entry.entry_id)
            }
            ActionKind::MemoCheck => {
                let path = entry
                    .payload
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown path");
                format!(
                    "- Checked memo freshness for `{path}` [action_log:{}]",
                    entry.entry_id
                )
            }
            _ => format!(
                "- Recorded {:?} [action_log:{}]",
                entry.kind, entry.entry_id
            ),
        };
        facts.push(fact);
    }
    if facts.is_empty() {
        facts.push("(none captured)".to_string());
    }
    format!(
        "## Goal\n{goal}\n\n## Current hypothesis\nNo inference configured; inspect the action log and working set.\n\n## Established facts\n{}\n\n## Ruled out\n(none identified)\n\n## Open questions\n- What should the next harness verify first?\n\n## Next planned action\nReview the hydration payload, then verify one captured fact with TriSeek before editing.\n",
        facts.join("\n")
    )
}
