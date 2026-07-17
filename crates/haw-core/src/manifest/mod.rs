//! The human-authored `haw.toml` manifest: remotes, repos, stacks, overlays.

pub mod edit;
pub mod import;
mod model;
mod toml_loader;

pub use model::{Defaults, Manifest, Overlay, Remote, Repo, RepoOverride, Stack};
pub use toml_loader::TomlLoader;

use std::path::{Path, PathBuf};

/// Errors produced while loading or validating a manifest.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("failed to read manifest at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid manifest TOML")]
    Parse(#[source] Box<toml::de::Error>),
    #[error("repo `{repo}` references unknown remote `{remote}`")]
    UnknownRemote { repo: String, remote: String },
    #[error("repo `{0}` must declare either `url` or `remote` + `repo`")]
    MissingSource(String),
    #[error("repo `{0}` declares both `url` and `remote`/`repo`")]
    AmbiguousSource(String),
    #[error("stack `{stack}` references unknown repo `{repo}`")]
    UnknownRepoInStack { stack: String, repo: String },
    #[error("overlay `{overlay}` references unknown repo `{repo}`")]
    UnknownRepoInOverlay { overlay: String, repo: String },
    #[error("remote `{remote}` declares unknown forge `{forge}` (use \"github\" or \"gitlab\")")]
    UnknownForge { remote: String, forge: String },
    #[error("repo `{repo}` depends on unknown repo `{dep}`")]
    UnknownDep { repo: String, dep: String },
    #[error("plugin `{plugin}` subscribes to unknown phase `{phase}` (valid phases: {valid})")]
    UnknownPluginPhase {
        plugin: String,
        phase: String,
        valid: String,
    },
    #[error("repo `{repo}`: {source}")]
    Insecure {
        repo: String,
        #[source]
        source: crate::security::SecurityError,
    },
}

/// Anything that can produce a [`Manifest`] from a file on disk.
///
/// TOML is the reference implementation; `west.yml` / repo `default.xml`
/// importers implement the same trait later.
pub trait ManifestLoader {
    fn load(&self, path: &Path) -> Result<Manifest, ManifestError>;
}
