use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitState {
    pub commit: Option<String>,
    pub dirty_files: Vec<String>,
    pub hunk_summary: HashMap<String, usize>,
}

pub fn capture(repo_root: &Path) -> Result<GitState> {
    let commit = git_output(repo_root, &["rev-parse", "HEAD"]).ok();
    let dirty_files = git_output(repo_root, &["status", "--porcelain"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.get(3..).map(str::trim).filter(|s| !s.is_empty()))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let mut hunk_summary = HashMap::new();
    let diff = git_output(repo_root, &["diff", "--numstat"]).unwrap_or_default();
    for line in diff.lines() {
        let parts = line.split('\t').collect::<Vec<_>>();
        if parts.len() >= 3 {
            let added = parts[0].parse::<usize>().unwrap_or(0);
            let removed = parts[1].parse::<usize>().unwrap_or(0);
            hunk_summary.insert(parts[2].to_string(), added + removed);
        }
    }
    Ok(GitState {
        commit,
        dirty_files,
        hunk_summary,
    })
}

fn git_output(repo_root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))?;
    if !output.status.success() {
        anyhow::bail!("git {} failed", args.join(" "));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
