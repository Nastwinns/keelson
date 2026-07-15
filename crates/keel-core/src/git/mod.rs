//! The abstraction keel-core uses for every git side effect.
//!
//! `keel-git` provides the production implementation (shell-out today,
//! gitoxide reads later). Tests can substitute a fake.

use std::path::{Path, PathBuf};

/// Errors surfaced by a [`GitBackend`].
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("could not run git: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("`{context}` failed: {stderr}")]
    Command { context: String, stderr: String },
    #[error("rev `{rev}` not found on {url}")]
    RevNotFound { url: String, rev: String },
    #[error("{path} has uncommitted changes; commit or stash them first")]
    Dirty { path: PathBuf },
    #[error(
        "branch `{branch}` in {path} has {count} local commit(s) not in the target; \
         push or remove them before syncing"
    )]
    LocalCommits {
        branch: String,
        path: PathBuf,
        count: u64,
    },
}

/// What kind of ref a manifest `rev` resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevKind {
    Branch,
    Tag,
    Sha,
}

/// A manifest `rev` resolved against the remote.
#[derive(Debug, Clone)]
pub struct ResolvedRev {
    pub sha: String,
    pub kind: RevKind,
}

/// Directory of the shared bare mirror for `url` under `cache_root`.
/// Stable, filesystem-safe, collision-resistant (FNV-1a 64 over the URL).
pub fn mirror_dir(cache_root: &Path, url: &str) -> PathBuf {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in url.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let stem: String = url
        .rsplit('/')
        .next()
        .unwrap_or("repo")
        .trim_end_matches(".git")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(40)
        .collect();
    cache_root.join(format!("{stem}-{hash:016x}.git"))
}

/// Every git operation Keelson needs, behind one trait.
pub trait GitBackend: Sync {
    /// Resolve a rev (branch, tag, or SHA) to a commit SHA without cloning.
    fn resolve_rev(&self, url: &str, rev: &str) -> Result<ResolvedRev, GitError>;
    /// Clone `url` to `dest`; `reference` shares objects with a local mirror
    /// via git alternates (a text file — no symlinks).
    fn clone_repo(&self, url: &str, dest: &Path, reference: Option<&Path>) -> Result<(), GitError>;
    /// Create or refresh the bare mirror of `url` at `mirror`.
    fn ensure_mirror(&self, url: &str, mirror: &Path) -> Result<(), GitError>;
    fn fetch(&self, repo: &Path) -> Result<(), GitError>;
    /// Check out `sha` on a real local branch named `branch` (never detached).
    fn checkout(&self, repo: &Path, sha: &str, branch: &str) -> Result<(), GitError>;
    fn create_branch(&self, repo: &Path, name: &str) -> Result<(), GitError>;
    /// Push `branch` to origin (sets upstream on first push).
    fn push_branch(&self, repo: &Path, branch: &str) -> Result<(), GitError>;
    fn head_sha(&self, repo: &Path) -> Result<String, GitError>;
    /// `None` means detached HEAD.
    fn current_branch(&self, repo: &Path) -> Result<Option<String>, GitError>;
    fn is_dirty(&self, repo: &Path) -> Result<bool, GitError>;
    fn is_repo(&self, repo: &Path) -> bool;
}
