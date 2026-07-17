//! Production [`GitBackend`]: shell-out to the user's `git`.
//!
//! Mutations always shell out (correctness, credential helpers, hooks).
//! Reads shell out too for now; gitoxide (`gix`) replaces them later behind
//! the same trait without touching callers.

pub mod parallel;

use std::path::Path;
use std::process::Command;

use haw_core::git::{CloneOpts, GitBackend, GitError, ResolvedRev, RevKind};

/// The default backend: runs `git` from PATH, prompts disabled.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShellGit;

/// Platform cache directory for shared bare mirrors (`--shared` mode).
pub fn default_cache_root() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("dev", "hawser", "hawser")
        .map(|dirs| dirs.cache_dir().join("mirrors"))
}

fn git_command(cwd: Option<&Path>) -> Command {
    let mut cmd = Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    // Belt-and-suspenders against RCE via a hostile `url` (git-remote-ext and
    // friends). Even if a bad url slips past validation, git refuses to run
    // remote-helper/local transports: only the user-space transports
    // (http/https/ssh/git) are allowed, and the `ext`/`fd` protocols that
    // execute commands are hard-disabled.
    cmd.arg("-c").arg("protocol.allow=never");
    // Local-path clones (`/path/to/repo`) are a supported source form; allow the
    // file transport when the user invokes it directly, but never via recursion
    // (submodules) where a hostile superproject could point it at anything.
    cmd.arg("-c").arg("protocol.file.allow=user");
    cmd.arg("-c").arg("protocol.http.allow=user");
    cmd.arg("-c").arg("protocol.https.allow=user");
    cmd.arg("-c").arg("protocol.ssh.allow=user");
    cmd.arg("-c").arg("protocol.git.allow=user");
    cmd.arg("-c").arg("protocol.ext.allow=never");
    cmd.arg("-c").arg("protocol.fd.allow=never");
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

/// A `git` command for submodule init/update. Submodule URLs come from a repo's
/// `.gitmodules` (not user-initiated), so the plain `git_command` allowlist
/// (`protocol.<x>.allow=user`) would refuse to fetch them. Allow the safe network
/// transports for the nested clone while STILL hard-disabling the command-executing
/// (`ext`/`fd`) and local (`file`) transports — those remain the RCE surface.
fn git_command_submodule(cwd: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.arg("-c").arg("protocol.allow=never");
    cmd.arg("-c").arg("protocol.http.allow=always");
    cmd.arg("-c").arg("protocol.https.allow=always");
    cmd.arg("-c").arg("protocol.ssh.allow=always");
    cmd.arg("-c").arg("protocol.git.allow=always");
    cmd.arg("-c").arg("protocol.file.allow=never");
    cmd.arg("-c").arg("protocol.ext.allow=never");
    cmd.arg("-c").arg("protocol.fd.allow=never");
    cmd.current_dir(cwd);
    cmd
}

fn is_full_sha(rev: &str) -> bool {
    rev.len() == 40 && rev.chars().all(|c| c.is_ascii_hexdigit())
}

/// Build the `git clone` argv (everything after `git`) for `opts`.
///
/// `--reference` (shared mirror), `--filter=<spec>` (partial clone), and
/// `--depth <N>` (shallow clone) are independent and compose. Extracted so the
/// argument order is unit-testable without spawning git.
fn clone_argv(url: &str, dest: &Path, opts: &CloneOpts) -> Vec<std::ffi::OsString> {
    use std::ffi::OsString;
    let mut argv: Vec<OsString> = vec!["clone".into()];
    if let Some(mirror) = &opts.reference {
        argv.push("--reference".into());
        argv.push(mirror.into());
    }
    // Partial clone: keeps all commits (any locked SHA stays reachable), fetch
    // blobs/trees lazily. Safe for pinned revs.
    if let Some(filter) = &opts.filter {
        argv.push(format!("--filter={filter}").into());
    }
    // Shallow clone: truncate history to N commits. Smaller, but an old locked
    // SHA may fall outside it — recovered at checkout time.
    if let Some(depth) = opts.depth {
        argv.push("--depth".into());
        argv.push(depth.to_string().into());
    }
    // NOTE: submodules are intentionally NOT recursed at clone time. `git clone
    // --recurse-submodules` is all-or-nothing: one broken/unreachable submodule
    // aborts the whole clone. Instead, when `opts.submodules` is set, `sync_repo`
    // runs the tolerant `update_submodules` AFTER the superproject is cloned, so a
    // single bad submodule is skipped with a warning rather than failing the sync.
    let _ = opts.submodules;
    // Terminate option parsing so a `url` beginning with `-` can never be
    // interpreted by git as an option (e.g. `--upload-pack=…`).
    argv.push("--".into());
    argv.push(url.into());
    argv.push(dest.into());
    argv
}

/// True when `sha` names a commit object already in `repo`.
fn sha_present(repo: &Path, sha: &str) -> bool {
    run(
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{sha}^{{commit}}"),
        ],
        Some(repo),
    )
    .is_ok()
}

/// Make `sha` reachable in a shallow `repo`, deepening progressively.
///
/// Reproducibility recovery for `--depth` clones. The locked SHA can lie
/// outside the truncated history; without this the checkout would fail or,
/// worse, land on the wrong commit. Steps, cheapest first:
/// 1. If the SHA is already present, do nothing.
/// 2. `git fetch --depth <N> origin <sha>` — a targeted deepen (works when the
///    server honors want-sha uploads; most do).
/// 3. `git fetch --unshallow` — last resort, converts to a full history.
///
/// Emits a clear message whenever a deepen/unshallow was needed so the cost of
/// a shallow clone against an old pin is visible, never silent.
fn ensure_sha_present(repo: &Path, sha: &str, depth: Option<u32>) -> Result<(), GitError> {
    if sha_present(repo, sha) {
        return Ok(());
    }
    // Targeted deepen: ask the server for exactly this commit.
    let depth_arg = depth.unwrap_or(1).to_string();
    let targeted = run(&["fetch", "--depth", &depth_arg, "origin", sha], Some(repo));
    if targeted.is_ok() && sha_present(repo, sha) {
        eprintln!(
            "note: deepened shallow clone to reach locked SHA {}",
            &sha[..12.min(sha.len())]
        );
        return Ok(());
    }
    // Last resort: unshallow to a full history.
    run(&["fetch", "--unshallow"], Some(repo))?;
    if sha_present(repo, sha) {
        eprintln!(
            "note: unshallowed clone to reach locked SHA {}",
            &sha[..12.min(sha.len())]
        );
        return Ok(());
    }
    Err(GitError::Command {
        context: format!("locate {sha} after deepen/unshallow"),
        stderr: "locked SHA not reachable even after unshallowing the clone".to_string(),
    })
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
        let out = run(&["ls-remote", "--heads", "--tags", "--", url], None)?;

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

    fn clone_repo(&self, url: &str, dest: &Path, opts: &CloneOpts) -> Result<(), GitError> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut cmd = git_command(None);
        cmd.args(clone_argv(url, dest, opts));
        let output = cmd.output()?;
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
            .arg("--")
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

    fn checkout(
        &self,
        repo: &Path,
        sha: &str,
        branch: &str,
        shallow_depth: Option<u32>,
    ) -> Result<(), GitError> {
        // Shallow clones may not contain an old locked SHA. Ensure it is
        // present before we try to branch onto it, deepening or unshallowing
        // as needed (and telling the user when we had to).
        if shallow_depth.is_some() {
            ensure_sha_present(repo, sha, shallow_depth)?;
        }
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

    fn update_submodules(&self, repo: &Path) -> Result<(), GitError> {
        // Tolerant, per-submodule init. `git submodule update --init --recursive`
        // is all-or-nothing — one broken/unreachable submodule (common in big
        // upstream repos like FreeRTOS: wolfSSL, AWS IoT SDKs, …) aborts the whole
        // update. We instead update each top-level submodule independently and
        // SKIP the ones that fail, so the fleet sync still succeeds.
        if !repo.join(".gitmodules").exists() {
            return Ok(()); // nothing to do
        }
        // Refresh submodule URLs from .gitmodules into .git/config.
        let _ = git_command_submodule(repo)
            .args(["submodule", "sync", "--recursive"])
            .output();
        // List declared submodule paths from .gitmodules.
        let listing = git_command_submodule(repo)
            .args([
                "config",
                "--file",
                ".gitmodules",
                "--get-regexp",
                r"^submodule\..*\.path$",
            ])
            .output()?;
        let stdout = String::from_utf8_lossy(&listing.stdout);
        let paths: Vec<String> = stdout
            .lines()
            .filter_map(|line| line.split_once(' ').map(|(_, p)| p.trim().to_string()))
            .filter(|p| !p.is_empty())
            .collect();
        for path in paths {
            let out = git_command_submodule(repo)
                .args(["submodule", "update", "--init", "--recursive", "--", &path])
                .output()?;
            if !out.status.success() {
                eprintln!(
                    "haw: skipped submodule '{path}': {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
        }
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn argv_strings(url: &str, opts: &CloneOpts) -> Vec<String> {
        clone_argv(url, Path::new("/tmp/dest"), opts)
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn plain_clone_has_no_mode_flags() {
        let argv = argv_strings("https://x/y.git", &CloneOpts::none());
        assert_eq!(argv, vec!["clone", "--", "https://x/y.git", "/tmp/dest"]);
    }

    #[test]
    fn double_dash_precedes_url() {
        // The `--` option terminator must sit immediately before the url so a
        // url beginning with `-` can never be parsed by git as an option.
        let argv = argv_strings("https://x/y.git", &CloneOpts::none());
        let dd = argv.iter().position(|a| a == "--").expect("--");
        assert_eq!(argv[dd + 1], "https://x/y.git");
    }

    #[test]
    fn double_dash_precedes_url_with_all_levers() {
        let opts = CloneOpts {
            reference: Some(PathBuf::from("/m.git")),
            filter: Some("blob:none".to_string()),
            depth: Some(1),
            submodules: true,
        };
        let argv = argv_strings("https://x/y.git", &opts);
        let dd = argv.iter().position(|a| a == "--").expect("--");
        assert_eq!(argv[dd + 1], "https://x/y.git");
        assert_eq!(argv[dd + 2], "/tmp/dest");
    }

    #[test]
    fn filter_reaches_git_argv() {
        let opts = CloneOpts {
            filter: Some("blob:none".to_string()),
            ..CloneOpts::none()
        };
        let argv = argv_strings("u", &opts);
        assert!(
            argv.contains(&"--filter=blob:none".to_string()),
            "argv = {argv:?}"
        );
        assert!(!argv.iter().any(|a| a == "--depth"));
    }

    #[test]
    fn depth_reaches_git_argv() {
        let opts = CloneOpts {
            depth: Some(1),
            ..CloneOpts::none()
        };
        let argv = argv_strings("u", &opts);
        let i = argv.iter().position(|a| a == "--depth").expect("--depth");
        assert_eq!(argv[i + 1], "1");
    }

    #[test]
    fn submodules_not_recursed_at_clone_time() {
        // `--recurse-submodules` is all-or-nothing (a broken submodule aborts the
        // clone), so we deliberately keep it OUT of the clone argv and handle
        // submodules with the tolerant post-clone `update_submodules` instead.
        let opts = CloneOpts {
            submodules: true,
            ..CloneOpts::none()
        };
        let argv = argv_strings("u", &opts);
        assert!(
            !argv.iter().any(|a| a == "--recurse-submodules"),
            "argv = {argv:?}"
        );
    }

    #[test]
    fn no_submodules_flag_absent_by_default() {
        let argv = argv_strings("u", &CloneOpts::none());
        assert!(!argv.iter().any(|a| a == "--recurse-submodules"));
    }

    #[test]
    fn reference_still_present_alongside_filter() {
        // Shared mode composes with partial clone.
        let opts = CloneOpts {
            reference: Some(PathBuf::from("/cache/mirror.git")),
            filter: Some("blob:none".to_string()),
            depth: None,
            ..CloneOpts::none()
        };
        let argv = argv_strings("u", &opts);
        let i = argv
            .iter()
            .position(|a| a == "--reference")
            .expect("--reference");
        assert_eq!(argv[i + 1], "/cache/mirror.git");
        assert!(argv.contains(&"--filter=blob:none".to_string()));
    }

    #[test]
    fn all_three_levers_compose() {
        let opts = CloneOpts {
            reference: Some(PathBuf::from("/m.git")),
            filter: Some("tree:0".to_string()),
            depth: Some(2),
            ..CloneOpts::none()
        };
        let argv = argv_strings("u", &opts);
        assert!(argv.contains(&"--reference".to_string()));
        assert!(argv.contains(&"--filter=tree:0".to_string()));
        assert!(argv.contains(&"--depth".to_string()));
    }
}
