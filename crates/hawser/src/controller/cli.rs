//! CLI-backed cockpit controller: live workspace/git/forge state for the TUI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use haw_core::git::GitBackend;
use haw_core::workspace::{CloneTuning, RepoStatus, Workspace, sync_repo};
use haw_core::{change, resolver};
use haw_forge::{Tokens, orchestrate};
use haw_git::ShellGit;
use haw_git::parallel::fan_out;

use crate::commands::change::{forge_label, render_ci_status, render_pr_state};
use crate::commands::merge::merge_repo;
use crate::{
    FILE_SIZE_CAP, default_jobs, discover_plugin_panels, fleet_repos, forge_for_repo,
    git_detail_report, git_grep, locked_sha, open_workspace, render_file_bytes,
    render_plugin_panel, repo_root, run_git, safe_join, shell_command,
};

/// TUI controller: adapts cockpit actions to `haw-core`/`haw-forge`.
/// Runs on the TUI worker thread.
/// Cheap change-fingerprint of a repo's git state: the mtimes of the files a
/// commit/checkout/stage touches (`.git/HEAD`, the index, and `packed-refs`),
/// plus the locked rev. Re-stat (the 4 git subprocesses per repo) only runs
/// when this changes; an unchanged repo reuses its cached [`RepoStatus`].
#[derive(Clone, PartialEq, Eq)]
struct RepoFingerprint {
    head_mtime: Option<Duration>,
    index_mtime: Option<Duration>,
    packed_refs_mtime: Option<Duration>,
    locked_rev: Option<String>,
}

/// `path`'s modified-time as a `Duration` since the epoch, or `None` when the
/// file is absent/unreadable. Absent maps to `None` (not an error) so a repo
/// with no packed-refs still fingerprints stably.
fn file_mtime(path: &Path) -> Option<Duration> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
}

impl RepoFingerprint {
    /// Fingerprint the repo checked out at `abs` (workspace-absolute), pinned to
    /// `locked_rev`. Stats only `.git` metadata — no subprocess.
    fn of(abs: &Path, locked_rev: Option<&str>) -> Self {
        let git = abs.join(".git");
        Self {
            head_mtime: file_mtime(&git.join("HEAD")),
            index_mtime: file_mtime(&git.join("index")),
            packed_refs_mtime: file_mtime(&git.join("packed-refs")),
            locked_rev: locked_rev.map(str::to_string),
        }
    }
}

/// TTL for the fleet PR/CI caches: re-opening the view within this window
/// reuses the last fetch instead of re-hitting the forge. A manual refetch
/// (`m`/`i`) bypasses it.
const FLEET_CACHE_TTL: Duration = Duration::from_secs(45);

/// A TTL'd fleet-forge result: the fetched rows and when they were fetched.
struct FleetCacheEntry<T> {
    fetched_at: Instant,
    rows: Vec<T>,
}

impl<T> FleetCacheEntry<T> {
    fn is_fresh(&self) -> bool {
        self.fetched_at.elapsed() < FLEET_CACHE_TTL
    }
}

/// Serve a fleet-forge result through its TTL cache. When `force` is false and
/// `cache` holds a still-fresh entry, its rows are returned without calling
/// `fetch` (no forge hit). Otherwise `fetch` runs and its result is cached.
/// A failed fetch leaves any existing (stale) entry untouched.
fn cached_fleet<T: Clone>(
    cache: &mut Option<FleetCacheEntry<T>>,
    force: bool,
    fetch: impl FnOnce() -> std::io::Result<Vec<T>>,
) -> std::io::Result<Vec<T>> {
    if !force
        && let Some(entry) = cache
        && entry.is_fresh()
    {
        return Ok(entry.rows.clone());
    }
    let rows = fetch()?;
    *cache = Some(FleetCacheEntry {
        fetched_at: Instant::now(),
        rows: rows.clone(),
    });
    Ok(rows)
}

#[derive(Default)]
pub(crate) struct CliController {
    /// Skip-unchanged snapshot cache: repo checkout path -> its last
    /// fingerprint and the `RepoStatus` computed then.
    status_cache: HashMap<PathBuf, (RepoFingerprint, RepoStatus)>,
    /// TTL cache for `fleet_prs` (keyed by kind = the PR view).
    prs_cache: Option<FleetCacheEntry<haw_tui::FleetPr>>,
    /// TTL cache for `fleet_ci` (keyed by kind = the CI view).
    ci_cache: Option<FleetCacheEntry<haw_tui::FleetCiRun>>,
}

impl CliController {
    fn workspace(&self) -> std::io::Result<Workspace> {
        open_workspace().map_err(std::io::Error::other)
    }

    /// Bounded, skip-unchanged fleet re-stat. Fingerprints every repo (cheap fs
    /// stats), reuses the cached status for repos whose `.git` metadata and
    /// locked rev are unchanged, and re-stats only the changed ones — in
    /// parallel, capped at [`default_jobs`]. Returns statuses in manifest/lock
    /// order and refreshes the cache.
    fn status_cached(&mut self, ws: &Workspace) -> std::io::Result<Vec<RepoStatus>> {
        self.status_cached_with(ws, &ShellGit)
    }

    /// [`Self::status_cached`] against a caller-supplied backend so tests can
    /// inject a fake that counts how many repos actually got re-stat'd.
    fn status_cached_with(
        &mut self,
        ws: &Workspace,
        backend: &dyn GitBackend,
    ) -> std::io::Result<Vec<RepoStatus>> {
        let entries = ws.status_entries(&[]).map_err(std::io::Error::other)?;

        // Serial, cheap pass: split into cache hits (reuse) and misses (re-stat).
        let mut hits: HashMap<PathBuf, RepoStatus> = HashMap::new();
        let mut misses: Vec<(usize, haw_core::workspace::StatusEntry, RepoFingerprint)> =
            Vec::new();
        for (i, entry) in entries.iter().enumerate() {
            let abs = ws.root.join(&entry.path);
            let fp = RepoFingerprint::of(&abs, entry.locked_rev.as_deref());
            match self.status_cache.get(&entry.path) {
                Some((cached_fp, cached_status)) if *cached_fp == fp => {
                    hits.insert(entry.path.clone(), cached_status.clone());
                }
                _ => misses.push((i, entry.clone(), fp)),
            }
        }

        // Parallel, expensive pass: re-stat only the changed repos.
        let fresh = fan_out(&misses, default_jobs(None), |(_, entry, _)| {
            ws.status_entry(entry, backend)
        });

        // Refresh the cache for the misses, dropping stale/removed repos.
        for ((_, entry, fp), status) in misses.iter().zip(&fresh) {
            if let Ok(status) = status {
                self.status_cache
                    .insert(entry.path.clone(), (fp.clone(), status.clone()));
            }
        }
        let present: std::collections::HashSet<&PathBuf> =
            entries.iter().map(|e| &e.path).collect();
        self.status_cache.retain(|path, _| present.contains(path));

        // Reassemble in original order: hits from cache, misses from the fan-out.
        let mut fresh_by_index: HashMap<usize, std::io::Result<RepoStatus>> = misses
            .into_iter()
            .map(|(i, _, _)| i)
            .zip(fresh)
            .map(|(i, r)| (i, r.map_err(std::io::Error::other)))
            .collect();
        let mut out = Vec::with_capacity(entries.len());
        for (i, entry) in entries.into_iter().enumerate() {
            if let Some(status) = hits.remove(&entry.path) {
                out.push(status);
            } else if let Some(status) = fresh_by_index.remove(&i) {
                out.push(status?);
            }
        }
        Ok(out)
    }

    fn sync_filtered(&self, stack: &str, repo: Option<&str>) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let backend = ShellGit;
        let plan = ws
            .plan_sync(stack, &[], &[], None, &CloneTuning::default(), &backend)
            .map_err(std::io::Error::other)?;
        let tasks: Vec<_> = plan
            .tasks
            .into_iter()
            .filter(|t| repo.is_none_or(|r| t.name == r))
            .collect();
        let results = fan_out(&tasks, default_jobs(None), |task| {
            (task.name.clone(), sync_repo(task, &backend))
        });
        let failures: Vec<&str> = results
            .iter()
            .filter(|(_, r)| r.is_err())
            .map(|(name, _)| name.as_str())
            .collect();
        if failures.is_empty() {
            Ok(format!("synced ({} repos)", results.len()))
        } else {
            Ok(format!("sync failed for: {}", failures.join(", ")))
        }
    }

    /// Run `cmd` across every real repo, or only the given marked set.
    fn run_cmd_filtered(&self, cmd: &str, only: Option<&[String]>) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let backend = ShellGit;
        let repos: Vec<(String, PathBuf)> = ws
            .manifest
            .repos
            .iter()
            .filter(|(name, _)| only.is_none_or(|set| set.iter().any(|r| r == *name)))
            .map(|(name, repo)| (name.clone(), ws.root.join(repo.checkout_path(name))))
            .filter(|(_, path)| backend.is_repo(path))
            .collect();
        let results = fan_out(&repos, default_jobs(None), |(name, path)| {
            let output = shell_command(cmd).current_dir(path).output();
            (name.clone(), output)
        });

        let mut report = format!("$ {cmd}\n");
        let mut failures = 0usize;
        for (name, result) in &results {
            report.push_str(&format!("── {name} ──\n"));
            match result {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    if stdout.trim().is_empty() && stderr.trim().is_empty() {
                        report.push_str("(no output)\n");
                    } else {
                        report.push_str(&stdout);
                        report.push_str(&stderr);
                    }
                    if !out.status.success() {
                        failures += 1;
                        report.push_str(&format!("(exit: {})\n", out.status));
                    }
                }
                Err(err) => {
                    failures += 1;
                    report.push_str(&format!("(failed to run: {err})\n"));
                }
            }
        }
        report.push_str(&format!(
            "ran in {}/{} repos",
            results.len() - failures,
            results.len()
        ));
        Ok(report)
    }

    /// Run every cloned repo's manifest `build`/`test` command, returning a text
    /// report for the cockpit's output overlay (`:build` / `:test`).
    fn build_or_test_report(&self, build: bool) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let backend = ShellGit;
        let verb = if build { "build" } else { "test" };
        let targets: Vec<(String, PathBuf, String)> = ws
            .manifest
            .repos
            .iter()
            .filter_map(|(name, repo)| {
                let cmd = if build { &repo.build } else { &repo.test };
                cmd.as_ref().map(|cmd| {
                    (
                        name.clone(),
                        ws.root.join(repo.checkout_path(name)),
                        cmd.clone(),
                    )
                })
            })
            .filter(|(_, path, _)| backend.is_repo(path))
            .collect();
        if targets.is_empty() {
            return Ok(format!(
                "no cloned repo declares a `{verb}` command in the manifest"
            ));
        }
        let results = fan_out(&targets, default_jobs(None), |(name, path, cmd)| {
            let output = shell_command(cmd).current_dir(path).output();
            (name.clone(), output)
        });
        let mut report = format!("$ haw {verb}\n");
        let mut failures = 0usize;
        for (name, result) in &results {
            report.push_str(&format!("── {name} ──\n"));
            match result {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    if stdout.trim().is_empty() && stderr.trim().is_empty() {
                        report.push_str("(no output)\n");
                    } else {
                        report.push_str(&stdout);
                        report.push_str(&stderr);
                    }
                    if !out.status.success() {
                        failures += 1;
                        report.push_str(&format!("(exit: {})\n", out.status));
                    }
                }
                Err(err) => {
                    failures += 1;
                    report.push_str(&format!("(failed to run: {err})\n"));
                }
            }
        }
        report.push_str(&format!(
            "{verb} ran in {}/{} repos",
            results.len() - failures,
            results.len()
        ));
        Ok(report)
    }

    /// Fetch every open PR/MR across the fleet (bounded-parallel in
    /// `orchestrate`). The cache-free inner fetch behind `fleet_prs_refresh`.
    fn fetch_fleet_prs() -> std::io::Result<Vec<haw_tui::FleetPr>> {
        let ws = open_workspace().map_err(std::io::Error::other)?;
        let tokens = Tokens::from_env();
        let mut out = Vec::new();
        let mut failed = Vec::new();
        for (name, result) in orchestrate::fleet_open_prs(&ws, &tokens) {
            match result {
                Ok(prs) => {
                    let forge = forge_label(&ws, &name);
                    out.extend(prs.into_iter().map(|pr| haw_tui::FleetPr {
                        repo: name.clone(),
                        forge: forge.clone(),
                        number: pr.number,
                        title: pr.title,
                        url: pr.url,
                        state: render_pr_state(pr.state).to_string(),
                        approved: pr.approved,
                        ci: pr.ci_passing,
                    }));
                }
                Err(_) => failed.push(name),
            }
        }
        if out.is_empty() && !failed.is_empty() {
            return Err(std::io::Error::other(format!(
                "PR/MR fetch failed for: {}",
                failed.join(", ")
            )));
        }
        Ok(out)
    }

    /// Fetch recent CI runs/pipelines across the fleet (bounded-parallel in
    /// `orchestrate`). The cache-free inner fetch behind `fleet_ci_refresh`.
    fn fetch_fleet_ci() -> std::io::Result<Vec<haw_tui::FleetCiRun>> {
        let ws = open_workspace().map_err(std::io::Error::other)?;
        let tokens = Tokens::from_env();
        let mut out = Vec::new();
        let mut failed = Vec::new();
        for (name, result) in orchestrate::fleet_ci_runs(&ws, &tokens) {
            match result {
                Ok(runs) => out.extend(runs.into_iter().map(|run| haw_tui::FleetCiRun {
                    repo: name.clone(),
                    id: run.id,
                    name: run.name,
                    branch: run.branch,
                    event: run.event,
                    status: render_ci_status(run.status).to_string(),
                    url: run.url,
                })),
                Err(_) => failed.push(name),
            }
        }
        if out.is_empty() && !failed.is_empty() {
            return Err(std::io::Error::other(format!(
                "CI fetch failed for: {}",
                failed.join(", ")
            )));
        }
        Ok(out)
    }
}

fn render_changeset(
    ws: &Workspace,
    id: &str,
    prs: Option<Vec<orchestrate::RepoPrStatus>>,
) -> std::io::Result<haw_tui::ChangesetSummary> {
    let statuses = change::status(ws, &ShellGit, id).map_err(std::io::Error::other)?;
    let changeset = change::Changeset::load(ws, id).map_err(std::io::Error::other)?;
    let repos = statuses
        .into_iter()
        .map(|s| {
            let entry = changeset.repos.iter().find(|r| r.name == s.name);
            let (pr, ci) = match &prs {
                Some(list) => match list.iter().find(|(name, _)| name == &s.name) {
                    Some((_, Some(Ok(status)))) => (
                        format!(
                            "#{} ● {}",
                            entry.and_then(|e| e.pr_number).unwrap_or_default(),
                            render_pr_state(status.state)
                        ),
                        match status.ci_passing {
                            Some(true) => "✓ passed".to_string(),
                            Some(false) => "✗ failed".to_string(),
                            None => "⏳ pending".to_string(),
                        },
                    ),
                    Some((_, Some(Err(_)))) => ("(error)".to_string(), "—".to_string()),
                    _ => ("—".to_string(), "—".to_string()),
                },
                None => match entry.and_then(|e| e.pr_number) {
                    Some(number) => (format!("#{number}"), "…".to_string()),
                    None => ("—".to_string(), "—".to_string()),
                },
            };
            let forge = forge_label(ws, &s.name);
            haw_tui::ChangeRepoRow {
                name: s.name,
                branch: s.branch,
                on_branch: s.on_branch,
                dirty: s.dirty,
                head: s.head,
                forge,
                pr,
                ci,
            }
        })
        .collect();
    Ok(haw_tui::ChangesetSummary {
        id: id.to_string(),
        repos,
    })
}

fn tree_text(ws: &Workspace) -> String {
    let mut out = String::new();
    for (i, (name, _)) in ws.manifest.stacks.iter().enumerate() {
        let Ok(resolution) = resolver::resolve(&ws.manifest, name, &[]) else {
            continue;
        };
        let last_stack = i == ws.manifest.stacks.len() - 1;
        out.push_str(if last_stack { "└─ " } else { "├─ " });
        out.push_str(name);
        out.push('\n');
        let stem = if last_stack { "   " } else { "│  " };
        for (j, repo) in resolution.repos.iter().enumerate() {
            let tee = if j == resolution.repos.len() - 1 {
                "└─"
            } else {
                "├─"
            };
            out.push_str(&format!("{stem}{tee} {}  {}\n", repo.name, repo.rev));
        }
    }
    out
}

impl haw_tui::Controller for CliController {
    fn snapshot(&mut self) -> std::io::Result<haw_tui::Snapshot> {
        let ws = self.workspace()?;
        let statuses = self.status_cached(&ws)?;
        let fleet = ws
            .manifest
            .stacks
            .iter()
            .map(|(stack, spec)| {
                (
                    stack.clone(),
                    statuses
                        .iter()
                        .filter(|s| spec.repos.contains(&s.name))
                        .cloned()
                        .collect(),
                )
            })
            .collect();
        let ids = change::Changeset::list(&ws).map_err(std::io::Error::other)?;
        let mut changesets = Vec::with_capacity(ids.len());
        for id in ids {
            changesets.push(render_changeset(&ws, &id, None)?);
        }
        let paths = ws
            .manifest
            .repos
            .iter()
            .map(|(name, repo)| (name.clone(), ws.root.join(repo.checkout_path(name))))
            .collect();
        let mut merges = Vec::new();
        for name in ws.manifest.repos.keys() {
            if let Some(plan) =
                haw_merge::load_plan(&ws.state_dir(), name).map_err(std::io::Error::other)?
            {
                let resolved = plan.slices.iter().filter(|s| s.resolved).count();
                merges.push((
                    name.clone(),
                    haw_tui::MergeBadge {
                        source: plan.source,
                        resolved,
                        total: plan.slices.len(),
                    },
                ));
            }
        }
        Ok(haw_tui::Snapshot {
            root_label: ws.root.display().to_string(),
            stacks: ws.manifest.stacks.keys().cloned().collect(),
            current_stack: ws.current_stack(),
            fleet,
            changesets,
            lock_present: ws.lock_path().exists(),
            paths,
            tree: tree_text(&ws),
            merges,
        })
    }

    fn changeset_prs(&mut self, id: &str) -> std::io::Result<haw_tui::ChangesetSummary> {
        let ws = self.workspace()?;
        let changeset = change::Changeset::load(&ws, id).map_err(std::io::Error::other)?;
        let prs = if changeset.repos.iter().any(|r| r.pr_number.is_some()) {
            let tokens = Tokens::from_env();
            Some(orchestrate::statuses(&ws, &tokens, id).map_err(std::io::Error::other)?)
        } else {
            None
        };
        render_changeset(&ws, id, prs)
    }

    fn sync_stack(&mut self, stack: &str) -> std::io::Result<String> {
        self.sync_filtered(stack, None)
    }

    fn sync_repo(&mut self, repo: &str) -> std::io::Result<String> {
        // Reuse the stack-agnostic union path so `s` on a repo works even when no
        // stack has been selected (was: pick_stack(None) → "no stack selected").
        self.sync_repos(std::slice::from_ref(&repo.to_string()))
    }

    fn sync_repos(&mut self, repos: &[String]) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let backend = ShellGit;
        // The TUI operates on the whole fleet, not a pre-selected stack. Use the
        // current stack if one is set; otherwise plan across EVERY stack (union) so
        // `s` works even before `haw switch` — then filter to the requested repos.
        let stacks: Vec<String> = match ws.pick_stack(None) {
            Ok(s) => vec![s],
            Err(_) => ws.manifest.stacks.keys().cloned().collect(),
        };
        let mut tasks = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for stack in &stacks {
            let plan = ws
                .plan_sync(stack, &[], &[], None, &CloneTuning::default(), &backend)
                .map_err(std::io::Error::other)?;
            for task in plan.tasks {
                if repos.iter().any(|r| r == &task.name) && seen.insert(task.name.clone()) {
                    tasks.push(task);
                }
            }
        }
        let results = fan_out(&tasks, default_jobs(None), |task| {
            (task.name.clone(), sync_repo(task, &backend))
        });
        let failures: Vec<&str> = results
            .iter()
            .filter(|(_, r)| r.is_err())
            .map(|(name, _)| name.as_str())
            .collect();
        if failures.is_empty() {
            Ok(format!("synced {} repo(s)", results.len()))
        } else {
            Ok(format!("sync failed for: {}", failures.join(", ")))
        }
    }

    fn switch(&mut self, stack: &str) -> std::io::Result<String> {
        let ws = self.workspace()?;
        ws.set_current_stack(stack).map_err(std::io::Error::other)?;
        let summary = self.sync_filtered(stack, None)?;
        Ok(format!("switched to `{stack}` — {summary}"))
    }

    fn pin(&mut self) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let lockfile = ws.pin(&ShellGit).map_err(std::io::Error::other)?;
        lockfile
            .save(&ws.lock_path())
            .map_err(std::io::Error::other)?;
        Ok(format!("pinned haw.lock ({} repos)", lockfile.repos.len()))
    }

    fn lock(&mut self) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let lockfile = ws
            .make_lock(&[], &ShellGit)
            .map_err(std::io::Error::other)?;
        lockfile
            .save(&ws.lock_path())
            .map_err(std::io::Error::other)?;
        Ok(format!("wrote haw.lock ({} repos)", lockfile.repos.len()))
    }

    fn run_cmd(&mut self, cmd: &str) -> std::io::Result<String> {
        self.run_cmd_filtered(cmd, None)
    }

    fn run_cmd_in(&mut self, cmd: &str, repos: &[String]) -> std::io::Result<String> {
        self.run_cmd_filtered(cmd, Some(repos))
    }

    fn build(&mut self) -> std::io::Result<String> {
        self.build_or_test_report(true)
    }

    fn test(&mut self) -> std::io::Result<String> {
        self.build_or_test_report(false)
    }

    fn verify(&mut self) -> std::io::Result<String> {
        let ws = self.workspace()?;
        if !ws.lock_path().exists() {
            return Ok("no haw.lock to verify against — run :lock first".to_string());
        }
        let statuses = ws.status(&[], &ShellGit).map_err(std::io::Error::other)?;
        let offenders: Vec<&RepoStatus> = statuses
            .iter()
            .filter(|s| s.missing || s.dirty || s.drift)
            .collect();
        let mut report = String::from("$ haw verify\n");
        if offenders.is_empty() {
            report.push_str(&format!(
                "✓ verified: tree matches haw.lock ({} repos)\n",
                statuses.len()
            ));
        } else {
            for s in &offenders {
                let why = if s.missing {
                    "not cloned"
                } else if s.dirty {
                    "dirty"
                } else {
                    "drift (head != lock)"
                };
                report.push_str(&format!("✗ {}  {why}\n", s.name));
            }
            report.push_str(&format!(
                "verify failed: {} repo(s) diverge from haw.lock\n",
                offenders.len()
            ));
        }
        Ok(report)
    }

    fn grep(
        &mut self,
        pattern: &str,
        stack: Option<&str>,
    ) -> std::io::Result<Vec<haw_tui::GrepHit>> {
        let ws = self.workspace()?;
        let repos = fleet_repos(&ws, stack)?;
        let results = fan_out(&repos, default_jobs(None), |(name, path)| {
            (name.clone(), git_grep(path, pattern))
        });
        let mut hits = Vec::new();
        for (name, out) in results {
            for line in out.lines() {
                if let Some(hit) = haw_tui::parse_grep_line(&name, line) {
                    hits.push(hit);
                }
            }
        }
        Ok(hits)
    }

    fn repo_fetch(&mut self, repo: &str) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let path = repo_root(&ws, repo)?;
        run_git(&path, &["fetch", "--all", "--prune"])?;
        Ok(format!("fetched {repo}"))
    }

    fn exec_in(&mut self, repo: &str, cmd: &str) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let path = repo_root(&ws, repo)?;
        let output = shell_command(cmd).current_dir(&path).output()?;
        let mut report = format!("$ {cmd}\n@ {}\n\n", path.display());
        report.push_str(&String::from_utf8_lossy(&output.stdout));
        report.push_str(&String::from_utf8_lossy(&output.stderr));
        if !output.status.success() {
            report.push_str(&format!("\n(exit: {})\n", output.status));
        }
        Ok(report)
    }

    fn change_start(&mut self, id: &str) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let changeset = change::start(&ws, &ShellGit, id, None, None, false, &[])
            .map_err(std::io::Error::other)?;
        Ok(format!(
            "changeset `{id}` started across {} repos",
            changeset.repos.len()
        ))
    }

    fn change_request(&mut self, id: &str, only: Option<Vec<String>>) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let tokens = Tokens::from_env();
        let outcomes = orchestrate::request(&ws, &ShellGit, &tokens, id, None, only.as_deref())
            .map_err(std::io::Error::other)?;
        let failed = outcomes.iter().filter(|o| o.result.is_err()).count();
        Ok(match failed {
            0 => format!("requested `{id}` ({} PR/MRs)", outcomes.len()),
            n => format!("requested `{id}` — {n} repo(s) failed"),
        })
    }

    fn change_land(&mut self, id: &str) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let tokens = Tokens::from_env();
        let outcomes = orchestrate::land(&ws, &tokens, id).map_err(std::io::Error::other)?;
        match outcomes.iter().find(|o| o.result.is_err()) {
            Some(outcome) => Ok(format!("landing stopped at `{}`", outcome.name)),
            None => Ok(format!("landed `{id}` ({} repos)", outcomes.len())),
        }
    }

    fn pr_merge(&mut self, repo: &str, number: u64) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let (forge, url) = forge_for_repo(&ws, repo)?;
        forge
            .merge_pr(&url, number)
            .map_err(std::io::Error::other)?;
        Ok(format!("merged {repo}#{number}"))
    }

    fn pr_approve(&mut self, repo: &str, number: u64) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let (forge, url) = forge_for_repo(&ws, repo)?;
        forge
            .approve_pr(&url, number)
            .map_err(std::io::Error::other)?;
        Ok(format!("approved {repo}#{number}"))
    }

    fn pr_checkout(&mut self, repo: &str, number: u64) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let spec = ws.manifest.repos.get(repo).ok_or_else(|| {
            std::io::Error::other(format!("repo `{repo}` is not in the manifest"))
        })?;
        let path = ws.root.join(spec.checkout_path(repo));
        if !ShellGit.is_repo(&path) {
            return Err(std::io::Error::other(format!(
                "repo `{repo}` is not cloned at {}; run `haw sync` first",
                path.display()
            )));
        }
        // Pick the forge-specific pull ref: GitHub exposes `pull/N/head`,
        // GitLab exposes `merge-requests/N/head`.
        let pull_ref = match forge_label(&ws, repo).as_str() {
            "gitlab" => format!("merge-requests/{number}/head"),
            _ => format!("pull/{number}/head"),
        };
        let branch = format!("haw-pr-{number}");
        run_git(&path, &["fetch", "origin", &format!("{pull_ref}:{branch}")])?;
        run_git(&path, &["checkout", &branch])?;
        Ok(format!("checked out {repo} PR #{number} → {branch}"))
    }

    fn merge_cleanup(&mut self, repo: &str) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let (name, path) = merge_repo(&ws, Some(repo)).map_err(std::io::Error::other)?;
        let report = haw_merge::cleanup(
            &haw_merge::git::GitMerge,
            &path,
            &ws.state_dir(),
            &name,
            None,
        )
        .map_err(std::io::Error::other)?;
        Ok(format!(
            "merged {} slice(s) into `{}` ({}); dropped `{}`",
            report.slices,
            report.target,
            &report.merge_sha[..8.min(report.merge_sha.len())],
            report.integration
        ))
    }

    fn fleet_prs(&mut self) -> std::io::Result<Vec<haw_tui::FleetPr>> {
        self.fleet_prs_refresh(false)
    }

    fn fleet_prs_refresh(&mut self, force: bool) -> std::io::Result<Vec<haw_tui::FleetPr>> {
        cached_fleet(&mut self.prs_cache, force, Self::fetch_fleet_prs)
    }

    fn fleet_ci(&mut self) -> std::io::Result<Vec<haw_tui::FleetCiRun>> {
        self.fleet_ci_refresh(false)
    }

    fn fleet_ci_refresh(&mut self, force: bool) -> std::io::Result<Vec<haw_tui::FleetCiRun>> {
        cached_fleet(&mut self.ci_cache, force, Self::fetch_fleet_ci)
    }

    fn governance(&mut self) -> std::io::Result<haw_tui::Governance> {
        let ws = self.workspace()?;
        let root = &ws.root;
        let plugins: Vec<haw_tui::GovPlugin> = ws
            .manifest
            .plugins
            .iter()
            .map(|(name, phases)| haw_tui::GovPlugin {
                name: name.clone(),
                phases: phases.clone(),
            })
            .collect();

        let mut artifacts = Vec::new();
        let state_dir = ws.state_dir();
        for (kind, sub) in [("sbom", "sbom"), ("provenance", "provenance")] {
            let dir = state_dir.join(sub);
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();
                artifacts.push(haw_tui::GovArtifact {
                    plugin: String::new(),
                    kind: kind.to_string(),
                    path: rel,
                    exists: true,
                });
            }
        }

        Ok(haw_tui::Governance {
            plugins,
            artifacts,
            findings: Vec::new(),
        })
    }

    fn plugin_panels(&mut self) -> std::io::Result<Vec<haw_tui::PluginPanel>> {
        let ws = self.workspace()?;
        Ok(discover_plugin_panels(ws.manifest.plugins.iter()))
    }

    fn plugin_render(&mut self, name: &str) -> std::io::Result<String> {
        let ws = self.workspace()?;
        render_plugin_panel(&ws, name)
    }

    fn merge_abort(&mut self, repo: &str) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let (name, path) = merge_repo(&ws, Some(repo)).map_err(std::io::Error::other)?;
        let plan = haw_merge::abort(&haw_merge::git::GitMerge, &path, &ws.state_dir(), &name)
            .map_err(std::io::Error::other)?;
        Ok(format!(
            "aborted merge of `{}`; back on `{}`",
            plan.source, plan.target
        ))
    }

    fn repo_detail(&mut self, repo: &str) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let spec = ws.manifest.repos.get(repo).ok_or_else(|| {
            std::io::Error::other(format!("repo `{repo}` is not in the manifest"))
        })?;
        let path = ws.root.join(spec.checkout_path(repo));
        Ok(git_detail_report(repo, &path))
    }

    fn pr_detail(&mut self, repo: &str, number: u64) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let (forge, url) = forge_for_repo(&ws, repo)?;
        forge.pr_detail(&url, number).map_err(std::io::Error::other)
    }

    fn ci_detail(&mut self, repo: &str, run_id: u64) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let (forge, url) = forge_for_repo(&ws, repo)?;
        forge
            .ci_run_detail(&url, run_id)
            .map_err(std::io::Error::other)
    }

    fn pr_diff(&mut self, repo: &str, number: u64) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let (forge, url) = forge_for_repo(&ws, repo)?;
        forge.pr_diff(&url, number).map_err(std::io::Error::other)
    }

    fn pr_files(&mut self, repo: &str, number: u64) -> std::io::Result<Vec<haw_tui::PrFileEntry>> {
        let ws = self.workspace()?;
        let (forge, url) = forge_for_repo(&ws, repo)?;
        let files = forge
            .pr_files(&url, number)
            .map_err(std::io::Error::other)?;
        Ok(files
            .into_iter()
            .map(|f| haw_tui::PrFileEntry {
                path: f.path,
                status: f.status,
            })
            .collect())
    }

    fn pr_file_content(&mut self, repo: &str, number: u64, path: &str) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let (forge, url) = forge_for_repo(&ws, repo)?;
        forge
            .pr_file_content(&url, number, path)
            .map_err(std::io::Error::other)
    }

    fn ci_logs(&mut self, repo: &str, run_id: u64) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let (forge, url) = forge_for_repo(&ws, repo)?;
        forge.ci_logs(&url, run_id).map_err(std::io::Error::other)
    }

    fn repo_tree(
        &mut self,
        repo: &str,
        subpath: &str,
        remote: bool,
        git_ref: Option<&str>,
    ) -> std::io::Result<Vec<haw_tui::FileEntry>> {
        let ws = self.workspace()?;
        if remote {
            let (forge, url) = forge_for_repo(&ws, repo)?;
            // An explicit picked ref wins; else fall back to the locked SHA
            // (today's behavior).
            let fallback = locked_sha(&ws, repo);
            let effective = git_ref.map(str::to_string).or(fallback);
            let entries = forge
                .repo_tree(&url, subpath, effective.as_deref())
                .map_err(std::io::Error::other)?;
            Ok(entries
                .into_iter()
                .map(|e| haw_tui::FileEntry {
                    name: e.name,
                    is_dir: e.is_dir,
                })
                .collect())
        } else if let Some(git_ref) = git_ref {
            // Local, AS OF a picked ref: read the tree via git ls-tree.
            let root = repo_root(&ws, repo)?;
            let entries =
                haw_git::ls_tree(&root, git_ref, subpath).map_err(std::io::Error::other)?;
            Ok(entries
                .into_iter()
                .filter(|(name, _)| name != ".git")
                .map(|(name, is_dir)| haw_tui::FileEntry { name, is_dir })
                .collect())
        } else {
            // Local, current checkout: read the working directory.
            let root = repo_root(&ws, repo)?;
            let dir = safe_join(&root, subpath)?;
            let mut out = Vec::new();
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if name == ".git" {
                    continue;
                }
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                out.push(haw_tui::FileEntry { name, is_dir });
            }
            Ok(out)
        }
    }

    fn file_content(
        &mut self,
        repo: &str,
        path: &str,
        remote: bool,
        git_ref: Option<&str>,
    ) -> std::io::Result<String> {
        let ws = self.workspace()?;
        if remote {
            let (forge, url) = forge_for_repo(&ws, repo)?;
            let fallback = locked_sha(&ws, repo);
            let effective = git_ref.map(str::to_string).or(fallback);
            return forge
                .file_blob(&url, path, effective.as_deref())
                .map_err(std::io::Error::other);
        }
        if let Some(git_ref) = git_ref {
            // Local, AS OF a picked ref: `git show <ref>:<path>`.
            let root = repo_root(&ws, repo)?;
            let text = haw_git::show_file(&root, git_ref, path).map_err(std::io::Error::other)?;
            return Ok(haw_forge::cap_lines(&text, 600));
        }
        let root = repo_root(&ws, repo)?;
        let file = safe_join(&root, path)?;
        let meta = std::fs::metadata(&file)?;
        if meta.len() > FILE_SIZE_CAP {
            return Ok(format!(
                "<file too large: {} bytes (cap {FILE_SIZE_CAP})>\n",
                meta.len()
            ));
        }
        let bytes = std::fs::read(&file)?;
        Ok(render_file_bytes(&bytes))
    }

    fn repo_file_paths(
        &mut self,
        repo: &str,
        remote: bool,
        git_ref: Option<&str>,
    ) -> std::io::Result<Vec<String>> {
        let ws = self.workspace()?;
        if remote {
            let (forge, url) = forge_for_repo(&ws, repo)?;
            let fallback = locked_sha(&ws, repo);
            let effective = git_ref.map(str::to_string).or(fallback);
            return forge
                .repo_file_paths(&url, effective.as_deref())
                .map_err(std::io::Error::other);
        }
        let root = repo_root(&ws, repo)?;
        match git_ref {
            Some(git_ref) => {
                haw_git::ls_tree_recursive(&root, git_ref).map_err(std::io::Error::other)
            }
            None => Ok(walk_working_dir(&root)),
        }
    }

    fn list_refs(&mut self, repo: &str, remote: bool) -> std::io::Result<Vec<haw_tui::RefEntry>> {
        let ws = self.workspace()?;
        if remote {
            let (forge, url) = forge_for_repo(&ws, repo)?;
            let refs = forge.list_refs(&url).map_err(std::io::Error::other)?;
            return Ok(refs
                .into_iter()
                .map(|r| haw_tui::RefEntry {
                    name: r.name,
                    kind: match r.kind {
                        haw_forge::ForgeRefKind::Branch => haw_tui::RefKind::Branch,
                        haw_forge::ForgeRefKind::Tag => haw_tui::RefKind::Tag,
                    },
                })
                .collect());
        }
        let root = repo_root(&ws, repo)?;
        let refs = haw_git::list_refs(&root).map_err(std::io::Error::other)?;
        Ok(refs
            .into_iter()
            .map(|r| haw_tui::RefEntry {
                name: r.name,
                kind: match r.kind {
                    haw_git::LocalRefKind::Head => haw_tui::RefKind::Head,
                    haw_git::LocalRefKind::Branch => haw_tui::RefKind::Branch,
                    haw_git::LocalRefKind::Tag => haw_tui::RefKind::Tag,
                },
            })
            .collect())
    }
}

/// Recursively collect every FILE path under `root` (posix `/`-separated,
/// relative to `root`), skipping `.git`, bounded at [`haw_git::LS_TREE_CAP`].
/// The working-dir counterpart of `git ls-tree -r` for the "current checkout"
/// (no picked ref) tree view.
fn walk_working_dir(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name == ".git" {
                continue;
            }
            let path = entry.path();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
                if out.len() >= haw_git::LS_TREE_CAP {
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod scaling_tests {
    use super::*;
    use haw_core::git::{CloneOpts, GitError, ResolvedRev, RevKind};
    use std::collections::HashSet;
    use std::path::Path;
    use std::sync::Mutex;

    /// A `GitBackend` that records which repos got re-stat'd (via `head_sha`,
    /// the first call `status_entry` makes on a present repo). Everything is a
    /// clean, on-lock repo — we only care about the re-stat count.
    #[derive(Default)]
    struct CountingGit {
        restatted: Mutex<Vec<PathBuf>>,
    }

    impl GitBackend for CountingGit {
        fn resolve_rev(&self, _url: &str, _rev: &str) -> Result<ResolvedRev, GitError> {
            Ok(ResolvedRev {
                sha: "sha".into(),
                kind: RevKind::Branch,
            })
        }
        fn clone_repo(&self, _u: &str, _d: &Path, _o: &CloneOpts) -> Result<(), GitError> {
            Ok(())
        }
        fn ensure_mirror(&self, _u: &str, _m: &Path) -> Result<(), GitError> {
            Ok(())
        }
        fn fetch(&self, _repo: &Path) -> Result<(), GitError> {
            Ok(())
        }
        fn checkout(&self, _r: &Path, _s: &str, _b: &str, _d: Option<u32>) -> Result<(), GitError> {
            Ok(())
        }
        fn update_submodules(&self, _repo: &Path) -> Result<(), GitError> {
            Ok(())
        }
        fn create_branch(&self, _repo: &Path, _name: &str) -> Result<(), GitError> {
            Ok(())
        }
        fn push_branch(&self, _repo: &Path, _branch: &str) -> Result<(), GitError> {
            Ok(())
        }
        fn head_sha(&self, repo: &Path) -> Result<String, GitError> {
            self.restatted.lock().unwrap().push(repo.to_path_buf());
            // Match the locked rev so nothing drifts (keeps the test focused).
            Ok("feedface".into())
        }
        fn ahead_behind(&self, _repo: &Path) -> Result<Option<(u64, u64)>, GitError> {
            Ok(None)
        }
        fn current_branch(&self, _repo: &Path) -> Result<Option<String>, GitError> {
            Ok(Some("main".into()))
        }
        fn is_dirty(&self, _repo: &Path) -> Result<bool, GitError> {
            Ok(false)
        }
        fn is_repo(&self, _repo: &Path) -> bool {
            true
        }
    }

    /// A workspace of two on-disk repos, each with a `.git/HEAD` we can touch to
    /// change its fingerprint. Returns the workspace and its two checkout paths.
    fn two_repo_workspace() -> (tempfile::TempDir, Workspace, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("haw.toml"),
            "[repo.a]\nurl = \"/r/a\"\nrev = \"main\"\n\n\
             [repo.b]\nurl = \"/r/b\"\nrev = \"main\"\n\n\
             [stack.s]\nrepos = [\"a\", \"b\"]\n",
        )
        .unwrap();
        // Pin both to `feedface` so status finds them on-lock (no drift noise).
        let locked = |name: &str| haw_core::lock::LockedRepo {
            name: name.to_string(),
            url: format!("/r/{name}"),
            path: name.into(),
            rev: "feedface".to_string(),
            source_rev: "main".to_string(),
            branch: "main".to_string(),
            groups: vec![],
        };
        haw_core::lock::Lockfile {
            version: haw_core::lock::LOCK_VERSION,
            repos: vec![locked("a"), locked("b")],
        }
        .save(&dir.path().join("haw.lock"))
        .unwrap();
        for name in ["a", "b"] {
            let git = dir.path().join(name).join(".git");
            std::fs::create_dir_all(&git).unwrap();
            std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        }
        let ws = Workspace::open(dir.path()).unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        (dir, ws, a, b)
    }

    #[test]
    fn snapshot_skips_unchanged_repos_and_restats_changed_ones() {
        let (_dir, ws, _a, b) = two_repo_workspace();
        let backend = CountingGit::default();
        let mut controller = CliController::default();

        // First snapshot: cold cache — both repos are re-stat'd.
        let first = controller.status_cached_with(&ws, &backend).unwrap();
        assert_eq!(first.len(), 2);
        {
            let restatted: HashSet<_> = backend.restatted.lock().unwrap().iter().cloned().collect();
            assert_eq!(restatted.len(), 2, "cold cache re-stats every repo");
        }
        backend.restatted.lock().unwrap().clear();

        // Second snapshot with nothing touched: warm cache — zero re-stats.
        let second = controller.status_cached_with(&ws, &backend).unwrap();
        assert_eq!(second.len(), 2);
        assert!(
            backend.restatted.lock().unwrap().is_empty(),
            "an unchanged fleet re-stats nothing"
        );

        // Change one repo's HEAD mtime; only that repo re-stats.
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(b.join(".git").join("HEAD"), "ref: refs/heads/dev\n").unwrap();
        let third = controller.status_cached_with(&ws, &backend).unwrap();
        assert_eq!(third.len(), 2);
        let restatted = backend.restatted.lock().unwrap().clone();
        assert_eq!(restatted, vec![b], "only the changed repo is re-stat'd");
    }

    #[test]
    fn fleet_ttl_cache_skips_the_forge_until_forced() {
        let calls = std::cell::Cell::new(0usize);
        let mut cache: Option<FleetCacheEntry<u8>> = None;
        let fetch = || {
            calls.set(calls.get() + 1);
            Ok(vec![1u8, 2, 3])
        };

        // First call populates the cache (one fetch).
        assert_eq!(
            cached_fleet(&mut cache, false, fetch).unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(calls.get(), 1);

        // Second call within the TTL reuses the cache — no forge hit.
        assert_eq!(
            cached_fleet(&mut cache, false, fetch).unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(calls.get(), 1, "a fresh cache must not re-hit the forge");

        // A forced refetch (the `m`/`i` key) bypasses the cache.
        assert_eq!(
            cached_fleet(&mut cache, true, fetch).unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(calls.get(), 2, "force must bypass the cache");
    }

    #[test]
    fn fleet_ttl_cache_expires_after_the_ttl() {
        // A stale entry (fetched long ago) triggers a fresh fetch.
        let mut cache = Some(FleetCacheEntry {
            fetched_at: Instant::now() - FLEET_CACHE_TTL - Duration::from_secs(1),
            rows: vec![9u8],
        });
        let calls = std::cell::Cell::new(0usize);
        let rows = cached_fleet(&mut cache, false, || {
            calls.set(calls.get() + 1);
            Ok(vec![7u8])
        })
        .unwrap();
        assert_eq!(rows, vec![7]);
        assert_eq!(calls.get(), 1, "an expired entry re-fetches");
    }
}
