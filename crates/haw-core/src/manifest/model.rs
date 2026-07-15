use std::path::PathBuf;
use std::str::FromStr;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::ManifestError;

/// The parsed `haw.toml`: remotes, repos, stacks, overlays.
///
/// User-facing lexicon: `[repo.NAME]` and `[stack.NAME]`. The original
/// `brick`/`product` spellings still parse as aliases; serialization emits
/// the canonical `repo`/`stack`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default, rename = "remote", skip_serializing_if = "IndexMap::is_empty")]
    pub remotes: IndexMap<String, Remote>,
    #[serde(
        default,
        rename = "repo",
        alias = "brick",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub repos: IndexMap<String, Repo>,
    #[serde(
        default,
        rename = "stack",
        alias = "product",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub stacks: IndexMap<String, Stack>,
    #[serde(
        default,
        rename = "overlay",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub overlays: IndexMap<String, Overlay>,
}

/// A named base URL repos can be cloned from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Remote {
    pub url: String,
    /// Explicit forge (`"github"` | `"gitlab"`) for hosts the URL heuristic
    /// misses; optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forge: Option<String>,
}

/// One Git repository: a full autonomous clone at a declared path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub rev: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,
    /// Repos this repo depends on; drives the topological order of
    /// `haw change land`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<String>,
    /// Shell command `haw build` runs in this repo (haw stays
    /// build-system-agnostic: it only shells out).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
    /// Shell command `haw test` runs in this repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test: Option<String>,
}

impl Repo {
    /// Checkout path in the workspace; defaults to the repo's name.
    pub fn checkout_path(&self, name: &str) -> PathBuf {
        self.path.clone().unwrap_or_else(|| PathBuf::from(name))
    }

    /// Full clone URL, either declared directly or joined from a named remote.
    pub fn clone_url(&self, remotes: &IndexMap<String, Remote>) -> Option<String> {
        if let Some(url) = &self.url {
            return Some(url.clone());
        }
        let remote = remotes.get(self.remote.as_deref()?)?;
        let repo = self.repo.as_deref()?;
        Some(format!("{}/{}", remote.url.trim_end_matches('/'), repo))
    }
}

/// A named composition (a "stack"): the set of repos it is built from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stack {
    #[serde(rename = "repos", alias = "bricks")]
    pub repos: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Named per-repo overrides applied on top of the manifest at resolve time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    #[serde(
        default,
        rename = "repo",
        alias = "brick",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub repos: IndexMap<String, RepoOverride>,
}

/// The fields an overlay may override on one repo.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl Manifest {
    /// Check referential integrity: repo sources, remote names, stack and
    /// overlay repo references.
    pub fn validate(&self) -> Result<(), ManifestError> {
        for (name, remote) in &self.remotes {
            if let Some(forge) = &remote.forge
                && !matches!(forge.as_str(), "github" | "gitlab")
            {
                return Err(ManifestError::UnknownForge {
                    remote: name.clone(),
                    forge: forge.clone(),
                });
            }
        }
        for (name, repo) in &self.repos {
            for dep in &repo.deps {
                if !self.repos.contains_key(dep) {
                    return Err(ManifestError::UnknownDep {
                        repo: name.clone(),
                        dep: dep.clone(),
                    });
                }
            }
        }
        for (name, repo) in &self.repos {
            match (&repo.url, &repo.remote, &repo.repo) {
                (Some(_), None, None) => {}
                (Some(_), _, _) => {
                    return Err(ManifestError::AmbiguousSource(name.clone()));
                }
                (None, Some(remote), Some(_)) => {
                    if !self.remotes.contains_key(remote) {
                        return Err(ManifestError::UnknownRemote {
                            repo: name.clone(),
                            remote: remote.clone(),
                        });
                    }
                }
                (None, _, _) => {
                    return Err(ManifestError::MissingSource(name.clone()));
                }
            }
        }
        for (name, stack) in &self.stacks {
            for repo in &stack.repos {
                if !self.repos.contains_key(repo) {
                    return Err(ManifestError::UnknownRepoInStack {
                        stack: name.clone(),
                        repo: repo.clone(),
                    });
                }
            }
        }
        for (name, overlay) in &self.overlays {
            for repo in overlay.repos.keys() {
                if !self.repos.contains_key(repo) {
                    return Err(ManifestError::UnknownRepoInOverlay {
                        overlay: name.clone(),
                        repo: repo.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

impl FromStr for Manifest {
    type Err = ManifestError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let manifest: Manifest =
            toml::from_str(text).map_err(|source| ManifestError::Parse(Box::new(source)))?;
        manifest.validate()?;
        Ok(manifest)
    }
}
