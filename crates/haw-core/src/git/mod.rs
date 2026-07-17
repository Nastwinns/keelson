//! The abstraction haw-core uses for every git side effect.
//!
//! `haw-git` provides the production implementation (shell-out today,
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

/// Options that shape a `git clone`, so `haw` scales to thousands of repos.
///
/// The three levers are independent and compose:
/// - `reference`: share objects with a local bare mirror (`--reference`).
/// - `filter`: partial clone (`--filter=<spec>`, e.g. `blob:none`). Keeps
///   ALL commits, so any locked SHA is reachable; blobs fetch lazily. This is
///   the reproducibility-safe lever for pinned revs.
/// - `depth`: shallow clone (`--depth <N>`). Smaller/faster, but the truncated
///   history may not contain an old locked SHA — see [`CloneOpts::depth`] and
///   the shallow-recovery path in the checkout step.
#[derive(Debug, Clone, Default)]
pub struct CloneOpts {
    /// Shared bare mirror to reference at clone time (`--reference`).
    pub reference: Option<PathBuf>,
    /// Partial-clone filter spec passed to `--filter=<spec>`.
    pub filter: Option<String>,
    /// Shallow-clone depth passed to `--depth <N>`.
    pub depth: Option<u32>,
    /// Recurse submodules at clone time (`--recurse-submodules`). Submodules
    /// follow the superproject's pinned commit, so this stays reproducible.
    pub submodules: bool,
}

impl CloneOpts {
    /// A plain clone with no reference, filter, or depth.
    pub fn none() -> Self {
        Self::default()
    }

    /// Set the shared-mirror reference (builder style).
    pub fn with_reference(mut self, reference: Option<PathBuf>) -> Self {
        self.reference = reference;
        self
    }
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

/// Every git operation hawser needs, behind one trait.
pub trait GitBackend: Sync {
    /// Resolve a rev (branch, tag, or SHA) to a commit SHA without cloning.
    fn resolve_rev(&self, url: &str, rev: &str) -> Result<ResolvedRev, GitError>;
    /// Clone `url` to `dest`. [`CloneOpts`] carries the optional shared-mirror
    /// reference (git alternates — a text file, no symlinks), a partial-clone
    /// `--filter`, and a shallow `--depth`.
    fn clone_repo(&self, url: &str, dest: &Path, opts: &CloneOpts) -> Result<(), GitError>;
    /// Create or refresh the bare mirror of `url` at `mirror`.
    fn ensure_mirror(&self, url: &str, mirror: &Path) -> Result<(), GitError>;
    fn fetch(&self, repo: &Path) -> Result<(), GitError>;
    /// Check out `sha` on a real local branch named `branch` (never detached).
    ///
    /// `shallow_depth` is `Some(N)` when the repo was cloned with `--depth N`:
    /// the target SHA may be outside the truncated history, so the backend must
    /// recover (deepen or unshallow) to reach the locked SHA before checking
    /// out. `None` means a full or partial clone, where the SHA is always
    /// present. Never leaves the repo off `sha` silently.
    fn checkout(
        &self,
        repo: &Path,
        sha: &str,
        branch: &str,
        shallow_depth: Option<u32>,
    ) -> Result<(), GitError>;
    /// Update and initialize submodules recursively on an existing clone
    /// (`git submodule update --init --recursive`). Submodules follow the
    /// superproject's checked-out (pinned) commit, so this is reproducible.
    fn update_submodules(&self, repo: &Path) -> Result<(), GitError>;
    fn create_branch(&self, repo: &Path, name: &str) -> Result<(), GitError>;
    /// Push `branch` to origin (sets upstream on first push).
    fn push_branch(&self, repo: &Path, branch: &str) -> Result<(), GitError>;
    fn head_sha(&self, repo: &Path) -> Result<String, GitError>;
    /// Commits ahead/behind the upstream branch; `None` without an upstream.
    fn ahead_behind(&self, repo: &Path) -> Result<Option<(u64, u64)>, GitError>;
    /// `None` means detached HEAD.
    fn current_branch(&self, repo: &Path) -> Result<Option<String>, GitError>;
    fn is_dirty(&self, repo: &Path) -> Result<bool, GitError>;
    fn is_repo(&self, repo: &Path) -> bool;
}
