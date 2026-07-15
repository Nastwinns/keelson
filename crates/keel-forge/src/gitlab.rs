//! GitLab [`Forge`] implementation over the REST v4 API (gitlab.com and
//! self-hosted instances).

use serde_json::{Value, json};

use crate::{Forge, ForgeError, PrHandle, PrSpec, PrState, PrStatus, repo_coords};

/// GitLab client. MRs map onto the forge-neutral PR vocabulary.
#[derive(Debug, Clone)]
pub struct GitLab {
    token: String,
}

impl GitLab {
    pub fn new(token: String) -> Self {
        Self { token }
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

    fn call(&self, method: &str, url: &str, body: Option<Value>) -> Result<Value, ForgeError> {
        let request = ureq::request(method, url).set("private-token", &self.token);
        let response = match body {
            Some(json) => request.send_json(json),
            None => request.call(),
        };
        match response {
            Ok(resp) => resp
                .into_json()
                .map_err(|err| ForgeError::Api(format!("invalid JSON from {url}: {err}"))),
            Err(ureq::Error::Status(code, resp)) => {
                let detail = resp.into_string().unwrap_or_default();
                Err(ForgeError::Api(format!(
                    "{method} {url} -> {code}: {detail}"
                )))
            }
            Err(err) => Err(ForgeError::Api(format!("{method} {url}: {err}"))),
        }
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
        let mr = self.call(
            "POST",
            &format!("{api}/merge_requests"),
            Some(json!({
                "title": spec.title,
                "description": spec.body,
                "source_branch": spec.source_branch,
                "target_branch": spec.target_branch,
            })),
        )?;
        Ok(PrHandle {
            url: mr["web_url"].as_str().unwrap_or_default().to_string(),
            number: mr["iid"].as_u64().unwrap_or_default(),
        })
    }

    fn pr_status(&self, repo_url: &str, number: u64) -> Result<PrStatus, ForgeError> {
        let api = self.project_api(repo_url)?;
        let mr = self.call("GET", &format!("{api}/merge_requests/{number}"), None)?;

        let approvals = self.call(
            "GET",
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
            "PUT",
            &format!("{api}/merge_requests/{number}/merge"),
            Some(json!({})),
        )?;
        Ok(())
    }

    fn update_pr_body(&self, repo_url: &str, number: u64, body: &str) -> Result<(), ForgeError> {
        let api = self.project_api(repo_url)?;
        self.call(
            "PUT",
            &format!("{api}/merge_requests/{number}"),
            Some(json!({ "description": body })),
        )?;
        Ok(())
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
