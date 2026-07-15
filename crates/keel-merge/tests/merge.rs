#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use keel_merge::git::GitMerge;
use keel_merge::{self, MergeError, Side};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "Keelson Test")
        .env("GIT_AUTHOR_EMAIL", "test@keelson.dev")
        .env("GIT_COMMITTER_NAME", "Keelson Test")
        .env("GIT_COMMITTER_EMAIL", "test@keelson.dev")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(repo: &Path, rel: &str, body: &str) {
    let path = repo.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A repo with `main` and a `feature` branch that both edit the same lines in
/// two directories, so merging `feature` into `main` conflicts in `src/` and
/// `docs/`.
fn conflicting_repo(root: &Path) -> PathBuf {
    let repo = init_repo(root);

    write(&repo, "src/lib.rs", "fn main() {}\n");
    write(&repo, "docs/readme.md", "hello\n");
    write(&repo, "LICENSE", "MIT\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);

    git(&repo, &["checkout", "-b", "feature"]);
    write(&repo, "src/lib.rs", "fn main() { feature(); }\n");
    write(&repo, "docs/readme.md", "hello from feature\n");
    git(&repo, &["commit", "-am", "feature work"]);

    git(&repo, &["checkout", "main"]);
    write(&repo, "src/lib.rs", "fn main() { mainline(); }\n");
    write(&repo, "docs/readme.md", "hello from main\n");
    git(&repo, &["commit", "-am", "mainline work"]);

    repo
}

// ---- pure slicing --------------------------------------------------------

#[test]
fn slices_group_by_top_level_component() {
    let paths = vec![
        PathBuf::from("src/b.rs"),
        PathBuf::from("src/a.rs"),
        PathBuf::from("docs/x.md"),
        PathBuf::from("LICENSE"),
    ];
    let slices = keel_merge::slice_conflicts(&paths);
    let names: Vec<&str> = slices.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["docs", "root", "src"],
        "sorted, root for top file"
    );

    let src = slices.iter().find(|s| s.name == "src").unwrap();
    assert_eq!(
        src.paths,
        vec![PathBuf::from("src/a.rs"), PathBuf::from("src/b.rs")],
        "paths within a slice are sorted"
    );
    assert!(slices.iter().all(|s| !s.resolved));
}

#[test]
fn integration_branch_name_is_sanitized() {
    assert_eq!(
        keel_merge::integration_branch("release/2.x"),
        "keel/merge/release-2.x"
    );
}

// ---- end to end against real git ----------------------------------------

#[test]
fn plan_slices_the_conflicts() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = conflicting_repo(tmp.path());
    let state = tmp.path().join(".keel");

    let plan = keel_merge::plan(&GitMerge, &repo, &state, "repo", "feature", None).unwrap();

    assert_eq!(plan.source, "feature");
    assert_eq!(plan.target, "main");
    assert_eq!(plan.integration, "keel/merge/feature");
    let names: Vec<&str> = plan.slices.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["docs", "src"], "two conflicting directories");
    assert!(state.join("merge").join("repo.toml").exists());
    assert_eq!(
        GitMerge_branch(&repo),
        "keel/merge/feature",
        "merge runs on the integration branch, not main"
    );
}

#[test]
fn resolve_each_slice_then_cleanup_lands_one_merge_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = conflicting_repo(tmp.path());
    let state = tmp.path().join(".keel");
    let main_before = git(&repo, &["rev-parse", "main"]);

    keel_merge::plan(&GitMerge, &repo, &state, "repo", "feature", None).unwrap();

    // Take theirs for src, ours for docs.
    let after_src =
        keel_merge::resolve(&GitMerge, &repo, &state, "repo", "src", Some(Side::Theirs)).unwrap();
    assert!(
        after_src
            .slices
            .iter()
            .find(|s| s.name == "src")
            .unwrap()
            .resolved
    );

    // Cleanup must refuse while docs is still unresolved.
    let err = keel_merge::cleanup(&GitMerge, &repo, &state, "repo", None).unwrap_err();
    assert!(matches!(err, MergeError::Unresolved(ref v) if v == &vec!["docs".to_string()]));

    keel_merge::resolve(&GitMerge, &repo, &state, "repo", "docs", Some(Side::Ours)).unwrap();

    let report =
        keel_merge::cleanup(&GitMerge, &repo, &state, "repo", Some("merge feature")).unwrap();
    assert_eq!(report.target, "main");
    assert_eq!(report.slices, 2);

    // Back on main, fast-forwarded to the sealed merge commit.
    assert_eq!(GitMerge_branch(&repo), "main");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), report.merge_sha);

    // Resolutions applied: src took feature, docs kept main.
    assert_eq!(
        std::fs::read_to_string(repo.join("src/lib.rs")).unwrap(),
        "fn main() { feature(); }\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("docs/readme.md")).unwrap(),
        "hello from main\n"
    );

    // It is a real merge commit with two parents, and main moved forward.
    assert_eq!(
        git(&repo, &["rev-list", "--count", "HEAD^@"])
            .lines()
            .count(),
        1
    );
    assert_ne!(git(&repo, &["rev-parse", "main"]), main_before);
    let parents = git(&repo, &["rev-list", "--parents", "-n", "1", "HEAD"]);
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "merge commit has 2 parents"
    );

    // Integration branch cleaned up, plan cleared.
    assert!(!branch_exists(&repo, "keel/merge/feature"));
    assert!(keel_merge::load_plan(&state, "repo").unwrap().is_none());
}

#[test]
fn abort_restores_main_and_clears_state() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = conflicting_repo(tmp.path());
    let state = tmp.path().join(".keel");
    let main_before = git(&repo, &["rev-parse", "main"]);

    keel_merge::plan(&GitMerge, &repo, &state, "repo", "feature", None).unwrap();
    assert_eq!(GitMerge_branch(&repo), "keel/merge/feature");

    keel_merge::abort(&GitMerge, &repo, &state, "repo").unwrap();

    assert_eq!(GitMerge_branch(&repo), "main");
    assert_eq!(git(&repo, &["rev-parse", "main"]), main_before);
    assert!(!branch_exists(&repo, "keel/merge/feature"));
    assert!(keel_merge::load_plan(&state, "repo").unwrap().is_none());
}

#[test]
fn plan_refused_when_already_planned() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = conflicting_repo(tmp.path());
    let state = tmp.path().join(".keel");

    keel_merge::plan(&GitMerge, &repo, &state, "repo", "feature", None).unwrap();
    let err = keel_merge::plan(&GitMerge, &repo, &state, "repo", "feature", None).unwrap_err();
    assert!(matches!(err, MergeError::PlanExists(_)));
}

#[test]
fn plan_rejects_already_merged_source() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = root_repo(tmp.path());
    let state = tmp.path().join(".keel");
    // `stale` branch points at an ancestor of main => nothing to merge.
    git(&repo, &["branch", "stale", "HEAD"]);
    write(&repo, "src/lib.rs", "fn main() { more(); }\n");
    git(&repo, &["commit", "-am", "advance main"]);

    let err = keel_merge::plan(&GitMerge, &repo, &state, "repo", "stale", None).unwrap_err();
    assert!(matches!(err, MergeError::NothingToMerge { .. }));
    assert_eq!(
        GitMerge_branch(&repo),
        "main",
        "integration branch cleaned up"
    );
    assert!(!branch_exists(&repo, "keel/merge/stale"));
}

// ---- helpers -------------------------------------------------------------

#[allow(non_snake_case)]
fn GitMerge_branch(repo: &Path) -> String {
    git(repo, &["symbolic-ref", "--short", "HEAD"])
}

fn branch_exists(repo: &Path, name: &str) -> bool {
    Command::new("git")
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{name}"),
        ])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success()
}

/// `git init` plus a repo-local identity, so keel's own git invocations
/// (which inherit no test env) work on CI runners without a global config.
fn init_repo(root: &Path) -> PathBuf {
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@keelson.dev"]);
    git(&repo, &["config", "user.name", "Keelson Test"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    repo
}

fn root_repo(root: &Path) -> PathBuf {
    let repo = init_repo(root);
    write(&repo, "src/lib.rs", "fn main() {}\n");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    repo
}
