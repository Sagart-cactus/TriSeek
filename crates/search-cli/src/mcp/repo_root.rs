//! Repo-root discovery for the MCP server.
//!
//! Resolution order:
//! 1. explicit `--repo` flag passed to `triseek mcp serve`
//! 2. `TRISEEK_REPO_ROOT` environment variable
//! 3. walk up from the current working directory looking for a `.git` marker
//! 4. fall back to the current working directory itself

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub fn resolve(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return path
            .canonicalize()
            .with_context(|| format!("failed to canonicalize --repo {}", path.display()));
    }
    if let Ok(env_path) = std::env::var("TRISEEK_REPO_ROOT") {
        let p = PathBuf::from(env_path);
        return p
            .canonicalize()
            .with_context(|| format!("failed to canonicalize TRISEEK_REPO_ROOT {}", p.display()));
    }
    let cwd = std::env::current_dir().context("failed to read current working directory")?;
    if let Some(git_root) = walk_up_for_git(&cwd) {
        return git_root
            .canonicalize()
            .context("failed to canonicalize git root");
    }
    cwd.canonicalize().context("failed to canonicalize cwd")
}

fn walk_up_for_git(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn walks_up_for_git_marker() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let nested = root.join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();
        let found = walk_up_for_git(&nested).unwrap();
        assert_eq!(found.canonicalize().unwrap(), root.canonicalize().unwrap());
    }

    #[test]
    fn walks_up_returns_none_without_git() {
        let tmp = TempDir::new().unwrap();
        assert!(walk_up_for_git(tmp.path()).is_none());
    }
}
