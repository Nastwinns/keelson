//! The machine-generated `keel.lock`: every repo pinned to an exact SHA.
//!
//! The lockfile covers **all** repos in the manifest (not just one stack),
//! so switching stacks never changes the lock. Overlays only take effect
//! when the lock is (re)generated.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Current lockfile schema version.
pub const LOCK_VERSION: u32 = 1;

/// Errors reading or writing a lockfile.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("failed to access lockfile at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid lockfile TOML")]
    Parse(#[source] Box<toml::de::Error>),
    #[error("could not serialize lockfile")]
    Serialize(#[from] toml::ser::Error),
    #[error("unsupported lockfile version {0} (this haw supports {LOCK_VERSION})")]
    UnsupportedVersion(u32),
}

/// The parsed `keel.lock`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    pub version: u32,
    #[serde(
        default,
        rename = "repo",
        alias = "brick",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub repos: Vec<LockedRepo>,
}

/// One repo pinned to an exact commit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct LockedRepo {
    pub name: String,
    pub url: String,
    pub path: PathBuf,
    /// The pinned commit SHA.
    pub rev: String,
    /// The manifest rev this SHA was resolved from.
    pub source_rev: String,
    /// The local branch repos are checked out on (never detached).
    pub branch: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,
}

impl Lockfile {
    pub fn load(path: &Path) -> Result<Self, LockError> {
        let text = std::fs::read_to_string(path).map_err(|source| LockError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let lock: Lockfile =
            toml::from_str(&text).map_err(|source| LockError::Parse(Box::new(source)))?;
        if lock.version != LOCK_VERSION {
            return Err(LockError::UnsupportedVersion(lock.version));
        }
        Ok(lock)
    }

    pub fn save(&self, path: &Path) -> Result<(), LockError> {
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text).map_err(|source| LockError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn get(&self, name: &str) -> Option<&LockedRepo> {
        self.repos.iter().find(|b| b.name == name)
    }
}
