#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use haw_core::change::{ChangeRepo, Changeset};
use haw_core::git::{CloneOpts, GitBackend, GitError, ResolvedRev, RevKind};
use haw_core::workspace::Workspace;
use haw_forge::orchestrate::{self, RepoFailure};
use haw_forge::{Forge, ForgeError, ForgeFactory, PrHandle, PrSpec, PrState, PrStatus};

struct FakeGit;

impl GitBackend for FakeGit {
    fn resolve_rev(&self, _url: &str, _rev: &str) -> Result<ResolvedRev, GitError> {
        Ok(ResolvedRev {
            sha: "a".repeat(40),
            kind: RevKind::Branch,
        })
    }
    fn clone_repo(&self, _url: &str, _dest: &Path, _opts: &CloneOpts) -> Result<(), GitError> {
        Ok(())
    }
    fn ensure_mirror(&self, _url: &str, _mirror: &Path) -> Result<(), GitError> {
        Ok(())
    }
    fn fetch(&self, _repo: &Path) -> Result<(), GitError> {
        Ok(())
    }
    fn checkout(
        &self,
        _repo: &Path,
        _sha: &str,
        _branch: &str,
        _shallow_depth: Option<u32>,
    ) -> Result<(), GitError> {
        Ok(())
    }
    fn update_submodules(&self, _repo: &Path) -> Result<(), GitError> {
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
    fn ahead_behind(&self, _repo: &Path) -> Result<Option<(u64, u64)>, GitError> {
        Ok(None)
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
    approved: Vec<String>,
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
    fn approve_pr(&self, repo_url: &str, _number: u64) -> Result<(), ForgeError> {
        self.journal
            .lock()
            .unwrap()
            .approved
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
    fn list_open_prs(&self, repo_url: &str) -> Result<Vec<haw_forge::OpenPr>, ForgeError> {
        Ok(vec![haw_forge::OpenPr {
            number: 7,
            title: format!("fix {repo_url}"),
            url: format!("https://forge.example{repo_url}/pull/7"),
            state: PrState::Open,
            approved: false,
            ci_passing: Some(true),
        }])
    }
    fn list_ci_runs(&self, repo_url: &str) -> Result<Vec<haw_forge::CiRun>, ForgeError> {
        Ok(vec![haw_forge::CiRun {
            id: 1,
            name: "build".to_string(),
            branch: "main".to_string(),
            event: "push".to_string(),
            status: haw_forge::CiStatus::Passed,
            url: format!("https://forge.example{repo_url}/runs/1"),
        }])
    }
    fn pr_detail(&self, _repo_url: &str, _number: u64) -> Result<String, ForgeError> {
        Ok(String::new())
    }
    fn ci_run_detail(&self, _repo_url: &str, _run_id: u64) -> Result<String, ForgeError> {
        Ok(String::new())
    }
    fn pr_diff(&self, _repo_url: &str, _number: u64) -> Result<String, ForgeError> {
        Ok(String::new())
    }
    fn ci_logs(&self, _repo_url: &str, _run_id: u64) -> Result<String, ForgeError> {
        Ok(String::new())
    }
    fn repo_tree(
        &self,
        _repo_url: &str,
        _subpath: &str,
        _git_ref: Option<&str>,
    ) -> Result<Vec<haw_forge::TreeEntry>, ForgeError> {
        Ok(vec![
            haw_forge::TreeEntry {
                name: "src".to_string(),
                is_dir: true,
            },
            haw_forge::TreeEntry {
                name: "README.md".to_string(),
                is_dir: false,
            },
        ])
    }
    fn file_blob(
        &self,
        _repo_url: &str,
        _path: &str,
        _git_ref: Option<&str>,
    ) -> Result<String, ForgeError> {
        Ok(String::new())
    }
    fn pr_files(
        &self,
        _repo_url: &str,
        _number: u64,
    ) -> Result<Vec<haw_forge::PrFile>, ForgeError> {
        Ok(vec![haw_forge::PrFile {
            path: "src/lib.rs".to_string(),
            status: "modified".to_string(),
        }])
    }
    fn pr_file_content(
        &self,
        _repo_url: &str,
        _number: u64,
        _path: &str,
    ) -> Result<String, ForgeError> {
        Ok(String::new())
    }
}

struct FakeFactory {
    journal: Arc<Mutex<Journal>>,
    fail_merge_for: Option<String>,
    merged_already: Vec<String>,
}

impl ForgeFactory for FakeFactory {
    fn client_for(
        &self,
        _url: &str,
        _hint: Option<haw_forge::ForgeKind>,
    ) -> Result<Box<dyn Forge>, ForgeError> {
        Ok(Box::new(FakeForge {
            journal: Arc::clone(&self.journal),
            fail_merge_for: self.fail_merge_for.clone(),
            merged_already: self.merged_already.clone(),
        }))
    }
}

fn workspace(dir: &Path) -> Workspace {
    std::fs::write(
        dir.join("haw.toml"),
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
        labels: Vec::new(),
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

    let outcomes = orchestrate::request(
        &ws,
        &FakeGit,
        &journal_factory(&journal),
        "FEAT-1",
        None,
        None,
    )
    .unwrap();
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

    let outcomes = orchestrate::request(
        &ws,
        &FakeGit,
        &journal_factory(&journal),
        "FEAT-1",
        None,
        None,
    )
    .unwrap();
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
fn land_follows_manifest_deps_topologically() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("haw.toml"),
        "[repo.kernel]\nurl = \"/git/kernel\"\nrev = \"main\"\ndeps = [\"app\"]\n\n\
         [repo.app]\nurl = \"/git/app\"\nrev = \"main\"\n\n\
         [stack.s]\nrepos = [\"kernel\", \"app\"]\n",
    )
    .unwrap();
    let ws = Workspace::open(dir.path()).unwrap();
    seed_changeset(&ws, true);
    let journal = Arc::new(Mutex::new(Journal::default()));

    let outcomes = orchestrate::land(&ws, &journal_factory(&journal), "FEAT-1").unwrap();
    assert!(outcomes.iter().all(|o| o.result.is_ok()));
    assert_eq!(
        journal.lock().unwrap().merged,
        vec!["/git/app", "/git/kernel"],
        "kernel deps on app, so app merges first"
    );
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

fn github_workspace(dir: &Path) -> Workspace {
    std::fs::write(
        dir.join("haw.toml"),
        "[repo.kernel]\nurl = \"https://github.com/acme/kernel.git\"\nrev = \"main\"\n\n\
         [repo.app]\nurl = \"https://github.com/acme/app.git\"\nrev = \"main\"\n\n\
         [repo.local]\nurl = \"/git/local\"\nrev = \"main\"\n\n\
         [stack.s]\nrepos = [\"kernel\", \"app\", \"local\"]\n",
    )
    .unwrap();
    Workspace::open(dir).unwrap()
}

#[test]
fn fleet_open_prs_covers_forge_repos_and_skips_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let ws = github_workspace(dir.path());
    let journal = Arc::new(Mutex::new(Journal::default()));

    let results = orchestrate::fleet_open_prs(&ws, &journal_factory(&journal));
    let names: Vec<&str> = results.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        vec!["kernel", "app"],
        "manifest order, `local` skipped"
    );
    for (_, result) in &results {
        let prs = result.as_ref().unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 7);
    }
}

#[test]
fn fleet_ci_runs_covers_forge_repos_and_skips_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let ws = github_workspace(dir.path());
    let journal = Arc::new(Mutex::new(Journal::default()));

    let results = orchestrate::fleet_ci_runs(&ws, &journal_factory(&journal));
    assert_eq!(results.len(), 2);
    for (_, result) in &results {
        let runs = result.as_ref().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, haw_forge::CiStatus::Passed);
    }
}
