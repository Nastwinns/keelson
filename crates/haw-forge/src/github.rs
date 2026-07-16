//! GitHub [`Forge`] implementation over `octocrab` (REST v3), driven from a
//! private current-thread tokio runtime so the trait stays synchronous.

use serde_json::{Value, json};

use crate::{Forge, ForgeError, PrHandle, PrSpec, PrState, PrStatus, repo_coords};

/// GitHub client: github.com and GitHub Enterprise (`/api/v3`).
pub struct GitHub {
    token: String,
    runtime: tokio::runtime::Runtime,
}

impl GitHub {
    pub fn new(token: String) -> Result<Self, ForgeError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| ForgeError::Api(format!("tokio runtime: {err}")))?;
        Ok(Self { token, runtime })
    }

    fn client(&self, host: &str) -> Result<octocrab::Octocrab, ForgeError> {
        let mut builder = octocrab::Octocrab::builder().personal_token(self.token.clone());
        if host != "github.com" {
            builder = builder
                .base_uri(api_base(host))
                .map_err(|err| ForgeError::Api(format!("invalid GitHub base uri: {err}")))?;
        }
        // octocrab's client spawns a tower::buffer worker on build, which needs a
        // live Tokio reactor; enter the runtime so the spawn doesn't panic.
        let _guard = self.runtime.enter();
        builder
            .build()
            .map_err(|err| ForgeError::Api(format!("GitHub client: {err}")))
    }

    fn split(&self, repo_url: &str) -> Result<(String, String), ForgeError> {
        let coords = repo_coords(repo_url)
            .ok_or_else(|| ForgeError::UnsupportedUrl(repo_url.to_string()))?;
        Ok((coords.host, coords.path))
    }

    fn get(&self, host: &str, route: &str) -> Result<Value, ForgeError> {
        let client = self.client(host)?;
        self.runtime
            .block_on(client.get::<Value, _, ()>(route, None))
            .map_err(|err| ForgeError::Api(format!("GET {route}: {err}")))
    }

    fn send(
        &self,
        host: &str,
        method: &str,
        route: &str,
        body: &Value,
    ) -> Result<Value, ForgeError> {
        let client = self.client(host)?;
        let call = async {
            match method {
                "POST" => client.post(route, Some(body)).await,
                "PATCH" => client.patch(route, Some(body)).await,
                "PUT" => client.put(route, Some(body)).await,
                other => unreachable!("unsupported method {other}"),
            }
        };
        self.runtime
            .block_on(call)
            .map_err(|err| ForgeError::Api(format!("{method} {route}: {err}")))
    }
}

/// REST base URL for a GitHub host.
pub fn api_base(host: &str) -> String {
    if host == "github.com" {
        "https://api.github.com".to_string()
    } else {
        format!("https://{host}/api/v3")
    }
}

fn state_of(pr: &Value) -> PrState {
    if pr["merged"].as_bool() == Some(true) {
        PrState::Merged
    } else if pr["state"].as_str() == Some("closed") {
        PrState::Closed
    } else if pr["draft"].as_bool() == Some(true) {
        PrState::Draft
    } else {
        PrState::Open
    }
}

impl Forge for GitHub {
    fn open_pr(&self, repo_url: &str, spec: &PrSpec) -> Result<PrHandle, ForgeError> {
        let (host, path) = self.split(repo_url)?;
        let pr = self.send(
            &host,
            "POST",
            &format!("/repos/{path}/pulls"),
            &json!({
                "title": spec.title,
                "body": spec.body,
                "head": spec.source_branch,
                "base": spec.target_branch,
            }),
        )?;
        let number = pr["number"].as_u64().unwrap_or_default();
        if !spec.labels.is_empty() {
            self.send(
                &host,
                "POST",
                &format!("/repos/{path}/issues/{number}/labels"),
                &json!({ "labels": spec.labels }),
            )?;
        }
        Ok(PrHandle {
            url: pr["html_url"].as_str().unwrap_or_default().to_string(),
            number,
        })
    }

    fn pr_status(&self, repo_url: &str, number: u64) -> Result<PrStatus, ForgeError> {
        let (host, path) = self.split(repo_url)?;
        let pr = self.get(&host, &format!("/repos/{path}/pulls/{number}"))?;

        let reviews = self.get(&host, &format!("/repos/{path}/pulls/{number}/reviews"))?;
        let approved = reviews
            .as_array()
            .is_some_and(|list| list.iter().any(|r| r["state"] == "APPROVED"));

        let ci_passing = match pr["head"]["sha"].as_str() {
            Some(sha) => {
                let status = self.get(&host, &format!("/repos/{path}/commits/{sha}/status"))?;
                match (
                    status["total_count"].as_u64().unwrap_or(0),
                    status["state"].as_str(),
                ) {
                    (0, _) | (_, Some("pending")) => None,
                    (_, Some("success")) => Some(true),
                    _ => Some(false),
                }
            }
            None => None,
        };

        Ok(PrStatus {
            state: state_of(&pr),
            approved,
            ci_passing,
            url: pr["html_url"].as_str().unwrap_or_default().to_string(),
        })
    }

    fn merge_pr(&self, repo_url: &str, number: u64) -> Result<(), ForgeError> {
        let (host, path) = self.split(repo_url)?;
        self.send(
            &host,
            "PUT",
            &format!("/repos/{path}/pulls/{number}/merge"),
            &json!({}),
        )?;
        Ok(())
    }

    fn approve_pr(&self, repo_url: &str, number: u64) -> Result<(), ForgeError> {
        let (host, path) = self.split(repo_url)?;
        self.send(
            &host,
            "POST",
            &format!("/repos/{path}/pulls/{number}/reviews"),
            &json!({ "event": "APPROVE" }),
        )?;
        Ok(())
    }

    fn update_pr_body(&self, repo_url: &str, number: u64, body: &str) -> Result<(), ForgeError> {
        let (host, path) = self.split(repo_url)?;
        self.send(
            &host,
            "PATCH",
            &format!("/repos/{path}/pulls/{number}"),
            &json!({ "body": body }),
        )?;
        Ok(())
    }

    fn list_open_prs(&self, repo_url: &str) -> Result<Vec<crate::OpenPr>, ForgeError> {
        let (host, path) = self.split(repo_url)?;
        let list = self.get(
            &host,
            &format!(
                "/repos/{path}/pulls?state=open&per_page={}",
                crate::OPEN_PRS_LIMIT
            ),
        )?;
        // The fleet-wide list stays cheap: one call per repo, no per-PR review/CI
        // enrichment (that N+1 made the view take tens of seconds). `approved`
        // and `ci_passing` are filled in on drill-in via `pr_status`.
        let out = list
            .as_array()
            .into_iter()
            .flatten()
            .map(|pr| crate::OpenPr {
                number: pr["number"].as_u64().unwrap_or_default(),
                title: pr["title"].as_str().unwrap_or_default().to_string(),
                url: pr["html_url"].as_str().unwrap_or_default().to_string(),
                state: state_of(pr),
                approved: false,
                ci_passing: None,
            })
            .collect();
        Ok(out)
    }

    fn list_ci_runs(&self, repo_url: &str) -> Result<Vec<crate::CiRun>, ForgeError> {
        let (host, path) = self.split(repo_url)?;
        let list = self.get(
            &host,
            &format!(
                "/repos/{path}/actions/runs?per_page={}",
                crate::CI_RUNS_LIMIT
            ),
        )?;
        let runs = list["workflow_runs"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        Ok(runs.iter().map(run_of).collect())
    }

    fn pr_detail(&self, repo_url: &str, number: u64) -> Result<String, ForgeError> {
        let (host, path) = self.split(repo_url)?;
        let pr = self.get(&host, &format!("/repos/{path}/pulls/{number}"))?;

        let title = pr["title"].as_str().unwrap_or_default();
        let head_branch = pr["head"]["ref"].as_str().unwrap_or("—");
        let head_sha = pr["head"]["sha"].as_str().unwrap_or_default();
        let base_branch = pr["base"]["ref"].as_str().unwrap_or("—");
        let html_url = pr["html_url"].as_str().unwrap_or_default();

        let mut out = String::new();
        out.push_str(&format!(
            "#{number} {title} — {}\n",
            match state_of(&pr) {
                PrState::Open => "open",
                PrState::Draft => "draft",
                PrState::Merged => "merged",
                PrState::Closed => "closed",
            }
        ));
        out.push_str(&format!(
            "head {head_branch} @ {}  ->  base {base_branch}\n",
            &head_sha[..7.min(head_sha.len())]
        ));
        if let Some(mergeable) = pr["mergeable"].as_bool() {
            out.push_str(&format!(
                "mergeable: {}\n",
                if mergeable { "yes" } else { "no" }
            ));
        }

        out.push_str("\n-- reviewers --\n");
        let reviews = self.get(&host, &format!("/repos/{path}/pulls/{number}/reviews"))?;
        match reviews.as_array().filter(|list| !list.is_empty()) {
            Some(list) => {
                for review in list {
                    let who = review["user"]["login"].as_str().unwrap_or("?");
                    let state = review["state"].as_str().unwrap_or("");
                    out.push_str(&format!("  {who}: {state}\n"));
                }
            }
            None => out.push_str("  (no reviews yet)\n"),
        }

        out.push_str("\n-- checks --\n");
        if head_sha.is_empty() {
            out.push_str("  (no head sha)\n");
        } else {
            let check_runs = self.get(
                &host,
                &format!("/repos/{path}/commits/{head_sha}/check-runs"),
            )?;
            let mut any = false;
            if let Some(list) = check_runs["check_runs"].as_array() {
                for check in list {
                    any = true;
                    let name = check["name"].as_str().unwrap_or("?");
                    let status = check["status"].as_str().unwrap_or("");
                    let conclusion = check["conclusion"].as_str().unwrap_or("—");
                    out.push_str(&format!("  {name}: {status}/{conclusion}\n"));
                }
            }
            // Fall back to the combined commit status for repos using the older
            // statuses API (or when no check-runs are registered).
            let status = self.get(&host, &format!("/repos/{path}/commits/{head_sha}/status"))?;
            if let Some(list) = status["statuses"].as_array() {
                for entry in list {
                    any = true;
                    let context = entry["context"].as_str().unwrap_or("?");
                    let state = entry["state"].as_str().unwrap_or("—");
                    out.push_str(&format!("  {context}: {state}\n"));
                }
            }
            if !any {
                out.push_str("  (no checks reported)\n");
            }
        }

        out.push_str("\n-- body --\n");
        let body = pr["body"].as_str().unwrap_or("");
        if body.trim().is_empty() {
            out.push_str("  (no description)\n");
        } else {
            for line in body.lines().take(60) {
                out.push_str(line);
                out.push('\n');
            }
        }

        out.push_str(&format!("\nurl: {html_url}\n"));
        Ok(out)
    }

    fn ci_run_detail(&self, repo_url: &str, run_id: u64) -> Result<String, ForgeError> {
        let (host, path) = self.split(repo_url)?;
        let run = self.get(&host, &format!("/repos/{path}/actions/runs/{run_id}"))?;

        let name = run["name"].as_str().unwrap_or("—");
        let status = run["status"].as_str().unwrap_or("");
        let conclusion = run["conclusion"].as_str().unwrap_or("—");
        let branch = run["head_branch"].as_str().unwrap_or("—");
        let event = run["event"].as_str().unwrap_or("—");
        let sha = run["head_sha"].as_str().unwrap_or_default();
        let html_url = run["html_url"].as_str().unwrap_or_default();

        let mut out = String::new();
        out.push_str(&format!("{name} — {status}/{conclusion}\n"));
        out.push_str(&format!(
            "branch {branch}  event {event}  @ {}\n",
            &sha[..7.min(sha.len())]
        ));

        out.push_str("\n-- jobs --\n");
        let jobs = self.get(&host, &format!("/repos/{path}/actions/runs/{run_id}/jobs"))?;
        match jobs["jobs"].as_array().filter(|list| !list.is_empty()) {
            Some(list) => {
                for job in list {
                    let job_name = job["name"].as_str().unwrap_or("?");
                    let job_status = job["status"].as_str().unwrap_or("");
                    let job_conclusion = job["conclusion"].as_str().unwrap_or("—");
                    out.push_str(&format!("  {job_name}: {job_status}/{job_conclusion}\n"));
                    if let Some(steps) = job["steps"].as_array() {
                        for step in steps.iter().take(30) {
                            let step_name = step["name"].as_str().unwrap_or("?");
                            let step_conclusion = step["conclusion"].as_str().unwrap_or("—");
                            out.push_str(&format!("    - {step_name}: {step_conclusion}\n"));
                        }
                    }
                }
            }
            None => out.push_str("  (no jobs reported)\n"),
        }

        out.push_str(&format!("\nurl: {html_url}\n"));
        Ok(out)
    }
}

/// Map a GitHub Actions run object to the forge-neutral [`crate::CiRun`].
fn run_of(run: &Value) -> crate::CiRun {
    let status = match (run["status"].as_str(), run["conclusion"].as_str()) {
        (Some("completed"), Some("success")) => crate::CiStatus::Passed,
        (Some("completed"), Some("cancelled")) => crate::CiStatus::Cancelled,
        (Some("completed"), _) => crate::CiStatus::Failed,
        (Some("queued" | "pending" | "waiting" | "requested"), _) => crate::CiStatus::Queued,
        _ => crate::CiStatus::Running,
    };
    crate::CiRun {
        id: run["id"].as_u64().unwrap_or_default(),
        name: run["name"].as_str().unwrap_or_default().to_string(),
        branch: run["head_branch"].as_str().unwrap_or_default().to_string(),
        event: run["event"].as_str().unwrap_or_default().to_string(),
        status,
        url: run["html_url"].as_str().unwrap_or_default().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::api_base;

    #[test]
    fn enterprise_hosts_get_the_v3_prefix() {
        assert_eq!(api_base("github.com"), "https://api.github.com");
        assert_eq!(
            api_base("github.corp.example"),
            "https://github.corp.example/api/v3"
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod runtime_check {
    // Building the octocrab client spawns a tower::buffer worker, which panicked
    // ("no reactor running") when done outside the Tokio runtime context. This
    // builds the client with no network to guard that regression.
    #[test]
    fn client_builds_inside_runtime_without_panic() {
        let gh = super::GitHub::new(String::new()).expect("runtime");
        assert!(gh.client("github.com").is_ok());
    }
}
