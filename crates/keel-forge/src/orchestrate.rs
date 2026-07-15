//! Forge-agnostic changeset lifecycle: push branches and open cross-linked
//! PR/MRs (`request`), aggregate their state (`statuses`), merge them in
//! changeset order (`land`).
//!
//! Changeset order follows the manifest/stack declaration order, which is the
//! product -> repo dependency order; `land` stops at the first failure.

use keel_core::change::{ChangeError, Changeset};
use keel_core::git::{GitBackend, GitError};
use keel_core::workspace::Workspace;

use crate::{ForgeError, ForgeFactory, ForgeKind, PrSpec, PrStatus, kind_from_key};

/// Errors from the changeset lifecycle that abort the whole operation
/// (per-repo failures are reported per repo instead).
#[derive(Debug, thiserror::Error)]
pub enum OrchestrateError {
    #[error(transparent)]
    Change(#[from] ChangeError),
    #[error("repo `{0}` is not in the manifest")]
    UnknownRepo(String),
    #[error("repo `{0}` has no resolvable clone URL")]
    Unsourced(String),
    #[error("dependency cycle among the changeset repos (involving `{0}`)")]
    DependencyCycle(String),
}

/// The manifest's explicit forge for a repo's remote, if declared.
fn forge_hint(ws: &Workspace, name: &str) -> Option<ForgeKind> {
    let repo = ws.manifest.repos.get(name)?;
    let remote = ws.manifest.remotes.get(repo.remote.as_deref()?)?;
    kind_from_key(remote.forge.as_deref()?)
}

/// Changeset repos reordered so that every repo comes after its manifest
/// `deps` (stable within ties: original changeset order).
fn topological(ws: &Workspace, changeset: &Changeset) -> Result<Vec<usize>, OrchestrateError> {
    let members: Vec<&str> = changeset.repos.iter().map(|r| r.name.as_str()).collect();
    let mut ordered: Vec<usize> = Vec::with_capacity(members.len());
    let mut placed = vec![false; members.len()];
    while ordered.len() < members.len() {
        let mut progressed = false;
        for index in 0..members.len() {
            if placed[index] {
                continue;
            }
            let deps = ws
                .manifest
                .repos
                .get(members[index])
                .map(|r| r.deps.as_slice())
                .unwrap_or_default();
            let ready = deps.iter().all(|dep| {
                members
                    .iter()
                    .position(|m| m == dep)
                    .is_none_or(|i| placed[i])
            });
            if ready {
                ordered.push(index);
                placed[index] = true;
                progressed = true;
            }
        }
        if !progressed {
            let stuck = members
                .iter()
                .zip(&placed)
                .find(|(_, placed)| !**placed)
                .map(|(name, _)| (*name).to_string())
                .unwrap_or_default();
            return Err(OrchestrateError::DependencyCycle(stuck));
        }
    }
    Ok(ordered)
}

/// What happened to one repo during `request` or `land`.
#[derive(Debug)]
pub struct RepoOutcome {
    pub name: String,
    pub url: Option<String>,
    pub result: Result<String, RepoFailure>,
}

/// Why one repo's step failed (others may still have succeeded).
#[derive(Debug, thiserror::Error)]
pub enum RepoFailure {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Forge(#[from] ForgeError),
    #[error("no PR/MR recorded; run `haw change request` first")]
    NoPr,
}

fn clone_url(ws: &Workspace, name: &str) -> Result<String, OrchestrateError> {
    let repo = ws
        .manifest
        .repos
        .get(name)
        .ok_or_else(|| OrchestrateError::UnknownRepo(name.to_string()))?;
    repo.clone_url(&ws.manifest.remotes)
        .ok_or_else(|| OrchestrateError::Unsourced(name.to_string()))
}

/// Base branch for a repo's PR: the locked branch when the manifest rev is a
/// branch, otherwise the explicit `base` (default `main`).
fn base_branch(ws: &Workspace, name: &str, base: Option<&str>) -> String {
    if let Some(base) = base {
        return base.to_string();
    }
    if let Ok(Some(lock)) = ws.read_lock()
        && let Some(entry) = lock.get(name)
        && entry.branch == entry.source_rev
    {
        return entry.branch.clone();
    }
    "main".to_string()
}

fn changeset_body(changeset: &Changeset) -> String {
    let mut body = format!(
        "Part of changeset `{}` — one feature across {} repos.\n\nRepos in this changeset:\n",
        changeset.id,
        changeset.repos.len()
    );
    for repo in &changeset.repos {
        match &repo.pr_url {
            Some(url) => body.push_str(&format!("- {} — {}\n", repo.name, url)),
            None => body.push_str(&format!("- {} — (no PR yet)\n", repo.name)),
        }
    }
    body.push_str("\nOpened by `haw change request`.\n");
    body
}

/// Push every changeset branch and open one PR/MR per repo, then cross-link
/// all descriptions. Repos that already carry a PR are left untouched.
/// The updated changeset is saved back to `.keel/`.
pub fn request(
    ws: &Workspace,
    backend: &dyn GitBackend,
    forges: &dyn ForgeFactory,
    id: &str,
    base: Option<&str>,
    only: Option<&[String]>,
) -> Result<Vec<RepoOutcome>, OrchestrateError> {
    let mut changeset = Changeset::load(ws, id)?;
    let mut outcomes = Vec::with_capacity(changeset.repos.len());

    for index in 0..changeset.repos.len() {
        let (name, branch, existing) = {
            let entry = &changeset.repos[index];
            (
                entry.name.clone(),
                entry.branch.clone(),
                entry.pr_url.clone(),
            )
        };
        if only.is_some_and(|list| !list.iter().any(|n| n == &name)) {
            continue;
        }
        if let Some(url) = existing {
            outcomes.push(RepoOutcome {
                name,
                url: Some(url.clone()),
                result: Ok(format!("already requested: {url}")),
            });
            continue;
        }
        let url = clone_url(ws, &name)?;
        let repo_dir = ws.root.join(
            ws.manifest
                .repos
                .get(&name)
                .ok_or_else(|| OrchestrateError::UnknownRepo(name.clone()))?
                .checkout_path(&name),
        );

        let attempt = || -> Result<crate::PrHandle, RepoFailure> {
            backend.push_branch(&repo_dir, &branch)?;
            let forge = forges.client_for(&url, forge_hint(ws, &name))?;
            let spec = PrSpec {
                title: format!("{}: {}", changeset.id, branch),
                body: changeset_body(&changeset),
                source_branch: branch.clone(),
                target_branch: base_branch(ws, &name, base),
                labels: changeset.labels.clone(),
            };
            Ok(forge.open_pr(&url, &spec)?)
        };
        match attempt() {
            Ok(handle) => {
                changeset.repos[index].pr_url = Some(handle.url.clone());
                changeset.repos[index].pr_number = Some(handle.number);
                outcomes.push(RepoOutcome {
                    name,
                    url: Some(url),
                    result: Ok(handle.url),
                });
            }
            Err(failure) => outcomes.push(RepoOutcome {
                name,
                url: Some(url),
                result: Err(failure),
            }),
        }
    }

    changeset.save(ws)?;

    let body = changeset_body(&changeset);
    for entry in &changeset.repos {
        let Some(number) = entry.pr_number else {
            continue;
        };
        let Ok(url) = clone_url(ws, &entry.name) else {
            continue;
        };
        if let Ok(forge) = forges.client_for(&url, forge_hint(ws, &entry.name)) {
            let _ = forge.update_pr_body(&url, number, &body);
        }
    }

    Ok(outcomes)
}

/// One changeset repo's PR/MR state: `None` when no PR was requested yet.
pub type RepoPrStatus = (String, Option<Result<PrStatus, RepoFailure>>);

/// PR/MR status per changeset repo; `None` when no PR was requested yet.
pub fn statuses(
    ws: &Workspace,
    forges: &dyn ForgeFactory,
    id: &str,
) -> Result<Vec<RepoPrStatus>, OrchestrateError> {
    let changeset = Changeset::load(ws, id)?;
    let mut out = Vec::with_capacity(changeset.repos.len());
    for entry in &changeset.repos {
        let Some(number) = entry.pr_number else {
            out.push((entry.name.clone(), None));
            continue;
        };
        let status = clone_url(ws, &entry.name).map_err(|_| ()).map_or_else(
            |()| Err(RepoFailure::NoPr),
            |url| {
                forges
                    .client_for(&url, forge_hint(ws, &entry.name))
                    .and_then(|forge| forge.pr_status(&url, number))
                    .map_err(RepoFailure::from)
            },
        );
        out.push((entry.name.clone(), Some(status)));
    }
    Ok(out)
}

/// Merge every PR/MR in topological order (manifest `deps` first, changeset
/// order within ties); already-merged entries are skipped and the first
/// failure stops the sequence (later repos stay unmerged).
pub fn land(
    ws: &Workspace,
    forges: &dyn ForgeFactory,
    id: &str,
) -> Result<Vec<RepoOutcome>, OrchestrateError> {
    let changeset = Changeset::load(ws, id)?;
    let order = topological(ws, &changeset)?;
    let mut outcomes = Vec::with_capacity(changeset.repos.len());
    for index in order {
        let entry = &changeset.repos[index];
        let Some(number) = entry.pr_number else {
            outcomes.push(RepoOutcome {
                name: entry.name.clone(),
                url: None,
                result: Err(RepoFailure::NoPr),
            });
            break;
        };
        let url = clone_url(ws, &entry.name)?;
        let attempt = || -> Result<String, RepoFailure> {
            let forge = forges.client_for(&url, forge_hint(ws, &entry.name))?;
            if forge.pr_status(&url, number)?.state == crate::PrState::Merged {
                return Ok("already merged".to_string());
            }
            forge.merge_pr(&url, number)?;
            Ok("merged".to_string())
        };
        let result = attempt();
        let stop = result.is_err();
        outcomes.push(RepoOutcome {
            name: entry.name.clone(),
            url: Some(url),
            result,
        });
        if stop {
            break;
        }
    }
    Ok(outcomes)
}
