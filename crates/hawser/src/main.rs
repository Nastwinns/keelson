use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use keel_core::git::GitBackend;
use keel_core::manifest::{ManifestLoader, TomlLoader, edit, import};
use keel_core::workspace::{MANIFEST_FILE, RepoStatus, SyncOutcome, Workspace, sync_repo};
use keel_core::{audit, change, hooks, resolver, snapshot};
use keel_forge::{PrState, Tokens, orchestrate};
use keel_git::ShellGit;
use keel_git::parallel::fan_out;
use serde_json::json;

/// Minimal ANSI painter: colored on a TTY, plain under `NO_COLOR` or when
/// piped; `CLICOLOR_FORCE=1` forces color even when piped (bat/eza convention).
/// Semantic helpers keep every command on one shared scheme:
/// cyan names, yellow revs, dim chrome, green/yellow/red state.
struct Palette {
    on: bool,
}

impl Palette {
    fn new() -> Self {
        let force = std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| v != "0");
        let on =
            std::env::var_os("NO_COLOR").is_none() && (force || std::io::stdout().is_terminal());
        Self { on }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.on {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    /// Repo/stack names: bold cyan.
    fn name(&self, text: &str) -> String {
        self.paint("1;36", text)
    }

    /// Revisions, tags, branches: yellow.
    fn rev(&self, text: &str) -> String {
        self.paint("33", text)
    }

    /// SHAs, paths, secondary chrome: dim.
    fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }

    /// Success marks and clean state: green.
    fn ok(&self, text: &str) -> String {
        self.paint("32", text)
    }

    /// Warnings (dirty): bold yellow.
    fn warn(&self, text: &str) -> String {
        self.paint("1;33", text)
    }

    /// Failures and drift: bold red.
    fn err(&self, text: &str) -> String {
        self.paint("1;31", text)
    }

    /// Table headers: bold + underline.
    fn header(&self, text: &str) -> String {
        self.paint("1;4", text)
    }

    /// Summary lines: bold.
    fn bold(&self, text: &str) -> String {
        self.paint("1", text)
    }
}

#[derive(Parser)]
#[command(name = "haw", version, about = "The beam that binds the repos")]
struct Cli {
    /// Path to the manifest.
    #[arg(long, global = true, default_value = "keel.toml")]
    manifest: PathBuf,

    /// No subcommand opens the TUI cockpit (same as `haw dash`).
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Bootstrap a workspace from a manifest file or URL.
    Init {
        /// Path or http(s) URL of an existing keel.toml.
        source: String,
    },
    /// Clone/update repos to the state in keel.lock (writes it if absent).
    Sync {
        /// CI contract: fail unless keel.lock exists (no rev resolution).
        #[arg(long)]
        locked: bool,
        #[arg(long = "stack", alias = "product")]
        stack: Option<String>,
        /// Overlays only apply when the lock is generated.
        #[arg(long)]
        overlay: Vec<String>,
        /// Only repos in these groups (repeatable).
        #[arg(long = "group")]
        groups: Vec<String>,
        /// Share objects with a local mirror cache (git alternates, no symlinks).
        #[arg(long)]
        shared: bool,
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
    },
    /// Resolve every repo's rev to a SHA and (re)write keel.lock.
    Lock {
        #[arg(long)]
        overlay: Vec<String>,
    },
    /// Pin keel.lock to each repo's current HEAD (no network).
    #[command(alias = "freeze")]
    Pin,
    /// Restore keel.lock to the manifest revs (same as `haw lock`).
    #[command(alias = "unfreeze")]
    Unpin {
        #[arg(long)]
        overlay: Vec<String>,
    },
    /// Add, remove, or list the repos of the manifest.
    #[command(alias = "brick")]
    Repo {
        #[command(subcommand)]
        command: RepoCommand,
    },
    /// Add, remove, or list the stacks of the manifest.
    #[command(alias = "product")]
    Stack {
        #[command(subcommand)]
        command: StackCommand,
    },
    /// Aggregated fleet status: branch, head, dirty, drift per repo.
    #[command(alias = "st")]
    Status {
        /// Only repos in these groups (repeatable).
        #[arg(long = "group")]
        groups: Vec<String>,
        /// `text` (default) or `json` (schema keel.status/1).
        #[arg(long, default_value = "text")]
        format: String,
        /// Exit 3 when any repo is missing, dirty, or drifted (CI gate).
        #[arg(long)]
        verify: bool,
    },
    /// Record a stack as current and sync it.
    Switch {
        stack: String,
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
    },
    /// Print the stack -> repo tree.
    #[command(alias = "graph")]
    Tree {
        #[arg(long = "stack", alias = "product")]
        stack: Option<String>,
        #[arg(long)]
        overlay: Vec<String>,
        /// `text` (default) or `json` (schema keel.tree/1).
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Run a command in every repo, in parallel.
    #[command(alias = "forall")]
    Run {
        /// The command (positional; `-c` also works, repo-tool style).
        #[arg(required_unless_present = "command_flag")]
        command: Option<String>,
        #[arg(short = 'c', long = "command", conflicts_with = "command")]
        command_flag: Option<String>,
        /// Only repos in these groups (repeatable).
        #[arg(long = "group")]
        groups: Vec<String>,
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
    },
    /// Cross-repo feature (changeset) workflow.
    Change {
        #[command(subcommand)]
        command: ChangeCommand,
    },
    /// Assert the on-disk tree matches keel.lock; exit 3 on drift (CI gate).
    Verify {
        /// `text` (default) or `json` (schema keel.status/1).
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Run each repo's `build` command from the manifest, in parallel.
    Build {
        /// Only repos in these groups (repeatable).
        #[arg(long = "group")]
        groups: Vec<String>,
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
    },
    /// Run each repo's `test` command from the manifest, in parallel.
    Test {
        /// Only repos in these groups (repeatable).
        #[arg(long = "group")]
        groups: Vec<String>,
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
    },
    /// Manage lifecycle hooks (.keel/hooks) and git integrity hooks.
    Hooks {
        #[command(subcommand)]
        command: HooksCommand,
    },
    /// Bundle baseline evidence (manifest, lock, audit log, status) for audits.
    Evidence {
        /// Output archive path.
        #[arg(long, default_value = "haw-evidence.tar.gz")]
        out: PathBuf,
    },
    /// Convert a west.yml or repo default.xml manifest to keel.toml.
    Import {
        /// Path to the foreign manifest.
        #[arg(long)]
        from: PathBuf,
    },
    /// Parallel collaborative merge: slice one big merge into reviewable units.
    Merge {
        #[command(subcommand)]
        command: MergeCommand,
    },
    /// Open the fleet dashboard (same as bare `haw`).
    #[command(alias = "tui")]
    Dash,
    /// Anything else runs a `haw-<name>` plugin from PATH.
    #[command(external_subcommand)]
    Plugin(Vec<String>),
}

#[derive(Subcommand)]
enum HooksCommand {
    /// Write a pre-commit hook in every repo that runs `haw verify`.
    Install,
    /// List the lifecycle hooks the workspace defines.
    List,
}

#[derive(Subcommand)]
enum RepoCommand {
    /// List repos with rev, path, and groups.
    List,
    /// Add a repo to the manifest (keeps your comments and formatting).
    Add {
        name: String,
        /// Full clone URL (or use --remote + --slug).
        #[arg(long, conflicts_with_all = ["remote", "slug"])]
        url: Option<String>,
        /// Named remote from [remote.X].
        #[arg(long, requires = "slug")]
        remote: Option<String>,
        /// Repository path under the remote.
        #[arg(long, alias = "repo", requires = "remote")]
        slug: Option<String>,
        #[arg(long, default_value = "main")]
        rev: String,
        /// Checkout path (default: the repo name).
        #[arg(long)]
        path: Option<String>,
        /// Group label (repeatable).
        #[arg(long = "group")]
        groups: Vec<String>,
    },
    /// Remove a repo (refused while a stack or overlay references it).
    Remove { name: String },
}

#[derive(Subcommand)]
enum StackCommand {
    /// List stacks and their repos.
    List,
    /// Add a stack composed of existing repos.
    Add {
        name: String,
        /// Repos in the stack.
        #[arg(
            long = "repos",
            alias = "bricks",
            value_delimiter = ',',
            required = true
        )]
        repos: Vec<String>,
    },
    /// Remove a stack.
    Remove { name: String },
}

#[derive(Subcommand)]
enum ChangeCommand {
    /// Create one branch across the affected repos.
    Start {
        id: String,
        /// Repos to include (default: all repos in the manifest).
        #[arg(long = "repos", alias = "bricks", value_delimiter = ',')]
        repos: Option<Vec<String>>,
        /// Branch name (default: change/<id>).
        #[arg(long)]
        branch: Option<String>,
        /// Adopt each repo's current branch instead of creating one.
        #[arg(long)]
        skip_branch: bool,
        /// Label forwarded to the PR/MRs at `change request` (repeatable).
        #[arg(long = "label")]
        labels: Vec<String>,
    },
    /// Per-repo branch + PR/MR review + CI dashboard for a changeset.
    Status { id: String },
    /// Push the changeset branches and open cross-linked PR/MRs.
    Request {
        id: String,
        /// Target branch for the PR/MRs (default: the locked branch, else main).
        #[arg(long)]
        base: Option<String>,
    },
    /// Merge the PR/MRs in dependency order; stops at the first failure.
    Land { id: String },
    /// Print a changeset repo's path (usable as: cd "$(haw change goto ID REPO)").
    Goto {
        id: String,
        /// Repo name; omit for an interactive picker.
        repo: Option<String>,
    },
    /// Save/restore the multi-repo state of a changeset.
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
    /// List recorded changesets.
    List,
}

#[derive(Subcommand)]
enum MergeCommand {
    /// Start merging <source> into the current branch; slice the conflicts.
    Plan {
        /// Branch/tag/SHA to merge in.
        source: String,
        /// Repo to merge in (default: the only repo, else required).
        #[arg(long)]
        repo: Option<String>,
        /// Integration branch name (default: keel/merge/<source>).
        #[arg(long)]
        into: Option<String>,
    },
    /// Resolve one slice of the in-progress merge.
    Resolve {
        slice: String,
        #[arg(long)]
        repo: Option<String>,
        /// Auto-resolve the whole slice to `ours` or `theirs` (else stage as edited).
        #[arg(long)]
        take: Option<TakeSide>,
    },
    /// Show the planned slices and their resolution state.
    Status {
        #[arg(long)]
        repo: Option<String>,
    },
    /// Seal the merge: commit it, fast-forward the target, drop temp branches.
    Cleanup {
        #[arg(long)]
        repo: Option<String>,
        /// Merge commit message (default: git's merge message).
        #[arg(long, short = 'm')]
        message: Option<String>,
    },
    /// Abort the planned merge and restore the target branch.
    Abort {
        #[arg(long)]
        repo: Option<String>,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum TakeSide {
    Ours,
    Theirs,
}

#[derive(Subcommand)]
enum SnapshotCommand {
    /// Record every repo's branch + HEAD under a name.
    Save { name: String },
    /// Check every repo back out to a saved state (refuses on dirty repos).
    Restore { name: String },
    /// List saved snapshots.
    List,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let Some(command) = cli.command else {
        dash()?;
        return Ok(ExitCode::SUCCESS);
    };
    match command {
        Command::Init { source } => init(&source)?,
        Command::Sync {
            locked,
            stack,
            overlay,
            groups,
            shared,
            jobs,
        } => sync(stack.as_deref(), &overlay, &groups, shared, locked, jobs)?,
        Command::Lock { overlay } => lock(&overlay)?,
        Command::Pin => pin()?,
        Command::Unpin { overlay } => unpin(&overlay)?,
        Command::Repo { command } => match command {
            RepoCommand::List => repo_list()?,
            RepoCommand::Add {
                name,
                url,
                remote,
                slug,
                rev,
                path,
                groups,
            } => repo_add(&name, url, remote, slug, rev, path, groups)?,
            RepoCommand::Remove { name } => repo_remove(&name)?,
        },
        Command::Stack { command } => match command {
            StackCommand::List => stack_list()?,
            StackCommand::Add { name, repos } => stack_add(&name, &repos)?,
            StackCommand::Remove { name } => stack_remove(&name)?,
        },
        Command::Status {
            groups,
            format,
            verify,
        } => return status(&groups, &format, verify),
        Command::Switch { stack, jobs } => switch(&stack, jobs)?,
        Command::Tree {
            stack,
            overlay,
            format,
        } => tree(&cli.manifest, stack.as_deref(), &overlay, &format)?,
        Command::Run {
            command,
            command_flag,
            groups,
            jobs,
        } => {
            let cmd = command
                .or(command_flag)
                .context("pass the command: haw run 'git fetch'")?;
            run_across(&cmd, &groups, jobs)?;
        }
        Command::Change { command } => match command {
            ChangeCommand::Start {
                id,
                repos,
                branch,
                skip_branch,
                labels,
            } => change_start(
                &id,
                repos.as_deref(),
                branch.as_deref(),
                skip_branch,
                &labels,
            )?,
            ChangeCommand::Status { id } => change_status(&id)?,
            ChangeCommand::Request { id, base } => change_request(&id, base.as_deref())?,
            ChangeCommand::Land { id } => change_land(&id)?,
            ChangeCommand::Goto { id, repo } => change_goto(&id, repo.as_deref())?,
            ChangeCommand::Snapshot { command } => match command {
                SnapshotCommand::Save { name } => snapshot_save(&name)?,
                SnapshotCommand::Restore { name } => snapshot_restore(&name)?,
                SnapshotCommand::List => snapshot_list()?,
            },
            ChangeCommand::List => change_list()?,
        },
        Command::Verify { format } => return verify(&format),
        Command::Build { groups, jobs } => build_or_test(true, &groups, jobs)?,
        Command::Test { groups, jobs } => build_or_test(false, &groups, jobs)?,
        Command::Hooks { command } => match command {
            HooksCommand::Install => hooks_install()?,
            HooksCommand::List => hooks_list()?,
        },
        Command::Evidence { out } => evidence(&out)?,
        Command::Import { from } => import_manifest(&from)?,
        Command::Merge { command } => match command {
            MergeCommand::Plan { source, repo, into } => {
                merge_plan(&source, repo.as_deref(), into.as_deref())?
            }
            MergeCommand::Resolve { slice, repo, take } => {
                merge_resolve(&slice, repo.as_deref(), take)?
            }
            MergeCommand::Status { repo } => merge_status(repo.as_deref())?,
            MergeCommand::Cleanup { repo, message } => {
                merge_cleanup(repo.as_deref(), message.as_deref())?
            }
            MergeCommand::Abort { repo } => merge_abort(repo.as_deref())?,
        },
        Command::Dash => dash()?,
        Command::Plugin(args) => return plugin(&args),
    }
    Ok(ExitCode::SUCCESS)
}

fn open_workspace() -> Result<Workspace> {
    let cwd = std::env::current_dir()?;
    Ok(Workspace::open(cwd)?)
}

fn default_jobs(flag: Option<usize>) -> usize {
    flag.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(4)
            .min(8)
    })
}

fn record(ws: &Workspace, op: &str, repo: Option<&str>, before: Option<&str>, after: Option<&str>) {
    if let Err(err) = audit::record(ws, op, repo, before, after) {
        eprintln!("warning: audit log not written: {err}");
    }
}

fn init(source: &str) -> Result<()> {
    let dest = PathBuf::from(MANIFEST_FILE);
    if dest.exists() {
        bail!("{MANIFEST_FILE} already exists here");
    }
    let text = if source.starts_with("http://") || source.starts_with("https://") {
        reqwest::blocking::get(source)
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::text)
            .with_context(|| format!("fetching {source}"))?
    } else {
        let path = Path::new(source);
        if !path.is_file() {
            bail!("{source} is not a file or URL");
        }
        std::fs::read_to_string(path)?
    };
    text.parse::<keel_core::manifest::Manifest>()
        .with_context(|| format!("{source} is not a valid manifest"))?;
    std::fs::write(&dest, text)?;
    println!("initialized workspace from {source}");
    println!("next: haw sync");
    Ok(())
}

fn sync(
    stack: Option<&str>,
    overlays: &[String],
    groups: &[String],
    shared: bool,
    locked: bool,
    jobs: Option<usize>,
) -> Result<()> {
    let ws = open_workspace()?;
    let stack = ws.pick_stack(stack)?;
    if locked && !ws.lock_path().exists() {
        bail!("--locked: no keel.lock — commit one (haw lock) before running CI syncs");
    }
    hooks::fire(&ws, hooks::Hook::PreSync, &json!({"stack": stack}))?;
    let backend = ShellGit;
    let cache_root = if shared {
        let root = keel_git::default_cache_root().context("no cache directory on this platform")?;
        println!("sharing objects via {}", root.display());
        Some(root)
    } else {
        None
    };
    let plan = ws.plan_sync(&stack, overlays, groups, cache_root.as_deref(), &backend)?;
    if plan.wrote_lock {
        println!("wrote keel.lock ({} repos pinned)", plan.tasks.len());
        record(&ws, "lock.write", None, None, None);
    } else if !overlays.is_empty() {
        println!("note: keel.lock exists — overlays ignored (run `haw lock` to re-resolve)");
    }

    let results = fan_out(&plan.tasks, default_jobs(jobs), |task| {
        sync_repo(task, &backend)
    });

    let c = Palette::new();
    let width = plan.tasks.iter().map(|t| t.name.len()).max().unwrap_or(4);
    let mut failures = 0usize;
    for (task, result) in plan.tasks.iter().zip(&results) {
        match result {
            Ok(outcome) => {
                let verb = match outcome {
                    SyncOutcome::Cloned => "cloned",
                    SyncOutcome::Updated => "updated",
                    SyncOutcome::AlreadySynced => "up to date",
                };
                println!(
                    "  {} {}  {}",
                    c.ok("✓"),
                    c.name(&format!("{:<width$}", task.name)),
                    c.dim(verb)
                );
                if *outcome != SyncOutcome::AlreadySynced {
                    record(&ws, "sync", Some(&task.name), None, Some(&task.target));
                }
            }
            Err(err) => {
                failures += 1;
                eprintln!("  {} {}  {err}", c.err("✗"), task.name);
            }
        }
    }
    println!(
        "{}",
        c.bold(&format!(
            "synced stack `{}` ({}/{} repos)",
            plan.stack,
            results.len() - failures,
            results.len()
        ))
    );
    if failures > 0 {
        bail!("{failures} repo(s) failed to sync");
    }
    hooks::fire(&ws, hooks::Hook::PostSync, &json!({"stack": plan.stack}))?;
    Ok(())
}

fn lock(overlays: &[String]) -> Result<()> {
    let ws = open_workspace()?;
    let backend = ShellGit;
    hooks::fire(&ws, hooks::Hook::PreLock, &json!({"overlays": overlays}))?;
    let lockfile = ws.make_lock(overlays, &backend)?;
    lockfile.save(&ws.lock_path())?;
    hooks::fire(&ws, hooks::Hook::PostLock, &json!({"overlays": overlays}))?;
    record(&ws, "lock.write", None, None, None);
    let c = Palette::new();
    println!(
        "{}",
        c.bold(&format!(
            "wrote keel.lock ({} repos pinned)",
            lockfile.repos.len()
        ))
    );
    let width = lockfile
        .repos
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(4);
    for repo in &lockfile.repos {
        println!(
            "  {}  {}  {} {}",
            c.name(&format!("{:<width$}", repo.name)),
            c.dim(&repo.rev[..12.min(repo.rev.len())]),
            c.dim("<-"),
            c.rev(&repo.source_rev)
        );
    }
    Ok(())
}

fn pin() -> Result<()> {
    let ws = open_workspace()?;
    let lockfile = ws.pin(&ShellGit)?;
    lockfile.save(&ws.lock_path())?;
    record(&ws, "lock.pin", None, None, None);
    let c = Palette::new();
    println!(
        "{}",
        c.bold(&format!(
            "pinned keel.lock to current HEADs ({} repos)",
            lockfile.repos.len()
        ))
    );
    let width = lockfile
        .repos
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(4);
    for repo in &lockfile.repos {
        println!(
            "  {}  {}  {}",
            c.name(&format!("{:<width$}", repo.name)),
            c.dim(&repo.rev[..8.min(repo.rev.len())]),
            c.rev(&format!("({})", repo.branch))
        );
    }
    Ok(())
}

fn unpin(overlays: &[String]) -> Result<()> {
    lock(overlays)?;
    println!("restored keel.lock to the manifest revs");
    Ok(())
}

fn repo_list() -> Result<()> {
    let ws = open_workspace()?;
    if ws.manifest.repos.is_empty() {
        println!("no repos — add one with `haw repo add <name> --url <url>`");
        return Ok(());
    }
    let c = Palette::new();
    let width = ws.manifest.repos.keys().map(String::len).max().unwrap_or(4);
    for (name, repo) in &ws.manifest.repos {
        let groups = if repo.groups.is_empty() {
            String::new()
        } else {
            format!("  [{}]", repo.groups.join(", "))
        };
        println!(
            "{}  {}  {}{}",
            c.name(&format!("{name:<width$}")),
            c.rev(&repo.rev),
            c.dim(&repo.checkout_path(name).display().to_string()),
            c.dim(&groups)
        );
    }
    Ok(())
}

fn repo_add(
    name: &str,
    url: Option<String>,
    remote: Option<String>,
    slug: Option<String>,
    rev: String,
    path: Option<String>,
    groups: Vec<String>,
) -> Result<()> {
    let ws = open_workspace()?;
    let spec = edit::NewRepo {
        name: name.to_string(),
        url,
        remote,
        repo: slug,
        rev,
        path,
        groups,
    };
    let text = std::fs::read_to_string(ws.manifest_path())?;
    let updated = edit::add_repo(&text, &spec)?;
    std::fs::write(ws.manifest_path(), updated)?;
    record(&ws, "repo.add", Some(name), None, None);
    println!("added repo `{name}`");
    println!("next: haw lock && haw sync");
    Ok(())
}

fn repo_remove(name: &str) -> Result<()> {
    let ws = open_workspace()?;
    let text = std::fs::read_to_string(ws.manifest_path())?;
    let updated = edit::remove_repo(&text, name)?;
    std::fs::write(ws.manifest_path(), updated)?;
    record(&ws, "repo.remove", Some(name), None, None);
    println!("removed repo `{name}` from the manifest");
    println!("note: its clone stays on disk; delete the directory if unwanted");
    Ok(())
}

fn stack_list() -> Result<()> {
    let ws = open_workspace()?;
    if ws.manifest.stacks.is_empty() {
        println!("no stacks — add one with `haw stack add <name> --repos a,b`");
        return Ok(());
    }
    let c = Palette::new();
    let current = ws.current_stack();
    for (name, stack) in &ws.manifest.stacks {
        let marker = if current.as_deref() == Some(name) {
            c.ok("*")
        } else {
            " ".to_string()
        };
        println!(
            "{marker} {}: {}",
            c.name(name),
            c.rev(&stack.repos.join(", "))
        );
    }
    Ok(())
}

fn stack_add(name: &str, repos: &[String]) -> Result<()> {
    let ws = open_workspace()?;
    let text = std::fs::read_to_string(ws.manifest_path())?;
    let updated = edit::add_stack(&text, name, repos)?;
    std::fs::write(ws.manifest_path(), updated)?;
    record(&ws, "stack.add", Some(name), None, None);
    println!("added stack `{name}` ({} repos)", repos.len());
    Ok(())
}

fn stack_remove(name: &str) -> Result<()> {
    let ws = open_workspace()?;
    let text = std::fs::read_to_string(ws.manifest_path())?;
    let updated = edit::remove_stack(&text, name)?;
    std::fs::write(ws.manifest_path(), updated)?;
    record(&ws, "stack.remove", Some(name), None, None);
    println!("removed stack `{name}`");
    Ok(())
}

fn status_json(statuses: &[RepoStatus]) -> serde_json::Value {
    json!({
        "schema": "keel.status/1",
        "repos": statuses.iter().map(|s| json!({
            "name": s.name,
            "path": s.path.to_string_lossy(),
            "missing": s.missing,
            "branch": s.branch,
            "head": s.head,
            "dirty": s.dirty,
            "locked_rev": s.locked_rev,
            "drift": s.drift,
            "ahead_behind": s.ahead_behind.map(|(a, b)| json!({"ahead": a, "behind": b})),
        })).collect::<Vec<_>>(),
    })
}

fn status(groups: &[String], format: &str, verify: bool) -> Result<ExitCode> {
    let ws = open_workspace()?;
    let statuses = ws.status(groups, &ShellGit)?;
    let failing = statuses.iter().any(|s| s.missing || s.dirty || s.drift);

    match format {
        "json" => println!("{}", serde_json::to_string_pretty(&status_json(&statuses))?),
        "text" => {
            if statuses.is_empty() {
                println!("no matching repos");
            } else {
                let c = Palette::new();
                let width = statuses
                    .iter()
                    .map(|s| s.name.len())
                    .max()
                    .unwrap_or(4)
                    .max(4);
                println!(
                    "{}",
                    c.header(&format!(
                        "{:<width$}  {:<24} {:<10} {:<6} DRIFT",
                        "REPO", "BRANCH", "HEAD", "DIRTY"
                    ))
                );
                for s in &statuses {
                    if s.missing {
                        println!(
                            "{}  {}",
                            c.name(&format!("{:<width$}", s.name)),
                            c.dim("(not cloned — run `haw sync`)")
                        );
                        continue;
                    }
                    let name = if s.dirty || s.drift {
                        c.warn(&format!("{:<width$}", s.name))
                    } else {
                        c.name(&format!("{:<width$}", s.name))
                    };
                    println!(
                        "{name}  {}  {} {} {}",
                        c.rev(&format!(
                            "{:<24}",
                            s.branch.as_deref().unwrap_or("(detached)")
                        )),
                        c.dim(&format!(
                            "{:<10}",
                            s.head
                                .as_deref()
                                .map(|h| &h[..8.min(h.len())])
                                .unwrap_or("—")
                        )),
                        if s.dirty {
                            c.warn(&format!("{:<6}", "yes"))
                        } else {
                            c.ok(&format!("{:<6}", "-"))
                        },
                        if s.drift { c.err("YES") } else { c.ok("-") },
                    );
                }
            }
        }
        other => bail!("unknown format `{other}` (use text or json)"),
    }
    if verify && failing {
        return Ok(ExitCode::from(3));
    }
    Ok(ExitCode::SUCCESS)
}

fn switch(stack: &str, jobs: Option<usize>) -> Result<()> {
    let ws = open_workspace()?;
    let stack = ws.pick_stack(Some(stack))?;
    ws.set_current_stack(&stack)?;
    record(&ws, "switch", None, None, Some(&stack));
    hooks::fire(&ws, hooks::Hook::PostSwitch, &json!({"stack": stack}))?;
    println!("switched to stack `{stack}`");
    sync(Some(&stack), &[], &[], false, false, jobs)
}

fn tree(path: &Path, stack: Option<&str>, overlays: &[String], format: &str) -> Result<()> {
    let manifest = TomlLoader.load(path)?;
    let selected: Vec<String> = match stack {
        Some(name) => vec![name.to_string()],
        None => manifest.stacks.keys().cloned().collect(),
    };
    if selected.is_empty() {
        println!("no stacks defined in {}", path.display());
        return Ok(());
    }

    if format == "json" {
        let mut stacks = Vec::with_capacity(selected.len());
        for name in &selected {
            let resolution = resolver::resolve(&manifest, name, overlays)?;
            stacks.push(json!({
                "name": name,
                "repos": resolution.repos.iter().map(|r| json!({
                    "name": r.name,
                    "rev": r.rev,
                    "url": r.url,
                    "path": r.path.to_string_lossy(),
                })).collect::<Vec<_>>(),
            }));
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"schema": "keel.tree/1", "stacks": stacks}))?
        );
        return Ok(());
    }
    if format != "text" {
        bail!("unknown format `{format}` (use text or json)");
    }

    let c = Palette::new();
    println!("{}", c.paint("2", &path.display().to_string()));
    for (i, name) in selected.iter().enumerate() {
        let resolution = resolver::resolve(&manifest, name, overlays)?;
        let last_stack = i == selected.len() - 1;
        let branch = if last_stack { "└─" } else { "├─" };
        println!("{} {}", c.paint("2", branch), c.paint("1;36", name));

        let stem = if last_stack { "   " } else { "│  " };
        let width = resolution
            .repos
            .iter()
            .map(|b| b.name.len())
            .max()
            .unwrap_or(0);
        for (j, repo) in resolution.repos.iter().enumerate() {
            let tee = if j == resolution.repos.len() - 1 {
                "└─"
            } else {
                "├─"
            };
            println!(
                "{}{} {}  {}  {}",
                c.paint("2", stem),
                c.paint("2", tee),
                format_args!("{:<width$}", repo.name),
                c.paint("33", &repo.rev),
                c.paint("2", &format!("({})", repo.url)),
            );
        }
    }
    Ok(())
}

fn run_across(command: &str, groups: &[String], jobs: Option<usize>) -> Result<()> {
    let ws = open_workspace()?;
    let backend = ShellGit;
    let repos: Vec<(String, PathBuf)> = match ws.read_lock()? {
        Some(lock) => lock
            .repos
            .iter()
            .filter(|b| resolver::group_match(&b.groups, groups))
            .map(|b| (b.name.clone(), ws.root.join(&b.path)))
            .collect(),
        None => ws
            .manifest
            .repos
            .iter()
            .filter(|(_, repo)| resolver::group_match(&repo.groups, groups))
            .map(|(name, repo)| (name.clone(), ws.root.join(repo.checkout_path(name))))
            .collect(),
    };
    let present: Vec<(String, PathBuf)> = repos
        .into_iter()
        .filter(|(_, path)| backend.is_repo(path))
        .collect();
    if present.is_empty() {
        bail!("no cloned repos — run `haw sync` first");
    }

    let results = fan_out(&present, default_jobs(jobs), |(name, path)| {
        let output = shell_command(command).current_dir(path).output();
        (name.clone(), output)
    });

    let total = results.len();
    let mut failures = 0usize;
    let c = Palette::new();
    for (name, output) in results {
        println!("{} {} {}", c.dim("──"), c.name(&name), c.dim("──"));
        match output {
            Ok(out) => {
                print!("{}", String::from_utf8_lossy(&out.stdout));
                eprint!("{}", String::from_utf8_lossy(&out.stderr));
                if !out.status.success() {
                    failures += 1;
                    eprintln!("(exit: {})", out.status);
                }
            }
            Err(err) => {
                failures += 1;
                eprintln!("(failed to run: {err})");
            }
        }
    }
    println!("ran in {}/{} repos", total - failures, total);
    if failures > 0 {
        bail!("command failed in {failures} repo(s)");
    }
    Ok(())
}

#[cfg(windows)]
fn shell_command(command: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("cmd");
    cmd.arg("/C").arg(command);
    cmd
}

#[cfg(not(windows))]
fn shell_command(command: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(command);
    cmd
}

fn change_start(
    id: &str,
    repos: Option<&[String]>,
    branch: Option<&str>,
    skip_branch: bool,
    labels: &[String],
) -> Result<()> {
    let ws = open_workspace()?;
    let changeset = change::start(&ws, &ShellGit, id, repos, branch, skip_branch, labels)?;
    record(&ws, "change.start", None, None, Some(id));
    hooks::fire(&ws, hooks::Hook::PostChangeStart, &json!({"id": id}))?;
    let c = Palette::new();
    println!(
        "{}",
        c.bold(&format!(
            "changeset `{}` started across {} repo(s):",
            changeset.id,
            changeset.repos.len()
        ))
    );
    let width = changeset
        .repos
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(4);
    for repo in &changeset.repos {
        println!(
            "  {}  {} {}",
            c.name(&format!("{:<width$}", repo.name)),
            c.dim("->"),
            c.rev(&repo.branch)
        );
    }
    Ok(())
}

fn render_pr_state(state: PrState) -> &'static str {
    match state {
        PrState::Open => "open",
        PrState::Draft => "draft",
        PrState::Merged => "merged",
        PrState::Closed => "closed",
    }
}

fn change_status(id: &str) -> Result<()> {
    let ws = open_workspace()?;
    let statuses = change::status(&ws, &ShellGit, id)?;
    let c = Palette::new();
    let width = statuses.iter().map(|s| s.name.len()).max().unwrap_or(4);
    println!("{}", c.bold(&format!("changeset `{id}`")));
    println!(
        "{}",
        c.header(&format!(
            "{:<width$}  {:<24} {:<9} {:<6} {:<10} PR",
            "REPO", "BRANCH", "ON IT", "DIRTY", "HEAD"
        ))
    );
    for s in &statuses {
        if s.missing {
            println!(
                "{}  {}",
                c.name(&format!("{:<width$}", s.name)),
                c.dim("(repo missing — run `haw sync`)")
            );
            continue;
        }
        println!(
            "{}  {}  {} {} {} —",
            c.name(&format!("{:<width$}", s.name)),
            c.rev(&format!("{:<24}", s.branch)),
            if s.on_branch {
                c.ok(&format!("{:<9}", "yes"))
            } else {
                c.err(&format!("{:<9}", "NO"))
            },
            if s.dirty {
                c.warn(&format!("{:<6}", "yes"))
            } else {
                c.ok(&format!("{:<6}", "-"))
            },
            c.dim(&format!(
                "{:<10}",
                s.head
                    .as_deref()
                    .map(|h| &h[..8.min(h.len())])
                    .unwrap_or("—")
            )),
        );
    }

    let changeset = change::Changeset::load(&ws, id)?;
    if changeset.repos.iter().any(|r| r.pr_number.is_some()) {
        println!();
        println!("PR/MRs:");
        let tokens = Tokens::from_env();
        for (name, status) in orchestrate::statuses(&ws, &tokens, id)? {
            match status {
                None => println!("  {name}  (no PR — run `haw change request`)"),
                Some(Ok(s)) => println!(
                    "  {name}  {}  approved: {}  ci: {}  {}",
                    render_pr_state(s.state),
                    if s.approved { "yes" } else { "no" },
                    match s.ci_passing {
                        Some(true) => "passing",
                        Some(false) => "FAILING",
                        None => "pending",
                    },
                    s.url
                ),
                Some(Err(err)) => println!("  {name}  (status unavailable: {err})"),
            }
        }
    } else {
        println!("(no PR/MRs yet — open them with `haw change request {id}`)");
    }
    Ok(())
}

fn change_request(id: &str, base: Option<&str>) -> Result<()> {
    let ws = open_workspace()?;
    let tokens = Tokens::from_env();
    let outcomes = orchestrate::request(&ws, &ShellGit, &tokens, id, base, None)?;
    let c = Palette::new();
    let mut failures = 0usize;
    for outcome in &outcomes {
        match &outcome.result {
            Ok(url) => {
                record(&ws, "change.request", Some(&outcome.name), None, Some(url));
                println!("  {} {}  {}", c.ok("✓"), c.name(&outcome.name), c.dim(url));
            }
            Err(err) => {
                failures += 1;
                eprintln!("  {} {}  {err}", c.err("✗"), outcome.name);
            }
        }
    }
    if failures > 0 {
        bail!("{failures} repo(s) failed; fix and re-run `haw change request {id}`");
    }
    println!(
        "requested changeset `{id}` ({} PR/MRs, cross-linked)",
        outcomes.len()
    );
    Ok(())
}

fn change_land(id: &str) -> Result<()> {
    let ws = open_workspace()?;
    let tokens = Tokens::from_env();
    let outcomes = orchestrate::land(&ws, &tokens, id)?;
    let c = Palette::new();
    let mut failed = false;
    for outcome in &outcomes {
        match &outcome.result {
            Ok(msg) => {
                record(&ws, "change.land", Some(&outcome.name), None, Some(id));
                println!("  {} {}  {}", c.ok("✓"), c.name(&outcome.name), c.dim(msg));
            }
            Err(err) => {
                failed = true;
                eprintln!("  {} {}  {err}", c.err("✗"), outcome.name);
            }
        }
    }
    if failed {
        bail!("landing stopped at the first failure; later repos stay unmerged");
    }
    println!("changeset `{id}` landed ({} repos)", outcomes.len());
    Ok(())
}

fn change_goto(id: &str, repo: Option<&str>) -> Result<()> {
    let ws = open_workspace()?;
    let changeset = change::Changeset::load(&ws, id)?;
    let path_of = |name: &str| -> Result<PathBuf> {
        let spec = ws
            .manifest
            .repos
            .get(name)
            .with_context(|| format!("repo `{name}` is not in the manifest"))?;
        Ok(ws.root.join(spec.checkout_path(name)))
    };

    let name = match repo {
        Some(name) => {
            if !changeset.repos.iter().any(|r| r.name == name) {
                bail!("repo `{name}` is not part of changeset `{id}`");
            }
            name.to_string()
        }
        None if std::io::stdin().is_terminal() => {
            for (index, entry) in changeset.repos.iter().enumerate() {
                eprintln!("  {}. {}  ({})", index + 1, entry.name, entry.branch);
            }
            eprint!("repo number: ");
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            let choice: usize = line.trim().parse().context("not a number")?;
            changeset
                .repos
                .get(choice.saturating_sub(1))
                .map(|entry| entry.name.clone())
                .context("choice out of range")?
        }
        None => {
            let names: Vec<&str> = changeset.repos.iter().map(|r| r.name.as_str()).collect();
            bail!(
                "pass a repo name (one of: {}) — interactive picker needs a terminal",
                names.join(", ")
            );
        }
    };
    println!("{}", path_of(&name)?.display());
    Ok(())
}

/// Resolve which repo the merge acts on and its absolute checkout path.
/// Defaults to the sole repo when the manifest has exactly one.
fn merge_repo(ws: &Workspace, repo: Option<&str>) -> Result<(String, PathBuf)> {
    let name = match repo {
        Some(name) => name.to_string(),
        None => {
            let mut names = ws.manifest.repos.keys();
            match (names.next(), names.next()) {
                (Some(only), None) => only.clone(),
                _ => bail!(
                    "pass --repo (manifest has {} repos)",
                    ws.manifest.repos.len()
                ),
            }
        }
    };
    let spec = ws
        .manifest
        .repos
        .get(&name)
        .with_context(|| format!("repo `{name}` is not in the manifest"))?;
    let path = ws.root.join(spec.checkout_path(&name));
    if !ShellGit.is_repo(&path) {
        bail!(
            "repo `{name}` is not cloned at {}; run `haw sync`",
            path.display()
        );
    }
    Ok((name, path))
}

fn merge_plan(source: &str, repo: Option<&str>, into: Option<&str>) -> Result<()> {
    let ws = open_workspace()?;
    let (name, path) = merge_repo(&ws, repo)?;
    let plan = keel_merge::plan(
        &keel_merge::git::GitMerge,
        &path,
        &ws.state_dir(),
        &name,
        source,
        into,
    )?;
    record(&ws, "merge.plan", Some(&name), None, Some(source));
    let c = Palette::new();
    println!(
        "{}",
        c.bold(&format!(
            "planned merge of `{}` into `{}` on `{}` ({} slice(s)):",
            plan.source,
            plan.target,
            plan.integration,
            plan.slices.len()
        ))
    );
    for slice in &plan.slices {
        println!(
            "  {} {}",
            c.name(&format!("{:<16}", slice.name)),
            c.dim(&format!("{} file(s)", slice.paths.len()))
        );
    }
    println!(
        "{}",
        c.dim("next: haw merge resolve <slice> [--take ours|theirs], then haw merge cleanup")
    );
    Ok(())
}

fn merge_resolve(slice: &str, repo: Option<&str>, take: Option<TakeSide>) -> Result<()> {
    let ws = open_workspace()?;
    let (name, path) = merge_repo(&ws, repo)?;
    let side = take.map(|t| match t {
        TakeSide::Ours => keel_merge::Side::Ours,
        TakeSide::Theirs => keel_merge::Side::Theirs,
    });
    let plan = keel_merge::resolve(
        &keel_merge::git::GitMerge,
        &path,
        &ws.state_dir(),
        &name,
        slice,
        side,
    )?;
    record(&ws, "merge.resolve", Some(&name), None, Some(slice));
    let c = Palette::new();
    let remaining = plan.unresolved();
    println!("{} resolved slice `{}`", c.ok("✓"), c.name(slice));
    if remaining.is_empty() {
        println!("{}", c.ok("all slices resolved — run `haw merge cleanup`"));
    } else {
        println!("remaining: {}", c.warn(&remaining.join(", ")));
    }
    Ok(())
}

fn merge_status(repo: Option<&str>) -> Result<()> {
    let ws = open_workspace()?;
    let (name, _) = merge_repo(&ws, repo)?;
    let Some(plan) = keel_merge::load_plan(&ws.state_dir(), &name)? else {
        println!("no merge planned for `{name}` — start one with `haw merge plan <source>`");
        return Ok(());
    };
    let c = Palette::new();
    println!(
        "{}",
        c.bold(&format!(
            "merge `{}` -> `{}` on `{}`",
            plan.source, plan.target, plan.integration
        ))
    );
    for slice in &plan.slices {
        let mark = if slice.resolved {
            c.ok("✓")
        } else {
            c.dim("·")
        };
        println!(
            "  {mark} {} {}",
            c.name(&format!("{:<16}", slice.name)),
            c.dim(&format!("{} file(s)", slice.paths.len()))
        );
    }
    Ok(())
}

fn merge_cleanup(repo: Option<&str>, message: Option<&str>) -> Result<()> {
    let ws = open_workspace()?;
    let (name, path) = merge_repo(&ws, repo)?;
    let report = keel_merge::cleanup(
        &keel_merge::git::GitMerge,
        &path,
        &ws.state_dir(),
        &name,
        message,
    )?;
    record(
        &ws,
        "merge.cleanup",
        Some(&name),
        None,
        Some(&report.merge_sha),
    );
    let c = Palette::new();
    println!(
        "{} {}",
        c.ok("✓"),
        c.bold(&format!(
            "merged {} slice(s) into `{}` ({}); dropped `{}`",
            report.slices,
            report.target,
            &report.merge_sha[..8.min(report.merge_sha.len())],
            report.integration
        ))
    );
    Ok(())
}

fn merge_abort(repo: Option<&str>) -> Result<()> {
    let ws = open_workspace()?;
    let (name, path) = merge_repo(&ws, repo)?;
    let plan = keel_merge::abort(&keel_merge::git::GitMerge, &path, &ws.state_dir(), &name)?;
    record(&ws, "merge.abort", Some(&name), None, Some(&plan.source));
    println!(
        "aborted merge of `{}`; back on `{}`",
        plan.source, plan.target
    );
    Ok(())
}

fn snapshot_save(name: &str) -> Result<()> {
    let ws = open_workspace()?;
    let snap = snapshot::save(&ws, &ShellGit, name)?;
    record(&ws, "snapshot.save", None, None, Some(name));
    println!("saved snapshot `{name}` ({} repos)", snap.repos.len());
    for repo in &snap.repos {
        println!(
            "  {}  {}  ({})",
            repo.name,
            &repo.sha[..8.min(repo.sha.len())],
            repo.branch.as_deref().unwrap_or("detached")
        );
    }
    Ok(())
}

fn snapshot_restore(name: &str) -> Result<()> {
    let ws = open_workspace()?;
    let snap = snapshot::restore(&ws, &ShellGit, name)?;
    record(&ws, "snapshot.restore", None, None, Some(name));
    println!("restored snapshot `{name}` ({} repos)", snap.repos.len());
    Ok(())
}

fn snapshot_list() -> Result<()> {
    let ws = open_workspace()?;
    let names = snapshot::Snapshot::list(&ws)?;
    if names.is_empty() {
        println!("no snapshots — save one with `haw change snapshot save <name>`");
        return Ok(());
    }
    for name in names {
        println!("{name}");
    }
    Ok(())
}

fn change_list() -> Result<()> {
    let ws = open_workspace()?;
    let ids = change::Changeset::list(&ws)?;
    if ids.is_empty() {
        println!("no changesets — start one with `haw change start <id>`");
        return Ok(());
    }
    for id in ids {
        println!("{id}");
    }
    Ok(())
}

fn verify(format: &str) -> Result<ExitCode> {
    let ws = open_workspace()?;
    if !ws.lock_path().exists() {
        bail!("no keel.lock to verify against — run `haw lock` first");
    }
    let statuses = ws.status(&[], &ShellGit)?;
    let offenders: Vec<&RepoStatus> = statuses
        .iter()
        .filter(|s| s.missing || s.dirty || s.drift)
        .collect();

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&status_json(&statuses))?);
    } else {
        for s in &offenders {
            let why = if s.missing {
                "not cloned"
            } else if s.dirty {
                "dirty"
            } else {
                "drift (head != lock)"
            };
            println!("  ✗ {}  {why}", s.name);
        }
    }
    if offenders.is_empty() {
        if format != "json" {
            println!(
                "verified: tree matches keel.lock ({} repos)",
                statuses.len()
            );
        }
        Ok(ExitCode::SUCCESS)
    } else {
        if format != "json" {
            eprintln!(
                "verify failed: {} repo(s) diverge from keel.lock",
                offenders.len()
            );
        }
        Ok(ExitCode::from(3))
    }
}

fn build_or_test(build: bool, groups: &[String], jobs: Option<usize>) -> Result<()> {
    let ws = open_workspace()?;
    let backend = ShellGit;
    let verb = if build { "build" } else { "test" };
    let targets: Vec<(String, PathBuf, String)> = ws
        .manifest
        .repos
        .iter()
        .filter(|(_, repo)| resolver::group_match(&repo.groups, groups))
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
        bail!("no cloned repo declares a `{verb}` command in the manifest");
    }

    let results = fan_out(&targets, default_jobs(jobs), |(name, path, cmd)| {
        let output = shell_command(cmd).current_dir(path).output();
        (name.clone(), output)
    });
    let total = results.len();
    let mut failures = 0usize;
    let c = Palette::new();
    for (name, output) in results {
        println!("{} {} {}", c.dim("──"), c.name(&name), c.dim("──"));
        match output {
            Ok(out) => {
                print!("{}", String::from_utf8_lossy(&out.stdout));
                eprint!("{}", String::from_utf8_lossy(&out.stderr));
                if !out.status.success() {
                    failures += 1;
                    eprintln!("(exit: {})", out.status);
                }
            }
            Err(err) => {
                failures += 1;
                eprintln!("(failed to run: {err})");
            }
        }
    }
    println!("{verb} ran in {}/{} repos", total - failures, total);
    if failures > 0 {
        bail!("{verb} failed in {failures} repo(s)");
    }
    Ok(())
}

fn hooks_install() -> Result<()> {
    let ws = open_workspace()?;
    let backend = ShellGit;
    let script = "#!/bin/sh\n# installed by `haw hooks install`\nhaw verify || {\n  echo 'keel: tree diverges from keel.lock (run haw sync or haw pin)' >&2\n  exit 1\n}\n";
    let mut installed = 0usize;
    for (name, repo) in &ws.manifest.repos {
        let path = ws.root.join(repo.checkout_path(name));
        if !backend.is_repo(&path) {
            continue;
        }
        let hook = path.join(".git").join("hooks").join("pre-commit");
        std::fs::write(&hook, script)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))?;
        }
        installed += 1;
        println!("  ✓ {name}  pre-commit -> haw verify");
    }
    if installed == 0 {
        bail!("no cloned repos — run `haw sync` first");
    }
    println!("installed the integrity pre-commit in {installed} repo(s)");
    Ok(())
}

fn hooks_list() -> Result<()> {
    let ws = open_workspace()?;
    let dir = ws.state_dir().join("hooks");
    let known = [
        "pre-sync",
        "post-sync",
        "pre-lock",
        "post-lock",
        "post-switch",
        "post-change-start",
    ];
    let mut any = false;
    for name in known {
        let path = dir.join(name);
        if path.exists() {
            any = true;
            println!("  {name}  {}", path.display());
        }
    }
    if !any {
        println!(
            "no lifecycle hooks — add executables under {}",
            dir.display()
        );
    }
    Ok(())
}

fn evidence(out: &Path) -> Result<()> {
    let ws = open_workspace()?;
    let staging = ws.state_dir().join("evidence");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;

    std::fs::copy(ws.manifest_path(), staging.join(MANIFEST_FILE))?;
    if ws.lock_path().exists() {
        std::fs::copy(ws.lock_path(), staging.join("keel.lock"))?;
    }
    let audit_log = ws.state_dir().join("audit.jsonl");
    if audit_log.exists() {
        std::fs::copy(&audit_log, staging.join("audit.jsonl"))?;
    }
    let statuses = ws.status(&[], &ShellGit)?;
    std::fs::write(
        staging.join("status.json"),
        serde_json::to_string_pretty(&status_json(&statuses))?,
    )?;
    std::fs::write(
        staging.join("tool.json"),
        serde_json::to_string_pretty(&json!({
            "schema": "keel.evidence/1",
            "tool": "keel",
            "version": env!("CARGO_PKG_VERSION"),
        }))?,
    )?;

    let status = std::process::Command::new("tar")
        .arg("-czf")
        .arg(std::env::current_dir()?.join(out))
        .arg("-C")
        .arg(&staging)
        .arg(".")
        .status()?;
    if !status.success() {
        bail!("tar failed while writing {}", out.display());
    }
    let _ = std::fs::remove_dir_all(&staging);
    record(
        &ws,
        "evidence",
        None,
        None,
        Some(&out.display().to_string()),
    );
    println!("wrote evidence bundle {}", out.display());
    Ok(())
}

fn plugin(args: &[String]) -> Result<ExitCode> {
    let Some((name, rest)) = args.split_first() else {
        bail!("empty plugin invocation");
    };
    let binary = format!("haw-{name}");
    let context = match open_workspace() {
        Ok(ws) => json!({
            "schema": "keel.plugin/1",
            "root": ws.root.to_string_lossy(),
            "stack": ws.current_stack(),
            "repos": ws.manifest.repos.iter().map(|(repo_name, repo)| json!({
                "name": repo_name,
                "path": ws.root.join(repo.checkout_path(repo_name)).to_string_lossy(),
                "rev": repo.rev,
                "groups": repo.groups,
            })).collect::<Vec<_>>(),
        }),
        Err(_) => json!({"schema": "keel.plugin/1"}),
    };

    use std::io::Write;
    let mut child = std::process::Command::new(&binary)
        .args(rest)
        .env("KEEL_JSON", context.to_string())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("no built-in `{name}` and no `{binary}` on PATH"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(context.to_string().as_bytes());
    }
    let status = child.wait()?;
    Ok(ExitCode::from(
        status.code().unwrap_or(1).clamp(0, 255) as u8
    ))
}

fn import_manifest(from: &Path) -> Result<()> {
    let dest = PathBuf::from(MANIFEST_FILE);
    if dest.exists() {
        bail!("{MANIFEST_FILE} already exists here");
    }
    let manifest = import::import(from)?;
    let text = toml::to_string_pretty(&manifest)?;
    std::fs::write(&dest, text)?;
    println!(
        "imported {} repo(s) from {} into {MANIFEST_FILE}",
        manifest.repos.len(),
        from.display()
    );
    println!(
        "one stack `{}` holds every repo — split it into real stacks as needed",
        import::DEFAULT_STACK
    );
    println!("next: haw lock && haw sync");
    Ok(())
}

/// TUI controller: adapts cockpit actions to `keel-core`/`keel-forge`.
/// Runs on the TUI worker thread.
struct CliController;

impl CliController {
    fn workspace(&self) -> std::io::Result<Workspace> {
        let cwd = std::env::current_dir()?;
        Workspace::open(cwd).map_err(std::io::Error::other)
    }

    fn sync_filtered(&self, stack: &str, repo: Option<&str>) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let backend = ShellGit;
        let plan = ws
            .plan_sync(stack, &[], &[], None, &backend)
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
}

fn render_changeset(
    ws: &Workspace,
    id: &str,
    prs: Option<Vec<orchestrate::RepoPrStatus>>,
) -> std::io::Result<keel_tui::ChangesetSummary> {
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
            keel_tui::ChangeRepoRow {
                name: s.name,
                branch: s.branch,
                on_branch: s.on_branch,
                dirty: s.dirty,
                head: s.head,
                pr,
                ci,
            }
        })
        .collect();
    Ok(keel_tui::ChangesetSummary {
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

impl keel_tui::Controller for CliController {
    fn snapshot(&mut self) -> std::io::Result<keel_tui::Snapshot> {
        let ws = self.workspace()?;
        let statuses = ws.status(&[], &ShellGit).map_err(std::io::Error::other)?;
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
        Ok(keel_tui::Snapshot {
            root_label: ws.root.display().to_string(),
            stacks: ws.manifest.stacks.keys().cloned().collect(),
            current_stack: ws.current_stack(),
            fleet,
            changesets,
            lock_present: ws.lock_path().exists(),
            paths,
            tree: tree_text(&ws),
        })
    }

    fn changeset_prs(&mut self, id: &str) -> std::io::Result<keel_tui::ChangesetSummary> {
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
        let ws = self.workspace()?;
        let stack = ws.pick_stack(None).map_err(std::io::Error::other)?;
        self.sync_filtered(&stack, Some(repo))
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
        Ok(format!("pinned keel.lock ({} repos)", lockfile.repos.len()))
    }

    fn lock(&mut self) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let lockfile = ws
            .make_lock(&[], &ShellGit)
            .map_err(std::io::Error::other)?;
        lockfile
            .save(&ws.lock_path())
            .map_err(std::io::Error::other)?;
        Ok(format!("wrote keel.lock ({} repos)", lockfile.repos.len()))
    }

    fn run_cmd(&mut self, cmd: &str) -> std::io::Result<String> {
        let ws = self.workspace()?;
        let backend = ShellGit;
        let repos: Vec<(String, PathBuf)> = ws
            .manifest
            .repos
            .iter()
            .map(|(name, repo)| (name.clone(), ws.root.join(repo.checkout_path(name))))
            .filter(|(_, path)| backend.is_repo(path))
            .collect();
        let results = fan_out(&repos, default_jobs(None), |(name, path)| {
            let ok = shell_command(cmd)
                .current_dir(path)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            (name.clone(), ok)
        });
        let failed: Vec<&str> = results
            .iter()
            .filter(|(_, ok)| !ok)
            .map(|(name, _)| name.as_str())
            .collect();
        if failed.is_empty() {
            Ok(format!("ran `{cmd}` in {} repos", results.len()))
        } else {
            Ok(format!("`{cmd}` failed in: {}", failed.join(", ")))
        }
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
}

fn dash() -> Result<()> {
    open_workspace()?;
    if let Some(path) = keel_tui::run(Box::new(CliController))? {
        println!("{}", path.display());
    }
    Ok(())
}
