//! PR/MR orchestration behind the [`Forge`] trait.
//!
//! [`github::GitHub`] and [`gitlab::GitLab`] implement the trait over plain
//! blocking HTTP; [`orchestrate`] drives the cross-repo changeset lifecycle
//! (request, status, land) forge-agnostically.

pub mod github;
pub mod gitlab;
pub mod orchestrate;

/// Which forge a remote URL belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeKind {
    GitHub,
    GitLab,
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

/// Errors from a forge implementation.
#[derive(Debug, thiserror::Error)]
pub enum ForgeError {
    #[error("{0} support is not implemented yet")]
    NotImplemented(&'static str),
    #[error("no forge recognized for {0}; only GitHub and GitLab are supported")]
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
    /// Rewrite the PR/MR description (used for cross-linking a changeset).
    fn update_pr_body(&self, repo_url: &str, number: u64, body: &str) -> Result<(), ForgeError>;
    /// Every open PR/MR on the repo, capped at [`OPEN_PRS_LIMIT`] (fleet-wide
    /// PR/MR view — independent of any changeset).
    fn list_open_prs(&self, repo_url: &str) -> Result<Vec<OpenPr>, ForgeError>;
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
        _ => None,
    }
}

/// API tokens, usually read from the environment by the front-end.
#[derive(Debug, Clone, Default)]
pub struct Tokens {
    pub github: Option<String>,
    pub gitlab: Option<String>,
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
        Self { github, gitlab }
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
        _ => ForgeKind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::{ForgeKind, RepoCoords, detect, repo_coords};

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
    fn unknown_for_everything_else() {
        assert_eq!(
            detect("https://bitbucket.org/acme/x.git"),
            ForgeKind::Unknown
        );
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
