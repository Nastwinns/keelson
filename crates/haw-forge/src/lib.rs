//! PR/MR orchestration behind the [`Forge`] trait.
//!
//! [`github::GitHub`] and [`gitlab::GitLab`] implement the trait over plain
//! blocking HTTP; [`orchestrate`] drives the cross-repo changeset lifecycle
//! (request, status, land) forge-agnostically.

pub mod bitbucket;
pub mod github;
pub mod gitlab;
pub mod http;
pub mod orchestrate;

/// Which forge a remote URL belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeKind {
    GitHub,
    GitLab,
    Bitbucket,
    Unknown,
}

/// A PR/MR to open.
#[derive(Debug, Clone)]
pub struct PrSpec {
    pub title: String,
    pub body: String,
    pub source_branch: String,
    pub target_branch: String,
    /// Labels applied to the PR/MR (from `change start --label`).
    pub labels: Vec<String>,
}

/// Handle to an opened PR/MR.
#[derive(Debug, Clone)]
pub struct PrHandle {
    pub url: String,
    pub number: u64,
}

/// Review/merge state of a PR/MR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Draft,
    Merged,
    Closed,
}

/// Aggregated PR/MR status for the dashboard.
#[derive(Debug, Clone)]
pub struct PrStatus {
    pub state: PrState,
    pub approved: bool,
    /// `None` while CI is pending or absent.
    pub ci_passing: Option<bool>,
    pub url: String,
}

/// One open PR/MR, for the fleet-wide PR/MR view (independent of any
/// changeset — every open PR/MR on the repo's forge, not just yours).
#[derive(Debug, Clone)]
pub struct OpenPr {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: PrState,
    pub approved: bool,
    /// `None` while CI is pending or absent.
    pub ci_passing: Option<bool>,
}

/// Hard cap on PRs fetched per repo — keeps the fleet-wide view's request
/// count bounded on repos with a long history of open PRs.
pub const OPEN_PRS_LIMIT: usize = 25;

/// Rendered status of a CI run/pipeline, forge-neutral.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiStatus {
    Passed,
    Failed,
    Running,
    Queued,
    Cancelled,
}

/// One CI run (GitHub Actions) or pipeline (GitLab), for the fleet-wide
/// CI view.
#[derive(Debug, Clone)]
pub struct CiRun {
    /// Run id (GitHub `run["id"]`) or pipeline id (GitLab `pipeline["id"]`);
    /// used to fetch a run's drill-in detail.
    pub id: u64,
    /// Workflow name (GitHub) or `#<pipeline id>` (GitLab).
    pub name: String,
    /// Branch (or ref) the run executed on.
    pub branch: String,
    /// Trigger: `push`, `pull_request`, `schedule`, ... (GitLab: `source`).
    pub event: String,
    pub status: CiStatus,
    pub url: String,
}

/// Hard cap on CI runs fetched per repo — keeps the fleet-wide view's
/// request count bounded on busy repos.
pub const CI_RUNS_LIMIT: usize = 15;

/// One entry in a repository directory listing, forge-neutral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub name: String,
    pub is_dir: bool,
}

/// Which kind of ref a [`ForgeRef`] names. Forge-local (haw-forge can't depend
/// on the TUI's richer `RefKind`); the front-end maps this onto its own enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeRefKind {
    Branch,
    Tag,
}

/// One selectable ref (branch or tag) on a forge repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeRef {
    pub name: String,
    pub kind: ForgeRefKind,
}

/// Cap on the number of file paths [`Forge::repo_file_paths`] returns before
/// truncation — keeps the recursive-tree fetch bounded on large repos.
pub const FILE_PATHS_CAP: usize = 5000;

/// Cap on the number of refs of each kind (branches, tags) that
/// [`Forge::list_refs`] returns — keeps the ref picker's fetch bounded.
pub const REFS_CAP: usize = 200;

/// One file changed by a PR/MR, forge-neutral. `status` is one of
/// `"added"`, `"modified"`, `"removed"`, or `"renamed"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrFile {
    pub path: String,
    pub status: String,
}

/// Cap on the number of lines a `file_blob` returns before truncation.
pub const FILE_LINE_CAP: usize = 600;

/// Errors from a forge implementation.
#[derive(Debug, thiserror::Error)]
pub enum ForgeError {
    #[error("{0} support is not implemented yet")]
    NotImplemented(&'static str),
    #[error("no forge recognized for {0}; only GitHub, GitLab, and Bitbucket are supported")]
    UnknownForge(String),
    #[error("cannot extract a repository path from {0}")]
    UnsupportedUrl(String),
    #[error("no {0} token; set {1}")]
    MissingToken(&'static str, &'static str),
    #[error("forge API error: {0}")]
    Api(String),
}

/// One forge (GitHub, GitLab, ...) driving PR/MRs for a repository URL.
pub trait Forge {
    fn open_pr(&self, repo_url: &str, spec: &PrSpec) -> Result<PrHandle, ForgeError>;
    fn pr_status(&self, repo_url: &str, number: u64) -> Result<PrStatus, ForgeError>;
    fn merge_pr(&self, repo_url: &str, number: u64) -> Result<(), ForgeError>;
    /// Approve the PR/MR on its forge (GitHub review event / GitLab approve).
    fn approve_pr(&self, repo_url: &str, number: u64) -> Result<(), ForgeError>;
    /// Rewrite the PR/MR description (used for cross-linking a changeset).
    fn update_pr_body(&self, repo_url: &str, number: u64, body: &str) -> Result<(), ForgeError>;
    /// Every open PR/MR on the repo, capped at [`OPEN_PRS_LIMIT`] (fleet-wide
    /// PR/MR view — independent of any changeset).
    fn list_open_prs(&self, repo_url: &str) -> Result<Vec<OpenPr>, ForgeError>;
    /// Recent CI runs/pipelines on the repo, newest first, capped at
    /// [`CI_RUNS_LIMIT`] (fleet-wide CI view).
    fn list_ci_runs(&self, repo_url: &str) -> Result<Vec<CiRun>, ForgeError>;
    /// A readable, plain-text drill-in report for one PR/MR: header, reviewers,
    /// checks, and body. No ANSI — the caller styles it.
    fn pr_detail(&self, repo_url: &str, number: u64) -> Result<String, ForgeError>;
    /// A readable, plain-text drill-in report for one CI run/pipeline: header,
    /// jobs, and steps. No ANSI — the caller styles it.
    fn ci_run_detail(&self, repo_url: &str, run_id: u64) -> Result<String, ForgeError>;
    /// The unified diff for one PR/MR as plain text, capped to a readable length
    /// (see [`DIFF_LINE_CAP`]). No ANSI — the caller styles it.
    fn pr_diff(&self, repo_url: &str, number: u64) -> Result<String, ForgeError>;
    /// The CI run/pipeline's job logs as plain text, concatenated with per-job
    /// headers and capped to a readable length (see [`LOG_LINE_CAP`]). Returns a
    /// clear message string (not an error) when logs are unavailable/expired.
    fn ci_logs(&self, repo_url: &str, run_id: u64) -> Result<String, ForgeError>;
    /// List the entries of `subpath` ("" = repo root) in the repo tree at
    /// `git_ref` (a branch/tag/sha; the forge default when `None`). Directories
    /// first is the caller's concern; this returns them forge-order.
    fn repo_tree(
        &self,
        repo_url: &str,
        subpath: &str,
        git_ref: Option<&str>,
    ) -> Result<Vec<TreeEntry>, ForgeError>;
    /// The raw text of the file at `path` in the repo tree at `git_ref`, capped
    /// to a readable length (see [`FILE_LINE_CAP`]). No ANSI.
    fn file_blob(
        &self,
        repo_url: &str,
        path: &str,
        git_ref: Option<&str>,
    ) -> Result<String, ForgeError>;
    /// The list of files changed by one PR/MR, capped at [`OPEN_PRS_LIMIT`]-scale
    /// pagination (see the impls). Each carries its path and change status.
    fn pr_files(&self, repo_url: &str, number: u64) -> Result<Vec<PrFile>, ForgeError>;
    /// The full text of a file changed by a PR/MR, resolved AT THE PR's head
    /// ref, capped to a readable length (see [`FILE_LINE_CAP`]). A file that is
    /// absent at that ref (e.g. a removed file, or a 404) returns a short note
    /// string, not an error.
    fn pr_file_content(
        &self,
        repo_url: &str,
        number: u64,
        path: &str,
    ) -> Result<String, ForgeError>;
    /// Every FILE path in the repo tree at `git_ref` (the forge default when
    /// `None`), as posix `/`-separated paths, recursive, capped + deduped at
    /// [`FILE_PATHS_CAP`]. Directories are omitted — the caller reconstructs the
    /// tree client-side. Used by the file browser's tree mode.
    fn repo_file_paths(
        &self,
        repo_url: &str,
        git_ref: Option<&str>,
    ) -> Result<Vec<String>, ForgeError>;
    /// The repo's branches then tags (each capped at [`REFS_CAP`]) for the ref
    /// picker. HEAD/default is left to the caller to surface.
    fn list_refs(&self, repo_url: &str) -> Result<Vec<ForgeRef>, ForgeError>;
}

/// Cap on the number of lines a `pr_diff` returns before truncation.
pub const DIFF_LINE_CAP: usize = 600;

/// Cap on the number of lines a `ci_logs` returns before truncation.
pub const LOG_LINE_CAP: usize = 800;

/// Truncate `text` to at most `cap` lines, appending a truncation note that
/// reports how many lines were dropped. A no-op when within the cap.
pub fn cap_lines(text: &str, cap: usize) -> String {
    let total = text.lines().count();
    if total <= cap {
        return text.to_string();
    }
    let mut out: String = text.lines().take(cap).collect::<Vec<_>>().join("\n");
    out.push_str(&format!("\n… (truncated, {} more line(s))\n", total - cap));
    out
}

/// Width (in cells) of the ASCII progress bar rendered in CI run details.
pub const PROGRESS_BAR_WIDTH: usize = 15;

/// Render an ASCII progress bar like `[██████████░░░░░]` filled to
/// `completed`/`total`. Kept ASCII (block chars only) so it works in every
/// terminal and in the plain-text detail view. A zero `total` renders empty.
pub fn progress_bar(completed: usize, total: usize) -> String {
    let completed = completed.min(total);
    let filled = (completed * PROGRESS_BAR_WIDTH)
        .checked_div(total)
        .unwrap_or(0);
    let percent = (completed * 100).checked_div(total).unwrap_or(0);
    let empty = PROGRESS_BAR_WIDTH - filled;
    format!(
        "[{}{}] {completed}/{total} jobs ({percent}%)",
        "█".repeat(filled),
        "░".repeat(empty),
    )
}

/// Turns a repo URL into a ready-to-call [`Forge`] client.
/// The production impl is [`Tokens`]; tests substitute fakes.
/// `hint` is the manifest's explicit `forge = "github" | "gitlab"` key on the
/// repo's remote; it wins over URL detection.
pub trait ForgeFactory: Sync {
    fn client_for(&self, url: &str, hint: Option<ForgeKind>) -> Result<Box<dyn Forge>, ForgeError>;
}

/// Parse a manifest `forge =` value.
pub fn kind_from_key(key: &str) -> Option<ForgeKind> {
    match key {
        "github" => Some(ForgeKind::GitHub),
        "gitlab" => Some(ForgeKind::GitLab),
        "bitbucket" => Some(ForgeKind::Bitbucket),
        _ => None,
    }
}

/// API tokens, usually read from the environment by the front-end.
#[derive(Clone, Default)]
pub struct Tokens {
    pub github: Option<String>,
    pub gitlab: Option<String>,
    pub bitbucket: Option<String>,
    /// When set alongside a Bitbucket token, auth switches from Bearer to HTTP
    /// Basic (`user:token`).
    pub bitbucket_user: Option<String>,
}

/// Redacting `Debug`: never prints the raw secret values, only whether each
/// token is set, so a stray `{:?}`/`dbg!`/error-context dump can't leak them.
/// `bitbucket_user` is not a secret and is shown verbatim.
impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redact = |opt: &Option<String>| if opt.is_some() { "<redacted>" } else { "None" };
        f.debug_struct("Tokens")
            .field("github", &redact(&self.github))
            .field("gitlab", &redact(&self.gitlab))
            .field("bitbucket", &redact(&self.bitbucket))
            .field("bitbucket_user", &self.bitbucket_user)
            .finish()
    }
}

impl Tokens {
    /// Read tokens from the conventional environment variables.
    /// `HAW_FORGE_TOKEN` is the generic fallback for both forges; a GitHub
    /// token is also reused from a logged-in `gh` CLI when the env is empty.
    pub fn from_env() -> Self {
        let first = |names: &[&str]| {
            names
                .iter()
                .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
        };
        let github = first(&[
            "HAW_GITHUB_TOKEN",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "HAW_FORGE_TOKEN",
        ])
        .or_else(gh_cli_token);
        let gitlab = first(&["HAW_GITLAB_TOKEN", "GITLAB_TOKEN", "HAW_FORGE_TOKEN"]);
        let bitbucket = first(&["HAW_BITBUCKET_TOKEN", "BITBUCKET_TOKEN", "HAW_FORGE_TOKEN"]);
        let bitbucket_user = first(&["BITBUCKET_USER"]);
        Self {
            github,
            gitlab,
            bitbucket,
            bitbucket_user,
        }
    }
}

/// Token of a logged-in `gh` CLI, if one is on PATH.
fn gh_cli_token() -> Option<String> {
    let output = std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!token.is_empty()).then_some(token)
}

impl ForgeFactory for Tokens {
    fn client_for(&self, url: &str, hint: Option<ForgeKind>) -> Result<Box<dyn Forge>, ForgeError> {
        match hint.unwrap_or_else(|| detect(url)) {
            ForgeKind::GitHub => {
                let token = self.github.clone().ok_or(ForgeError::MissingToken(
                    "GitHub",
                    "HAW_GITHUB_TOKEN or GITHUB_TOKEN",
                ))?;
                Ok(Box::new(github::GitHub::new(token)?))
            }
            ForgeKind::GitLab => {
                let token = self.gitlab.clone().ok_or(ForgeError::MissingToken(
                    "GitLab",
                    "HAW_GITLAB_TOKEN or GITLAB_TOKEN",
                ))?;
                Ok(Box::new(gitlab::GitLab::new(token)))
            }
            ForgeKind::Bitbucket => {
                let token = self.bitbucket.clone().ok_or(ForgeError::MissingToken(
                    "Bitbucket",
                    "HAW_BITBUCKET_TOKEN or BITBUCKET_TOKEN",
                ))?;
                Ok(Box::new(bitbucket::Bitbucket::new(
                    token,
                    self.bitbucket_user.clone(),
                )))
            }
            ForgeKind::Unknown => Err(ForgeError::UnknownForge(url.to_string())),
        }
    }
}

/// Host + repository path (`owner/repo`, or a nested GitLab group path)
/// extracted from a clone URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoCoords {
    pub host: String,
    pub path: String,
}

/// Parse an HTTP(S), ssh://, or scp-like (`git@host:path`) clone URL.
pub fn repo_coords(url: &str) -> Option<RepoCoords> {
    let (host, raw_path) = if let Some((_, after_scheme)) = url.split_once("://") {
        let (authority, path) = after_scheme.split_once('/')?;
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = host.split(':').next().unwrap_or(host);
        (host, path)
    } else if let Some((user_host, path)) = url.split_once(':') {
        let (_, host) = user_host.rsplit_once('@')?;
        (host, path)
    } else {
        return None;
    };
    let path = raw_path
        .trim_start_matches('/')
        .trim_end_matches('/')
        .trim_end_matches(".git");
    if host.is_empty() || path.is_empty() || !path.contains('/') {
        return None;
    }
    Some(RepoCoords {
        host: host.to_string(),
        path: path.to_string(),
    })
}

/// Host part of an HTTP(S), ssh://, or scp-like (`git@host:path`) git URL.
fn host_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    if let Some((_, after_scheme)) = url.split_once("://") {
        let authority = after_scheme.split(['/', '?']).next()?;
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        return Some(host.split(':').next().unwrap_or(host));
    }
    if let Some((user_host, _path)) = rest.split_once(':')
        && let Some((_, host)) = user_host.rsplit_once('@')
    {
        return Some(host);
    }
    None
}

/// Guess the forge from a remote URL. Self-hosted instances are matched by
/// hostname substring; anything else needs explicit configuration later.
pub fn detect(url: &str) -> ForgeKind {
    match host_of(url) {
        Some(host) if host.contains("github") => ForgeKind::GitHub,
        Some(host) if host.contains("gitlab") => ForgeKind::GitLab,
        Some(host) if host.contains("bitbucket") => ForgeKind::Bitbucket,
        _ => ForgeKind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ForgeKind, PROGRESS_BAR_WIDTH, RepoCoords, Tokens, cap_lines, detect, progress_bar,
        repo_coords,
    };

    #[test]
    fn tokens_debug_redacts_secrets() {
        let tokens = Tokens {
            github: Some("ghp_supersecret".to_string()),
            gitlab: Some("glpat-topsecret".to_string()),
            bitbucket: Some("bb-hunter2".to_string()),
            bitbucket_user: Some("ada".to_string()),
        };
        let dumped = format!("{tokens:?}");
        assert!(!dumped.contains("ghp_supersecret"), "{dumped}");
        assert!(!dumped.contains("glpat-topsecret"), "{dumped}");
        assert!(!dumped.contains("bb-hunter2"), "{dumped}");
        assert!(dumped.contains("<redacted>"), "{dumped}");
        // Non-secret fields are fine to show.
        assert!(dumped.contains("ada"), "{dumped}");
    }

    #[test]
    fn progress_bar_reports_ratio_percent_and_fill() {
        let bar = progress_bar(6, 9);
        assert!(bar.contains("6/9"), "{bar}");
        assert!(bar.contains("66%"), "{bar}");
        // 6/9 * 15 = 10 filled cells, 5 empty.
        assert_eq!(bar.matches('█').count(), 10, "{bar}");
        assert_eq!(bar.matches('░').count(), 5, "{bar}");
    }

    #[test]
    fn progress_bar_full_and_empty_edges() {
        let full = progress_bar(9, 9);
        assert!(full.contains("9/9"));
        assert!(full.contains("100%"));
        assert_eq!(full.matches('█').count(), PROGRESS_BAR_WIDTH);
        // A zero total never divides by zero and renders an empty bar.
        let none = progress_bar(0, 0);
        assert!(none.contains("0/0"));
        assert!(none.contains("0%"));
        assert_eq!(none.matches('░').count(), PROGRESS_BAR_WIDTH);
    }

    #[test]
    fn cap_lines_is_noop_within_cap() {
        assert_eq!(cap_lines("a\nb\nc", 5), "a\nb\nc");
        assert_eq!(cap_lines("a\nb\nc", 3), "a\nb\nc");
    }

    #[test]
    fn cap_lines_truncates_and_notes_the_remainder() {
        let text = "l1\nl2\nl3\nl4\nl5";
        let out = cap_lines(text, 2);
        assert!(out.starts_with("l1\nl2\n"));
        assert!(out.contains("truncated, 3 more line(s)"));
        assert!(!out.contains("l3"));
    }

    #[test]
    fn detects_github() {
        assert_eq!(detect("https://github.com/acme/x.git"), ForgeKind::GitHub);
        assert_eq!(detect("git@github.com:acme/x.git"), ForgeKind::GitHub);
        assert_eq!(
            detect("ssh://git@github.enterprise.local/acme/x.git"),
            ForgeKind::GitHub
        );
    }

    #[test]
    fn detects_gitlab_including_self_hosted() {
        assert_eq!(detect("https://gitlab.com/acme/x.git"), ForgeKind::GitLab);
        assert_eq!(
            detect("git@gitlab.company.com:firmware/kernel.git"),
            ForgeKind::GitLab
        );
    }

    #[test]
    fn detects_bitbucket_including_self_hosted() {
        assert_eq!(
            detect("https://bitbucket.org/acme/x.git"),
            ForgeKind::Bitbucket
        );
        assert_eq!(detect("git@bitbucket.org:acme/x.git"), ForgeKind::Bitbucket);
        assert_eq!(
            detect("ssh://git@bitbucket.company.com/team/repo.git"),
            ForgeKind::Bitbucket
        );
    }

    #[test]
    fn bitbucket_coords_parse_workspace_and_slug() {
        assert_eq!(
            repo_coords("https://bitbucket.org/acme/widget.git"),
            Some(RepoCoords {
                host: "bitbucket.org".into(),
                path: "acme/widget".into(),
            })
        );
        assert_eq!(
            repo_coords("git@bitbucket.org:acme/widget.git"),
            Some(RepoCoords {
                host: "bitbucket.org".into(),
                path: "acme/widget".into(),
            })
        );
    }

    #[test]
    fn unknown_for_everything_else() {
        assert_eq!(detect("/tmp/local/repo"), ForgeKind::Unknown);
        assert_eq!(detect("file:///tmp/local/repo"), ForgeKind::Unknown);
    }

    #[test]
    fn coords_from_every_url_shape() {
        let expect = |host: &str, path: &str| {
            Some(RepoCoords {
                host: host.into(),
                path: path.into(),
            })
        };
        assert_eq!(
            repo_coords("https://github.com/acme/x.git"),
            expect("github.com", "acme/x")
        );
        assert_eq!(
            repo_coords("git@github.com:acme/x.git"),
            expect("github.com", "acme/x")
        );
        assert_eq!(
            repo_coords("ssh://git@gitlab.company.com/fw/nested/kernel.git"),
            expect("gitlab.company.com", "fw/nested/kernel")
        );
        assert_eq!(repo_coords("/tmp/local/repo"), None);
    }
}
