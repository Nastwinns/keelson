//! GitLab [`Forge`] implementation over the REST v4 API via `reqwest`
//! (gitlab.com and self-hosted instances).

use reqwest::Method;
use reqwest::blocking::Client;
use serde_json::{Value, json};

use crate::{Forge, ForgeError, PrHandle, PrSpec, PrState, PrStatus, repo_coords};

/// GitLab client. MRs map onto the forge-neutral PR vocabulary.
#[derive(Debug, Clone)]
pub struct GitLab {
    token: String,
    http: Client,
}

impl GitLab {
    pub fn new(token: String) -> Self {
        Self {
            token,
            http: Client::new(),
        }
    }

    fn project_api(&self, repo_url: &str) -> Result<String, ForgeError> {
        let coords = repo_coords(repo_url)
            .ok_or_else(|| ForgeError::UnsupportedUrl(repo_url.to_string()))?;
        Ok(format!(
            "https://{}/api/v4/projects/{}",
            coords.host,
            encode_path(&coords.path)
        ))
    }

    fn call(&self, method: Method, url: &str, body: Option<Value>) -> Result<Value, ForgeError> {
        let mut request = self
            .http
            .request(method.clone(), url)
            .header("PRIVATE-TOKEN", &self.token);
        if let Some(json) = body {
            request = request.json(&json);
        }
        let response = request
            .send()
            .map_err(|err| ForgeError::Api(format!("{method} {url}: {err}")))?;
        let status = response.status();
        if !status.is_success() {
            let detail = response.text().unwrap_or_default();
            return Err(ForgeError::Api(format!(
                "{method} {url} -> {status}: {detail}"
            )));
        }
        response
            .json()
            .map_err(|err| ForgeError::Api(format!("invalid JSON from {url}: {err}")))
    }
}

/// URL-encode a project path for the `/projects/:id` API (`/` -> `%2F`).
pub fn encode_path(path: &str) -> String {
    path.replace('/', "%2F")
}

fn state_of(mr: &Value) -> PrState {
    match mr["state"].as_str() {
        Some("merged") => PrState::Merged,
        Some("closed") => PrState::Closed,
        _ if mr["draft"].as_bool() == Some(true) => PrState::Draft,
        _ => PrState::Open,
    }
}

impl Forge for GitLab {
    fn open_pr(&self, repo_url: &str, spec: &PrSpec) -> Result<PrHandle, ForgeError> {
        let api = self.project_api(repo_url)?;
        let mut payload = json!({
            "title": spec.title,
            "description": spec.body,
            "source_branch": spec.source_branch,
            "target_branch": spec.target_branch,
        });
        if !spec.labels.is_empty() {
            payload["labels"] = Value::String(spec.labels.join(","));
        }
        let mr = self.call(
            Method::POST,
            &format!("{api}/merge_requests"),
            Some(payload),
        )?;
        Ok(PrHandle {
            url: mr["web_url"].as_str().unwrap_or_default().to_string(),
            number: mr["iid"].as_u64().unwrap_or_default(),
        })
    }

    fn pr_status(&self, repo_url: &str, number: u64) -> Result<PrStatus, ForgeError> {
        let api = self.project_api(repo_url)?;
        let mr = self.call(Method::GET, &format!("{api}/merge_requests/{number}"), None)?;

        let approvals = self.call(
            Method::GET,
            &format!("{api}/merge_requests/{number}/approvals"),
            None,
        )?;
        let approved = approvals["approved"].as_bool().unwrap_or(false);

        let ci_passing = match mr["head_pipeline"]["status"].as_str() {
            None | Some("created" | "pending" | "running" | "waiting_for_resource") => None,
            Some("success") => Some(true),
            Some(_) => Some(false),
        };

        Ok(PrStatus {
            state: state_of(&mr),
            approved,
            ci_passing,
            url: mr["web_url"].as_str().unwrap_or_default().to_string(),
        })
    }

    fn merge_pr(&self, repo_url: &str, number: u64) -> Result<(), ForgeError> {
        let api = self.project_api(repo_url)?;
        self.call(
            Method::PUT,
            &format!("{api}/merge_requests/{number}/merge"),
            Some(json!({})),
        )?;
        Ok(())
    }

    fn update_pr_body(&self, repo_url: &str, number: u64, body: &str) -> Result<(), ForgeError> {
        let api = self.project_api(repo_url)?;
        self.call(
            Method::PUT,
            &format!("{api}/merge_requests/{number}"),
            Some(json!({ "description": body })),
        )?;
        Ok(())
    }

    fn list_open_prs(&self, repo_url: &str) -> Result<Vec<crate::OpenPr>, ForgeError> {
        let api = self.project_api(repo_url)?;
        let list = self.call(
            Method::GET,
            &format!(
                "{api}/merge_requests?state=opened&per_page={}",
                crate::OPEN_PRS_LIMIT
            ),
            None,
        )?;
        // Cheap fleet list: one call per repo. CI status is free from the list
        // payload; the per-MR approvals call (an N+1) is deferred to drill-in.
        let out = list
            .as_array()
            .into_iter()
            .flatten()
            .map(|mr| crate::OpenPr {
                number: mr["iid"].as_u64().unwrap_or_default(),
                title: mr["title"].as_str().unwrap_or_default().to_string(),
                url: mr["web_url"].as_str().unwrap_or_default().to_string(),
                state: state_of(mr),
                approved: false,
                ci_passing: match mr["head_pipeline"]["status"].as_str() {
                    None | Some("created" | "pending" | "running" | "waiting_for_resource") => None,
                    Some("success") => Some(true),
                    Some(_) => Some(false),
                },
            })
            .collect();
        Ok(out)
    }

    fn list_ci_runs(&self, repo_url: &str) -> Result<Vec<crate::CiRun>, ForgeError> {
        let api = self.project_api(repo_url)?;
        let list = self.call(
            Method::GET,
            &format!("{api}/pipelines?per_page={}", crate::CI_RUNS_LIMIT),
            None,
        )?;
        let pipelines = list.as_array().cloned().unwrap_or_default();
        Ok(pipelines.iter().map(run_of).collect())
    }

    fn pr_detail(&self, repo_url: &str, number: u64) -> Result<String, ForgeError> {
        let api = self.project_api(repo_url)?;
        let mr = self.call(Method::GET, &format!("{api}/merge_requests/{number}"), None)?;

        let title = mr["title"].as_str().unwrap_or_default();
        let source_branch = mr["source_branch"].as_str().unwrap_or("—");
        let target_branch = mr["target_branch"].as_str().unwrap_or("—");
        let head_sha = mr["sha"].as_str().unwrap_or_default();
        let web_url = mr["web_url"].as_str().unwrap_or_default();

        let mut out = String::new();
        out.push_str(&format!(
            "#{number} {title} — {}\n",
            match state_of(&mr) {
                PrState::Open => "open",
                PrState::Draft => "draft",
                PrState::Merged => "merged",
                PrState::Closed => "closed",
            }
        ));
        out.push_str(&format!(
            "head {source_branch} @ {}  ->  base {target_branch}\n",
            &head_sha[..7.min(head_sha.len())]
        ));
        if let Some(status) = mr["merge_status"].as_str() {
            out.push_str(&format!("mergeable: {status}\n"));
        }

        out.push_str("\n-- reviewers --\n");
        match self.call(
            Method::GET,
            &format!("{api}/merge_requests/{number}/approvals"),
            None,
        ) {
            Ok(approvals) => {
                let approved = approvals["approved"].as_bool().unwrap_or(false);
                out.push_str(&format!(
                    "  approved: {}\n",
                    if approved { "yes" } else { "no" }
                ));
                if let Some(list) = approvals["approved_by"].as_array() {
                    for entry in list {
                        let who = entry["user"]["username"].as_str().unwrap_or("?");
                        out.push_str(&format!("  {who}: approved\n"));
                    }
                }
            }
            Err(err) => out.push_str(&format!("  (approvals unavailable: {err})\n")),
        }

        out.push_str("\n-- checks --\n");
        match mr["head_pipeline"]["id"].as_u64() {
            Some(pipeline_id) => {
                let pipeline_status = mr["head_pipeline"]["status"].as_str().unwrap_or("—");
                out.push_str(&format!("  pipeline #{pipeline_id}: {pipeline_status}\n"));
                match self.call(
                    Method::GET,
                    &format!("{api}/pipelines/{pipeline_id}/jobs"),
                    None,
                ) {
                    Ok(jobs) => {
                        if let Some(list) = jobs.as_array() {
                            for job in list {
                                let job_name = job["name"].as_str().unwrap_or("?");
                                let job_status = job["status"].as_str().unwrap_or("—");
                                out.push_str(&format!("  {job_name}: {job_status}\n"));
                            }
                        }
                    }
                    Err(err) => out.push_str(&format!("  (jobs unavailable: {err})\n")),
                }
            }
            None => out.push_str("  (no pipeline for this MR)\n"),
        }

        out.push_str("\n-- body --\n");
        let body = mr["description"].as_str().unwrap_or("");
        if body.trim().is_empty() {
            out.push_str("  (no description)\n");
        } else {
            for line in body.lines().take(60) {
                out.push_str(line);
                out.push('\n');
            }
        }

        out.push_str(&format!("\nurl: {web_url}\n"));
        Ok(out)
    }

    fn ci_run_detail(&self, repo_url: &str, run_id: u64) -> Result<String, ForgeError> {
        let api = self.project_api(repo_url)?;
        let pipeline = self.call(Method::GET, &format!("{api}/pipelines/{run_id}"), None)?;

        let status = pipeline["status"].as_str().unwrap_or("—");
        let branch = pipeline["ref"].as_str().unwrap_or("—");
        let source = pipeline["source"].as_str().unwrap_or("—");
        let sha = pipeline["sha"].as_str().unwrap_or_default();
        let web_url = pipeline["web_url"].as_str().unwrap_or_default();

        let mut out = String::new();
        out.push_str(&format!("pipeline #{run_id} — {status}\n"));
        out.push_str(&format!(
            "branch {branch}  source {source}  @ {}\n",
            &sha[..7.min(sha.len())]
        ));

        out.push_str("\n-- jobs --\n");
        let jobs = self.call(Method::GET, &format!("{api}/pipelines/{run_id}/jobs"), None)?;
        match jobs.as_array().filter(|list| !list.is_empty()) {
            Some(list) => {
                for job in list {
                    let job_name = job["name"].as_str().unwrap_or("?");
                    let stage = job["stage"].as_str().unwrap_or("—");
                    let job_status = job["status"].as_str().unwrap_or("—");
                    out.push_str(&format!("  [{stage}] {job_name}: {job_status}\n"));
                }
            }
            None => out.push_str("  (no jobs reported)\n"),
        }

        out.push_str(&format!("\nweb_url: {web_url}\n"));
        Ok(out)
    }
}

/// Map a GitLab pipeline object to the forge-neutral [`crate::CiRun`].
fn run_of(pipeline: &Value) -> crate::CiRun {
    let status = match pipeline["status"].as_str() {
        Some("success") => crate::CiStatus::Passed,
        Some("failed") => crate::CiStatus::Failed,
        Some("canceled" | "skipped") => crate::CiStatus::Cancelled,
        Some("created" | "pending" | "waiting_for_resource" | "preparing" | "scheduled") => {
            crate::CiStatus::Queued
        }
        _ => crate::CiStatus::Running,
    };
    let id = pipeline["id"].as_u64().unwrap_or_default();
    crate::CiRun {
        id,
        name: format!("#{id}"),
        branch: pipeline["ref"].as_str().unwrap_or_default().to_string(),
        event: pipeline["source"].as_str().unwrap_or_default().to_string(),
        status,
        url: pipeline["web_url"].as_str().unwrap_or_default().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::encode_path;

    #[test]
    fn nested_group_paths_are_encoded() {
        assert_eq!(encode_path("fw/nested/kernel"), "fw%2Fnested%2Fkernel");
    }
}
