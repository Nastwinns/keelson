//! Manifest + stack + overlays -> the concrete set of repos to materialize.

use std::path::PathBuf;

use crate::manifest::{Manifest, Overlay, Repo};

/// One repo after resolution: where to clone from, what to check out, where to put it.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedRepo {
    pub name: String,
    pub url: String,
    pub rev: String,
    pub path: PathBuf,
    pub groups: Vec<String>,
}

/// True when `groups` passes the `filter`: an empty filter matches everything,
/// otherwise at least one group must match.
pub fn group_match(groups: &[String], filter: &[String]) -> bool {
    filter.is_empty() || groups.iter().any(|g| filter.contains(g))
}

/// Drop repos whose groups don't match the filter.
pub fn filter_groups(resolution: &mut Resolution, filter: &[String]) {
    resolution
        .repos
        .retain(|repo| group_match(&repo.groups, filter));
}

/// The repos of one stack with all overlays applied.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolution {
    pub stack: String,
    pub repos: Vec<ResolvedRepo>,
}

/// Errors produced while resolving a stack.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("unknown stack `{0}`")]
    UnknownStack(String),
    #[error("unknown overlay `{0}`")]
    UnknownOverlay(String),
    #[error("stack `{stack}` references unknown repo `{repo}`")]
    UnknownRepo { stack: String, repo: String },
    #[error("repo `{0}` has no usable source")]
    UnsourcedRepo(String),
}

fn active_overlays<'m>(
    manifest: &'m Manifest,
    overlays: &[String],
) -> Result<Vec<&'m Overlay>, ResolveError> {
    let mut active = Vec::with_capacity(overlays.len());
    for name in overlays {
        let overlay = manifest
            .overlays
            .get(name)
            .ok_or_else(|| ResolveError::UnknownOverlay(name.clone()))?;
        active.push(overlay);
    }
    Ok(active)
}

fn resolve_one(
    manifest: &Manifest,
    name: &str,
    repo: &Repo,
    active: &[&Overlay],
) -> Result<ResolvedRepo, ResolveError> {
    let mut rev = repo.rev.clone();
    let mut path = repo.checkout_path(name);
    for overlay in active {
        if let Some(over) = overlay.repos.get(name) {
            if let Some(r) = &over.rev {
                rev = r.clone();
            }
            if let Some(p) = &over.path {
                path = p.clone();
            }
        }
    }
    let url = repo
        .clone_url(&manifest.remotes)
        .ok_or_else(|| ResolveError::UnsourcedRepo(name.to_string()))?;
    Ok(ResolvedRepo {
        name: name.to_string(),
        url,
        rev,
        path,
        groups: repo.groups.clone(),
    })
}

/// Resolve `stack` against `manifest`, applying `overlays` in order
/// (later overlays win).
pub fn resolve(
    manifest: &Manifest,
    stack: &str,
    overlays: &[String],
) -> Result<Resolution, ResolveError> {
    let spec = manifest
        .stacks
        .get(stack)
        .ok_or_else(|| ResolveError::UnknownStack(stack.to_string()))?;
    let active = active_overlays(manifest, overlays)?;

    let mut repos = Vec::with_capacity(spec.repos.len());
    for name in &spec.repos {
        let repo = manifest
            .repos
            .get(name)
            .ok_or_else(|| ResolveError::UnknownRepo {
                stack: stack.to_string(),
                repo: name.clone(),
            })?;
        repos.push(resolve_one(manifest, name, repo, &active)?);
    }

    Ok(Resolution {
        stack: stack.to_string(),
        repos,
    })
}

/// Resolve every repo in the manifest (manifest order), applying `overlays`.
/// This is what lockfile generation uses: the lock covers all repos.
pub fn resolve_all(
    manifest: &Manifest,
    overlays: &[String],
) -> Result<Vec<ResolvedRepo>, ResolveError> {
    let active = active_overlays(manifest, overlays)?;
    let mut repos = Vec::with_capacity(manifest.repos.len());
    for (name, repo) in &manifest.repos {
        repos.push(resolve_one(manifest, name, repo, &active)?);
    }
    Ok(repos)
}
