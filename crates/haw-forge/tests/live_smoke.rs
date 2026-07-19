//! Gated live-forge smoke test — hits a REAL public forge to catch silent
//! forge-API drift (pagination, 302 log redirects, raw-content Accept headers,
//! JSON shape changes) that the JSON-fake suite can't see.
//!
//! It is `#[ignore]` by default AND guarded by the `HAW_LIVE_FORGE` env var, so
//! a plain `cargo test` / CI run never runs it and never touches the network.
//!
//! Run it explicitly with:
//!
//! ```sh
//! HAW_LIVE_FORGE=1 cargo test -p haw-forge --test live_smoke -- --ignored
//! ```
//!
//! The reads target a public repo, but GitHub's API rejects an *empty* bearer
//! token with 401 and rate-limits anonymous callers hard, so the client is
//! built from a token discovered in the environment (`GITHUB_TOKEN` / `GH_TOKEN`,
//! or `gh auth token`). If the live guard is set but no token is found, the
//! test skips cleanly with a note rather than failing on auth.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use haw_forge::{Forge, github::GitHub};

/// A stable, long-lived public GitHub repo + PR used for read-only smoke checks.
/// `octocat/Hello-World` is GitHub's own demo repo; PR #1 has been closed and
/// immutable for years, giving a deterministic target.
const REPO_URL: &str = "https://github.com/octocat/Hello-World";
const PR_NUMBER: u64 = 1;

/// Returns `true` when the env guard opts this run into hitting the network.
/// Prints a skip note and returns `false` otherwise, so an accidental
/// `--ignored` run without the guard exits cleanly instead of failing.
fn live_enabled() -> bool {
    match std::env::var("HAW_LIVE_FORGE") {
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") => true,
        _ => {
            eprintln!(
                "live_smoke: HAW_LIVE_FORGE not set to 1 — skipping (network guard). \
                 Run with: HAW_LIVE_FORGE=1 cargo test -p haw-forge --test live_smoke -- --ignored"
            );
            false
        }
    }
}

/// Discover a GitHub token for public reads: env first, then `gh auth token`.
/// Returns `None` (test skips) when nothing is available.
fn github_token() -> Option<String> {
    for var in ["GITHUB_TOKEN", "GH_TOKEN"] {
        if let Ok(tok) = std::env::var(var) {
            let tok = tok.trim().to_string();
            if !tok.is_empty() {
                return Some(tok);
            }
        }
    }
    // Fall back to the `gh` CLI's stored credential, if installed + logged in.
    let out = std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let tok = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if tok.is_empty() { None } else { Some(tok) }
}

/// Build the GitHub client, or `None` (skip) when no token is discoverable —
/// GitHub rejects empty-bearer requests with 401, so a token is required.
fn github() -> Option<GitHub> {
    match github_token() {
        Some(tok) => Some(GitHub::new(tok).expect("construct GitHub forge")),
        None => {
            eprintln!(
                "live_smoke: no GitHub token (GITHUB_TOKEN / GH_TOKEN / `gh auth token`) — \
                 skipping; GitHub's API rejects anonymous/empty-token reads."
            );
            None
        }
    }
}

#[test]
#[ignore = "hits the live GitHub API; guarded by HAW_LIVE_FORGE=1"]
fn github_pr_files_parse() {
    if !live_enabled() {
        return;
    }
    let Some(gh) = github() else { return };
    let files = gh
        .pr_files(REPO_URL, PR_NUMBER)
        .expect("pr_files against a real public PR");
    assert!(!files.is_empty(), "expected the PR to report changed files");
    for f in &files {
        assert!(!f.path.is_empty(), "PrFile.path should be populated");
        assert!(
            matches!(
                f.status.as_str(),
                "added" | "modified" | "removed" | "renamed"
            ),
            "unexpected PrFile.status {:?} — forge status mapping drifted",
            f.status
        );
    }
    eprintln!("live_smoke: pr_files returned {} file(s)", files.len());
}

#[test]
#[ignore = "hits the live GitHub API; guarded by HAW_LIVE_FORGE=1"]
fn github_pr_file_content_and_diff_parse() {
    if !live_enabled() {
        return;
    }
    let Some(gh) = github() else { return };

    // Grab the first changed file and fetch its raw content at the PR head ref.
    // This exercises the raw-content Accept header + head-sha resolution path.
    let files = gh.pr_files(REPO_URL, PR_NUMBER).expect("pr_files");
    let first = files.first().expect("PR has at least one file");
    let content = gh
        .pr_file_content(REPO_URL, PR_NUMBER, &first.path)
        .expect("pr_file_content");
    assert!(
        !content.is_empty(),
        "pr_file_content returned nothing for {:?}",
        first.path
    );

    // The unified diff should mention at least one changed path.
    let diff = gh.pr_diff(REPO_URL, PR_NUMBER).expect("pr_diff");
    assert!(
        diff.contains("diff --git") || diff.contains("+++") || diff.contains("@@"),
        "pr_diff output doesn't look like a unified diff:\n{diff}"
    );
    eprintln!(
        "live_smoke: pr_file_content {} byte(s), pr_diff {} byte(s)",
        content.len(),
        diff.len()
    );
}

#[test]
#[ignore = "hits the live GitHub API; guarded by HAW_LIVE_FORGE=1"]
fn github_list_ci_runs_parse() {
    if !live_enabled() {
        return;
    }
    let Some(gh) = github() else { return };
    // `octocat/Hello-World` has no Actions, so this legitimately returns an
    // empty list — the point here is that the Actions endpoint + pagination
    // still parse without error. A repo with Actions would return non-empty.
    let runs = gh
        .list_ci_runs(REPO_URL)
        .expect("list_ci_runs must parse the Actions API response");
    for run in &runs {
        assert!(!run.name.is_empty(), "CiRun.name should be populated");
        assert!(!run.url.is_empty(), "CiRun.url should be populated");
    }
    eprintln!("live_smoke: list_ci_runs returned {} run(s)", runs.len());
}

#[test]
#[ignore = "hits the live GitHub API; guarded by HAW_LIVE_FORGE=1"]
fn github_list_refs_and_file_paths_parse() {
    if !live_enabled() {
        return;
    }
    let Some(gh) = github() else { return };
    let refs = gh
        .list_refs(REPO_URL)
        .expect("list_refs must parse the branches+tags API responses");
    assert!(
        refs.iter().any(|r| r.name == "master"),
        "octocat/Hello-World has a `master` branch: {refs:?}"
    );
    let paths = gh
        .repo_file_paths(REPO_URL, None)
        .expect("repo_file_paths must parse the recursive-tree API response");
    assert!(
        paths.iter().any(|p| p == "README"),
        "octocat/Hello-World has a top-level README: {paths:?}"
    );
    eprintln!(
        "live_smoke: list_refs={} refs, repo_file_paths={} paths",
        refs.len(),
        paths.len()
    );
}
