//! Changesets: one feature spanning several repos — one logical branch
//! created across N repos, later linked to N PR/MRs.
//!
//! State lives in `.keel/changesets/<id>.toml`, outside the manifest.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::git::{GitBackend, GitError};
use crate::workspace::Workspace;

/// Errors in the changeset workflow.
#[derive(Debug, thiserror::Error)]
pub enum ChangeError {
    #[error("failed to access {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid changeset TOML")]
    Parse(#[source] Box<toml::de::Error>),
    #[error("could not serialize changeset")]
    Serialize(#[from] toml::ser::Error),
    #[error("changeset `{0}` already exists")]
    AlreadyExists(String),
    #[error("changeset `{0}` not found")]
    NotFound(String),
    #[error("repo `{0}` is not in the manifest")]
    UnknownRepo(String),
    #[error("repo `{name}` is not cloned at {path}; run `haw sync` first")]
    MissingRepoRepo { name: String, path: PathBuf },
    #[error("repo `{0}` is on a detached HEAD; check out a branch first")]
    Detached(String),
    #[error(transparent)]
    Git(#[from] GitError),
}

/// One repo participating in a changeset, with its feature branch and,
/// once `change request` ran, its PR/MR.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeRepo {
    pub name: String,
    pub branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u64>,
}

/// A feature across several repos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Changeset {
    pub id: String,
    /// Labels forwarded to the PR/MRs at `change request`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(
        default,
        rename = "repo",
        alias = "brick",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub repos: Vec<ChangeRepo>,
}

/// Observed state of one repo inside a changeset, for `change status`.
#[derive(Debug, Clone)]
pub struct ChangeRepoStatus {
    pub name: String,
    pub branch: String,
    pub missing: bool,
    /// True when the repo is currently checked out on the changeset branch.
    pub on_branch: bool,
    pub dirty: bool,
    pub head: Option<String>,
}

/// Default branch name for a changeset id.
pub fn default_branch(id: &str) -> String {
    format!("change/{}", id.replace([' ', ':'], "-"))
}

fn changesets_dir(ws: &Workspace) -> PathBuf {
    ws.state_dir().join("changesets")
}

fn changeset_path(ws: &Workspace, id: &str) -> PathBuf {
    changesets_dir(ws).join(format!("{id}.toml"))
}

impl Changeset {
    pub fn load(ws: &Workspace, id: &str) -> Result<Self, ChangeError> {
        let path = changeset_path(ws, id);
        if !path.exists() {
            return Err(ChangeError::NotFound(id.to_string()));
        }
        let text =
            std::fs::read_to_string(&path).map_err(|source| ChangeError::Io { path, source })?;
        toml::from_str(&text).map_err(|source| ChangeError::Parse(Box::new(source)))
    }

    pub fn save(&self, ws: &Workspace) -> Result<(), ChangeError> {
        let dir = changesets_dir(ws);
        std::fs::create_dir_all(&dir).map_err(|source| ChangeError::Io { path: dir, source })?;
        let path = changeset_path(ws, &self.id);
        let text = toml::to_string_pretty(self)?;
        std::fs::write(&path, text).map_err(|source| ChangeError::Io { path, source })
    }

    /// Ids of all recorded changesets, sorted.
    pub fn list(ws: &Workspace) -> Result<Vec<String>, ChangeError> {
        let dir = changesets_dir(ws);
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let entries =
            std::fs::read_dir(&dir).map_err(|source| ChangeError::Io { path: dir, source })?;
        let mut ids: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".toml").map(str::to_string)
            })
            .collect();
        ids.sort();
        Ok(ids)
    }
}

/// Start a changeset: create (or adopt, with `skip_branch`) one branch across
/// the given repos. `repos: None` means every repo in the manifest.
pub fn start(
    ws: &Workspace,
    backend: &dyn GitBackend,
    id: &str,
    repos: Option<&[String]>,
    branch: Option<&str>,
    skip_branch: bool,
    labels: &[String],
) -> Result<Changeset, ChangeError> {
    if changeset_path(ws, id).exists() {
        return Err(ChangeError::AlreadyExists(id.to_string()));
    }

    let names: Vec<String> = match repos {
        Some(list) => list.to_vec(),
        None => ws.manifest.repos.keys().cloned().collect(),
    };
    let branch_name = branch.map_or_else(|| default_branch(id), str::to_string);

    let mut resolved = Vec::with_capacity(names.len());
    for name in &names {
        let repo = ws
            .manifest
            .repos
            .get(name)
            .ok_or_else(|| ChangeError::UnknownRepo(name.clone()))?;
        let path = ws.root.join(repo.checkout_path(name));
        if !backend.is_repo(&path) {
            return Err(ChangeError::MissingRepoRepo {
                name: name.clone(),
                path,
            });
        }
        resolved.push((name.clone(), path));
    }

    let mut entries = Vec::with_capacity(resolved.len());
    for (name, path) in resolved {
        let entry_branch = if skip_branch {
            backend
                .current_branch(&path)?
                .ok_or_else(|| ChangeError::Detached(name.clone()))?
        } else {
            backend.create_branch(&path, &branch_name)?;
            branch_name.clone()
        };
        entries.push(ChangeRepo {
            name,
            branch: entry_branch,
            pr_url: None,
            pr_number: None,
        });
    }

    let changeset = Changeset {
        id: id.to_string(),
        labels: labels.to_vec(),
        repos: entries,
    };
    changeset.save(ws)?;
    Ok(changeset)
}

/// Per-repo state of a changeset.
pub fn status(
    ws: &Workspace,
    backend: &dyn GitBackend,
    id: &str,
) -> Result<Vec<ChangeRepoStatus>, ChangeError> {
    let changeset = Changeset::load(ws, id)?;
    let mut out = Vec::with_capacity(changeset.repos.len());
    for entry in &changeset.repos {
        let Some(repo) = ws.manifest.repos.get(&entry.name) else {
            out.push(ChangeRepoStatus {
                name: entry.name.clone(),
                branch: entry.branch.clone(),
                missing: true,
                on_branch: false,
                dirty: false,
                head: None,
            });
            continue;
        };
        let path = ws.root.join(repo.checkout_path(&entry.name));
        if !backend.is_repo(&path) {
            out.push(ChangeRepoStatus {
                name: entry.name.clone(),
                branch: entry.branch.clone(),
                missing: true,
                on_branch: false,
                dirty: false,
                head: None,
            });
            continue;
        }
        let current = backend.current_branch(&path)?;
        out.push(ChangeRepoStatus {
            name: entry.name.clone(),
            branch: entry.branch.clone(),
            missing: false,
            on_branch: current.as_deref() == Some(entry.branch.as_str()),
            dirty: backend.is_dirty(&path)?,
            head: Some(backend.head_sha(&path)?),
        });
    }
    Ok(out)
}
