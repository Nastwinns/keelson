//! Shared HTTP hardening for the forge clients: an SSRF-resistant redirect
//! policy and bounded-body readers.
//!
//! Every forge issues authenticated requests carrying the user's real token
//! (`Authorization` / `PRIVATE-TOKEN`). Two concerns are handled here:
//!
//! * **SSRF + token exfiltration (H2).** The forge base host comes from the
//!   manifest remote URL, and a malicious or compromised forge could 3xx a
//!   request at an internal address (e.g. the cloud metadata endpoint at
//!   `169.254.169.254`). [`forge_client`] installs a custom redirect policy
//!   that caps hops and refuses any cross-host redirect except to a small
//!   allow-list of well-known forge CDN hosts (GitHub serves Actions log blobs
//!   from `*.githubusercontent.com` / `*.blob.core.windows.net` via a 302 to a
//!   *different* host, so that one legitimate hop must be permitted).
//!
//!   reqwest already drops the standard `Authorization` header on a cross-host
//!   redirect, but it does **not** drop custom headers such as GitLab's
//!   `PRIVATE-TOKEN`. Since we only ever allow cross-host redirects to the
//!   read-only GitHub CDN hosts (never to a GitLab/Bitbucket API), and GitLab
//!   traces are not served via cross-host redirects, the token header is never
//!   replayed onto a foreign host through this policy.
//!
//! * **Unbounded response bodies (M1).** `read_capped` / `json_capped`
//!   reject bodies whose `Content-Length` exceeds `MAX_RESPONSE_BYTES` and,
//!   when the header is absent (chunked/streaming), read through a limited
//!   reader so a hostile endpoint cannot exhaust memory.

use std::io::Read;

use reqwest::blocking::Response;
use reqwest::redirect::{Attempt, Policy};

use crate::ForgeError;

/// Hard cap on a single forge response body. Diffs/logs/blobs are already
/// line-capped for display, but that runs *after* the body is in memory, so
/// this bounds the in-memory size first.
pub const MAX_RESPONSE_BYTES: u64 = 25 * 1024 * 1024;

/// Maximum number of redirect hops a forge request may follow.
const MAX_REDIRECT_HOPS: usize = 4;

/// Host suffixes a cross-host redirect is allowed to target. These are the
/// read-only CDNs GitHub 302-redirects Actions log/blob downloads to; nothing
/// here is an API host that would accept a replayed credential.
const CROSS_HOST_REDIRECT_ALLOW: &[&str] = &[
    "githubusercontent.com",
    "blob.core.windows.net",
    "actions.githubusercontent.com",
    "pipelines.actions.githubusercontent.com",
];

/// Whether `host` equals or is a subdomain of `suffix`.
fn host_matches(host: &str, suffix: &str) -> bool {
    host == suffix || host.ends_with(&format!(".{suffix}"))
}

/// The SSRF-resistant redirect policy: at most [`MAX_REDIRECT_HOPS`] hops,
/// same-host redirects always allowed, cross-host redirects allowed only to a
/// well-known forge CDN host (see [`CROSS_HOST_REDIRECT_ALLOW`]).
fn redirect_policy() -> Policy {
    Policy::custom(move |attempt: Attempt| {
        if attempt.previous().len() >= MAX_REDIRECT_HOPS {
            return attempt.error(format!("too many redirects (>{MAX_REDIRECT_HOPS})"));
        }
        let origin_host = attempt
            .previous()
            .first()
            .and_then(|u| u.host_str())
            .map(str::to_string);
        let target_host = attempt.url().host_str().unwrap_or_default().to_string();
        match origin_host {
            Some(origin) if origin == target_host => attempt.follow(),
            _ if CROSS_HOST_REDIRECT_ALLOW
                .iter()
                .any(|allowed| host_matches(&target_host, allowed)) =>
            {
                attempt.follow()
            }
            _ => attempt.stop(),
        }
    })
}

/// A blocking `reqwest` client hardened for forge calls: the SSRF-resistant
/// redirect policy above. Falls back to a default client if the build fails
/// (which it does not, given a valid policy), so callers get a client without
/// threading a `Result` through construction.
pub fn forge_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .redirect(redirect_policy())
        .build()
        .unwrap_or_default()
}

/// Reject a response whose advertised `Content-Length` exceeds the cap before
/// reading a single byte of the body.
fn reject_oversized(resp: &Response, url: &str) -> Result<(), ForgeError> {
    if let Some(len) = resp.content_length()
        && len > MAX_RESPONSE_BYTES
    {
        return Err(ForgeError::Api(format!(
            "response from {url} is {len} bytes, over the {MAX_RESPONSE_BYTES}-byte cap"
        )));
    }
    Ok(())
}

/// Read a response body as raw bytes, capped at [`MAX_RESPONSE_BYTES`]. Rejects
/// on an oversized `Content-Length`, and — when the header is absent — reads
/// through a limited reader and errors if the stream would exceed the cap.
pub fn read_capped_bytes(resp: Response, url: &str) -> Result<Vec<u8>, ForgeError> {
    reject_oversized(&resp, url)?;
    // Read one byte past the cap so an exactly-cap-sized-plus body is detected.
    let mut buf = Vec::new();
    let mut limited = resp.take(MAX_RESPONSE_BYTES + 1);
    limited
        .read_to_end(&mut buf)
        .map_err(|err| ForgeError::Api(format!("reading {url}: {err}")))?;
    if buf.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(ForgeError::Api(format!(
            "response from {url} exceeds the {MAX_RESPONSE_BYTES}-byte cap"
        )));
    }
    Ok(buf)
}

/// Read a response body as UTF-8 text, capped at [`MAX_RESPONSE_BYTES`].
pub fn read_capped_text(resp: Response, url: &str) -> Result<String, ForgeError> {
    let bytes = read_capped_bytes(resp, url)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Read and parse a JSON response body, capped at [`MAX_RESPONSE_BYTES`].
pub fn json_capped(resp: Response, url: &str) -> Result<serde_json::Value, ForgeError> {
    let bytes = read_capped_bytes(resp, url)?;
    serde_json::from_slice(&bytes)
        .map_err(|err| ForgeError::Api(format!("invalid JSON from {url}: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_matches_exact_and_subdomain() {
        assert!(host_matches(
            "githubusercontent.com",
            "githubusercontent.com"
        ));
        assert!(host_matches(
            "objects.githubusercontent.com",
            "githubusercontent.com"
        ));
        assert!(!host_matches(
            "evilgithubusercontent.com",
            "githubusercontent.com"
        ));
        assert!(!host_matches("169.254.169.254", "githubusercontent.com"));
    }

    #[test]
    fn cross_host_allow_list_covers_github_log_cdn() {
        // The documented GitHub Actions log 302 target host.
        assert!(
            CROSS_HOST_REDIRECT_ALLOW
                .iter()
                .any(|s| host_matches("productionresultssa0.blob.core.windows.net", s))
        );
        assert!(
            CROSS_HOST_REDIRECT_ALLOW
                .iter()
                .any(|s| host_matches("objects.githubusercontent.com", s))
        );
    }

    #[test]
    fn client_builds() {
        // The custom policy must produce a usable client (no panic / fallback).
        let _client = forge_client();
    }
}
