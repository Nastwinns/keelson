//! Production [`MergeBackend`]: shell-out to the user's `git`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{MergeBackend, MergeError, Side};

/// The default merge backend: runs `git` from PATH, prompts disabled.
#[derive(Debug, Clone, Copy, Default)]
pub struct GitMerge;

fn git(args: &[&str], cwd: &Path) -> Result<String, MergeError> {
    let output = Command::new("git")
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(cwd)
        .args(args)
        .output()
        .map_err(MergeError::Spawn)?;
    if !output.status.success() {
        return Err(MergeError::Command {
            context: format!("git {}", args.join(" ")),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Outcome of a git run that is allowed to exit non-zero (e.g. `merge`, which
/// exits 1 on conflict).
struct GitRun {
    ok: bool,
    stdout: String,
    stderr: String,
}

/// Run `git`, never erroring on a non-zero exit.
fn git_status(args: &[&str], cwd: &Path) -> Result<GitRun, MergeError> {
    let output = Command::new("git")
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(cwd)
        .args(args)
        .output()
        .map_err(MergeError::Spawn)?;
    Ok(GitRun {
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

fn lines_to_paths(out: &str) -> Vec<PathBuf> {
    out.lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

impl MergeBackend for GitMerge {
    fn current_branch(&self, repo: &Path) -> Result<Option<String>, MergeError> {
        let run = git_status(&["symbolic-ref", "--short", "-q", "HEAD"], repo)?;
        Ok(if run.ok && !run.stdout.is_empty() {
            Some(run.stdout)
        } else {
            None
        })
    }

    fn head_sha(&self, repo: &Path) -> Result<String, MergeError> {
        git(&["rev-parse", "HEAD"], repo)
    }

    fn is_dirty(&self, repo: &Path) -> Result<bool, MergeError> {
        Ok(!git(&["status", "--porcelain"], repo)?.is_empty())
    }

    fn branch_exists(&self, repo: &Path, name: &str) -> Result<bool, MergeError> {
        let reference = format!("refs/heads/{name}");
        let run = git_status(&["rev-parse", "--verify", "--quiet", &reference], repo)?;
        Ok(run.ok)
    }

    fn create_branch_at(&self, repo: &Path, name: &str, start: &str) -> Result<(), MergeError> {
        git(&["checkout", "-b", name, start], repo)?;
        Ok(())
    }

    fn switch_branch(&self, repo: &Path, name: &str) -> Result<(), MergeError> {
        git(&["checkout", name], repo)?;
        Ok(())
    }

    fn start_merge(&self, repo: &Path, source: &str) -> Result<Vec<PathBuf>, MergeError> {
        let run = git_status(&["merge", "--no-commit", "--no-ff", source], repo)?;
        if run.ok {
            return Ok(Vec::new());
        }
        if !self.merge_in_progress(repo)? {
            let detail = if run.stderr.is_empty() {
                run.stdout
            } else {
                run.stderr
            };
            return Err(MergeError::Command {
                context: format!("git merge --no-commit --no-ff {source}"),
                stderr: detail,
            });
        }
        self.conflicted_paths(repo)
    }

    fn conflicted_paths(&self, repo: &Path) -> Result<Vec<PathBuf>, MergeError> {
        let out = git(&["diff", "--name-only", "--diff-filter=U"], repo)?;
        Ok(lines_to_paths(&out))
    }

    fn take_side(&self, repo: &Path, path: &Path, side: Side) -> Result<(), MergeError> {
        let flag = match side {
            Side::Ours => "--ours",
            Side::Theirs => "--theirs",
        };
        let path_str = path.to_string_lossy();
        git(&["checkout", flag, "--", &path_str], repo)?;
        git(&["add", "--", &path_str], repo)?;
        Ok(())
    }

    fn stage(&self, repo: &Path, paths: &[PathBuf]) -> Result<(), MergeError> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut args = vec!["add", "--"];
        let owned: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        args.extend(owned.iter().map(String::as_str));
        git(&args, repo)?;
        Ok(())
    }

    fn commit(&self, repo: &Path, message: Option<&str>) -> Result<(), MergeError> {
        match message {
            Some(msg) => git(&["commit", "-m", msg], repo)?,
            None => git(&["commit", "--no-edit"], repo)?,
        };
        Ok(())
    }

    fn merge_in_progress(&self, repo: &Path) -> Result<bool, MergeError> {
        let git_dir = git(&["rev-parse", "--git-dir"], repo)?;
        Ok(repo.join(&git_dir).join("MERGE_HEAD").exists())
    }

    fn abort_merge(&self, repo: &Path) -> Result<(), MergeError> {
        git(&["merge", "--abort"], repo)?;
        Ok(())
    }

    fn fast_forward(&self, repo: &Path, from: &str) -> Result<(), MergeError> {
        git(&["merge", "--ff-only", from], repo)?;
        Ok(())
    }

    fn delete_branch(&self, repo: &Path, name: &str) -> Result<(), MergeError> {
        git(&["branch", "-D", name], repo)?;
        Ok(())
    }
}
