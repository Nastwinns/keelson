//! Pure planning layer for `haw publish`.
//!
//! The binary uses [`plan_upload`] to turn a (target, base URL, package name,
//! version, file) tuple into the exact HTTP request it *would* make — method,
//! URL, and auth scheme — with no network I/O. `haw publish --dry-run` renders
//! this plan; the real command feeds the same plan into `reqwest`. Keeping the
//! request construction pure means every URL/auth rule is unit-testable off the
//! wire.

use std::fmt;

/// A generic/raw artifact registry `haw publish` can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Nexus raw-hosted repository (`PUT`, Basic auth).
    Nexus,
    /// Artifactory generic repository (`PUT`, Bearer token).
    Artifactory,
    /// GitLab generic packages API (`PUT`, `PRIVATE-TOKEN` header).
    GitLab,
    /// Bitbucket repository Downloads (`POST` multipart, Basic auth).
    Bitbucket,
}

impl Target {
    /// Parse the `--to` value.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "nexus" => Ok(Target::Nexus),
            "artifactory" => Ok(Target::Artifactory),
            "gitlab" => Ok(Target::GitLab),
            "bitbucket" => Ok(Target::Bitbucket),
            other => Err(format!(
                "unknown target `{other}` (use nexus, artifactory, gitlab, or bitbucket)"
            )),
        }
    }

    /// The `--to` spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Target::Nexus => "nexus",
            Target::Artifactory => "artifactory",
            Target::GitLab => "gitlab",
            Target::Bitbucket => "bitbucket",
        }
    }
}

/// The HTTP verb an upload uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Put,
    /// Multipart `POST` (Bitbucket Downloads).
    PostMultipart,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Put => "PUT",
            Method::PostMultipart => "POST (multipart)",
        }
    }
}

/// How the request authenticates. The tuple values are env-var-sourced
/// credentials, held so the binary can attach them to the real request; the
/// [`Auth::scheme`] label is what `--dry-run` prints (never the secret).
#[derive(Clone, PartialEq, Eq)]
pub enum Auth {
    /// HTTP Basic (`user:pass`).
    Basic { user: String, pass: String },
    /// `Authorization: Bearer <token>`.
    Bearer(String),
    /// GitLab `PRIVATE-TOKEN: <token>` header.
    PrivateToken(String),
}

impl Auth {
    /// The human label shown by `--dry-run` (no secret material).
    pub fn scheme(&self) -> &'static str {
        match self {
            Auth::Basic { .. } => "Basic",
            Auth::Bearer(_) => "Bearer",
            Auth::PrivateToken(_) => "PRIVATE-TOKEN",
        }
    }
}

// SECURITY: hold live credentials, so a derived `Debug` would leak the password
// or token into logs/panics. Print only the non-secret scheme label.
impl fmt::Debug for Auth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Auth({}, <redacted>)", self.scheme())
    }
}

/// A fully-resolved plan for uploading one file to one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadPlan {
    pub method: Method,
    pub url: String,
    pub auth: Auth,
    /// The base name sent to the registry (path component of the file).
    pub file: String,
}

impl fmt::Display for UploadPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}  (auth: {})",
            self.method.as_str(),
            self.url,
            self.auth.scheme()
        )
    }
}

/// Trim a single trailing slash so `{base}/...` joins cleanly.
fn trim_base(base: &str) -> &str {
    base.strip_suffix('/').unwrap_or(base)
}

/// Build the upload plan for one `(target, base, name, version, file)` tuple.
///
/// `base` is the already-resolved base URL (env override applied by the
/// caller), `file` is the artifact's base name (no directories). This is the
/// single source of truth for URL/auth construction across all four targets;
/// it performs no I/O so it is exhaustively unit-tested.
#[allow(clippy::too_many_arguments)]
pub fn plan_upload(
    target: Target,
    base: &str,
    repo: &str,
    project_id: &str,
    name: &str,
    version: &str,
    file: &str,
    auth: Auth,
) -> UploadPlan {
    let base = trim_base(base);
    let (method, url) = match target {
        Target::Nexus => (
            Method::Put,
            format!("{base}/repository/{repo}/{name}/{version}/{file}"),
        ),
        Target::Artifactory => (
            Method::Put,
            format!("{base}/{repo}/{name}/{version}/{file}"),
        ),
        Target::GitLab => (
            Method::Put,
            format!("{base}/api/v4/projects/{project_id}/packages/generic/{name}/{version}/{file}"),
        ),
        Target::Bitbucket => (
            Method::PostMultipart,
            format!("{base}/2.0/repositories/{repo}/downloads"),
        ),
    };
    UploadPlan {
        method,
        url,
        auth,
        file: file.to_string(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn basic() -> Auth {
        Auth::Basic {
            user: "u".into(),
            pass: "p".into(),
        }
    }

    #[test]
    fn auth_debug_never_leaks_secrets() {
        let secret = "s3cr3t-token";
        for auth in [
            Auth::Basic {
                user: "alice".into(),
                pass: secret.into(),
            },
            Auth::Bearer(secret.into()),
            Auth::PrivateToken(secret.into()),
        ] {
            let dbg = format!("{auth:?}");
            assert!(!dbg.contains(secret), "Auth Debug leaked secret: {dbg}");
            assert!(dbg.contains(auth.scheme()));
            assert!(dbg.contains("redacted"));
        }
        // The pass field must not leak via UploadPlan's derived Debug either.
        let plan = UploadPlan {
            method: Method::Put,
            url: "https://x/y".into(),
            auth: Auth::Basic {
                user: "alice".into(),
                pass: secret.into(),
            },
            file: "f".into(),
        };
        assert!(!format!("{plan:?}").contains(secret));
    }

    #[test]
    fn target_parse_roundtrip() {
        for t in [
            Target::Nexus,
            Target::Artifactory,
            Target::GitLab,
            Target::Bitbucket,
        ] {
            assert_eq!(Target::parse(t.as_str()).unwrap(), t);
        }
        assert_eq!(Target::parse("NEXUS").unwrap(), Target::Nexus);
        assert!(Target::parse("s3").is_err());
    }

    #[test]
    fn nexus_url() {
        let p = plan_upload(
            Target::Nexus,
            "https://nexus.example.com",
            "raw-hosted",
            "",
            "fleet",
            "1.2.3",
            "app.bin",
            basic(),
        );
        assert_eq!(p.method, Method::Put);
        assert_eq!(
            p.url,
            "https://nexus.example.com/repository/raw-hosted/fleet/1.2.3/app.bin"
        );
        assert_eq!(p.auth.scheme(), "Basic");
    }

    #[test]
    fn artifactory_url() {
        let p = plan_upload(
            Target::Artifactory,
            "https://acme.jfrog.io/artifactory",
            "generic-local",
            "",
            "fleet",
            "abc1234",
            "sbom.json",
            Auth::Bearer("t".into()),
        );
        assert_eq!(p.method, Method::Put);
        assert_eq!(
            p.url,
            "https://acme.jfrog.io/artifactory/generic-local/fleet/abc1234/sbom.json"
        );
        assert_eq!(p.auth.scheme(), "Bearer");
    }

    #[test]
    fn gitlab_url() {
        let p = plan_upload(
            Target::GitLab,
            "https://gitlab.com",
            "",
            "42",
            "fleet",
            "unversioned",
            "haw-evidence.tar.gz",
            Auth::PrivateToken("t".into()),
        );
        assert_eq!(p.method, Method::Put);
        assert_eq!(
            p.url,
            "https://gitlab.com/api/v4/projects/42/packages/generic/fleet/unversioned/haw-evidence.tar.gz"
        );
        assert_eq!(p.auth.scheme(), "PRIVATE-TOKEN");
    }

    #[test]
    fn bitbucket_url() {
        let p = plan_upload(
            Target::Bitbucket,
            "https://api.bitbucket.org",
            "acme/fleet",
            "",
            "fleet",
            "1.0",
            "release.zip",
            basic(),
        );
        assert_eq!(p.method, Method::PostMultipart);
        assert_eq!(
            p.url,
            "https://api.bitbucket.org/2.0/repositories/acme/fleet/downloads"
        );
    }

    #[test]
    fn trailing_slash_on_base_is_trimmed() {
        let p = plan_upload(
            Target::Nexus,
            "https://nexus.example.com/",
            "raw-hosted",
            "",
            "fleet",
            "1.0",
            "f.bin",
            basic(),
        );
        assert_eq!(
            p.url,
            "https://nexus.example.com/repository/raw-hosted/fleet/1.0/f.bin"
        );
    }

    #[test]
    fn display_renders_method_url_and_auth_but_not_secret() {
        let p = plan_upload(
            Target::Artifactory,
            "https://a.example",
            "generic-local",
            "",
            "fleet",
            "1.0",
            "f.bin",
            Auth::Bearer("SUPERSECRET".into()),
        );
        let rendered = p.to_string();
        assert!(rendered.contains("PUT"));
        assert!(rendered.contains("https://a.example/generic-local/fleet/1.0/f.bin"));
        assert!(rendered.contains("auth: Bearer"));
        assert!(!rendered.contains("SUPERSECRET"));
    }
}
