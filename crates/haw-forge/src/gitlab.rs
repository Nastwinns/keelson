//! GitLab [`Forge`] implementation over the REST v4 API via `reqwest`
//! (gitlab.com and self-hosted instances).

use reqwest::Method;
use reqwest::blocking::Client;
use serde_json::{Value, json};

use crate::{Forge, ForgeError, PrHandle, PrSpec, PrState, PrStatus, repo_coords};

/// GitLab client. MRs map onto the forge-neutral PR vocabulary.
#[derive(Clone)]
pub struct GitLab {
    token: String,
    http: Client,
}

/// Redacting `Debug`: never prints the raw token, only whether it is set.
impl std::fmt::Debug for GitLab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitLab")
            .field(
                "token",
                &if self.token.is_empty() {
                    "<empty>"
                } else {
                    "<redacted>"
                },
            )
            .finish_non_exhaustive()
    }
}

impl GitLab {
    pub fn new(token: String) -> Self {
        Self {
            token,
            // Hardened client: SSRF-resistant redirect policy. GitLab traces are
            // served inline (no cross-host redirect), so the `PRIVATE-TOKEN`
            // header is never replayed onto a foreign host by this policy.
            http: crate::http::forge_client(),
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
        crate::http::json_capped(response, url)
    }

    /// Raw-text GET (job traces are plain text, not JSON). Returns `Ok(None)`
    /// on 404 (no trace / expired).
    fn call_text(&self, url: &str) -> Result<Option<String>, ForgeError> {
        let response = self
            .http
            .get(url)
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .map_err(|err| ForgeError::Api(format!("GET {url}: {err}")))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let status = response.status();
        if !status.is_success() {
            let detail = response.text().unwrap_or_default();
            return Err(ForgeError::Api(format!("GET {url} -> {status}: {detail}")));
        }
        crate::http::read_capped_text(response, url).map(Some)
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

    fn approve_pr(&self, repo_url: &str, number: u64) -> Result<(), ForgeError> {
        let api = self.project_api(repo_url)?;
        self.call(
            Method::POST,
            &format!("{api}/merge_requests/{number}/approve"),
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
        let (state_emoji, state_label) = pr_state_badge(state_of(&mr));
        out.push_str(&format!(
            "{state_emoji} #{number} {title} — {state_label}\n"
        ));
        out.push_str(&format!(
            "🌿 head {source_branch} @ {}  ->  base {target_branch}\n",
            &head_sha[..7.min(head_sha.len())]
        ));
        if let Some(status) = mr["merge_status"].as_str() {
            out.push_str(&format!("mergeable: {status}\n"));
        }

        out.push_str("\n👤 -- reviewers --\n");
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

        out.push_str("\n✅ -- checks --\n");
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

        out.push_str("\n📄 -- body --\n");
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

        let jobs = self.call(Method::GET, &format!("{api}/pipelines/{run_id}/jobs"), None)?;
        let job_list = jobs.as_array().cloned().unwrap_or_default();

        let mut out = String::new();
        let total = job_list.len();
        let completed = job_list
            .iter()
            .filter(|job| is_finished(job["status"].as_str().unwrap_or("")))
            .count();
        let finished = is_finished(status);
        let bar = crate::progress_bar(if finished { total } else { completed }, total);
        let (phase_emoji, phase) = pipeline_phase(status);
        out.push_str(&format!("progress: {bar}  ·  {phase_emoji} {phase}\n"));

        out.push_str(&format!(
            "{} pipeline #{run_id} — {status}\n",
            ci_emoji(status)
        ));
        out.push_str(&format!(
            "🌿 branch {branch}  source {source}  @ {}\n",
            &sha[..7.min(sha.len())]
        ));

        out.push_str("\n🧩 -- jobs --\n");
        match Some(&job_list).filter(|list| !list.is_empty()) {
            Some(list) => {
                for job in list {
                    let job_name = job["name"].as_str().unwrap_or("?");
                    let stage = job["stage"].as_str().unwrap_or("—");
                    let job_status = job["status"].as_str().unwrap_or("—");
                    let runner = job_runner(job);
                    out.push_str(&format!(
                        "  {} [{stage}] {job_name}: {job_status}{runner}\n",
                        ci_emoji(job_status)
                    ));
                }
            }
            None => out.push_str("  (no jobs reported)\n"),
        }

        out.push_str(&format!("\nweb_url: {web_url}\n"));
        Ok(out)
    }

    fn pr_diff(&self, repo_url: &str, number: u64) -> Result<String, ForgeError> {
        let api = self.project_api(repo_url)?;
        let changes = self.call(
            Method::GET,
            &format!("{api}/merge_requests/{number}/changes"),
            None,
        )?;
        let list = changes["changes"].as_array().cloned().unwrap_or_default();
        if list.is_empty() {
            return Ok(format!("(no changes for !{number})\n"));
        }
        let mut out = String::new();
        for change in &list {
            let old_path = change["old_path"].as_str().unwrap_or("?");
            let new_path = change["new_path"].as_str().unwrap_or("?");
            if old_path == new_path {
                out.push_str(&format!("diff --git a/{old_path} b/{new_path}\n"));
            } else {
                out.push_str(&format!(
                    "diff --git a/{old_path} b/{new_path}\nrename from {old_path}\nrename to {new_path}\n"
                ));
            }
            let patch = change["diff"].as_str().unwrap_or("");
            out.push_str(patch);
            if !patch.ends_with('\n') {
                out.push('\n');
            }
        }
        Ok(crate::cap_lines(&out, crate::DIFF_LINE_CAP))
    }

    fn ci_logs(&self, repo_url: &str, run_id: u64) -> Result<String, ForgeError> {
        let api = self.project_api(repo_url)?;
        let jobs = self.call(Method::GET, &format!("{api}/pipelines/{run_id}/jobs"), None)?;
        let list = jobs.as_array().cloned().unwrap_or_default();
        if list.is_empty() {
            return Ok(format!("(no jobs for pipeline #{run_id})\n"));
        }
        // Failed jobs first, like the GitHub side.
        let mut ordered: Vec<&Value> = list.iter().collect();
        ordered.sort_by_key(|job| u8::from(job["status"].as_str() != Some("failed")));

        let mut out = String::new();
        for job in ordered.into_iter().take(6) {
            let job_id = job["id"].as_u64().unwrap_or_default();
            let job_name = job["name"].as_str().unwrap_or("?");
            let status = job["status"].as_str().unwrap_or("—");
            let runner = job_runner(job);
            out.push_str(&format!("📜 == {job_name} ({status}){runner} ==\n"));
            match self.call_text(&format!("{api}/jobs/{job_id}/trace")) {
                Ok(Some(trace)) if !trace.trim().is_empty() => {
                    let lines: Vec<&str> = trace.lines().collect();
                    let tail = lines.len().saturating_sub(200);
                    if tail > 0 {
                        out.push_str(&format!("… ({tail} earlier line(s) omitted)\n"));
                    }
                    for line in &lines[tail..] {
                        out.push_str(line);
                        out.push('\n');
                    }
                }
                Ok(_) => out.push_str("  (trace unavailable — expired or empty)\n"),
                Err(err) => out.push_str(&format!("  (trace unavailable: {err})\n")),
            }
            out.push('\n');
        }
        Ok(crate::cap_lines(&out, crate::LOG_LINE_CAP))
    }

    fn repo_tree(
        &self,
        repo_url: &str,
        subpath: &str,
        git_ref: Option<&str>,
    ) -> Result<Vec<crate::TreeEntry>, ForgeError> {
        let api = self.project_api(repo_url)?;
        let sub = subpath.trim_matches('/');
        let mut url = format!("{api}/repository/tree?per_page=100");
        if !sub.is_empty() {
            url.push_str(&format!("&path={}", encode_path(sub)));
        }
        if let Some(git_ref) = git_ref {
            url.push_str(&format!("&ref={git_ref}"));
        }
        let list = self.call(Method::GET, &url, None)?;
        let out = list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let name = entry["name"].as_str()?.to_string();
                let is_dir = entry["type"].as_str() == Some("tree");
                Some(crate::TreeEntry { name, is_dir })
            })
            .collect();
        Ok(out)
    }

    fn file_blob(
        &self,
        repo_url: &str,
        path: &str,
        git_ref: Option<&str>,
    ) -> Result<String, ForgeError> {
        let api = self.project_api(repo_url)?;
        let file = path.trim_start_matches('/');
        let mut url = format!("{api}/repository/files/{}/raw", encode_path(file));
        if let Some(git_ref) = git_ref {
            url.push_str(&format!("?ref={git_ref}"));
        }
        match self.call_text(&url)? {
            Some(text) => Ok(crate::cap_lines(&text, crate::FILE_LINE_CAP)),
            None => Ok(format!("(no file at {file} — not found)\n")),
        }
    }

    fn pr_files(&self, repo_url: &str, number: u64) -> Result<Vec<crate::PrFile>, ForgeError> {
        let api = self.project_api(repo_url)?;
        let mr = self.call(
            Method::GET,
            &format!("{api}/merge_requests/{number}/changes"),
            None,
        )?;
        let changes = mr["changes"].as_array().cloned().unwrap_or_default();
        let out = changes
            .iter()
            .filter_map(|change| {
                let new_path = change["new_path"].as_str();
                let old_path = change["old_path"].as_str();
                let path = new_path.or(old_path)?.to_string();
                let status = if change["new_file"].as_bool() == Some(true) {
                    "added"
                } else if change["deleted_file"].as_bool() == Some(true) {
                    "removed"
                } else if change["renamed_file"].as_bool() == Some(true) {
                    "renamed"
                } else {
                    "modified"
                };
                Some(crate::PrFile {
                    path,
                    status: status.to_string(),
                })
            })
            .collect();
        Ok(out)
    }

    fn pr_file_content(
        &self,
        repo_url: &str,
        number: u64,
        path: &str,
    ) -> Result<String, ForgeError> {
        let api = self.project_api(repo_url)?;
        let mr = self.call(Method::GET, &format!("{api}/merge_requests/{number}"), None)?;
        // Prefer the head sha; fall back to the source branch name.
        let git_ref = mr["sha"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| mr["source_branch"].as_str())
            .unwrap_or_default();
        if git_ref.is_empty() {
            return Ok("(file not present at this ref)\n".to_string());
        }
        let file = path.trim_start_matches('/');
        let url = format!(
            "{api}/repository/files/{}/raw?ref={git_ref}",
            encode_path(file)
        );
        match self.call_text(&url)? {
            Some(text) => Ok(crate::cap_lines(&text, crate::FILE_LINE_CAP)),
            None => Ok("(file not present at this ref)\n".to_string()),
        }
    }

    fn repo_file_paths(
        &self,
        repo_url: &str,
        git_ref: Option<&str>,
    ) -> Result<Vec<String>, ForgeError> {
        let api = self.project_api(repo_url)?;
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        // Recursive tree, page-walked (100/page) until empty, the cap, or a
        // sane page ceiling so a huge repo can't loop unbounded.
        let max_pages = crate::FILE_PATHS_CAP.div_ceil(100).max(1) + 1;
        for page in 1..=max_pages {
            let mut url = format!("{api}/repository/tree?recursive=true&per_page=100&page={page}");
            if let Some(git_ref) = git_ref.filter(|r| !r.is_empty()) {
                url.push_str(&format!("&ref={git_ref}"));
            }
            let list = self.call(Method::GET, &url, None)?;
            let entries = list.as_array().cloned().unwrap_or_default();
            if entries.is_empty() {
                break;
            }
            let before = out.len();
            append_gitlab_tree_paths(&entries, &mut out, &mut seen);
            if out.len() >= crate::FILE_PATHS_CAP {
                out.truncate(crate::FILE_PATHS_CAP);
                break;
            }
            // A short page means the last one.
            if entries.len() < 100 && out.len() == before + entries.len() {
                break;
            }
            if entries.len() < 100 {
                break;
            }
        }
        Ok(out)
    }

    fn list_refs(&self, repo_url: &str) -> Result<Vec<crate::ForgeRef>, ForgeError> {
        let api = self.project_api(repo_url)?;
        let branches = self.call(
            Method::GET,
            &format!("{api}/repository/branches?per_page=100"),
            None,
        )?;
        let tags = self.call(
            Method::GET,
            &format!("{api}/repository/tags?per_page=100"),
            None,
        )?;
        Ok(refs_from_gitlab(&branches, &tags))
    }
}

/// Append the FILE paths (`type == "blob"`) of one GitLab tree page into `out`,
/// deduping via `seen`. Directories (`type == "tree"`) are dropped.
fn append_gitlab_tree_paths(
    entries: &[Value],
    out: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    for entry in entries {
        if entry["type"].as_str() != Some("blob") {
            continue;
        }
        if let Some(path) = entry["path"].as_str()
            && seen.insert(path.to_string())
        {
            out.push(path.to_string());
            if out.len() >= crate::FILE_PATHS_CAP {
                return;
            }
        }
    }
}

/// Branches then tags from GitLab `/repository/branches` + `/repository/tags`
/// list responses, each capped at [`crate::REFS_CAP`].
fn refs_from_gitlab(branches: &Value, tags: &Value) -> Vec<crate::ForgeRef> {
    let mut out = Vec::new();
    for b in branches
        .as_array()
        .into_iter()
        .flatten()
        .take(crate::REFS_CAP)
    {
        if let Some(name) = b["name"].as_str() {
            out.push(crate::ForgeRef {
                name: name.to_string(),
                kind: crate::ForgeRefKind::Branch,
            });
        }
    }
    for t in tags.as_array().into_iter().flatten().take(crate::REFS_CAP) {
        if let Some(name) = t["name"].as_str() {
            out.push(crate::ForgeRef {
                name: name.to_string(),
                kind: crate::ForgeRefKind::Tag,
            });
        }
    }
    out
}

/// Whether a GitLab job/pipeline `status` is a terminal (finished) state.
fn is_finished(status: &str) -> bool {
    matches!(
        status,
        "success" | "failed" | "canceled" | "skipped" | "manual"
    )
}

/// A leading status emoji for a GitLab job/pipeline `status`.
fn ci_emoji(status: &str) -> &'static str {
    match status {
        "success" => "✅",
        "failed" => "❌",
        "canceled" | "skipped" => "⏹",
        "created" | "pending" | "waiting_for_resource" | "preparing" | "scheduled" => "⏳",
        _ => "🔄",
    }
}

/// Overall pipeline phase label + emoji for the progress line.
fn pipeline_phase(status: &str) -> (&'static str, String) {
    match status {
        "success" => ("✅", "passed".to_string()),
        "failed" => ("❌", "failed".to_string()),
        "canceled" | "skipped" => ("⏹", status.to_string()),
        "created" | "pending" | "waiting_for_resource" | "preparing" | "scheduled" => {
            ("⏳", "queued".to_string())
        }
        _ => ("🔄", "running".to_string()),
    }
}

/// Emoji + label for a PR/MR state, used in the drill-in detail header.
fn pr_state_badge(state: PrState) -> (&'static str, &'static str) {
    match state {
        PrState::Open => ("🟢", "open"),
        PrState::Draft => ("📝", "draft"),
        PrState::Merged => ("🟣", "merged"),
        PrState::Closed => ("🔴", "closed"),
    }
}

/// ` on <runner>` suffix for a GitLab job, using the runner's description/name
/// then falling back to its `tag_list`. Empty when neither is present.
fn job_runner(job: &Value) -> String {
    let runner = job["runner"]["description"]
        .as_str()
        .or_else(|| job["runner"]["name"].as_str())
        .filter(|s| !s.is_empty());
    if let Some(name) = runner {
        return format!("  on {name}");
    }
    let tags: Vec<&str> = job["tag_list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t.as_str())
        .collect();
    if tags.is_empty() {
        String::new()
    } else {
        format!("  on {}", tags.join(","))
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
    use super::{GitLab, encode_path};

    #[test]
    fn nested_group_paths_are_encoded() {
        assert_eq!(encode_path("fw/nested/kernel"), "fw%2Fnested%2Fkernel");
    }

    #[test]
    fn debug_redacts_token() {
        let client = GitLab::new("glpat-supersecret".to_string());
        let dumped = format!("{client:?}");
        assert!(!dumped.contains("glpat-supersecret"), "{dumped}");
        assert!(dumped.contains("<redacted>"), "{dumped}");
    }

    use super::{append_gitlab_tree_paths, refs_from_gitlab};
    use crate::ForgeRefKind;
    use serde_json::json;

    #[test]
    fn tree_page_keeps_blobs_dropping_trees() {
        let entries = vec![
            json!({ "path": "src", "type": "tree" }),
            json!({ "path": "src/main.rs", "type": "blob" }),
            json!({ "path": "Cargo.toml", "type": "blob" }),
        ];
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        append_gitlab_tree_paths(&entries, &mut out, &mut seen);
        assert_eq!(
            out,
            vec!["src/main.rs".to_string(), "Cargo.toml".to_string()]
        );
    }

    #[test]
    fn refs_lists_branches_then_tags() {
        let branches = json!([{ "name": "main" }, { "name": "dev" }]);
        let tags = json!([{ "name": "v2.0" }]);
        let refs = refs_from_gitlab(&branches, &tags);
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].kind, ForgeRefKind::Branch);
        assert_eq!(refs[2].name, "v2.0");
        assert_eq!(refs[2].kind, ForgeRefKind::Tag);
    }
}
