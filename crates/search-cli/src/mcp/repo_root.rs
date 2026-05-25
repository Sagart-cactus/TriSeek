//! Repo-root discovery for the MCP server.
//!
//! Resolution order:
//! 1. explicit `--repo` flag passed to `triseek mcp serve`
//! 2. `TRISEEK_REPO_ROOT` environment variable
//! 3. walk up from the current working directory looking for a `.git` marker
//! 4. leave the MCP server rootless so individual tool calls can require a root

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootSafety {
    Safe,
    Broad,
    UnsafeImplicit,
}

#[derive(Debug, Clone)]
pub struct ResolvedRoot {
    pub root: PathBuf,
    pub safety: RootSafety,
}

pub fn resolve(explicit: Option<&Path>) -> Result<PathBuf> {
    match resolve_startup(explicit)? {
        Some(resolved) => Ok(resolved.root),
        None => {
            let cwd =
                std::env::current_dir().context("failed to read current working directory")?;
            anyhow::bail!(
                "TriSeek could not detect a repository root from {}; start the server with --repo <PATH>, set TRISEEK_REPO_ROOT, or run it from inside a git repository",
                cwd.display()
            )
        }
    }
}

pub fn resolve_startup(explicit: Option<&Path>) -> Result<Option<ResolvedRoot>> {
    if let Some(path) = explicit {
        let root = canonicalize_dir(path, "--repo")?;
        let safety = classify_root(&root, true);
        return Ok(Some(ResolvedRoot { root, safety }));
    }
    if let Ok(env_path) = std::env::var("TRISEEK_REPO_ROOT") {
        let p = PathBuf::from(env_path);
        let root = canonicalize_dir(&p, "TRISEEK_REPO_ROOT")?;
        let safety = classify_root(&root, true);
        return Ok(Some(ResolvedRoot { root, safety }));
    }
    let cwd = std::env::current_dir().context("failed to read current working directory")?;
    resolve_startup_from_cwd(&cwd)
}

fn resolve_startup_from_cwd(cwd: &Path) -> Result<Option<ResolvedRoot>> {
    if let Some(git_root) = walk_up_for_git(cwd) {
        let root = canonicalize_dir(&git_root, "git root")?;
        let safety = classify_root(&root, false);
        if safety != RootSafety::UnsafeImplicit {
            return Ok(Some(ResolvedRoot { root, safety }));
        }
    }
    Ok(None)
}

pub fn resolve_explicit_tool_root(path: &Path) -> Result<ResolvedRoot> {
    let root = canonicalize_dir(path, "root")?;
    let safety = classify_root(&root, true);
    Ok(ResolvedRoot { root, safety })
}

fn canonicalize_dir(path: &Path, label: &str) -> Result<PathBuf> {
    let root = path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {label} {}", path.display()))?;
    if !root.is_dir() {
        anyhow::bail!("{label} {} is not a directory", root.display());
    }
    Ok(root)
}

pub fn classify_root(root: &Path, explicit: bool) -> RootSafety {
    if is_filesystem_root(root) {
        return if explicit {
            RootSafety::Broad
        } else {
            RootSafety::UnsafeImplicit
        };
    }
    if is_home_dir(root) || is_common_home_broad_dir(root) {
        return if explicit {
            RootSafety::Broad
        } else {
            RootSafety::UnsafeImplicit
        };
    }
    RootSafety::Safe
}

fn is_filesystem_root(root: &Path) -> bool {
    root.parent().is_none()
}

fn is_home_dir(root: &Path) -> bool {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|home| home.canonicalize().ok())
        .is_some_and(|home| home == root)
}

fn is_common_home_broad_dir(root: &Path) -> bool {
    let Some(home) = std::env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|home| home.canonicalize().ok())
    else {
        return false;
    };
    ["Desktop", "Documents", "Downloads", "Library", "Projects"]
        .into_iter()
        .any(|name| home.join(name) == root)
        || home.join("Documents").join("Projects") == root
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

    #[test]
    fn resolve_from_cwd_returns_none_without_git_marker() {
        let tmp = TempDir::new().unwrap();

        let result = resolve_startup_from_cwd(tmp.path()).unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn resolve_from_cwd_returns_git_root() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let nested = tmp.path().join("a/b");
        std::fs::create_dir_all(&nested).unwrap();

        let result = resolve_startup_from_cwd(&nested).unwrap().unwrap();

        assert_eq!(result.root, tmp.path().canonicalize().unwrap());
    }
}
