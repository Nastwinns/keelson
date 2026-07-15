//! Surgical manifest edits that preserve the user's comments and formatting
//! (`toml_edit`). Every edit re-validates the result before returning it.

use toml_edit::{DocumentMut, Item, Table, value};

use super::{Manifest, ManifestError};

/// Errors while editing a manifest.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("invalid manifest TOML")]
    Syntax(#[source] Box<toml_edit::TomlError>),
    #[error(transparent)]
    Invalid(#[from] ManifestError),
    #[error("repo `{0}` already exists")]
    RepoExists(String),
    #[error("repo `{0}` not found")]
    RepoNotFound(String),
    #[error("stack `{0}` already exists")]
    StackExists(String),
    #[error("stack `{0}` not found")]
    StackNotFound(String),
    #[error("repo `{repo}` is used by stack `{stack}`; remove it there first")]
    ReferencedByStack { repo: String, stack: String },
    #[error("repo `{repo}` is used by overlay `{overlay}`; remove it there first")]
    ReferencedByOverlay { repo: String, overlay: String },
}

/// A repo to add to the manifest.
#[derive(Debug, Clone, Default)]
pub struct NewRepo {
    pub name: String,
    pub url: Option<String>,
    pub remote: Option<String>,
    pub repo: Option<String>,
    pub rev: String,
    pub path: Option<String>,
    pub groups: Vec<String>,
}

fn parse_doc(text: &str) -> Result<(DocumentMut, Manifest), EditError> {
    let doc: DocumentMut = text
        .parse()
        .map_err(|source| EditError::Syntax(Box::new(source)))?;
    let manifest: Manifest = text.parse()?;
    Ok((doc, manifest))
}

fn finish(doc: DocumentMut) -> Result<String, EditError> {
    let out = doc.to_string();
    out.parse::<Manifest>()?;
    Ok(out)
}

/// Comment/blank block at the very top of the file, before the first entry.
fn leading_trivia(text: &str) -> &str {
    let mut end = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.trim_end().is_empty() {
            end += line.len();
        } else {
            break;
        }
    }
    &text[..end]
}

/// Removing the first table also removes the file-header comment attached to
/// it as decor; put the header back when that happens.
fn keep_header(original: &str, mut edited: String) -> String {
    let trivia = leading_trivia(original);
    if !trivia.trim().is_empty() && !edited.starts_with(trivia) {
        edited.insert_str(0, trivia);
    }
    edited
}

fn section<'d>(doc: &'d mut DocumentMut, key: &str) -> &'d mut Table {
    let item = doc.entry(key).or_insert(Item::Table(Table::new()));
    if item.as_table_mut().is_none() {
        *item = Item::Table(Table::new());
    }
    let Some(table) = item.as_table_mut() else {
        unreachable!("section was just made a table")
    };
    table.set_implicit(true);
    table
}

/// Add `[repo.NAME]` to the manifest text.
pub fn add_repo(text: &str, spec: &NewRepo) -> Result<String, EditError> {
    let (mut doc, manifest) = parse_doc(text)?;
    if manifest.repos.contains_key(&spec.name) {
        return Err(EditError::RepoExists(spec.name.clone()));
    }

    let mut entry = Table::new();
    if let Some(url) = &spec.url {
        entry["url"] = value(url);
    }
    if let Some(remote) = &spec.remote {
        entry["remote"] = value(remote);
    }
    if let Some(repo) = &spec.repo {
        entry["repo"] = value(repo);
    }
    entry["rev"] = value(&spec.rev);
    if let Some(path) = &spec.path {
        entry["path"] = value(path);
    }
    if !spec.groups.is_empty() {
        let mut groups = toml_edit::Array::new();
        for group in &spec.groups {
            groups.push(group);
        }
        entry["groups"] = value(groups);
    }

    section(&mut doc, "repo")[&spec.name] = Item::Table(entry);
    finish(doc)
}

/// Remove `[repo.NAME]`; refuses while a stack or overlay still references it.
pub fn remove_repo(text: &str, name: &str) -> Result<String, EditError> {
    let (mut doc, manifest) = parse_doc(text)?;
    if !manifest.repos.contains_key(name) {
        return Err(EditError::RepoNotFound(name.to_string()));
    }
    for (stack, spec) in &manifest.stacks {
        if spec.repos.iter().any(|r| r == name) {
            return Err(EditError::ReferencedByStack {
                repo: name.to_string(),
                stack: stack.clone(),
            });
        }
    }
    for (overlay, spec) in &manifest.overlays {
        if spec.repos.contains_key(name) {
            return Err(EditError::ReferencedByOverlay {
                repo: name.to_string(),
                overlay: overlay.clone(),
            });
        }
    }
    section(&mut doc, "repo").remove(name);
    finish(doc).map(|out| keep_header(text, out))
}

/// Add `[stack.NAME]` with the given repos.
pub fn add_stack(text: &str, name: &str, repos: &[String]) -> Result<String, EditError> {
    let (mut doc, manifest) = parse_doc(text)?;
    if manifest.stacks.contains_key(name) {
        return Err(EditError::StackExists(name.to_string()));
    }
    let mut entry = Table::new();
    let mut list = toml_edit::Array::new();
    for repo in repos {
        list.push(repo);
    }
    entry["repos"] = value(list);
    section(&mut doc, "stack")[name] = Item::Table(entry);
    finish(doc)
}

/// Remove `[stack.NAME]`.
pub fn remove_stack(text: &str, name: &str) -> Result<String, EditError> {
    let (mut doc, manifest) = parse_doc(text)?;
    if !manifest.stacks.contains_key(name) {
        return Err(EditError::StackNotFound(name.to_string()));
    }
    section(&mut doc, "stack").remove(name);
    finish(doc).map(|out| keep_header(text, out))
}
