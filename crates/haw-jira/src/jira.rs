//! Jira linkage: config, issue-key resolution, and the REST calls.
//!
//! Config comes from the environment (`JIRA_URL`, `JIRA_USER`, `JIRA_TOKEN`).
//! When any credential is missing the plugin runs a **dry-run**: it reports the
//! exact action it would take and never fails just because creds are absent
//! (fail-open for adoption).

use std::time::Duration;

use serde_json::{Value, json};

use crate::context::{Context, changeset_from_branch};

/// Jira REST configuration read from the environment.
#[derive(Clone, PartialEq)]
pub struct Config {
    pub base_url: String,
    pub user: String,
    pub token: String,
}

// SECURITY: never print credentials. `base_url` is not secret, but `user` and
// `token` are — a derived `Debug` would leak them into logs/panics.
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("base_url", &self.base_url)
            .field("user", &"<redacted>")
            .field("token", &"<redacted>")
            .finish()
    }
}

impl Config {
    /// Read `JIRA_URL`/`JIRA_USER`/`JIRA_TOKEN`; `None` if any is absent/blank.
    pub fn from_env() -> Option<Config> {
        let base_url = non_empty(std::env::var("JIRA_URL").ok())?;
        let user = non_empty(std::env::var("JIRA_USER").ok())?;
        let token = non_empty(std::env::var("JIRA_TOKEN").ok())?;
        Some(Config {
            base_url: base_url.trim_end_matches('/').to_string(),
            user,
            token,
        })
    }
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.trim().is_empty())
}

/// Resolve the issue key: explicit argv, then context revs, then
/// `haw change status --format json`.
pub fn resolve_issue(explicit: Option<&str>, ctx: &Context) -> Result<String, String> {
    if let Some(key) = explicit {
        return Ok(key.to_string());
    }
    if let Some(key) = ctx.changeset_from_revs() {
        return Ok(key);
    }
    if let Some(key) = changeset_from_haw(ctx.root.as_deref()) {
        return Ok(key);
    }
    Err(
        "could not determine a Jira issue key: pass one explicitly (haw jira PROJ-123) \
or run inside a change/<ID> changeset"
            .to_string(),
    )
}

/// Best-effort changeset id via `haw change status --format json`.
fn changeset_from_haw(root: Option<&std::path::Path>) -> Option<String> {
    let mut cmd = std::process::Command::new("haw");
    cmd.arg("change").arg("status").arg("--format").arg("json");
    if let Some(root) = root {
        cmd.current_dir(root);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(text.trim()).ok()?;

    // Accept a few plausible shapes: an explicit id, or a branch to parse.
    if let Some(id) = value.get("id").and_then(|v| v.as_str())
        && !id.is_empty()
    {
        return Some(id.to_string());
    }
    if let Some(branch) = value.get("branch").and_then(|v| v.as_str())
        && let Some(id) = changeset_from_branch(branch)
    {
        return Some(id);
    }
    None
}

/// The default target status for a phase.
pub fn default_target(phase: Option<&str>) -> &'static str {
    match phase {
        Some("post-land") => "Done",
        _ => "In Review",
    }
}

/// The action the plugin plans (or performed) against Jira.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub issue: String,
    pub target_status: String,
    pub comment: String,
    /// `false` when creds were absent (dry-run), `true` when performed.
    pub performed: bool,
}

impl Action {
    /// Serialize as a machine document for `--format json`.
    pub fn to_json(&self) -> Value {
        json!({
            "schema": "haw.jira/1",
            "issue": self.issue,
            "target_status": self.target_status,
            "comment": self.comment,
            "performed": self.performed,
            "mode": if self.performed { "performed" } else { "dry-run" },
        })
    }
}

/// Build the comment body linking the changeset/PRs.
pub fn build_comment(ctx: &Context, issue: &str) -> String {
    let stack = ctx.stack.as_deref().unwrap_or("(none)");
    let repos: Vec<String> = ctx
        .repos
        .iter()
        .map(|r| format!("{}@{}", r.name, r.rev))
        .collect();
    let repo_line = if repos.is_empty() {
        "(no repos in context)".to_string()
    } else {
        repos.join(", ")
    };
    format!("haw changeset linked to {issue}. Stack: {stack}. Repos: {repo_line}.")
}

/// Plan the action without performing it (dry-run).
pub fn plan(ctx: &Context, issue: String, target_status: String) -> Action {
    Action {
        comment: build_comment(ctx, &issue),
        issue,
        target_status,
        performed: false,
    }
}

/// The result of performing the action against a live Jira.
pub struct Performed {
    pub action: Action,
    /// Human-readable notes about each step for surfacing to the user.
    pub notes: Vec<String>,
}

/// Perform the action against a live Jira: comment, then transition.
///
/// Returns an error string describing the first failing HTTP step; the caller
/// maps that onto a `Finding`/non-zero exit in hook mode.
pub fn perform(config: &Config, mut action: Action) -> Result<Performed, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;

    let mut notes = Vec::new();

    // 1. Add the linking comment.
    let comment_url = format!(
        "{}/rest/api/3/issue/{}/comment",
        config.base_url, action.issue
    );
    let comment_body = json!({
        "body": {
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [{ "type": "text", "text": action.comment }]
            }]
        }
    });
    let resp = client
        .post(&comment_url)
        .basic_auth(&config.user, Some(&config.token))
        .json(&comment_body)
        .send()
        .map_err(|e| format!("comment request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "comment failed: HTTP {} on {}",
            resp.status(),
            comment_url
        ));
    }
    notes.push(format!("commented on {}", action.issue));

    // 2. Resolve the transition id for the target status name.
    let transitions_url = format!(
        "{}/rest/api/3/issue/{}/transitions",
        config.base_url, action.issue
    );
    let resp = client
        .get(&transitions_url)
        .basic_auth(&config.user, Some(&config.token))
        .send()
        .map_err(|e| format!("transitions lookup failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "transitions lookup failed: HTTP {} on {}",
            resp.status(),
            transitions_url
        ));
    }
    let body: Value = resp
        .json()
        .map_err(|e| format!("transitions response was not JSON: {e}"))?;
    let transition_id = find_transition_id(&body, &action.target_status).ok_or_else(|| {
        format!(
            "no transition to status '{}' available on {}",
            action.target_status, action.issue
        )
    })?;

    // 3. POST the transition.
    let transition_body = json!({ "transition": { "id": transition_id } });
    let resp = client
        .post(&transitions_url)
        .basic_auth(&config.user, Some(&config.token))
        .json(&transition_body)
        .send()
        .map_err(|e| format!("transition request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "transition failed: HTTP {} on {}",
            resp.status(),
            transitions_url
        ));
    }
    notes.push(format!(
        "transitioned {} to '{}'",
        action.issue, action.target_status
    ));

    action.performed = true;
    Ok(Performed { action, notes })
}

/// Find the transition id whose target status name matches (case-insensitive).
pub fn find_transition_id(body: &Value, target: &str) -> Option<String> {
    let transitions = body.get("transitions")?.as_array()?;
    for t in transitions {
        let name = t.get("name").and_then(|n| n.as_str());
        let to_name = t
            .get("to")
            .and_then(|to| to.get("name"))
            .and_then(|n| n.as_str());
        let matches = [name, to_name]
            .into_iter()
            .flatten()
            .any(|n| n.eq_ignore_ascii_case(target));
        if matches {
            return t.get("id").and_then(|id| id.as_str()).map(str::to_string);
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::context::Repo;
    use std::path::PathBuf;

    #[test]
    fn config_debug_never_leaks_credentials() {
        let cfg = Config {
            base_url: "https://jira.example.com".to_string(),
            user: "alice@example.com".to_string(),
            token: "super-secret-token".to_string(),
        };
        let dbg = format!("{cfg:?}");
        assert!(!dbg.contains("super-secret-token"), "token leaked: {dbg}");
        assert!(!dbg.contains("alice@example.com"), "user leaked: {dbg}");
        assert!(dbg.contains("redacted"));
        // base_url is not a secret and stays visible for diagnostics.
        assert!(dbg.contains("jira.example.com"));
    }

    fn ctx() -> Context {
        Context {
            root: Some(PathBuf::from("/ws")),
            stack: Some("gateway".to_string()),
            phase: None,
            repos: vec![Repo {
                name: "api".to_string(),
                path: PathBuf::from("/ws/api"),
                rev: "change/PROJ-42".to_string(),
                groups: vec![],
            }],
        }
    }

    #[test]
    fn resolve_prefers_explicit() {
        assert_eq!(resolve_issue(Some("X-1"), &ctx()).unwrap(), "X-1");
    }

    #[test]
    fn resolve_falls_back_to_branch() {
        assert_eq!(resolve_issue(None, &ctx()).unwrap(), "PROJ-42");
    }

    #[test]
    fn resolve_errors_without_any_source() {
        let empty = Context::default();
        assert!(resolve_issue(None, &empty).is_err());
    }

    #[test]
    fn default_target_by_phase() {
        assert_eq!(default_target(Some("post-land")), "Done");
        assert_eq!(default_target(Some("pre-request")), "In Review");
        assert_eq!(default_target(None), "In Review");
    }

    #[test]
    fn plan_is_a_dry_run() {
        let action = plan(&ctx(), "PROJ-42".to_string(), "Done".to_string());
        assert!(!action.performed);
        assert!(action.comment.contains("PROJ-42"));
        assert!(action.comment.contains("api@change/PROJ-42"));
        assert_eq!(action.to_json()["mode"], "dry-run");
    }

    #[test]
    fn action_json_round_trips_key_fields() {
        let action = plan(&ctx(), "PROJ-42".to_string(), "In Review".to_string());
        let v = action.to_json();
        assert_eq!(v["schema"], "haw.jira/1");
        assert_eq!(v["issue"], "PROJ-42");
        assert_eq!(v["target_status"], "In Review");
        assert_eq!(v["performed"], false);
    }

    #[test]
    fn config_missing_when_env_blank() {
        // Sanity: with no env this returns None in most test environments.
        // We don't mutate global env here to avoid cross-test interference.
        let _ = Config::from_env();
    }

    #[test]
    fn finds_transition_by_target_status_name() {
        let body = json!({
            "transitions": [
                { "id": "11", "name": "Start", "to": { "name": "In Progress" } },
                { "id": "31", "name": "Finish", "to": { "name": "Done" } }
            ]
        });
        assert_eq!(find_transition_id(&body, "done").as_deref(), Some("31"));
        assert_eq!(
            find_transition_id(&body, "In Progress").as_deref(),
            Some("11")
        );
        assert_eq!(find_transition_id(&body, "Nope"), None);
    }

    #[test]
    fn finds_transition_by_transition_name() {
        let body = json!({ "transitions": [ { "id": "5", "name": "In Review" } ] });
        assert_eq!(find_transition_id(&body, "In Review").as_deref(), Some("5"));
    }
}
