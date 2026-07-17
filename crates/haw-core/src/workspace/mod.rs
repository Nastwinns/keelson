//! The on-disk workspace: `haw.toml`, `haw.lock`, the repos, and the
//! `.haw/` state directory. Sync planning and status live here; execution
//! goes through a [`GitBackend`].

use std::path::{Path, PathBuf};

use crate::git::{GitBackend, GitError, RevKind};
use crate::lock::{LOCK_VERSION, LockError, LockedRepo, Lockfile};
use crate::manifest::{Manifest, ManifestError, ManifestLoader, TomlLoader};
use crate::resolver::{self, ResolveError};

pub const MANIFEST_FILE: &str = "haw.toml";
pub const LOCK_FILE: &str = "haw.lock";
pub const STATE_DIR: &str = ".haw";

/// Errors opening or reading workspace state.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("no {MANIFEST_FILE} found in {0}")]
    NotAWorkspace(PathBuf),
    #[error("failed to access {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unknown stack `{0}`")]
    UnknownStack(String),
    #[error("no stack selected; pass --stack or `haw switch` (available: {available})")]
    StackRequired { available: String },
}

/// Errors while planning or executing a sync.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Lock(#[from] LockError),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    #[error("repo `{0}` is not in {LOCK_FILE}; run `haw lock` to regenerate it")]
    MissingLockEntry(String),
    #[error("repo `{0}` is not cloned; run `haw sync` first")]
    NotCloned(String),
}

/// A workspace rooted at the directory containing its manifest.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub manifest: Manifest,
    /// Path the manifest was actually loaded from; usually `root/haw.toml`,
    /// but [`Workspace::open_manifest`] allows any file name or location.
    manifest_file: PathBuf,
}

/// Everything needed to bring one repo to its target state.
#[derive(Debug, Clone)]
pub struct RepoTask {
    pub name: String,
    pub url: String,
    /// Absolute checkout path.
    pub path: PathBuf,
    /// Path as recorded in the lock (workspace-relative).
    pub rel_path: PathBuf,
    /// Target commit SHA.
    pub target: String,
    pub source_rev: String,
    /// The real local branch to check out on.
    pub branch: String,
    /// Shared bare mirror to reference at clone time (`--shared` mode).
    pub mirror: Option<PathBuf>,
    /// Partial-clone `--filter` spec (e.g. `blob:none`); keeps all commits.
    pub filter: Option<String>,
    /// Shallow-clone `--depth`; may need deepening to reach an old locked SHA.
    pub depth: Option<u32>,
    /// Recurse git submodules at clone/update time.
    pub submodules: bool,
}

/// Clone-mode tuning applied to every task in a sync plan.
///
/// Resolved by the binary as CLI-flag-over-manifest-`[defaults]`, then handed
/// to [`Workspace::plan_sync`] which copies it onto each [`RepoTask`].
#[derive(Debug, Clone, Default)]
pub struct CloneTuning {
    /// Partial-clone `--filter` spec (e.g. `blob:none`, `tree:0`).
    pub filter: Option<String>,
    /// Shallow-clone `--depth`.
    pub depth: Option<u32>,
    /// Override submodule recursion to true for the whole run (the
    /// `--recurse-submodules` flag). `None` leaves each repo's own setting
    /// (per-repo `submodules` OR `[defaults] submodules`).
    pub submodules: Option<bool>,
}

/// The full set of repo tasks for one stack.
#[derive(Debug, Clone)]
pub struct SyncPlan {
    pub stack: String,
    pub tasks: Vec<RepoTask>,
    /// True when this plan generated and wrote a fresh lockfile.
    pub wrote_lock: bool,
}

/// What `sync_repo` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOutcome {
    Cloned,
    Updated,
    AlreadySynced,
}

/// Observed state of one repo, for `haw status` and the TUI.
#[derive(Debug, Clone)]
pub struct RepoStatus {
    pub name: String,
    /// Workspace-relative path.
    pub path: PathBuf,
    pub missing: bool,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub dirty: bool,
    pub locked_rev: Option<String>,
    /// True when HEAD differs from the locked rev.
    pub drift: bool,
    /// Commits ahead/behind upstream; `None` without an upstream.
    pub ahead_behind: Option<(u64, u64)>,
    pub groups: Vec<String>,
}

/// Local branch name for a locked repo: branches keep their name, tags and
/// SHAs get a `haw/` prefix so the checkout is never detached.
pub fn branch_for(source_rev: &str, kind: RevKind) -> String {
    match kind {
        RevKind::Branch => source_rev.to_string(),
        RevKind::Tag | RevKind::Sha => format!("haw/{}", source_rev.replace('/', "-")),
    }
}

impl Workspace {
    /// Open the workspace rooted at `root` (must contain `haw.toml`).
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, WorkspaceError> {
        let root = root.into();
        let manifest_file = root.join(MANIFEST_FILE);
        if !manifest_file.exists() {
            return Err(WorkspaceError::NotAWorkspace(root));
        }
        let manifest = TomlLoader.load(&manifest_file)?;
        Ok(Self {
            root,
            manifest,
            manifest_file,
        })
    }

    /// Open using an explicit manifest path, which may have any file name or
    /// live outside the workspace root; the root becomes its parent
    /// directory (where `haw.lock` and `.haw/` are still expected).
    pub fn open_manifest(path: impl Into<PathBuf>) -> Result<Self, WorkspaceError> {
        let manifest_file = path.into();
        if !manifest_file.exists() {
            return Err(WorkspaceError::NotAWorkspace(manifest_file));
        }
        let root = manifest_file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let manifest = TomlLoader.load(&manifest_file)?;
        Ok(Self {
            root,
            manifest,
            manifest_file,
        })
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.manifest_file.clone()
    }

    pub fn lock_path(&self) -> PathBuf {
        self.root.join(LOCK_FILE)
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join(STATE_DIR)
    }

    pub fn read_lock(&self) -> Result<Option<Lockfile>, LockError> {
        let path = self.lock_path();
        if path.exists() {
            Lockfile::load(&path).map(Some)
        } else {
            Ok(None)
        }
    }

    /// The stack recorded by the last `haw switch`, if any.
    pub fn current_stack(&self) -> Option<String> {
        std::fs::read_to_string(self.state_dir().join("stack"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn set_current_stack(&self, name: &str) -> Result<(), WorkspaceError> {
        let dir = self.state_dir();
        let path = dir.join("stack");
        std::fs::create_dir_all(&dir).map_err(|source| WorkspaceError::Io { path: dir, source })?;
        std::fs::write(&path, name).map_err(|source| WorkspaceError::Io { path, source })
    }

    /// Pick the stack to operate on: explicit flag > recorded switch >
    /// the only stack > error.
    pub fn pick_stack(&self, flag: Option<&str>) -> Result<String, WorkspaceError> {
        let validate = |name: &str| {
            if self.manifest.stacks.contains_key(name) {
                Ok(name.to_string())
            } else {
                Err(WorkspaceError::UnknownStack(name.to_string()))
            }
        };
        if let Some(name) = flag {
            return validate(name);
        }
        if let Some(name) = self.current_stack() {
            return validate(&name);
        }
        let mut names = self.manifest.stacks.keys();
        match (names.next(), names.next()) {
            (Some(only), None) => Ok(only.clone()),
            _ => Err(WorkspaceError::StackRequired {
                available: self
                    .manifest
                    .stacks
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
            }),
        }
    }

    /// Resolve every manifest repo's rev to a SHA and build a fresh lockfile.
    pub fn make_lock(
        &self,
        overlays: &[String],
        backend: &dyn GitBackend,
    ) -> Result<Lockfile, SyncError> {
        let resolved = resolver::resolve_all(&self.manifest, overlays)?;
        let mut repos = Vec::with_capacity(resolved.len());
        for rb in resolved {
            let r = backend.resolve_rev(&rb.url, &rb.rev)?;
            let branch = branch_for(&rb.rev, r.kind);
            repos.push(LockedRepo {
                name: rb.name,
                url: rb.url,
                path: rb.path,
                rev: r.sha,
                source_rev: rb.rev,
                branch,
                groups: rb.groups,
            });
        }
        Ok(Lockfile {
            version: LOCK_VERSION,
            repos,
        })
    }

    /// Snapshot the workspace: a lockfile pinning every repo to its current
    /// HEAD (and current branch). No network. `haw unpin` (= `haw lock`)
    /// restores the lock to the manifest revs.
    pub fn pin(&self, backend: &dyn GitBackend) -> Result<Lockfile, SyncError> {
        let mut repos = match self.read_lock()? {
            Some(lock) => lock.repos,
            None => resolver::resolve_all(&self.manifest, &[])?
                .into_iter()
                .map(|rb| LockedRepo {
                    name: rb.name,
                    url: rb.url,
                    path: rb.path,
                    rev: String::new(),
                    source_rev: rb.rev,
                    branch: String::new(),
                    groups: rb.groups,
                })
                .collect(),
        };
        for entry in &mut repos {
            let abs = self.root.join(&entry.path);
            if !backend.is_repo(&abs) {
                return Err(SyncError::NotCloned(entry.name.clone()));
            }
            entry.rev = backend.head_sha(&abs)?;
            match backend.current_branch(&abs)? {
                Some(branch) => entry.branch = branch,
                None if entry.branch.is_empty() => {
                    entry.branch = format!("haw/pin-{}", &entry.rev[..8.min(entry.rev.len())]);
                }
                None => {}
            }
        }
        Ok(Lockfile {
            version: LOCK_VERSION,
            repos,
        })
    }

    /// Build the sync plan for `stack`. Uses the existing lock; generates
    /// and writes one when absent. Overlays only apply to lock generation.
    /// A non-empty `groups` filter limits the plan to matching repos.
    /// `cache_root` enables shared object storage: clones reference a bare
    /// mirror kept under it.
    pub fn plan_sync(
        &self,
        stack: &str,
        overlays: &[String],
        groups: &[String],
        cache_root: Option<&std::path::Path>,
        tuning: &CloneTuning,
        backend: &dyn GitBackend,
    ) -> Result<SyncPlan, SyncError> {
        let mut resolution = resolver::resolve(&self.manifest, stack, overlays)?;
        resolver::filter_groups(&mut resolution, groups);
        let (lock, wrote_lock) = match self.read_lock()? {
            Some(lock) => (lock, false),
            None => {
                let lock = self.make_lock(overlays, backend)?;
                lock.save(&self.lock_path())?;
                (lock, true)
            }
        };

        let mut tasks = Vec::with_capacity(resolution.repos.len());
        for rb in &resolution.repos {
            let locked = lock
                .get(&rb.name)
                .ok_or_else(|| SyncError::MissingLockEntry(rb.name.clone()))?;
            tasks.push(RepoTask {
                name: locked.name.clone(),
                url: locked.url.clone(),
                path: self.root.join(&locked.path),
                rel_path: locked.path.clone(),
                target: locked.rev.clone(),
                source_rev: locked.source_rev.clone(),
                branch: locked.branch.clone(),
                mirror: cache_root.map(|root| crate::git::mirror_dir(root, &locked.url)),
                filter: tuning.filter.clone(),
                depth: tuning.depth,
                submodules: tuning.submodules.unwrap_or(rb.submodules),
            });
        }
        Ok(SyncPlan {
            stack: resolution.stack,
            tasks,
            wrote_lock,
        })
    }

    /// Observed state of every repo (lock order when a lock exists).
    /// A non-empty `groups` filter limits the report to matching repos.
    pub fn status(
        &self,
        groups: &[String],
        backend: &dyn GitBackend,
    ) -> Result<Vec<RepoStatus>, SyncError> {
        let entries: Vec<(String, PathBuf, Option<String>, Vec<String>)> = match self.read_lock()? {
            Some(lock) => lock
                .repos
                .iter()
                .filter(|b| resolver::group_match(&b.groups, groups))
                .map(|b| {
                    (
                        b.name.clone(),
                        b.path.clone(),
                        Some(b.rev.clone()),
                        b.groups.clone(),
                    )
                })
                .collect(),
            None => self
                .manifest
                .repos
                .iter()
                .filter(|(_, repo)| resolver::group_match(&repo.groups, groups))
                .map(|(name, repo)| {
                    (
                        name.clone(),
                        repo.checkout_path(name),
                        None,
                        repo.groups.clone(),
                    )
                })
                .collect(),
        };

        let mut statuses = Vec::with_capacity(entries.len());
        for (name, path, locked_rev, repo_groups) in entries {
            let abs = self.root.join(&path);
            if !backend.is_repo(&abs) {
                statuses.push(RepoStatus {
                    name,
                    path,
                    missing: true,
                    branch: None,
                    head: None,
                    dirty: false,
                    locked_rev,
                    drift: false,
                    ahead_behind: None,
                    groups: repo_groups,
                });
                continue;
            }
            let head = backend.head_sha(&abs)?;
            let drift = locked_rev.as_deref().is_some_and(|rev| rev != head);
            statuses.push(RepoStatus {
                name,
                path,
                missing: false,
                branch: backend.current_branch(&abs)?,
                head: Some(head),
                dirty: backend.is_dirty(&abs)?,
                locked_rev,
                drift,
                ahead_behind: backend.ahead_behind(&abs)?,
                groups: repo_groups,
            });
        }
        Ok(statuses)
    }
}

/// Bring one repo to its target state. Safe to run in parallel across repos.
pub fn sync_repo(task: &RepoTask, backend: &dyn GitBackend) -> Result<SyncOutcome, GitError> {
    if !backend.is_repo(&task.path) {
        if let Some(mirror) = &task.mirror {
            backend.ensure_mirror(&task.url, mirror)?;
        }
        let opts = crate::git::CloneOpts {
            reference: task.mirror.clone(),
            filter: task.filter.clone(),
            depth: task.depth,
            submodules: task.submodules,
        };
        backend.clone_repo(&task.url, &task.path, &opts)?;
        backend.checkout(&task.path, &task.target, &task.branch, task.depth)?;
        if task.submodules {
            backend.update_submodules(&task.path)?;
        }
        return Ok(SyncOutcome::Cloned);
    }
    if backend.is_dirty(&task.path)? {
        return Err(GitError::Dirty {
            path: task.path.clone(),
        });
    }
    let on_target = backend.head_sha(&task.path)? == task.target
        && backend.current_branch(&task.path)?.as_deref() == Some(task.branch.as_str());
    if on_target {
        // Even when the superproject is already on target, submodules may be
        // uninitialized on a repo cloned before submodules were enabled.
        if task.submodules {
            backend.update_submodules(&task.path)?;
        }
        return Ok(SyncOutcome::AlreadySynced);
    }
    backend.fetch(&task.path)?;
    // An existing checkout may itself be a shallow clone; pass the depth so
    // checkout can deepen to the locked SHA if the fetch didn't bring it in.
    backend.checkout(&task.path, &task.target, &task.branch, task.depth)?;
    if task.submodules {
        backend.update_submodules(&task.path)?;
    }
    Ok(SyncOutcome::Updated)
}
