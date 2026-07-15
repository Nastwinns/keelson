#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use haw_core::git::{GitBackend, GitError, RevKind};
use haw_git::ShellGit;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn make_source_repo(root: &Path) -> PathBuf {
    let src = root.join("source");
    std::fs::create_dir_all(&src).unwrap();
    git(&src, &["init", "-b", "main"]);
    git(&src, &["config", "user.email", "test@keelson.dev"]);
    git(&src, &["config", "user.name", "Keelson Test"]);
    std::fs::write(src.join("README.md"), "hello\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-m", "initial"]);
    git(&src, &["tag", "-a", "v1", "-m", "release v1"]);
    src
}

#[test]
fn resolves_branch_tag_and_sha() {
    let tmp = tempfile::tempdir().unwrap();
    let src = make_source_repo(tmp.path());
    let url = src.to_string_lossy().into_owned();
    let head = git(&src, &["rev-parse", "main"]);

    let branch = ShellGit.resolve_rev(&url, "main").unwrap();
    assert_eq!(branch.sha, head);
    assert_eq!(branch.kind, RevKind::Branch);

    let tag = ShellGit.resolve_rev(&url, "v1").unwrap();
    assert_eq!(
        tag.sha, head,
        "annotated tag must resolve to the peeled commit"
    );
    assert_eq!(tag.kind, RevKind::Tag);

    let sha = ShellGit.resolve_rev(&url, &head).unwrap();
    assert_eq!(sha.sha, head);
    assert_eq!(sha.kind, RevKind::Sha);

    let missing = ShellGit.resolve_rev(&url, "does-not-exist");
    assert!(matches!(missing, Err(GitError::RevNotFound { .. })));
}

#[test]
fn clone_checkout_and_introspect() {
    let tmp = tempfile::tempdir().unwrap();
    let src = make_source_repo(tmp.path());
    let url = src.to_string_lossy().into_owned();
    let head = git(&src, &["rev-parse", "main"]);
    let dest = tmp.path().join("clones").join("repo");

    assert!(!ShellGit.is_repo(&dest));
    ShellGit.clone_repo(&url, &dest, None).unwrap();
    assert!(ShellGit.is_repo(&dest));

    ShellGit.checkout(&dest, &head, "main").unwrap();
    assert_eq!(ShellGit.head_sha(&dest).unwrap(), head);
    assert_eq!(
        ShellGit.current_branch(&dest).unwrap().as_deref(),
        Some("main")
    );
    assert!(!ShellGit.is_dirty(&dest).unwrap());

    ShellGit.checkout(&dest, &head, "haw/v1").unwrap();
    assert_eq!(
        ShellGit.current_branch(&dest).unwrap().as_deref(),
        Some("haw/v1"),
        "tag pins check out on a real haw/ branch, never detached"
    );

    std::fs::write(dest.join("scratch.txt"), "wip\n").unwrap();
    assert!(ShellGit.is_dirty(&dest).unwrap());
}

#[test]
fn refuses_to_discard_local_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let src = make_source_repo(tmp.path());
    let url = src.to_string_lossy().into_owned();
    let old = git(&src, &["rev-parse", "main"]);
    let dest = tmp.path().join("repo");

    ShellGit.clone_repo(&url, &dest, None).unwrap();
    ShellGit.checkout(&dest, &old, "main").unwrap();

    git(&dest, &["config", "user.email", "test@keelson.dev"]);
    git(&dest, &["config", "user.name", "Keelson Test"]);
    std::fs::write(dest.join("local.txt"), "local work\n").unwrap();
    git(&dest, &["add", "."]);
    git(&dest, &["commit", "-m", "local only"]);

    let err = ShellGit.checkout(&dest, &old, "main").unwrap_err();
    assert!(matches!(err, GitError::LocalCommits { count: 1, .. }));
}

#[test]
fn shared_clone_references_the_mirror() {
    let tmp = tempfile::tempdir().unwrap();
    let src = make_source_repo(tmp.path());
    let url = src.to_string_lossy().into_owned();
    let mirror = haw_core::git::mirror_dir(&tmp.path().join("cache"), &url);
    let dest = tmp.path().join("repo");

    ShellGit.ensure_mirror(&url, &mirror).unwrap();
    assert!(mirror.join("HEAD").exists(), "mirror is a bare repo");
    ShellGit.ensure_mirror(&url, &mirror).unwrap();

    ShellGit.clone_repo(&url, &dest, Some(&mirror)).unwrap();
    let alternates = dest
        .join(".git")
        .join("objects")
        .join("info")
        .join("alternates");
    assert!(
        alternates.exists(),
        "shared clone records the mirror in objects/info/alternates (a text file)"
    );
}

#[test]
fn create_branch_and_fetch() {
    let tmp = tempfile::tempdir().unwrap();
    let src = make_source_repo(tmp.path());
    let url = src.to_string_lossy().into_owned();
    let dest = tmp.path().join("repo");

    ShellGit.clone_repo(&url, &dest, None).unwrap();
    ShellGit.create_branch(&dest, "change/FEAT-1").unwrap();
    assert_eq!(
        ShellGit.current_branch(&dest).unwrap().as_deref(),
        Some("change/FEAT-1")
    );
    ShellGit.fetch(&dest).unwrap();
}
