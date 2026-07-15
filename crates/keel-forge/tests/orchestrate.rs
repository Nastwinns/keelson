#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use keel_core::change::{ChangeRepo, Changeset};
use keel_core::git::{GitBackend, GitError, ResolvedRev, RevKind};
use keel_core::workspace::Workspace;
use keel_forge::orchestrate::{self, RepoFailure};
use keel_forge::{Forge, ForgeError, ForgeFactory, PrHandle, PrSpec, PrState, PrStatus};

struct FakeGit;

impl GitBackend for FakeGit {
    fn resolve_rev(&self, _url: &str, _rev: &str) -> Result<ResolvedRev, GitError> {
        Ok(ResolvedRev {
            sha: "a".repeat(40),
            kind: RevKind::Branch,
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
    fn push_branch(&self, _repo: &Path, _branch: &str) -> Result<(), GitError> {
        Ok(())
    }
    fn head_sha(&self, _repo: &Path) -> Result<String, GitError> {
        Ok("b".repeat(40))
    }
    fn current_branch(&self, _repo: &Path) -> Result<Option<String>, GitError> {
        Ok(Some("main".to_string()))
    }
    fn is_dirty(&self, _repo: &Path) -> Result<bool, GitError> {
        Ok(false)
    }
    fn is_repo(&self, _repo: &Path) -> bool {
        true
    }
}

#[derive(Default)]
struct Journal {
    opened: Vec<String>,
    merged: Vec<String>,
    bodies: Vec<(String, String)>,
}

struct FakeForge {
    journal: Arc<Mutex<Journal>>,
    fail_merge_for: Option<String>,
    merged_already: Vec<String>,
}

impl Forge for FakeForge {
    fn open_pr(&self, repo_url: &str, _spec: &PrSpec) -> Result<PrHandle, ForgeError> {
        let mut journal = self.journal.lock().unwrap();
        journal.opened.push(repo_url.to_string());
        let number = journal.opened.len() as u64;
        Ok(PrHandle {
            url: format!("https://forge.example{repo_url}/pull/{number}"),
            number,
        })
    }
    fn pr_status(&self, repo_url: &str, number: u64) -> Result<PrStatus, ForgeError> {
        let state = if self.merged_already.iter().any(|u| repo_url.ends_with(u)) {
            PrState::Merged
        } else {
            PrState::Open
        };
        Ok(PrStatus {
            state,
            approved: true,
            ci_passing: Some(true),
            url: format!("https://forge.example{repo_url}/pull/{number}"),
        })
    }
    fn merge_pr(&self, repo_url: &str, _number: u64) -> Result<(), ForgeError> {
        if self
            .fail_merge_for
            .as_deref()
            .is_some_and(|s| repo_url.ends_with(s))
        {
            return Err(ForgeError::Api("merge rejected".to_string()));
        }
        self.journal
            .lock()
            .unwrap()
            .merged
            .push(repo_url.to_string());
        Ok(())
    }
    fn update_pr_body(&self, repo_url: &str, _number: u64, body: &str) -> Result<(), ForgeError> {
        self.journal
            .lock()
            .unwrap()
            .bodies
            .push((repo_url.to_string(), body.to_string()));
        Ok(())
    }
}

struct FakeFactory {
    journal: Arc<Mutex<Journal>>,
    fail_merge_for: Option<String>,
    merged_already: Vec<String>,
}

impl ForgeFactory for FakeFactory {
    fn client_for(&self, _url: &str) -> Result<Box<dyn Forge>, ForgeError> {
        Ok(Box::new(FakeForge {
            journal: Arc::clone(&self.journal),
            fail_merge_for: self.fail_merge_for.clone(),
            merged_already: self.merged_already.clone(),
        }))
    }
}

fn workspace(dir: &Path) -> Workspace {
    std::fs::write(
        dir.join("keel.toml"),
        "[repo.kernel]\nurl = \"/git/kernel\"\nrev = \"main\"\n\n\
         [repo.app]\nurl = \"/git/app\"\nrev = \"main\"\n\n\
         [stack.s]\nrepos = [\"kernel\", \"app\"]\n",
    )
    .unwrap();
    Workspace::open(dir).unwrap()
}

fn seed_changeset(ws: &Workspace, with_prs: bool) {
    let changeset = Changeset {
        id: "FEAT-1".to_string(),
        repos: vec![
            ChangeRepo {
                name: "kernel".to_string(),
                branch: "change/FEAT-1".to_string(),
                pr_url: with_prs.then(|| "https://x/pull/1".to_string()),
                pr_number: with_prs.then_some(1),
            },
            ChangeRepo {
                name: "app".to_string(),
                branch: "change/FEAT-1".to_string(),
                pr_url: with_prs.then(|| "https://x/pull/2".to_string()),
                pr_number: with_prs.then_some(2),
            },
        ],
    };
    changeset.save(ws).unwrap();
}

fn journal_factory(journal: &Arc<Mutex<Journal>>) -> FakeFactory {
    FakeFactory {
        journal: Arc::clone(journal),
        fail_merge_for: None,
        merged_already: Vec::new(),
    }
}

#[test]
fn request_opens_and_cross_links_all_prs() {
    let dir = tempfile::tempdir().unwrap();
    let ws = workspace(dir.path());
    seed_changeset(&ws, false);
    let journal = Arc::new(Mutex::new(Journal::default()));

    let outcomes =
        orchestrate::request(&ws, &FakeGit, &journal_factory(&journal), "FEAT-1", None).unwrap();
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().all(|o| o.result.is_ok()));

    let saved = Changeset::load(&ws, "FEAT-1").unwrap();
    assert!(saved.repos.iter().all(|r| r.pr_number.is_some()));

    let journal = journal.lock().unwrap();
    assert_eq!(journal.opened, vec!["/git/kernel", "/git/app"]);
    assert_eq!(journal.bodies.len(), 2, "every PR body is cross-linked");
    assert!(
        journal.bodies[0].1.contains("/git/app/pull/"),
        "kernel's body links app's PR"
    );
}

#[test]
fn request_is_idempotent_for_already_opened_prs() {
    let dir = tempfile::tempdir().unwrap();
    let ws = workspace(dir.path());
    seed_changeset(&ws, true);
    let journal = Arc::new(Mutex::new(Journal::default()));

    let outcomes =
        orchestrate::request(&ws, &FakeGit, &journal_factory(&journal), "FEAT-1", None).unwrap();
    assert!(outcomes.iter().all(|o| o.result.is_ok()));
    assert!(
        journal.lock().unwrap().opened.is_empty(),
        "no duplicate PRs"
    );
}

#[test]
fn land_merges_in_order_and_stops_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let ws = workspace(dir.path());
    seed_changeset(&ws, true);
    let journal = Arc::new(Mutex::new(Journal::default()));
    let factory = FakeFactory {
        journal: Arc::clone(&journal),
        fail_merge_for: Some("/git/kernel".to_string()),
        merged_already: Vec::new(),
    };

    let outcomes = orchestrate::land(&ws, &factory, "FEAT-1").unwrap();
    assert_eq!(outcomes.len(), 1, "stops at the first failure");
    assert!(matches!(outcomes[0].result, Err(RepoFailure::Forge(_))));
    assert!(journal.lock().unwrap().merged.is_empty());
}

#[test]
fn land_skips_already_merged_and_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let ws = workspace(dir.path());
    seed_changeset(&ws, true);
    let journal = Arc::new(Mutex::new(Journal::default()));
    let factory = FakeFactory {
        journal: Arc::clone(&journal),
        fail_merge_for: None,
        merged_already: vec!["/git/kernel".to_string()],
    };

    let outcomes = orchestrate::land(&ws, &factory, "FEAT-1").unwrap();
    assert_eq!(outcomes.len(), 2);
    assert_eq!(outcomes[0].result.as_deref().unwrap(), "already merged");
    assert_eq!(
        journal.lock().unwrap().merged,
        vec!["/git/app"],
        "only the unmerged repo is merged"
    );
}
