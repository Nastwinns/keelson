#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use keel_core::git::RevKind;
use keel_core::lock::{LOCK_VERSION, LockError, LockedRepo, Lockfile};
use keel_core::manifest::Manifest;
use keel_core::resolver;
use keel_core::workspace::branch_for;

fn sample_lock() -> Lockfile {
    Lockfile {
        version: LOCK_VERSION,
        repos: vec![LockedRepo {
            name: "kernel".into(),
            url: "git@gitlab.company.com:firmware/kernel.git".into(),
            path: PathBuf::from("kernel"),
            rev: "a".repeat(40),
            source_rev: "v6.1.2".into(),
            branch: "keel/v6.1.2".into(),
            groups: vec!["firmware".into()],
        }],
    }
}

#[test]
fn lockfile_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keel.lock");
    let lock = sample_lock();
    lock.save(&path).unwrap();
    let loaded = Lockfile::load(&path).unwrap();
    assert_eq!(lock, loaded);
    assert_eq!(loaded.get("kernel").unwrap().source_rev, "v6.1.2");
    assert!(loaded.get("ghost").is_none());
}

#[test]
fn lockfile_rejects_future_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keel.lock");
    std::fs::write(&path, "version = 99\n").unwrap();
    assert!(matches!(
        Lockfile::load(&path),
        Err(LockError::UnsupportedVersion(99))
    ));
}

#[test]
fn branch_policy_never_detaches() {
    assert_eq!(branch_for("main", RevKind::Branch), "main");
    assert_eq!(branch_for("v6.1.2", RevKind::Tag), "keel/v6.1.2");
    assert_eq!(branch_for("release/2.x", RevKind::Tag), "keel/release-2.x");
    let sha = "b".repeat(40);
    assert_eq!(branch_for(&sha, RevKind::Sha), format!("keel/{sha}"));
}

#[test]
fn resolve_all_covers_every_repo_with_overlays() {
    let manifest: Manifest = r#"
[remote.r]
url = "git@example.com:org"

[repo.a]
remote = "r"
repo = "a.git"
rev = "main"

[repo.b]
remote = "r"
repo = "b.git"
rev = "v1"

[stack.p]
repos = ["a"]

[overlay.dev.repo.b]
rev = "main"
"#
    .parse()
    .unwrap();

    let all = resolver::resolve_all(&manifest, &[]).unwrap();
    assert_eq!(all.len(), 2, "lock covers all repos, not just stack p");

    let dev = resolver::resolve_all(&manifest, &["dev".into()]).unwrap();
    assert_eq!(dev[1].rev, "main");
}

#[test]
fn group_filter_limits_resolution() {
    let manifest: Manifest = r#"
[remote.r]
url = "git@example.com:org"

[repo.kernel]
remote = "r"
repo = "kernel.git"
rev = "main"
groups = ["firmware"]

[repo.docs]
remote = "r"
repo = "docs.git"
rev = "main"
groups = ["docs"]

[repo.tools]
remote = "r"
repo = "tools.git"
rev = "main"

[stack.all]
repos = ["kernel", "docs", "tools"]
"#
    .parse()
    .unwrap();

    let mut res = resolver::resolve(&manifest, "all", &[]).unwrap();
    resolver::filter_groups(&mut res, &["firmware".into()]);
    assert_eq!(res.repos.len(), 1);
    assert_eq!(res.repos[0].name, "kernel");

    let mut all = resolver::resolve(&manifest, "all", &[]).unwrap();
    resolver::filter_groups(&mut all, &[]);
    assert_eq!(all.repos.len(), 3, "empty filter keeps everything");

    assert!(resolver::group_match(&[], &[]));
    assert!(
        !resolver::group_match(&[], &["firmware".into()]),
        "ungrouped repos are excluded by an active group filter"
    );
}

mod pin_tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use keel_core::git::{GitBackend, GitError, ResolvedRev, RevKind};
    use keel_core::workspace::Workspace;

    struct FakeGit {
        heads: HashMap<PathBuf, (String, Option<String>)>,
    }

    impl GitBackend for FakeGit {
        fn resolve_rev(&self, _url: &str, rev: &str) -> Result<ResolvedRev, GitError> {
            Ok(ResolvedRev {
                sha: "c".repeat(40),
                kind: if rev == "main" {
                    RevKind::Branch
                } else {
                    RevKind::Tag
                },
            })
        }
        fn clone_repo(
            &self,
            _url: &str,
            _dest: &Path,
            _reference: Option<&Path>,
        ) -> Result<(), GitError> {
            Ok(())
        }
        fn ensure_mirror(&self, _url: &str, _mirror: &Path) -> Result<(), GitError> {
            Ok(())
        }
        fn fetch(&self, _repo: &Path) -> Result<(), GitError> {
            Ok(())
        }
        fn checkout(&self, _repo: &Path, _sha: &str, _branch: &str) -> Result<(), GitError> {
            Ok(())
        }
        fn create_branch(&self, _repo: &Path, _name: &str) -> Result<(), GitError> {
            Ok(())
        }
        fn head_sha(&self, repo: &Path) -> Result<String, GitError> {
            Ok(self.heads[repo].0.clone())
        }
        fn current_branch(&self, repo: &Path) -> Result<Option<String>, GitError> {
            Ok(self.heads[repo].1.clone())
        }
        fn is_dirty(&self, _repo: &Path) -> Result<bool, GitError> {
            Ok(false)
        }
        fn is_repo(&self, repo: &Path) -> bool {
            self.heads.contains_key(repo)
        }
    }

    #[test]
    fn pin_snapshots_heads_without_network() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("keel.toml"),
            "[repo.a]\nurl = \"/r/a\"\nrev = \"main\"\n\n[stack.s]\nrepos = [\"a\"]\n",
        )
        .unwrap();
        let ws = Workspace::open(dir.path()).unwrap();

        let head = "d".repeat(40);
        let fake = FakeGit {
            heads: HashMap::from([(
                dir.path().join("a"),
                (head.clone(), Some("feature/x".to_string())),
            )]),
        };
        let lock = ws.pin(&fake).unwrap();
        assert_eq!(lock.repos.len(), 1);
        assert_eq!(lock.repos[0].rev, head);
        assert_eq!(lock.repos[0].branch, "feature/x");
        assert_eq!(lock.repos[0].source_rev, "main");
    }
}

mod edit_tests {
    #![allow(clippy::unwrap_used)]

    use keel_core::manifest::edit::{self, EditError, NewRepo};

    const BASE: &str = "# my workspace\n\n[repo.a]\nurl = \"/r/a\"   # keep me\nrev = \"main\"\n\n[stack.s]\nrepos = [\"a\"]\n";

    #[test]
    fn add_repo_preserves_comments() {
        let spec = NewRepo {
            name: "b".into(),
            url: Some("/r/b".into()),
            rev: "v1".into(),
            groups: vec!["fw".into()],
            ..Default::default()
        };
        let out = edit::add_repo(BASE, &spec).unwrap();
        assert!(out.contains("# my workspace"));
        assert!(out.contains("# keep me"));
        assert!(out.contains("[repo.b]"));
        assert!(out.contains("groups = [\"fw\"]"));
    }

    #[test]
    fn add_duplicate_repo_fails() {
        let spec = NewRepo {
            name: "a".into(),
            url: Some("/r/a2".into()),
            rev: "main".into(),
            ..Default::default()
        };
        assert!(matches!(
            edit::add_repo(BASE, &spec),
            Err(EditError::RepoExists(_))
        ));
    }

    #[test]
    fn remove_repo_refused_while_referenced() {
        assert!(matches!(
            edit::remove_repo(BASE, "a"),
            Err(EditError::ReferencedByStack { .. })
        ));
        let no_stack = edit::remove_stack(BASE, "s").unwrap();
        let out = edit::remove_repo(&no_stack, "a").unwrap();
        assert!(!out.contains("[repo.a]"));
        assert!(out.contains("# my workspace"));
    }

    #[test]
    fn add_stack_validates_repo_names() {
        let out = edit::add_stack(BASE, "s2", &["a".into()]).unwrap();
        assert!(out.contains("[stack.s2]"));
        assert!(edit::add_stack(BASE, "bad", &["ghost".into()]).is_err());
    }
}
