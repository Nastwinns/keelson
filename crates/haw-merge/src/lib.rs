//! Optional mergetopus-style collaborative merge (Phase 6).
//!
//! A big, conflict-heavy merge is hard to review as one lump. Keelson slices it
//! into disjoint units by top-level path, so the conflicts can be resolved (and
//! reviewed) piece by piece, then sealed into a single clean merge commit.
//!
//! The merge runs on a dedicated integration branch: the target branch is only
//! fast-forwarded onto it at [`cleanup`], so the whole operation stays abortable
//! and never leaves the target branch half-merged.
//!
//! Git side effects go through the [`MergeBackend`] trait; [`git::GitMerge`] is
//! the production shell-out impl. State lives in `<state_dir>/merge/<repo>.toml`.

pub mod git;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// State-file schema version. Unknown versions are a hard error.
pub const PLAN_VERSION: u32 = 1;

/// Which side of a conflict to accept when auto-resolving a slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The integration branch (the branch being merged into).
    Ours,
    /// The incoming source branch.
    Theirs,
}

/// Errors in the collaborative merge workflow.
#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    #[error("could not run git: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("`{context}` failed: {stderr}")]
    Command { context: String, stderr: String },
    #[error("failed to access {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid merge plan TOML")]
    Parse(#[source] Box<toml::de::Error>),
    #[error("could not serialize merge plan")]
    Serialize(#[from] toml::ser::Error),
    #[error("unsupported merge plan version {0}; upgrade haw")]
    UnsupportedVersion(u32),
    #[error("{path} has uncommitted changes; commit or stash them first")]
    Dirty { path: PathBuf },
    #[error("{0} is on a detached HEAD; check out a branch first")]
    Detached(PathBuf),
    #[error("a merge is already planned for `{0}`; run `haw merge cleanup` or `abort` first")]
    PlanExists(String),
    #[error("no merge planned for `{0}`; run `haw merge plan <source>` first")]
    NoPlan(String),
    #[error("slice `{0}` is not in the plan")]
    UnknownSlice(String),
    #[error("no merge in progress; the plan is stale — run `haw merge abort`")]
    NoMergeInProgress,
    #[error("`{incoming}` is already merged into `{target}`; nothing to do")]
    NothingToMerge { incoming: String, target: String },
    #[error("unresolved slice(s): {}; resolve them before cleanup", .0.join(", "))]
    Unresolved(Vec<String>),
}

/// One disjoint unit of a sliced merge: the conflicting paths sharing a
/// top-level component, resolved and reviewed together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slice {
    pub name: String,
    pub paths: Vec<PathBuf>,
    #[serde(default)]
    pub resolved: bool,
}

/// A planned collaborative merge, persisted between CLI invocations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergePlan {
    pub version: u32,
    /// Manifest repo name this merge belongs to.
    pub repo: String,
    /// The branch being merged in.
    pub source: String,
    /// The branch the merge is being resolved on.
    pub integration: String,
    /// The branch that gets fast-forwarded at cleanup.
    pub target: String,
    /// HEAD of `target` when the merge was planned.
    pub base: String,
    pub slices: Vec<Slice>,
}

impl MergePlan {
    /// Names of slices still awaiting resolution.
    pub fn unresolved(&self) -> Vec<String> {
        self.slices
            .iter()
            .filter(|s| !s.resolved)
            .map(|s| s.name.clone())
            .collect()
    }

    fn slice_mut(&mut self, name: &str) -> Result<&mut Slice, MergeError> {
        self.slices
            .iter_mut()
            .find(|s| s.name == name)
            .ok_or_else(|| MergeError::UnknownSlice(name.to_string()))
    }
}

/// Every git operation the collaborative merge needs, behind one trait so the
/// orchestration is testable against a real repo or a fake.
pub trait MergeBackend {
    fn current_branch(&self, repo: &Path) -> Result<Option<String>, MergeError>;
    fn head_sha(&self, repo: &Path) -> Result<String, MergeError>;
    fn is_dirty(&self, repo: &Path) -> Result<bool, MergeError>;
    fn branch_exists(&self, repo: &Path, name: &str) -> Result<bool, MergeError>;
    /// `git checkout -b <name> <start>` — create and switch to `name` at `start`.
    fn create_branch_at(&self, repo: &Path, name: &str, start: &str) -> Result<(), MergeError>;
    /// `git checkout <name>` — switch to an existing branch.
    fn switch_branch(&self, repo: &Path, name: &str) -> Result<(), MergeError>;
    /// `git merge --no-commit --no-ff <source>`. Returns the conflicting paths
    /// (empty when the merge is clean and fully staged).
    fn start_merge(&self, repo: &Path, source: &str) -> Result<Vec<PathBuf>, MergeError>;
    fn conflicted_paths(&self, repo: &Path) -> Result<Vec<PathBuf>, MergeError>;
    /// Accept one side of `path`'s conflict and stage it.
    fn take_side(&self, repo: &Path, path: &Path, side: Side) -> Result<(), MergeError>;
    /// Stage `paths` (marks conflicts the user resolved by hand as done).
    fn stage(&self, repo: &Path, paths: &[PathBuf]) -> Result<(), MergeError>;
    /// Complete the in-progress merge. `message` = None uses the default merge message.
    fn commit(&self, repo: &Path, message: Option<&str>) -> Result<(), MergeError>;
    fn merge_in_progress(&self, repo: &Path) -> Result<bool, MergeError>;
    fn abort_merge(&self, repo: &Path) -> Result<(), MergeError>;
    /// `git merge --ff-only <from>` on the current branch.
    fn fast_forward(&self, repo: &Path, from: &str) -> Result<(), MergeError>;
    fn delete_branch(&self, repo: &Path, name: &str) -> Result<(), MergeError>;
}

/// A default integration branch name for merging `source`.
pub fn integration_branch(source: &str) -> String {
    format!("haw/merge/{}", sanitize(source))
}

fn sanitize(s: &str) -> String {
    s.replace(['/', ' ', ':'], "-")
}

/// Partition conflicting paths into slices by their top-level component.
/// Deterministic: slices and their paths are sorted. Root-level files land in a
/// `root` slice. Pure — no git, no I/O.
pub fn slice_conflicts(paths: &[PathBuf]) -> Vec<Slice> {
    let mut groups: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for path in paths {
        let key = path
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .filter(|_| path.components().count() > 1)
            .unwrap_or_else(|| "root".to_string());
        groups.entry(key).or_default().push(path.clone());
    }
    groups
        .into_iter()
        .map(|(name, mut paths)| {
            paths.sort();
            paths.dedup();
            Slice {
                name,
                paths,
                resolved: false,
            }
        })
        .collect()
}

fn plan_path(state_dir: &Path, repo: &str) -> PathBuf {
    state_dir.join("merge").join(format!("{repo}.toml"))
}

/// Load the merge plan for `repo`, if one exists.
pub fn load_plan(state_dir: &Path, repo: &str) -> Result<Option<MergePlan>, MergeError> {
    let path = plan_path(state_dir, repo);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|source| MergeError::Io { path, source })?;
    let plan: MergePlan =
        toml::from_str(&text).map_err(|source| MergeError::Parse(Box::new(source)))?;
    if plan.version != PLAN_VERSION {
        return Err(MergeError::UnsupportedVersion(plan.version));
    }
    Ok(Some(plan))
}

fn save_plan(state_dir: &Path, plan: &MergePlan) -> Result<(), MergeError> {
    let dir = state_dir.join("merge");
    std::fs::create_dir_all(&dir).map_err(|source| MergeError::Io {
        path: dir.clone(),
        source,
    })?;
    let path = plan_path(state_dir, &plan.repo);
    let text = toml::to_string_pretty(plan)?;
    std::fs::write(&path, text).map_err(|source| MergeError::Io { path, source })
}

fn remove_plan(state_dir: &Path, repo: &str) -> Result<(), MergeError> {
    let path = plan_path(state_dir, repo);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|source| MergeError::Io { path, source })?;
    }
    Ok(())
}

/// Plan a collaborative merge of `source` into the current branch of `repo`.
///
/// Creates an integration branch, starts the merge on it (leaving conflicts in
/// the worktree), and slices the conflicts for piecewise resolution. `into`
/// overrides the integration branch name.
pub fn plan(
    be: &dyn MergeBackend,
    repo_path: &Path,
    state_dir: &Path,
    repo: &str,
    source: &str,
    into: Option<&str>,
) -> Result<MergePlan, MergeError> {
    if load_plan(state_dir, repo)?.is_some() {
        return Err(MergeError::PlanExists(repo.to_string()));
    }
    if be.is_dirty(repo_path)? {
        return Err(MergeError::Dirty {
            path: repo_path.to_path_buf(),
        });
    }
    let target = be
        .current_branch(repo_path)?
        .ok_or_else(|| MergeError::Detached(repo_path.to_path_buf()))?;
    let base = be.head_sha(repo_path)?;
    let integration = into.map_or_else(|| integration_branch(source), str::to_string);

    be.create_branch_at(repo_path, &integration, &target)?;

    let conflicts = match be.start_merge(repo_path, source) {
        Ok(conflicts) => conflicts,
        Err(err) => {
            let _ = be.switch_branch(repo_path, &target);
            let _ = be.delete_branch(repo_path, &integration);
            return Err(err);
        }
    };

    if conflicts.is_empty() && !be.merge_in_progress(repo_path)? {
        be.switch_branch(repo_path, &target)?;
        be.delete_branch(repo_path, &integration)?;
        return Err(MergeError::NothingToMerge {
            incoming: source.to_string(),
            target,
        });
    }

    let slices = slice_conflicts(&conflicts);
    let plan = MergePlan {
        version: PLAN_VERSION,
        repo: repo.to_string(),
        source: source.to_string(),
        integration,
        target,
        base,
        slices,
    };
    save_plan(state_dir, &plan)?;
    Ok(plan)
}

/// Resolve one slice of the in-progress merge. With `take`, every path in the
/// slice is auto-resolved to that side; without it, the slice's paths are staged
/// as-is (the user having edited them by hand). Returns the updated plan.
pub fn resolve(
    be: &dyn MergeBackend,
    repo_path: &Path,
    state_dir: &Path,
    repo: &str,
    slice: &str,
    take: Option<Side>,
) -> Result<MergePlan, MergeError> {
    let mut plan =
        load_plan(state_dir, repo)?.ok_or_else(|| MergeError::NoPlan(repo.to_string()))?;
    if !be.merge_in_progress(repo_path)? {
        return Err(MergeError::NoMergeInProgress);
    }
    let entry = plan.slice_mut(slice)?;
    match take {
        Some(side) => {
            for path in &entry.paths {
                be.take_side(repo_path, path, side)?;
            }
        }
        None => be.stage(repo_path, &entry.paths)?,
    }
    entry.resolved = true;
    save_plan(state_dir, &plan)?;
    Ok(plan)
}

/// Outcome of a completed collaborative merge.
#[derive(Debug, Clone, PartialEq)]
pub struct CleanupReport {
    pub target: String,
    pub integration: String,
    pub merge_sha: String,
    pub slices: usize,
}

/// Seal the merge: require every slice resolved, commit the merge on the
/// integration branch, fast-forward the target onto it, then delete the
/// integration branch and clear the plan.
pub fn cleanup(
    be: &dyn MergeBackend,
    repo_path: &Path,
    state_dir: &Path,
    repo: &str,
    message: Option<&str>,
) -> Result<CleanupReport, MergeError> {
    let plan = load_plan(state_dir, repo)?.ok_or_else(|| MergeError::NoPlan(repo.to_string()))?;
    if !be.merge_in_progress(repo_path)? {
        return Err(MergeError::NoMergeInProgress);
    }
    let unresolved = plan.unresolved();
    if !unresolved.is_empty() {
        return Err(MergeError::Unresolved(unresolved));
    }
    let remaining = be.conflicted_paths(repo_path)?;
    if !remaining.is_empty() {
        return Err(MergeError::Unresolved(
            remaining
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        ));
    }

    be.commit(repo_path, message)?;
    let merge_sha = be.head_sha(repo_path)?;
    be.switch_branch(repo_path, &plan.target)?;
    be.fast_forward(repo_path, &plan.integration)?;
    be.delete_branch(repo_path, &plan.integration)?;
    remove_plan(state_dir, repo)?;

    Ok(CleanupReport {
        target: plan.target,
        integration: plan.integration,
        merge_sha,
        slices: plan.slices.len(),
    })
}

/// Abort a planned merge: undo the in-progress merge, return to the target
/// branch, delete the integration branch, and clear the plan.
pub fn abort(
    be: &dyn MergeBackend,
    repo_path: &Path,
    state_dir: &Path,
    repo: &str,
) -> Result<MergePlan, MergeError> {
    let plan = load_plan(state_dir, repo)?.ok_or_else(|| MergeError::NoPlan(repo.to_string()))?;
    if be.merge_in_progress(repo_path)? {
        be.abort_merge(repo_path)?;
    }
    if be.current_branch(repo_path)?.as_deref() == Some(plan.integration.as_str()) {
        be.switch_branch(repo_path, &plan.target)?;
    }
    if be.branch_exists(repo_path, &plan.integration)? {
        be.delete_branch(repo_path, &plan.integration)?;
    }
    remove_plan(state_dir, repo)?;
    Ok(plan)
}
