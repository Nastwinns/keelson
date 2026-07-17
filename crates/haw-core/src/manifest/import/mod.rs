//! Importers: west `west.yml` and repo-tool `default.xml` -> the same
//! in-memory [`Manifest`] the TOML loader produces.

use std::path::Path;

use indexmap::IndexMap;
use serde::Deserialize;

use super::{Manifest, ManifestError, Remote, Repo, Stack};

/// Errors converting a foreign manifest.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("failed to read {path}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "cannot tell the format of {0} (use a .yml/.yaml west manifest or a .xml repo manifest)"
    )]
    UnknownFormat(std::path::PathBuf),
    #[error("invalid west manifest")]
    West(#[source] Box<serde_yaml::Error>),
    #[error("invalid repo-tool manifest")]
    RepoXml(#[source] Box<quick_xml::DeError>),
    #[error("imported manifest failed validation")]
    Invalid(#[from] ManifestError),
}

/// The stack every import generates (foreign formats have no composition).
pub const DEFAULT_STACK: &str = "main";

#[derive(Debug, Deserialize)]
struct WestFile {
    manifest: WestManifest,
}

#[derive(Debug, Default, Deserialize)]
struct WestManifest {
    #[serde(default)]
    defaults: WestDefaults,
    #[serde(default)]
    remotes: Vec<WestRemote>,
    #[serde(default)]
    projects: Vec<WestProject>,
}

#[derive(Debug, Default, Deserialize)]
struct WestDefaults {
    remote: Option<String>,
    revision: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WestRemote {
    name: String,
    #[serde(rename = "url-base")]
    url_base: String,
}

#[derive(Debug, Deserialize)]
struct WestProject {
    name: String,
    #[serde(default)]
    remote: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    revision: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    groups: Vec<String>,
}

/// Convert a `west.yml` document.
pub fn from_west_str(text: &str) -> Result<Manifest, ImportError> {
    let west: WestFile =
        serde_yaml::from_str(text).map_err(|source| ImportError::West(Box::new(source)))?;
    let west = west.manifest;

    let mut remotes = IndexMap::new();
    for remote in &west.remotes {
        remotes.insert(
            remote.name.clone(),
            Remote {
                url: remote.url_base.clone(),
                forge: None,
            },
        );
    }

    let mut repos = IndexMap::new();
    for project in &west.projects {
        let key = project.name.rsplit('/').next().unwrap_or(&project.name);
        let rev = project
            .revision
            .clone()
            .or_else(|| west.defaults.revision.clone())
            .unwrap_or_else(|| "main".to_string());
        let fallback_remote = || {
            west.defaults
                .remote
                .clone()
                .or_else(|| (west.remotes.len() == 1).then(|| west.remotes[0].name.clone()))
        };
        let (url, remote, slug) = match &project.url {
            Some(url) => (Some(url.clone()), None, None),
            None => (
                None,
                project.remote.clone().or_else(fallback_remote),
                Some(project.name.clone()),
            ),
        };
        repos.insert(
            key.to_string(),
            Repo {
                remote,
                repo: slug,
                url,
                rev,
                path: project.path.clone().map(Into::into),
                groups: project.groups.clone(),
                submodules: false,
                deps: Vec::new(),
                build: None,
                test: None,
            },
        );
    }

    finish(remotes, repos)
}

#[derive(Debug, Deserialize)]
struct XmlManifest {
    #[serde(rename = "remote", default)]
    remotes: Vec<XmlRemote>,
    #[serde(rename = "default")]
    default: Option<XmlDefault>,
    #[serde(rename = "project", default)]
    projects: Vec<XmlProject>,
}

#[derive(Debug, Deserialize)]
struct XmlRemote {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@fetch")]
    fetch: String,
}

#[derive(Debug, Default, Deserialize)]
struct XmlDefault {
    #[serde(rename = "@revision")]
    revision: Option<String>,
    #[serde(rename = "@remote")]
    remote: Option<String>,
}

#[derive(Debug, Deserialize)]
struct XmlProject {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@path")]
    path: Option<String>,
    #[serde(rename = "@revision")]
    revision: Option<String>,
    #[serde(rename = "@remote")]
    remote: Option<String>,
    #[serde(rename = "@groups")]
    groups: Option<String>,
}

/// Convert a repo-tool `default.xml` document.
pub fn from_repo_xml_str(text: &str) -> Result<Manifest, ImportError> {
    let xml: XmlManifest =
        quick_xml::de::from_str(text).map_err(|source| ImportError::RepoXml(Box::new(source)))?;
    let defaults = xml.default.unwrap_or_default();

    let mut remotes = IndexMap::new();
    for remote in &xml.remotes {
        remotes.insert(
            remote.name.clone(),
            Remote {
                url: remote.fetch.trim_end_matches('/').to_string(),
                forge: None,
            },
        );
    }

    let mut repos = IndexMap::new();
    for project in &xml.projects {
        let key = project.name.rsplit('/').next().unwrap_or(&project.name);
        let rev = project
            .revision
            .clone()
            .or_else(|| defaults.revision.clone())
            .unwrap_or_else(|| "main".to_string());
        let groups = project
            .groups
            .as_deref()
            .map(|list| {
                list.split(',')
                    .map(str::trim)
                    .filter(|g| !g.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        repos.insert(
            key.to_string(),
            Repo {
                remote: project.remote.clone().or_else(|| defaults.remote.clone()),
                repo: Some(project.name.clone()),
                url: None,
                rev,
                path: Some(
                    project
                        .path
                        .clone()
                        .unwrap_or_else(|| project.name.clone())
                        .into(),
                ),
                groups,
                submodules: false,
                deps: Vec::new(),
                build: None,
                test: None,
            },
        );
    }

    finish(remotes, repos)
}

fn finish(
    remotes: IndexMap<String, Remote>,
    repos: IndexMap<String, Repo>,
) -> Result<Manifest, ImportError> {
    let stack = Stack {
        repos: repos.keys().cloned().collect(),
        description: Some("imported — split into real stacks as needed".to_string()),
    };
    let manifest = Manifest {
        defaults: Default::default(),
        remotes,
        repos,
        stacks: IndexMap::from([(DEFAULT_STACK.to_string(), stack)]),
        overlays: IndexMap::new(),
        plugins: IndexMap::new(),
    };
    manifest.validate()?;
    Ok(manifest)
}

/// Convert a foreign manifest file, picking the format from its extension.
pub fn import(path: &Path) -> Result<Manifest, ImportError> {
    let text = std::fs::read_to_string(path).map_err(|source| ImportError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("yml" | "yaml") => from_west_str(&text),
        Some("xml") => from_repo_xml_str(&text),
        _ => Err(ImportError::UnknownFormat(path.to_path_buf())),
    }
}
