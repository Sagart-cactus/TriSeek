use crate::handoff_metadata::GitHandoffMetadata;
use anyhow::{Context, Result, bail};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use time::OffsetDateTime;

#[derive(Debug, Clone, Default)]
pub struct GitHandoffOptions {
    pub branch: Option<String>,
    pub remote: Option<String>,
    pub message: Option<String>,
    pub continue_existing: bool,
    pub interactive: bool,
}

pub fn prepare(
    repo_root: &Path,
    session_id: &str,
    options: &GitHandoffOptions,
) -> Result<GitHandoffMetadata> {
    ensure_git_repo(repo_root)?;
    let remote = options
        .remote
        .clone()
        .or_else(|| upstream_remote(repo_root).ok().flatten())
        .unwrap_or_else(|| "origin".to_string());
    let branch = options
        .branch
        .clone()
        .unwrap_or_else(|| default_branch_name(session_id));
    let base_branch = current_branch(repo_root).ok();
    let base_commit = git_stdout(repo_root, &["rev-parse", "HEAD"]).ok();
    let stashed = stash_if_dirty(repo_root, session_id)?;

    let branch_exists = local_branch_exists(repo_root, &branch)?;
    let continue_existing = options.continue_existing
        || (branch_exists && options.interactive && prompt_continue_existing(&branch)?);
    if branch_exists && !continue_existing {
        restore_stash(repo_root, stashed)?;
        bail!(
            "branch `{branch}` already exists; rerun with --continue to continue this handoff or --branch <name> to choose another branch"
        );
    }
    if branch_exists {
        git(repo_root, &["switch", &branch])?;
    } else {
        git(repo_root, &["switch", "-c", &branch])?;
    }
    restore_stash(repo_root, stashed)?;

    let dirty_commit_created = if is_dirty(repo_root)? {
        git(repo_root, &["add", "-A"])?;
        let message = options
            .message
            .clone()
            .unwrap_or_else(|| format!("triseek handoff {session_id}"));
        git(repo_root, &["commit", "-m", &message])?;
        true
    } else {
        false
    };

    git(repo_root, &["push", "-u", &remote, &branch])?;
    let commit = git_stdout(repo_root, &["rev-parse", "HEAD"])?;
    let remote_url = git_stdout(repo_root, &["remote", "get-url", &remote])?;
    Ok(GitHandoffMetadata {
        remote,
        remote_url,
        branch,
        commit,
        base_branch,
        base_commit,
        dirty_commit_created,
        pushed_at: OffsetDateTime::now_utc().unix_timestamp(),
    })
}

pub fn restore(repo_root: &Path, metadata: &GitHandoffMetadata) -> Result<()> {
    ensure_git_repo(repo_root)?;
    let stashed = stash_if_dirty(repo_root, "resume")?;
    let remote_ref = format!("refs/remotes/{}/{}", metadata.remote, metadata.branch);
    git(
        repo_root,
        &[
            "fetch",
            &metadata.remote,
            &format!("refs/heads/{}:{remote_ref}", metadata.branch),
        ],
    )?;

    if local_branch_exists(repo_root, &metadata.branch)? {
        git(repo_root, &["switch", &metadata.branch])?;
        git(repo_root, &["merge", "--ff-only", &remote_ref])?;
    } else {
        git(repo_root, &["switch", "-c", &metadata.branch, &remote_ref])?;
    }

    let actual_commit = git_stdout(repo_root, &["rev-parse", "HEAD"])?;
    if actual_commit != metadata.commit {
        restore_stash(repo_root, stashed)?;
        bail!(
            "restored branch `{}` at commit `{actual_commit}`, expected `{}`",
            metadata.branch,
            metadata.commit
        );
    }

    restore_stash(repo_root, stashed)
}

pub fn validate_checkout(
    repo_root: &Path,
    expected_commit: Option<&str>,
    expected_dirty_files: Option<&[String]>,
) -> Result<()> {
    if let Some(expected_commit) = expected_commit {
        let actual_commit = git_stdout(repo_root, &["rev-parse", "HEAD"])?;
        if actual_commit != expected_commit {
            bail!("checkout is at `{actual_commit}`, expected commit `{expected_commit}`");
        }
    }
    if let Some(expected_dirty_files) = expected_dirty_files {
        let mut actual_dirty_files = dirty_files(repo_root)?;
        let mut expected_dirty_files = expected_dirty_files.to_vec();
        actual_dirty_files.sort();
        expected_dirty_files.sort();
        if actual_dirty_files != expected_dirty_files {
            bail!(
                "checkout dirty files differ from snapshot; actual {:?}, expected {:?}",
                actual_dirty_files,
                expected_dirty_files
            );
        }
    }
    Ok(())
}

fn default_branch_name(session_id: &str) -> String {
    let safe_session = session_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("triseek/handoff/{safe_session}")
}

fn ensure_git_repo(repo_root: &Path) -> Result<()> {
    git_stdout(repo_root, &["rev-parse", "--show-toplevel"]).map(|_| ())
}

fn prompt_continue_existing(branch: &str) -> Result<bool> {
    eprint!("Branch `{branch}` already exists. Continue this handoff branch? [y/N] ");
    std::io::stderr().flush().ok();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("read handoff branch prompt")?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn upstream_remote(repo_root: &Path) -> Result<Option<String>> {
    let upstream = match git_stdout(repo_root, &["rev-parse", "--abbrev-ref", "@{u}"]) {
        Ok(upstream) => upstream,
        Err(_) => return Ok(None),
    };
    Ok(upstream
        .split_once('/')
        .map(|(remote, _)| remote.to_string()))
}

fn current_branch(repo_root: &Path) -> Result<String> {
    git_stdout(repo_root, &["branch", "--show-current"])
}

fn local_branch_exists(repo_root: &Path, branch: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .current_dir(repo_root)
        .output()
        .context("run git show-ref")?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => bail!(
            "git show-ref failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

fn stash_if_dirty(repo_root: &Path, session_id: &str) -> Result<Option<String>> {
    if !is_dirty(repo_root)? {
        return Ok(None);
    }
    let before = git_stdout(repo_root, &["stash", "list"])?;
    git(
        repo_root,
        &[
            "stash",
            "push",
            "-u",
            "-m",
            &format!("triseek handoff {session_id}"),
        ],
    )?;
    let after = git_stdout(repo_root, &["stash", "list"])?;
    let new_entry = after
        .lines()
        .find(|line| !before.lines().any(|old| old == *line))
        .and_then(|line| line.split_once(':').map(|(name, _)| name.to_string()))
        .unwrap_or_else(|| "stash@{0}".to_string());
    Ok(Some(new_entry))
}

fn restore_stash(repo_root: &Path, stash_ref: Option<String>) -> Result<()> {
    if let Some(stash_ref) = stash_ref {
        git(repo_root, &["stash", "pop", &stash_ref]).with_context(|| {
            format!("failed to apply stashed work `{stash_ref}`; resolve conflicts before retrying")
        })?;
    }
    Ok(())
}

fn is_dirty(repo_root: &Path) -> Result<bool> {
    Ok(!git_stdout(repo_root, &["status", "--porcelain"])?.is_empty())
}

fn dirty_files(repo_root: &Path) -> Result<Vec<String>> {
    Ok(git_stdout(repo_root, &["status", "--porcelain"])?
        .lines()
        .filter_map(|line| line.get(3..).map(str::trim).filter(|path| !path.is_empty()))
        .map(ToString::to_string)
        .collect())
}

fn git(repo_root: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))?;
    if output.status.success() {
        Ok(())
    } else {
        bail!(
            "git {} failed\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }
}

fn git_stdout(repo_root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        bail!(
            "git {} failed\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_stdout(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn init_remote_and_clone() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let remote = tmp.path().join("remote.git");
        let clone = tmp.path().join("work");
        git(tmp.path(), &["init", "--bare", remote.to_str().unwrap()]);
        git(
            tmp.path(),
            &["clone", remote.to_str().unwrap(), clone.to_str().unwrap()],
        );
        git(&clone, &["config", "user.email", "triseek@example.com"]);
        git(&clone, &["config", "user.name", "TriSeek Test"]);
        std::fs::write(clone.join("README.md"), "initial\n").expect("write readme");
        git(&clone, &["add", "README.md"]);
        git(&clone, &["commit", "-m", "initial"]);
        git(&clone, &["push", "-u", "origin", "HEAD:main"]);
        git(&clone, &["switch", "-c", "feature"]);
        (tmp, clone)
    }

    #[test]
    fn prepare_creates_default_handoff_branch_commits_dirty_tree_and_pushes() {
        let (_tmp, clone) = init_remote_and_clone();
        std::fs::write(clone.join("work.txt"), "handoff work\n").expect("write work");

        let metadata = prepare(
            &clone,
            "session_demo",
            &GitHandoffOptions {
                branch: None,
                remote: None,
                message: None,
                continue_existing: false,
                interactive: false,
            },
        )
        .expect("prepare git handoff");

        assert_eq!(metadata.remote, "origin");
        assert_eq!(metadata.branch, "triseek/handoff/session_demo");
        assert!(metadata.dirty_commit_created);
        assert_eq!(
            git_stdout(&clone, &["branch", "--show-current"]),
            metadata.branch
        );
        assert_eq!(git_stdout(&clone, &["rev-parse", "HEAD"]), metadata.commit);
        assert_eq!(
            git_stdout(&clone, &["ls-remote", "origin", &metadata.branch])
                .split_whitespace()
                .next(),
            Some(metadata.commit.as_str())
        );
        assert_eq!(git_stdout(&clone, &["status", "--porcelain"]), "");
    }

    #[test]
    fn restore_fetches_handoff_branch_and_reapplies_local_changes() {
        let (tmp, clone_a) = init_remote_and_clone();
        std::fs::write(clone_a.join("work.txt"), "handoff work\n").expect("write work");
        let metadata = prepare(
            &clone_a,
            "session_demo",
            &GitHandoffOptions {
                branch: None,
                remote: None,
                message: None,
                continue_existing: false,
                interactive: false,
            },
        )
        .expect("prepare git handoff");

        let clone_b = tmp.path().join("other");
        let remote = tmp.path().join("remote.git");
        git(
            tmp.path(),
            &["clone", remote.to_str().unwrap(), clone_b.to_str().unwrap()],
        );
        git(&clone_b, &["switch", "main"]);
        std::fs::write(clone_b.join("local.txt"), "target local work\n").expect("write local");

        restore(&clone_b, &metadata).expect("restore git handoff");

        assert_eq!(
            git_stdout(&clone_b, &["branch", "--show-current"]),
            metadata.branch
        );
        assert_eq!(
            git_stdout(&clone_b, &["rev-parse", "HEAD"]),
            metadata.commit
        );
        assert_eq!(
            std::fs::read_to_string(clone_b.join("work.txt")).expect("read handoff work"),
            "handoff work\n"
        );
        assert_eq!(
            std::fs::read_to_string(clone_b.join("local.txt")).expect("read local work"),
            "target local work\n"
        );
    }

    #[test]
    fn prepare_clean_tree_pushes_branch_without_creating_commit() {
        let (_tmp, clone) = init_remote_and_clone();
        let start_commit = git_stdout(&clone, &["rev-parse", "HEAD"]);

        let metadata = prepare(
            &clone,
            "session_clean",
            &GitHandoffOptions {
                branch: None,
                remote: None,
                message: None,
                continue_existing: false,
                interactive: false,
            },
        )
        .expect("prepare clean git handoff");

        assert!(!metadata.dirty_commit_created);
        assert_eq!(metadata.commit, start_commit);
        assert_eq!(
            git_stdout(&clone, &["ls-remote", "origin", &metadata.branch])
                .split_whitespace()
                .next(),
            Some(metadata.commit.as_str())
        );
    }

    #[test]
    fn prepare_existing_branch_requires_continue_when_non_interactive() {
        let (_tmp, clone) = init_remote_and_clone();
        git(&clone, &["switch", "-c", "triseek/handoff/session_exists"]);
        git(&clone, &["switch", "feature"]);

        let error = prepare(
            &clone,
            "session_exists",
            &GitHandoffOptions {
                branch: None,
                remote: None,
                message: None,
                continue_existing: false,
                interactive: false,
            },
        )
        .expect_err("existing handoff branch should require continue");

        assert!(error.to_string().contains("--continue"));
        assert_eq!(git_stdout(&clone, &["branch", "--show-current"]), "feature");
    }

    #[test]
    fn validate_checkout_rejects_wrong_commit() {
        let (_tmp, clone) = init_remote_and_clone();
        let old_commit = git_stdout(&clone, &["rev-parse", "HEAD"]);
        std::fs::write(clone.join("next.txt"), "next\n").expect("write next");
        git(&clone, &["add", "next.txt"]);
        git(&clone, &["commit", "-m", "next"]);

        let error = validate_checkout(&clone, Some(&old_commit), Some(&[]))
            .expect_err("wrong commit should fail validation");

        assert!(error.to_string().contains("expected commit"));
    }
}
