//! Snapshots: save and restore the multi-repo state of the workspace
//! (per-repo branch + commit), RepoFleet-style.
//!
//! State lives in `.keel/snapshots/<name>.toml`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::git::{GitBackend, GitError};
use crate::workspace::Workspace;

/// Errors in the snapshot workflow.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("failed to access {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid snapshot TOML")]
    Parse(#[source] Box<toml::de::Error>),
    #[error("could not serialize snapshot")]
    Serialize(#[from] toml::ser::Error),
    #[error("snapshot `{0}` already exists")]
    AlreadyExists(String),
    #[error("snapshot `{0}` not found")]
    NotFound(String),
    #[error("repo `{name}` is not cloned at {path}; run `keel sync` first")]
    MissingRepo { name: String, path: PathBuf },
    #[error("repo `{0}` has uncommitted changes; commit or stash before restoring")]
    Dirty(String),
    #[error(transparent)]
    Git(#[from] GitError),
}

/// One repo's saved position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRepo {
    pub name: String,
    /// Branch the repo was on (`keel/detached` markers never occur: detached
    /// HEADs are saved by SHA on a `keel/snap-*` branch at restore time).
    pub branch: Option<String>,
    pub sha: String,
}

/// A named multi-repo state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub name: String,
    #[serde(default, rename = "repo", skip_serializing_if = "Vec::is_empty")]
    pub repos: Vec<SnapshotRepo>,
}

fn snapshots_dir(ws: &Workspace) -> PathBuf {
    ws.state_dir().join("snapshots")
}

fn snapshot_path(ws: &Workspace, name: &str) -> PathBuf {
    snapshots_dir(ws).join(format!("{name}.toml"))
}

impl Snapshot {
    pub fn load(ws: &Workspace, name: &str) -> Result<Self, SnapshotError> {
        let path = snapshot_path(ws, name);
        if !path.exists() {
            return Err(SnapshotError::NotFound(name.to_string()));
        }
        let text =
            std::fs::read_to_string(&path).map_err(|source| SnapshotError::Io { path, source })?;
        toml::from_str(&text).map_err(|source| SnapshotError::Parse(Box::new(source)))
    }

    pub fn save_file(&self, ws: &Workspace) -> Result<(), SnapshotError> {
        let dir = snapshots_dir(ws);
        std::fs::create_dir_all(&dir).map_err(|source| SnapshotError::Io { path: dir, source })?;
        let path = snapshot_path(ws, &self.name);
        let text = toml::to_string_pretty(self)?;
        std::fs::write(&path, text).map_err(|source| SnapshotError::Io { path, source })
    }

    /// Names of all recorded snapshots, sorted.
    pub fn list(ws: &Workspace) -> Result<Vec<String>, SnapshotError> {
        let dir = snapshots_dir(ws);
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let entries =
            std::fs::read_dir(&dir).map_err(|source| SnapshotError::Io { path: dir, source })?;
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let file = e.file_name().to_string_lossy().into_owned();
                file.strip_suffix(".toml").map(str::to_string)
            })
            .collect();
        names.sort();
        Ok(names)
    }
}

/// Record every cloned repo's branch + HEAD under `name`.
pub fn save(
    ws: &Workspace,
    backend: &dyn GitBackend,
    name: &str,
) -> Result<Snapshot, SnapshotError> {
    if snapshot_path(ws, name).exists() {
        return Err(SnapshotError::AlreadyExists(name.to_string()));
    }
    let mut repos = Vec::with_capacity(ws.manifest.repos.len());
    for (repo_name, repo) in &ws.manifest.repos {
        let path = ws.root.join(repo.checkout_path(repo_name));
        if !backend.is_repo(&path) {
            return Err(SnapshotError::MissingRepo {
                name: repo_name.clone(),
                path,
            });
        }
        repos.push(SnapshotRepo {
            name: repo_name.clone(),
            branch: backend.current_branch(&path)?,
            sha: backend.head_sha(&path)?,
        });
    }
    let snapshot = Snapshot {
        name: name.to_string(),
        repos,
    };
    snapshot.save_file(ws)?;
    Ok(snapshot)
}

/// Bring every saved repo back to its recorded branch + commit.
/// Refuses on dirty repos; never runs the network.
pub fn restore(
    ws: &Workspace,
    backend: &dyn GitBackend,
    name: &str,
) -> Result<Snapshot, SnapshotError> {
    let snapshot = Snapshot::load(ws, name)?;
    let mut targets = Vec::with_capacity(snapshot.repos.len());
    for entry in &snapshot.repos {
        let Some(repo) = ws.manifest.repos.get(&entry.name) else {
            continue;
        };
        let path = ws.root.join(repo.checkout_path(&entry.name));
        if !backend.is_repo(&path) {
            return Err(SnapshotError::MissingRepo {
                name: entry.name.clone(),
                path,
            });
        }
        if backend.is_dirty(&path)? {
            return Err(SnapshotError::Dirty(entry.name.clone()));
        }
        targets.push((entry, path));
    }
    for (entry, path) in targets {
        let branch = entry
            .branch
            .clone()
            .unwrap_or_else(|| format!("keel/snap-{}", &entry.sha[..8.min(entry.sha.len())]));
        backend.checkout(&path, &entry.sha, &branch)?;
    }
    Ok(snapshot)
}
