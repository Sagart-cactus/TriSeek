//! Ripgrep backend adapter.
//!
//! Shells out to the local `rg` binary for content search when the adaptive
//! router selects the ripgrep path. Parses `rg --json` output into the shared
//! [`SearchExecution`] shape so callers see uniform results regardless of
//! which backend ran.

use anyhow::{Context, Result, bail};
use search_core::{
    CaseMode, ProcessMetrics, QueryRequest, SearchHit, SearchKind, SearchLineMatch, SearchMetrics,
    SearchSummary,
};
use search_index::{SearchExecution, default_searchable_hidden_roots};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

pub fn run_rg_search(
    repo_root: &Path,
    request: &QueryRequest,
    summary_only: bool,
) -> Result<SearchExecution> {
    if matches!(request.kind, SearchKind::Path) {
        bail!("path queries should not route to ripgrep execution");
    }
    let started = Instant::now();
    let mut command = Command::new("rg");
    command.current_dir(repo_root);
    command.arg("--color").arg("never");
    command.arg("--no-heading");

    if summary_only {
        command.arg("--count");
    } else {
        command.arg("--json");
        command.arg("--line-number");
    }

    if request.include_hidden {
        command.arg("--hidden");
    }
    if request.include_binary {
        command.arg("--text");
    }
    if matches!(request.case_mode, CaseMode::Insensitive) {
        command.arg("--ignore-case");
    }
    if matches!(request.kind, SearchKind::Literal | SearchKind::Auto) {
        command.arg("--fixed-strings");
    }
    for extension in request.normalized_extensions() {
        command.arg("-g").arg(format!("*.{extension}"));
    }
    for glob in &request.globs {
        command.arg("-g").arg(glob);
    }
    for prefix in &request.path_prefixes {
        let mut normalized = prefix.trim_end_matches('/').to_string();
        normalized.push_str("/**");
        command.arg("-g").arg(normalized);
    }
    for name in &request.exact_names {
        command.arg("-g").arg(format!("**/{name}"));
    }
    for path in &request.exact_paths {
        command.arg("-g").arg(path);
    }
    if let Some(max_results) = request.max_results {
        command.arg("--max-count").arg(max_results.to_string());
    }
    command.arg(&request.pattern);
    command.arg(".");
    if !request.include_hidden {
        for path in default_searchable_hidden_roots(repo_root) {
            command.arg(path);
        }
    }

    let output = command.output().context("failed to execute rg")?;
    if !output.status.success() && output.status.code() != Some(1) {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }

    if summary_only {
        let mut files_with_matches = 0_usize;
        let mut total_line_matches = 0_usize;
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let Some((_, count)) = line.rsplit_once(':') else {
                continue;
            };
            let count = count.trim().parse::<usize>().unwrap_or_default();
            if count > 0 {
                files_with_matches += 1;
                total_line_matches += count;
            }
        }
        return Ok(SearchExecution {
            summary: SearchSummary {
                files_with_matches,
                total_line_matches,
            },
            metrics: SearchMetrics {
                process: ProcessMetrics {
                    wall_millis: started.elapsed().as_secs_f64() * 1_000.0,
                    user_cpu_millis: None,
                    system_cpu_millis: None,
                    max_rss_kib: None,
                },
                candidate_docs: 0,
                verified_docs: 0,
                matches_returned: total_line_matches,
                bytes_scanned: 0,
                index_bytes_read: None,
            },
            ..SearchExecution::default()
        });
    }

    let mut grouped = BTreeMap::<String, Vec<SearchLineMatch>>::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let value: Value = serde_json::from_str(line)?;
        if value.get("type").and_then(Value::as_str) != Some("match") {
            continue;
        }
        let data = &value["data"];
        let path = data["path"]["text"]
            .as_str()
            .or_else(|| data["path"]["bytes"].as_str())
            .unwrap_or_default()
            .to_string();
        let line_text = data["lines"]["text"]
            .as_str()
            .map(|value| value.trim_end_matches(['\n', '\r']).to_string())
            .unwrap_or_default();
        let line_number = data["line_number"].as_u64().unwrap_or_default() as usize;
        let column = data["submatches"]
            .get(0)
            .and_then(|submatch| submatch.get("start"))
            .and_then(Value::as_u64)
            .map(|value| value as usize + 1)
            .unwrap_or(1);
        grouped.entry(path).or_default().push(SearchLineMatch {
            line_number,
            column,
            line_text,
        });
    }

    let mut hits = Vec::with_capacity(grouped.len());
    let mut total_line_matches = 0_usize;
    for (path, lines) in grouped {
        total_line_matches += lines.len();
        hits.push(SearchHit::Content { path, lines });
    }

    Ok(SearchExecution {
        summary: SearchSummary {
            files_with_matches: hits.len(),
            total_line_matches,
        },
        metrics: SearchMetrics {
            process: ProcessMetrics {
                wall_millis: started.elapsed().as_secs_f64() * 1_000.0,
                user_cpu_millis: None,
                system_cpu_millis: None,
                max_rss_kib: None,
            },
            candidate_docs: 0,
            verified_docs: 0,
            matches_returned: total_line_matches,
            bytes_scanned: 0,
            index_bytes_read: None,
        },
        hits,
    })
}

#[cfg(test)]
mod tests {
    use super::run_rg_search;
    use search_core::{CaseMode, QueryRequest, SearchKind};
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn fixture_repo() -> TempDir {
        let root = TempDir::new().expect("tempdir");
        fs::create_dir_all(root.path().join("src")).expect("src dir");
        fs::create_dir_all(root.path().join(".github/workflows")).expect("github dir");
        fs::create_dir_all(root.path().join(".venv")).expect("venv dir");
        fs::write(
            root.path().join("src/lib.rs"),
            "pub fn visible() { println!(\"visible\"); }\n",
        )
        .expect("write lib");
        fs::write(
            root.path().join(".github/workflows/ci.yml"),
            "name: CI\non: [workflow_dispatch]\n",
        )
        .expect("write workflow");
        fs::write(root.path().join(".gitignore"), "target/\n").expect("write gitignore");
        fs::write(
            root.path().join(".venv/secret.txt"),
            "should_not_match_hidden_default\n",
        )
        .expect("write venv");
        root
    }

    #[test]
    fn rg_searches_allowlisted_hidden_paths_by_default() {
        if Command::new("rg").arg("--version").output().is_err() {
            return;
        }

        let repo = fixture_repo();
        let workflow = run_rg_search(
            repo.path(),
            &QueryRequest {
                kind: SearchKind::Literal,
                pattern: "workflow_dispatch".to_string(),
                case_mode: CaseMode::Sensitive,
                ..QueryRequest::default()
            },
            false,
        )
        .expect("rg workflow search");
        assert_eq!(workflow.summary.files_with_matches, 1);

        let gitignore = run_rg_search(
            repo.path(),
            &QueryRequest {
                kind: SearchKind::Literal,
                pattern: "target/".to_string(),
                case_mode: CaseMode::Sensitive,
                ..QueryRequest::default()
            },
            false,
        )
        .expect("rg dotfile search");
        assert_eq!(gitignore.summary.files_with_matches, 1);

        let skipped = run_rg_search(
            repo.path(),
            &QueryRequest {
                kind: SearchKind::Literal,
                pattern: "should_not_match_hidden_default".to_string(),
                case_mode: CaseMode::Sensitive,
                ..QueryRequest::default()
            },
            false,
        )
        .expect("rg skipped hidden search");
        assert_eq!(skipped.summary.files_with_matches, 0);
    }
}
