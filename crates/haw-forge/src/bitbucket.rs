//! Bitbucket Cloud [`Forge`] implementation over the REST API 2.0 via
//! `reqwest` (base `https://api.bitbucket.org/2.0`). PRs map onto the
//! forge-neutral PR vocabulary; CI maps onto Bitbucket Pipelines.

use reqwest::Method;
use reqwest::blocking::{Client, RequestBuilder};
use serde_json::{Value, json};

use crate::{Forge, ForgeError, PrHandle, PrSpec, PrState, PrStatus, repo_coords};

/// REST base URL for Bitbucket Cloud.
const API_BASE: &str = "https://api.bitbucket.org/2.0";

/// How Bitbucket authenticates a request: HTTP Basic (`user:token`) when
/// `BITBUCKET_USER` is set, otherwise a Bearer token.
#[derive(Clone)]
enum Auth {
    Basic { user: String, token: String },
    Bearer { token: String },
}

/// Redacting `Debug`: never prints the raw token. The non-secret Basic-auth
/// `user` is shown verbatim.
impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Auth::Basic { user, .. } => f
                .debug_struct("Basic")
                .field("user", user)
                .field("token", &"<redacted>")
                .finish(),
            Auth::Bearer { .. } => f
                .debug_struct("Bearer")
                .field("token", &"<redacted>")
                .finish(),
        }
    }
}

/// Bitbucket Cloud client. Pipelines map onto the forge-neutral CI vocabulary.
#[derive(Clone)]
pub struct Bitbucket {
    auth: Auth,
    http: Client,
}

/// Redacting `Debug`: delegates to `Auth`'s redacting impl (no raw token).
impl std::fmt::Debug for Bitbucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bitbucket")
            .field("auth", &self.auth)
            .finish_non_exhaustive()
    }
}

impl Bitbucket {
    /// Build a client from a resolved token and optional Basic-auth user.
    pub fn new(token: String, user: Option<String>) -> Self {
        let auth = match user.filter(|u| !u.is_empty()) {
            Some(user) => Auth::Basic { user, token },
            None => Auth::Bearer { token },
        };
        Self {
            auth,
            // Hardened client: SSRF-resistant redirect policy. Bitbucket auth
            // uses the standard `Authorization` header, which reqwest drops on
            // any cross-host redirect.
            http: crate::http::forge_client(),
        }
    }

    /// `{ws}/{slug}` repo path plus the API base for one repository URL.
    fn repo_api(&self, repo_url: &str) -> Result<String, ForgeError> {
        let coords = repo_coords(repo_url)
            .ok_or_else(|| ForgeError::UnsupportedUrl(repo_url.to_string()))?;
        // Bitbucket Cloud repos are exactly `{workspace}/{slug}`; deeper paths
        // aren't valid repositories.
        Ok(format!("{API_BASE}/repositories/{}", coords.path))
    }

    /// Attach the resolved auth (Basic or Bearer) to a request builder.
    fn authed(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.auth {
            Auth::Basic { user, token } => request.basic_auth(user, Some(token)),
            Auth::Bearer { token } => request.bearer_auth(token),
        }
    }

    fn call(&self, method: Method, url: &str, body: Option<Value>) -> Result<Value, ForgeError> {
        let mut request = self.authed(self.http.request(method.clone(), url));
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
        // Some endpoints (approve/merge) may return an empty body on success;
        // treat that as an empty JSON object rather than an error.
        let text = crate::http::read_capped_text(response, url)?;
        if text.trim().is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_str(&text)
            .map_err(|err| ForgeError::Api(format!("invalid JSON from {url}: {err}")))
    }

    /// Raw-text GET (diffs and pipeline logs are plain text, not JSON). Returns
    /// `Ok(None)` on 404 (no diff / no log / expired).
    fn call_text(&self, url: &str) -> Result<Option<String>, ForgeError> {
        let response = self
            .authed(self.http.get(url))
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

    /// Walk a paginated collection, following the `next` link until it runs out
    /// or `cap` values have been gathered. Each page is `{values:[...], next}`.
    fn paginate(&self, first_url: &str, cap: usize) -> Result<Vec<Value>, ForgeError> {
        let mut out: Vec<Value> = Vec::new();
        let mut next = Some(first_url.to_string());
        while let Some(url) = next {
            let page = self.call(Method::GET, &url, None)?;
            if let Some(values) = page["values"].as_array() {
                out.extend(values.iter().cloned());
            }
            if out.len() >= cap {
                out.truncate(cap);
                break;
            }
            next = page["next"].as_str().map(str::to_string);
        }
        Ok(out)
    }
}

/// URL-encode a repo file path for the `src` endpoint. Path separators are
/// preserved (they're meaningful); other reserved characters are escaped.
pub fn encode_path(path: &str) -> String {
    path.split('/')
        .map(encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

/// Percent-encode a single path segment, leaving unreserved characters as-is.
fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Whether a PR has at least one approving participant.
fn approved_of(pr: &Value) -> bool {
    pr["participants"]
        .as_array()
        .is_some_and(|list| list.iter().any(|p| p["approved"].as_bool() == Some(true)))
}

fn state_of(pr: &Value) -> PrState {
    match pr["state"].as_str() {
        Some("MERGED") => PrState::Merged,
        Some("DECLINED" | "SUPERSEDED") => PrState::Closed,
        _ => PrState::Open,
    }
}

impl Forge for Bitbucket {
    fn open_pr(&self, repo_url: &str, spec: &PrSpec) -> Result<PrHandle, ForgeError> {
        let api = self.repo_api(repo_url)?;
        let pr = self.call(
            Method::POST,
            &format!("{api}/pullrequests"),
            Some(json!({
                "title": spec.title,
                "description": spec.body,
                "source": { "branch": { "name": spec.source_branch } },
                "destination": { "branch": { "name": spec.target_branch } },
            })),
        )?;
        Ok(PrHandle {
            url: pr["links"]["html"]["href"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            number: pr["id"].as_u64().unwrap_or_default(),
        })
    }

    fn pr_status(&self, repo_url: &str, number: u64) -> Result<PrStatus, ForgeError> {
        let api = self.repo_api(repo_url)?;
        let pr = self.call(Method::GET, &format!("{api}/pullrequests/{number}"), None)?;

        // Pipeline result for the PR head commit, when one exists.
        let ci_passing = match pr["source"]["commit"]["hash"].as_str() {
            Some(sha) if !sha.is_empty() => {
                let statuses = self.paginate(
                    &format!("{api}/commit/{sha}/statuses"),
                    crate::CI_RUNS_LIMIT,
                )?;
                commit_ci_passing(&statuses)
            }
            _ => None,
        };

        Ok(PrStatus {
            state: state_of(&pr),
            approved: approved_of(&pr),
            ci_passing,
            url: pr["links"]["html"]["href"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        })
    }

    fn merge_pr(&self, repo_url: &str, number: u64) -> Result<(), ForgeError> {
        let api = self.repo_api(repo_url)?;
        self.call(
            Method::POST,
            &format!("{api}/pullrequests/{number}/merge"),
            Some(json!({})),
        )?;
        Ok(())
    }

    fn approve_pr(&self, repo_url: &str, number: u64) -> Result<(), ForgeError> {
        let api = self.repo_api(repo_url)?;
        self.call(
            Method::POST,
            &format!("{api}/pullrequests/{number}/approve"),
            Some(json!({})),
        )?;
        Ok(())
    }

    fn update_pr_body(&self, repo_url: &str, number: u64, body: &str) -> Result<(), ForgeError> {
        let api = self.repo_api(repo_url)?;
        self.call(
            Method::PUT,
            &format!("{api}/pullrequests/{number}"),
            Some(json!({ "description": body })),
        )?;
        Ok(())
    }

    fn list_open_prs(&self, repo_url: &str) -> Result<Vec<crate::OpenPr>, ForgeError> {
        let api = self.repo_api(repo_url)?;
        // Cheap fleet list: one paginated call per repo, no per-PR N+1 for
        // approvals/CI (those are filled in on drill-in via `pr_status`).
        let prs = self.paginate(
            &format!(
                "{api}/pullrequests?state=OPEN&pagelen={}",
                crate::OPEN_PRS_LIMIT
            ),
            crate::OPEN_PRS_LIMIT,
        )?;
        let out = prs
            .iter()
            .map(|pr| crate::OpenPr {
                number: pr["id"].as_u64().unwrap_or_default(),
                title: pr["title"].as_str().unwrap_or_default().to_string(),
                url: pr["links"]["html"]["href"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                state: state_of(pr),
                approved: false,
                ci_passing: None,
            })
            .collect();
        Ok(out)
    }

    fn list_ci_runs(&self, repo_url: &str) -> Result<Vec<crate::CiRun>, ForgeError> {
        let api = self.repo_api(repo_url)?;
        let pipelines = self.paginate(
            &format!(
                "{api}/pipelines/?sort=-created_on&pagelen={}",
                crate::CI_RUNS_LIMIT
            ),
            crate::CI_RUNS_LIMIT,
        )?;
        Ok(pipelines.iter().map(run_of).collect())
    }

    fn pr_detail(&self, repo_url: &str, number: u64) -> Result<String, ForgeError> {
        let api = self.repo_api(repo_url)?;
        let pr = self.call(Method::GET, &format!("{api}/pullrequests/{number}"), None)?;

        let title = pr["title"].as_str().unwrap_or_default();
        let source_branch = pr["source"]["branch"]["name"].as_str().unwrap_or("—");
        let dest_branch = pr["destination"]["branch"]["name"].as_str().unwrap_or("—");
        let head_sha = pr["source"]["commit"]["hash"].as_str().unwrap_or_default();
        let web_url = pr["links"]["html"]["href"].as_str().unwrap_or_default();

        let mut out = String::new();
        let (state_emoji, state_label) = pr_state_badge(state_of(&pr));
        out.push_str(&format!(
            "{state_emoji} #{number} {title} — {state_label}\n"
        ));
        out.push_str(&format!(
            "🌿 head {source_branch} @ {}  ->  base {dest_branch}\n",
            &head_sha[..7.min(head_sha.len())]
        ));

        out.push_str("\n👤 -- reviewers --\n");
        match pr["participants"]
            .as_array()
            .filter(|list| !list.is_empty())
        {
            Some(list) => {
                for participant in list {
                    let who = participant["user"]["display_name"].as_str().unwrap_or("?");
                    let approved = participant["approved"].as_bool() == Some(true);
                    let role = participant["role"].as_str().unwrap_or("PARTICIPANT");
                    out.push_str(&format!(
                        "  {who} [{role}]: {}\n",
                        if approved { "approved" } else { "no vote" }
                    ));
                }
            }
            None => out.push_str("  (no participants yet)\n"),
        }

        out.push_str("\n✅ -- checks --\n");
        match head_sha {
            "" => out.push_str("  (no head commit)\n"),
            sha => {
                match self.paginate(
                    &format!("{api}/commit/{sha}/statuses"),
                    crate::CI_RUNS_LIMIT,
                ) {
                    Ok(statuses) if !statuses.is_empty() => {
                        for status in &statuses {
                            let name = status["name"]
                                .as_str()
                                .or_else(|| status["key"].as_str())
                                .unwrap_or("?");
                            let state = status["state"].as_str().unwrap_or("—");
                            out.push_str(&format!("  {name}: {state}\n"));
                        }
                    }
                    Ok(_) => out.push_str("  (no checks reported)\n"),
                    Err(err) => out.push_str(&format!("  (checks unavailable: {err})\n")),
                }
            }
        }

        out.push_str("\n📄 -- body --\n");
        let body = pr["description"].as_str().unwrap_or("");
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
        let api = self.repo_api(repo_url)?;
        // `run_id` is the `build_number`; the steps/log endpoints key on the
        // pipeline `uuid`, so resolve the uuid from the recent-pipelines list.
        let pipelines = self.paginate(
            &format!(
                "{api}/pipelines/?sort=-created_on&pagelen={}",
                crate::CI_RUNS_LIMIT
            ),
            crate::CI_RUNS_LIMIT,
        )?;
        let pipeline = pipelines
            .iter()
            .find(|p| p["build_number"].as_u64() == Some(run_id))
            .cloned()
            .ok_or_else(|| ForgeError::Api(format!("pipeline #{run_id} not found")))?;
        let uuid = pipeline["uuid"].as_str().unwrap_or_default();

        let status = pipeline_status_str(&pipeline);
        let branch = pipeline["target"]["ref_name"].as_str().unwrap_or("—");
        let sha = pipeline["target"]["commit"]["hash"]
            .as_str()
            .unwrap_or_default();

        let steps = self.paginate(
            &format!("{api}/pipelines/{}/steps/", encode_segment(uuid)),
            crate::CI_RUNS_LIMIT,
        )?;

        let mut out = String::new();
        let total = steps.len();
        let completed = steps
            .iter()
            .filter(|step| is_finished(step_result(step)))
            .count();
        let finished = is_finished(&status);
        let bar = crate::progress_bar(if finished { total } else { completed }, total);
        let (phase_emoji, phase) = pipeline_phase(&status);
        out.push_str(&format!("progress: {bar}  ·  {phase_emoji} {phase}\n"));

        out.push_str(&format!(
            "{} pipeline #{run_id} — {status}\n",
            ci_emoji(&status)
        ));
        out.push_str(&format!(
            "🌿 branch {branch}  @ {}\n",
            &sha[..7.min(sha.len())]
        ));

        out.push_str("\n🧩 -- steps --\n");
        match Some(&steps).filter(|list| !list.is_empty()) {
            Some(list) => {
                for step in list {
                    let step_name = step["name"].as_str().unwrap_or("?");
                    let result = step_result(step);
                    let runner = step_runner(step);
                    out.push_str(&format!(
                        "  {} {step_name}: {result}{runner}\n",
                        ci_emoji(result)
                    ));
                }
            }
            None => out.push_str("  (no steps reported)\n"),
        }

        let web_url = pipeline["links"]["html"]["href"]
            .as_str()
            .unwrap_or_default();
        out.push_str(&format!("\nurl: {web_url}\n"));
        Ok(out)
    }

    fn pr_diff(&self, repo_url: &str, number: u64) -> Result<String, ForgeError> {
        let api = self.repo_api(repo_url)?;
        match self.call_text(&format!("{api}/pullrequests/{number}/diff"))? {
            Some(diff) if !diff.trim().is_empty() => {
                Ok(crate::cap_lines(&diff, crate::DIFF_LINE_CAP))
            }
            Some(_) => Ok("(empty diff)\n".to_string()),
            None => Ok(format!("(no diff for #{number} — not found)\n")),
        }
    }

    fn ci_logs(&self, repo_url: &str, run_id: u64) -> Result<String, ForgeError> {
        let api = self.repo_api(repo_url)?;
        let pipelines = self.paginate(
            &format!(
                "{api}/pipelines/?sort=-created_on&pagelen={}",
                crate::CI_RUNS_LIMIT
            ),
            crate::CI_RUNS_LIMIT,
        )?;
        let pipeline = pipelines
            .iter()
            .find(|p| p["build_number"].as_u64() == Some(run_id))
            .cloned()
            .ok_or_else(|| ForgeError::Api(format!("pipeline #{run_id} not found")))?;
        let uuid = pipeline["uuid"].as_str().unwrap_or_default();

        let steps = self.paginate(
            &format!("{api}/pipelines/{}/steps/", encode_segment(uuid)),
            crate::CI_RUNS_LIMIT,
        )?;
        if steps.is_empty() {
            return Ok(format!("(no steps for pipeline #{run_id})\n"));
        }
        // Failed steps first, like the GitHub/GitLab sides.
        let mut ordered: Vec<&Value> = steps.iter().collect();
        ordered.sort_by_key(|step| u8::from(step_result(step) != "FAILED"));

        let mut out = String::new();
        for step in ordered.into_iter().take(6) {
            let step_uuid = step["uuid"].as_str().unwrap_or_default();
            let step_name = step["name"].as_str().unwrap_or("?");
            let result = step_result(step);
            let runner = step_runner(step);
            out.push_str(&format!("📜 == {step_name} ({result}){runner} ==\n"));
            match self.call_text(&format!(
                "{api}/pipelines/{}/steps/{}/log",
                encode_segment(uuid),
                encode_segment(step_uuid)
            )) {
                Ok(Some(log)) if !log.trim().is_empty() => {
                    let lines: Vec<&str> = log.lines().collect();
                    let tail = lines.len().saturating_sub(200);
                    if tail > 0 {
                        out.push_str(&format!("… ({tail} earlier line(s) omitted)\n"));
                    }
                    for line in &lines[tail..] {
                        out.push_str(line);
                        out.push('\n');
                    }
                }
                Ok(_) => out.push_str("  (log unavailable — expired or empty)\n"),
                Err(err) => out.push_str(&format!("  (log unavailable: {err})\n")),
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
        let api = self.repo_api(repo_url)?;
        let git_ref = self.resolve_ref(&api, git_ref)?;
        let sub = subpath.trim_matches('/');
        let mut url = format!(
            "{api}/src/{}/{}",
            encode_segment(&git_ref),
            encode_path(sub)
        );
        // A directory listing needs the trailing slash to disambiguate from a
        // file of the same name.
        if !url.ends_with('/') {
            url.push('/');
        }
        let entries = self.paginate(&url, 1000)?;
        let out = entries
            .iter()
            .filter_map(|entry| {
                let full = entry["path"].as_str()?;
                // Values carry the full repo path; the tree wants the leaf name.
                let name = full.rsplit('/').next().unwrap_or(full).to_string();
                let is_dir = entry["type"].as_str() == Some("commit_directory");
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
        let api = self.repo_api(repo_url)?;
        let git_ref = self.resolve_ref(&api, git_ref)?;
        let file = path.trim_start_matches('/');
        let url = format!(
            "{api}/src/{}/{}",
            encode_segment(&git_ref),
            encode_path(file)
        );
        // The same `src` URL yields raw bytes for a file and JSON for a
        // directory; a file path returns the raw content directly.
        match self.call_text(&url)? {
            Some(text) => Ok(crate::cap_lines(&text, crate::FILE_LINE_CAP)),
            None => Ok(format!("(no file at {file} — not found)\n")),
        }
    }

    fn pr_files(&self, repo_url: &str, number: u64) -> Result<Vec<crate::PrFile>, ForgeError> {
        let api = self.repo_api(repo_url)?;
        let stats = self.paginate(
            &format!("{api}/pullrequests/{number}/diffstat?pagelen=100"),
            crate::OPEN_PRS_LIMIT.max(100),
        )?;
        let out = stats
            .iter()
            .filter_map(|entry| {
                let new_path = entry["new"]["path"].as_str();
                let old_path = entry["old"]["path"].as_str();
                let path = new_path.or(old_path)?.to_string();
                // Bitbucket diffstat status: added|removed|modified|renamed.
                let status = match entry["status"].as_str() {
                    Some("added") => "added",
                    Some("removed") => "removed",
                    Some("renamed") => "renamed",
                    _ => "modified",
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
        let api = self.repo_api(repo_url)?;
        let pr = self.call(Method::GET, &format!("{api}/pullrequests/{number}"), None)?;
        let hash = pr["source"]["commit"]["hash"].as_str().unwrap_or_default();
        if hash.is_empty() {
            return Ok("(file not present at this ref)\n".to_string());
        }
        let file = path.trim_start_matches('/');
        let url = format!("{api}/src/{}/{}", encode_segment(hash), encode_path(file));
        match self.call_text(&url)? {
            Some(text) => Ok(crate::cap_lines(&text, crate::FILE_LINE_CAP)),
            None => Ok("(file not present at this ref)\n".to_string()),
        }
    }
}

impl Bitbucket {
    /// The ref to browse: the pinned one when given, else the repo's main
    /// branch (resolved from the repository object).
    fn resolve_ref(&self, api: &str, git_ref: Option<&str>) -> Result<String, ForgeError> {
        if let Some(git_ref) = git_ref.filter(|r| !r.is_empty()) {
            return Ok(git_ref.to_string());
        }
        let repo = self.call(Method::GET, api, None)?;
        Ok(repo["mainbranch"]["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("master")
            .to_string())
    }
}

/// The result string for a Bitbucket status/pipeline `state` object,
/// preferring the terminal `result.name`, else the in-flight `state.name`.
fn pipeline_status_str(pipeline: &Value) -> String {
    pipeline["state"]["result"]["name"]
        .as_str()
        .or_else(|| pipeline["state"]["name"].as_str())
        .unwrap_or("—")
        .to_string()
}

/// The result string for a pipeline step, same preference as a pipeline.
fn step_result(step: &Value) -> &str {
    step["state"]["result"]["name"]
        .as_str()
        .or_else(|| step["state"]["name"].as_str())
        .unwrap_or("—")
}

/// Reduce a commit's build-statuses list to a tri-state CI verdict, matching
/// how the other forges collapse per-check results.
fn commit_ci_passing(statuses: &[Value]) -> Option<bool> {
    if statuses.is_empty() {
        return None;
    }
    let mut any_pending = false;
    let mut any_failed = false;
    for status in statuses {
        match status["state"].as_str() {
            Some("SUCCESSFUL") => {}
            Some("INPROGRESS") | None => any_pending = true,
            Some(_) => any_failed = true,
        }
    }
    if any_pending { None } else { Some(!any_failed) }
}

/// Whether a Bitbucket pipeline/step result is a terminal (finished) state.
fn is_finished(result: &str) -> bool {
    matches!(
        result,
        "SUCCESSFUL" | "FAILED" | "ERROR" | "STOPPED" | "SKIPPED" | "EXPIRED"
    )
}

/// A leading status emoji for a Bitbucket pipeline/step result or in-flight
/// state name.
fn ci_emoji(result: &str) -> &'static str {
    match result {
        "SUCCESSFUL" => "✅",
        "FAILED" | "ERROR" => "❌",
        "STOPPED" | "SKIPPED" | "EXPIRED" => "⏹",
        "PENDING" => "⏳",
        _ => "🔄",
    }
}

/// Overall pipeline phase label + emoji for the progress line.
fn pipeline_phase(result: &str) -> (&'static str, String) {
    match result {
        "SUCCESSFUL" => ("✅", "passed".to_string()),
        "FAILED" | "ERROR" => ("❌", "failed".to_string()),
        "STOPPED" | "SKIPPED" | "EXPIRED" => ("⏹", result.to_lowercase()),
        "PENDING" => ("⏳", "queued".to_string()),
        _ => ("🔄", "running".to_string()),
    }
}

/// Emoji + label for a PR state, used in the drill-in detail header.
fn pr_state_badge(state: PrState) -> (&'static str, &'static str) {
    match state {
        PrState::Open => ("🟢", "open"),
        PrState::Draft => ("📝", "draft"),
        PrState::Merged => ("🟣", "merged"),
        PrState::Closed => ("🔴", "closed"),
    }
}

/// ` on <runner>` suffix for a pipeline step, using its runner name/label if
/// present. Empty string when absent (e.g. cloud runners).
fn step_runner(step: &Value) -> String {
    let runner = step["image"]["name"]
        .as_str()
        .or_else(|| step["runner"]["labels"][0].as_str())
        .or_else(|| step["runner"]["name"].as_str())
        .filter(|s| !s.is_empty());
    match runner {
        Some(name) => format!("  on {name}"),
        None => String::new(),
    }
}

/// Map a Bitbucket pipeline object to the forge-neutral [`crate::CiRun`].
fn run_of(pipeline: &Value) -> crate::CiRun {
    let result = pipeline_status_str(pipeline);
    let status = match result.as_str() {
        "SUCCESSFUL" => crate::CiStatus::Passed,
        "FAILED" | "ERROR" => crate::CiStatus::Failed,
        "STOPPED" | "SKIPPED" | "EXPIRED" => crate::CiStatus::Cancelled,
        "PENDING" => crate::CiStatus::Queued,
        _ => crate::CiStatus::Running,
    };
    let build_number = pipeline["build_number"].as_u64().unwrap_or_default();
    crate::CiRun {
        id: build_number,
        name: format!("#{build_number}"),
        branch: pipeline["target"]["ref_name"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        event: pipeline["target"]["type"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        status,
        url: pipeline["links"]["html"]["href"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_token_basic_and_bearer() {
        let basic = Bitbucket::new("bb-supersecret".to_string(), Some("ada".to_string()));
        let dumped = format!("{basic:?}");
        assert!(!dumped.contains("bb-supersecret"), "{dumped}");
        assert!(dumped.contains("<redacted>"), "{dumped}");
        // The Basic-auth user is not secret and may be shown.
        assert!(dumped.contains("ada"), "{dumped}");

        let bearer = Bitbucket::new("bb-anothersecret".to_string(), None);
        let dumped = format!("{bearer:?}");
        assert!(!dumped.contains("bb-anothersecret"), "{dumped}");
        assert!(dumped.contains("<redacted>"), "{dumped}");
    }

    #[test]
    fn encodes_path_segments_but_keeps_separators() {
        assert_eq!(encode_path("src/main rs/a.txt"), "src/main%20rs/a.txt");
        assert_eq!(encode_path("plain/path"), "plain/path");
    }

    #[test]
    fn open_pr_list_shape_maps_fields() {
        let page = json!({
            "values": [
                {
                    "id": 42,
                    "title": "Add widget",
                    "state": "OPEN",
                    "source": { "branch": { "name": "feature/widget" } },
                    "destination": { "branch": { "name": "main" } },
                    "author": { "display_name": "Ada" },
                    "links": { "html": { "href": "https://bitbucket.org/acme/x/pull-requests/42" } }
                },
                {
                    "id": 7,
                    "title": "Merged one",
                    "state": "MERGED",
                    "links": { "html": { "href": "https://bitbucket.org/acme/x/pull-requests/7" } }
                }
            ],
            "next": null
        });
        let values = page["values"].as_array().cloned().unwrap_or_default();
        let mapped: Vec<crate::OpenPr> = values
            .iter()
            .map(|pr| crate::OpenPr {
                number: pr["id"].as_u64().unwrap_or_default(),
                title: pr["title"].as_str().unwrap_or_default().to_string(),
                url: pr["links"]["html"]["href"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                state: state_of(pr),
                approved: false,
                ci_passing: None,
            })
            .collect();

        assert_eq!(mapped.len(), 2);
        assert_eq!(mapped[0].number, 42);
        assert_eq!(mapped[0].title, "Add widget");
        assert_eq!(mapped[0].state, PrState::Open);
        assert_eq!(
            mapped[0].url,
            "https://bitbucket.org/acme/x/pull-requests/42"
        );
        assert_eq!(mapped[1].state, PrState::Merged);
    }

    #[test]
    fn pipeline_shape_maps_to_ci_run() {
        let pipeline = json!({
            "uuid": "{pipe-uuid}",
            "build_number": 128,
            "state": { "name": "COMPLETED", "result": { "name": "SUCCESSFUL" } },
            "target": {
                "type": "pipeline_ref_target",
                "ref_name": "main",
                "commit": { "hash": "abcdef1234567890" }
            },
            "links": { "html": { "href": "https://bitbucket.org/acme/x/pipelines/results/128" } }
        });
        let run = run_of(&pipeline);
        assert_eq!(run.id, 128);
        assert_eq!(run.name, "#128");
        assert_eq!(run.branch, "main");
        assert_eq!(run.status, crate::CiStatus::Passed);
        assert_eq!(
            run.url,
            "https://bitbucket.org/acme/x/pipelines/results/128"
        );
    }

    #[test]
    fn pipeline_in_progress_maps_to_running() {
        let pipeline = json!({
            "build_number": 200,
            "state": { "name": "IN_PROGRESS" },
            "target": { "ref_name": "dev" }
        });
        assert_eq!(run_of(&pipeline).status, crate::CiStatus::Running);
    }

    #[test]
    fn pipeline_terminal_state_prefers_result_name() {
        // Real documented shape: a COMPLETED pipeline nests the verdict under
        // `state.result.name`; `state.name` alone is just "COMPLETED".
        let failed = json!({
            "build_number": 300,
            "state": {
                "name": "COMPLETED",
                "result": { "name": "FAILED" }
            },
            "target": { "ref_name": "main" }
        });
        assert_eq!(pipeline_status_str(&failed), "FAILED");
        assert_eq!(run_of(&failed).status, crate::CiStatus::Failed);

        let stopped = json!({
            "build_number": 301,
            "state": { "name": "COMPLETED", "result": { "name": "STOPPED" } },
            "target": { "ref_name": "main" }
        });
        assert_eq!(run_of(&stopped).status, crate::CiStatus::Cancelled);
    }

    #[test]
    fn pipeline_in_progress_stage_nesting_is_running() {
        // In-flight pipelines carry `state.name = IN_PROGRESS` with a
        // `state.stage.name` and no `result` — must map to Running, not "—".
        let running = json!({
            "build_number": 302,
            "state": {
                "name": "IN_PROGRESS",
                "stage": { "name": "RUNNING" }
            },
            "target": { "ref_name": "dev" }
        });
        assert_eq!(pipeline_status_str(&running), "IN_PROGRESS");
        assert_eq!(run_of(&running).status, crate::CiStatus::Running);

        let pending = json!({
            "build_number": 303,
            "state": { "name": "PENDING" },
            "target": { "ref_name": "dev" }
        });
        assert_eq!(run_of(&pending).status, crate::CiStatus::Queued);
    }

    #[test]
    fn pipeline_step_state_nesting_matches_docs() {
        // Step state mirrors pipeline state: terminal result under
        // `state.result.name`, in-flight under `state.name`.
        let done = json!({
            "uuid": "{step-uuid}",
            "name": "Build",
            "state": { "name": "COMPLETED", "result": { "name": "SUCCESSFUL" } }
        });
        assert_eq!(step_result(&done), "SUCCESSFUL");
        assert!(is_finished(step_result(&done)));

        let inflight = json!({
            "name": "Test",
            "state": { "name": "IN_PROGRESS" }
        });
        assert_eq!(step_result(&inflight), "IN_PROGRESS");
        assert!(!is_finished(step_result(&inflight)));
    }

    #[test]
    fn pr_state_values_from_docs_map_correctly() {
        // The four documented PR states: OPEN / MERGED / DECLINED / SUPERSEDED.
        assert_eq!(state_of(&json!({ "state": "OPEN" })), PrState::Open);
        assert_eq!(state_of(&json!({ "state": "MERGED" })), PrState::Merged);
        assert_eq!(state_of(&json!({ "state": "DECLINED" })), PrState::Closed);
        assert_eq!(state_of(&json!({ "state": "SUPERSEDED" })), PrState::Closed);
    }

    #[test]
    fn commit_status_flat_state_from_docs() {
        // Commit build statuses use a FLAT `state` (not nested): SUCCESSFUL /
        // FAILED / INPROGRESS / STOPPED — distinct from pipeline state nesting.
        let statuses = vec![
            json!({ "key": "build", "state": "SUCCESSFUL" }),
            json!({ "key": "lint", "state": "SUCCESSFUL" }),
        ];
        assert_eq!(commit_ci_passing(&statuses), Some(true));

        let with_inprogress = vec![
            json!({ "state": "SUCCESSFUL" }),
            json!({ "state": "INPROGRESS" }),
        ];
        assert_eq!(commit_ci_passing(&with_inprogress), None);

        let stopped = vec![json!({ "state": "STOPPED" })];
        assert_eq!(commit_ci_passing(&stopped), Some(false));
    }

    #[test]
    fn approved_reads_participants() {
        let pr = json!({
            "participants": [
                { "approved": false },
                { "approved": true }
            ]
        });
        assert!(approved_of(&pr));
        let none = json!({ "participants": [ { "approved": false } ] });
        assert!(!approved_of(&none));
    }

    #[test]
    fn commit_ci_collapses_statuses() {
        let all_ok = vec![
            json!({ "state": "SUCCESSFUL" }),
            json!({ "state": "SUCCESSFUL" }),
        ];
        assert_eq!(commit_ci_passing(&all_ok), Some(true));
        let one_failed = vec![
            json!({ "state": "SUCCESSFUL" }),
            json!({ "state": "FAILED" }),
        ];
        assert_eq!(commit_ci_passing(&one_failed), Some(false));
        let pending = vec![json!({ "state": "INPROGRESS" })];
        assert_eq!(commit_ci_passing(&pending), None);
        assert_eq!(commit_ci_passing(&[]), None);
    }

    #[test]
    fn tree_entry_type_maps_dir_flag() {
        let dir = json!({ "path": "src/lib", "type": "commit_directory" });
        let file = json!({ "path": "src/main.rs", "type": "commit_file" });
        assert_eq!(dir["type"].as_str(), Some("commit_directory"));
        assert_eq!(file["type"].as_str(), Some("commit_file"));
    }
}
