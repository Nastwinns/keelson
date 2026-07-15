//! Production [`GitBackend`]: shell-out to the user's `git`.
//!
//! Mutations always shell out (correctness, credential helpers, hooks).
//! Reads shell out too for now; gitoxide (`gix`) replaces them later behind
//! the same trait without touching callers.

pub mod parallel;

use std::path::Path;
use std::process::Command;

use haw_core::git::{GitBackend, GitError, ResolvedRev, RevKind};

/// The default backend: runs `git` from PATH, prompts disabled.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShellGit;

/// Platform cache directory for shared bare mirrors (`--shared` mode).
pub fn default_cache_root() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("dev", "keelson", "keelson")
        .map(|dirs| dirs.cache_dir().join("mirrors"))
}

fn git_command(cwd: Option<&Path>) -> Command {
    let mut cmd = Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd
}

fn run(args: &[&str], cwd: Option<&Path>) -> Result<String, GitError> {
    let output = git_command(cwd).args(args).output()?;
    if !output.status.success() {
        return Err(GitError::Command {
            context: format!("git {}", args.join(" ")),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn is_full_sha(rev: &str) -> bool {
    rev.len() == 40 && rev.chars().all(|c| c.is_ascii_hexdigit())
}

impl GitBackend for ShellGit {
    fn resolve_rev(&self, url: &str, rev: &str) -> Result<ResolvedRev, GitError> {
        if is_full_sha(rev) {
            return Ok(ResolvedRev {
                sha: rev.to_ascii_lowercase(),
                kind: RevKind::Sha,
            });
        }
        let head_ref = format!("refs/heads/{rev}");
        let tag_ref = format!("refs/tags/{rev}");
        let out = run(&["ls-remote", "--heads", "--tags", url], None)?;

        let mut head = None;
        let mut tag = None;
        let mut peeled_tag = None;
        for line in out.lines() {
            let Some((sha, reference)) = line.split_once('\t') else {
                continue;
            };
            if reference == head_ref {
                head = Some(sha.to_string());
            } else if reference == tag_ref {
                tag = Some(sha.to_string());
            } else if reference == format!("{tag_ref}^{{}}") {
                peeled_tag = Some(sha.to_string());
            }
        }
        if let Some(sha) = head {
            return Ok(ResolvedRev {
                sha,
                kind: RevKind::Branch,
            });
        }
        if let Some(sha) = peeled_tag.or(tag) {
            return Ok(ResolvedRev {
                sha,
                kind: RevKind::Tag,
            });
        }
        Err(GitError::RevNotFound {
            url: url.to_string(),
            rev: rev.to_string(),
        })
    }

    fn clone_repo(&self, url: &str, dest: &Path, reference: Option<&Path>) -> Result<(), GitError> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut cmd = git_command(None);
        cmd.arg("clone");
        if let Some(mirror) = reference {
            cmd.arg("--reference").arg(mirror);
        }
        let output = cmd.arg(url).arg(dest).output()?;
        if !output.status.success() {
            return Err(GitError::Command {
                context: format!("git clone {url}"),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(())
    }

    fn ensure_mirror(&self, url: &str, mirror: &Path) -> Result<(), GitError> {
        if mirror.join("HEAD").exists() {
            run(&["fetch", "--prune"], Some(mirror))?;
            return Ok(());
        }
        if let Some(parent) = mirror.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let output = git_command(None)
            .arg("clone")
            .arg("--mirror")
            .arg(url)
            .arg(mirror)
            .output()?;
        if !output.status.success() {
            return Err(GitError::Command {
                context: format!("git clone --mirror {url}"),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(())
    }

    fn fetch(&self, repo: &Path) -> Result<(), GitError> {
        run(
            &["fetch", "--tags", "--force", "--prune", "origin"],
            Some(repo),
        )?;
        Ok(())
    }

    fn checkout(&self, repo: &Path, sha: &str, branch: &str) -> Result<(), GitError> {
        let branch_ref = format!("refs/heads/{branch}");
        let exists = run(
            &["rev-parse", "--verify", "--quiet", &branch_ref],
            Some(repo),
        )
        .is_ok();
        if exists {
            let range = format!("{sha}..{branch_ref}");
            let count: u64 = run(&["rev-list", "--count", &range], Some(repo))?
                .parse()
                .unwrap_or(0);
            if count > 0 {
                return Err(GitError::LocalCommits {
                    branch: branch.to_string(),
                    path: repo.to_path_buf(),
                    count,
                });
            }
        }
        run(&["checkout", "-B", branch, sha], Some(repo))?;
        Ok(())
    }

    fn create_branch(&self, repo: &Path, name: &str) -> Result<(), GitError> {
        run(&["checkout", "-b", name], Some(repo))?;
        Ok(())
    }

    fn push_branch(&self, repo: &Path, branch: &str) -> Result<(), GitError> {
        run(&["push", "--set-upstream", "origin", branch], Some(repo))?;
        Ok(())
    }

    fn head_sha(&self, repo: &Path) -> Result<String, GitError> {
        run(&["rev-parse", "HEAD"], Some(repo))
    }

    fn ahead_behind(&self, repo: &Path) -> Result<Option<(u64, u64)>, GitError> {
        let counts = match run(
            &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
            Some(repo),
        ) {
            Ok(out) => out,
            Err(GitError::Command { .. }) => return Ok(None),
            Err(err) => return Err(err),
        };
        let mut parts = counts.split_whitespace();
        match (
            parts.next().and_then(|n| n.parse().ok()),
            parts.next().and_then(|n| n.parse().ok()),
        ) {
            (Some(ahead), Some(behind)) => Ok(Some((ahead, behind))),
            _ => Ok(None),
        }
    }

    fn current_branch(&self, repo: &Path) -> Result<Option<String>, GitError> {
        match run(&["symbolic-ref", "--short", "-q", "HEAD"], Some(repo)) {
            Ok(branch) => Ok(Some(branch)),
            Err(GitError::Command { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn is_dirty(&self, repo: &Path) -> Result<bool, GitError> {
        Ok(!run(&["status", "--porcelain"], Some(repo))?.is_empty())
    }

    fn is_repo(&self, repo: &Path) -> bool {
        repo.join(".git").exists()
    }
}
