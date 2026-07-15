//! GitHub [`Forge`] implementation over the REST v3 API.

use serde_json::{Value, json};

use crate::{Forge, ForgeError, PrHandle, PrSpec, PrState, PrStatus, repo_coords};

/// GitHub client: github.com and GitHub Enterprise (`/api/v3`).
#[derive(Debug, Clone)]
pub struct GitHub {
    token: String,
}

impl GitHub {
    pub fn new(token: String) -> Self {
        Self { token }
    }

    fn repo_api(&self, repo_url: &str) -> Result<String, ForgeError> {
        let coords = repo_coords(repo_url)
            .ok_or_else(|| ForgeError::UnsupportedUrl(repo_url.to_string()))?;
        Ok(format!("{}/repos/{}", api_base(&coords.host), coords.path))
    }

    fn call(&self, method: &str, url: &str, body: Option<Value>) -> Result<Value, ForgeError> {
        let request = ureq::request(method, url)
            .set("authorization", &format!("Bearer {}", self.token))
            .set("accept", "application/vnd.github+json")
            .set("user-agent", "keel");
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
        let api = self.repo_api(repo_url)?;
        let pr = self.call(
            "POST",
            &format!("{api}/pulls"),
            Some(json!({
                "title": spec.title,
                "body": spec.body,
                "head": spec.source_branch,
                "base": spec.target_branch,
            })),
        )?;
        Ok(PrHandle {
            url: pr["html_url"].as_str().unwrap_or_default().to_string(),
            number: pr["number"].as_u64().unwrap_or_default(),
        })
    }

    fn pr_status(&self, repo_url: &str, number: u64) -> Result<PrStatus, ForgeError> {
        let api = self.repo_api(repo_url)?;
        let pr = self.call("GET", &format!("{api}/pulls/{number}"), None)?;

        let reviews = self.call("GET", &format!("{api}/pulls/{number}/reviews"), None)?;
        let approved = reviews
            .as_array()
            .is_some_and(|list| list.iter().any(|r| r["state"] == "APPROVED"));

        let ci_passing = match pr["head"]["sha"].as_str() {
            Some(sha) => {
                let status = self.call("GET", &format!("{api}/commits/{sha}/status"), None)?;
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
        let api = self.repo_api(repo_url)?;
        self.call(
            "PUT",
            &format!("{api}/pulls/{number}/merge"),
            Some(json!({})),
        )?;
        Ok(())
    }

    fn update_pr_body(&self, repo_url: &str, number: u64, body: &str) -> Result<(), ForgeError> {
        let api = self.repo_api(repo_url)?;
        self.call(
            "PATCH",
            &format!("{api}/pulls/{number}"),
            Some(json!({ "body": body })),
        )?;
        Ok(())
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
