use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use keel_core::git::GitBackend;
use keel_core::manifest::{ManifestLoader, TomlLoader, edit};
use keel_core::workspace::{MANIFEST_FILE, SyncOutcome, Workspace, sync_repo};
use keel_core::{change, resolver};
use keel_git::ShellGit;
use keel_git::parallel::fan_out;

/// Minimal ANSI painter: colored on a TTY, plain under `NO_COLOR` or when piped.
struct Palette {
    on: bool,
}

impl Palette {
    fn new() -> Self {
        let on = std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal();
        Self { on }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.on {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
}

#[derive(Parser)]
#[command(name = "keel", version, about = "The beam that binds the repos")]
struct Cli {
    /// Path to the manifest.
    #[arg(long, global = true, default_value = "keel.toml")]
    manifest: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Bootstrap a workspace from a manifest file.
    Init {
        /// Path to an existing keel.toml to copy here.
        source: PathBuf,
    },
    /// Clone/update repos to the state in keel.lock (writes it if absent).
    Sync {
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
    /// Restore keel.lock to the manifest revs (same as `keel lock`).
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
    Status {
        /// Only repos in these groups (repeatable).
        #[arg(long = "group")]
        groups: Vec<String>,
    },
    /// Record a stack as current and sync it.
    Switch {
        stack: String,
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
    },
    /// Print the stack -> repo tree.
    Graph {
        #[arg(long = "stack", alias = "product")]
        stack: Option<String>,
        #[arg(long)]
        overlay: Vec<String>,
    },
    /// Run a command in every repo, in parallel.
    Forall {
        #[arg(short = 'c', long = "command")]
        command: String,
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
    /// Save/restore named multi-repo states (branch + commit per repo).
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
    /// Launch the fleet dashboard.
    Tui,
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

#[derive(Subcommand)]
enum RepoCommand {
    /// List repos with rev, path, and groups.
    List,
    /// Add a repo to the manifest (keeps your comments and formatting).
    Add {
        name: String,
        /// Full clone URL (or use --remote + --repo).
        #[arg(long, conflicts_with_all = ["remote", "repo"])]
        url: Option<String>,
        /// Named remote from [remote.X].
        #[arg(long, requires = "repo")]
        remote: Option<String>,
        /// Repository path under the remote.
        #[arg(long, requires = "remote")]
        repo: Option<String>,
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
    },
    /// Per-repo branch + PR/MR dashboard for a changeset.
    Status { id: String },
    /// Push the changeset branches and open cross-linked PR/MRs.
    Request {
        id: String,
        /// Target branch for the PR/MRs (default: the locked branch, else main).
        #[arg(long)]
        base: Option<String>,
    },
    /// Merge the changeset PR/MRs in order; stops at the first failure.
    Land { id: String },
    /// Print a changeset repo's path (usable as: cd "$(keel change goto ID REPO)").
    Goto {
        id: String,
        /// Repo name; omit for an interactive picker.
        repo: Option<String>,
    },
    /// List recorded changesets.
    List,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { source } => init(&source),
        Command::Sync {
            stack,
            overlay,
            groups,
            shared,
            jobs,
        } => sync(stack.as_deref(), &overlay, &groups, shared, jobs),
        Command::Lock { overlay } => lock(&overlay),
        Command::Pin => pin(),
        Command::Unpin { overlay } => unpin(&overlay),
        Command::Repo { command } => match command {
            RepoCommand::List => repo_list(),
            RepoCommand::Add {
                name,
                url,
                remote,
                repo,
                rev,
                path,
                groups,
            } => repo_add(&name, url, remote, repo, rev, path, groups),
            RepoCommand::Remove { name } => repo_remove(&name),
        },
        Command::Stack { command } => match command {
            StackCommand::List => stack_list(),
            StackCommand::Add { name, repos } => stack_add(&name, &repos),
            StackCommand::Remove { name } => stack_remove(&name),
        },
        Command::Status { groups } => status(&groups),
        Command::Switch { stack, jobs } => switch(&stack, jobs),
        Command::Graph { stack, overlay } => graph(&cli.manifest, stack.as_deref(), &overlay),
        Command::Forall {
            command,
            groups,
            jobs,
        } => forall(&command, &groups, jobs),
        Command::Change { command } => match command {
            ChangeCommand::Start {
                id,
                repos,
                branch,
                skip_branch,
            } => change_start(&id, repos.as_deref(), branch.as_deref(), skip_branch),
            ChangeCommand::Status { id } => change_status(&id),
            ChangeCommand::Request { id, base } => change_request(&id, base.as_deref()),
            ChangeCommand::Land { id } => change_land(&id),
            ChangeCommand::Goto { id, repo } => change_goto(&id, repo.as_deref()),
            ChangeCommand::List => change_list(),
        },
        Command::Snapshot { command } => match command {
            SnapshotCommand::Save { name } => snapshot_save(&name),
            SnapshotCommand::Restore { name } => snapshot_restore(&name),
            SnapshotCommand::List => snapshot_list(),
        },
        Command::Tui => tui(),
    }
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

fn init(source: &Path) -> Result<()> {
    let dest = PathBuf::from(MANIFEST_FILE);
    if dest.exists() {
        bail!("{MANIFEST_FILE} already exists here");
    }
    if !source.is_file() {
        bail!(
            "{} is not a file (URL bootstrap lands with forge integration; pass a local path)",
            source.display()
        );
    }
    TomlLoader
        .load(source)
        .with_context(|| format!("{} is not a valid manifest", source.display()))?;
    std::fs::copy(source, &dest)
        .with_context(|| format!("copying {} to {MANIFEST_FILE}", source.display()))?;
    println!("initialized workspace from {}", source.display());
    println!("next: keel sync");
    Ok(())
}

fn sync(
    stack: Option<&str>,
    overlays: &[String],
    groups: &[String],
    shared: bool,
    jobs: Option<usize>,
) -> Result<()> {
    let ws = open_workspace()?;
    let stack = ws.pick_stack(stack)?;
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
    } else if !overlays.is_empty() {
        println!("note: keel.lock exists — overlays ignored (run `keel lock` to re-resolve)");
    }

    let results = fan_out(&plan.tasks, default_jobs(jobs), |task| {
        (task.name.clone(), sync_repo(task, &backend))
    });

    let mut failures = 0usize;
    for (name, result) in &results {
        match result {
            Ok(SyncOutcome::Cloned) => println!("  ✓ {name}  cloned"),
            Ok(SyncOutcome::Updated) => println!("  ✓ {name}  updated"),
            Ok(SyncOutcome::AlreadySynced) => println!("  ✓ {name}  up to date"),
            Err(err) => {
                failures += 1;
                eprintln!("  ✗ {name}  {err}");
            }
        }
    }
    println!(
        "synced stack `{}` ({}/{} repos)",
        plan.stack,
        results.len() - failures,
        results.len()
    );
    if failures > 0 {
        bail!("{failures} repo(s) failed to sync");
    }
    Ok(())
}

fn lock(overlays: &[String]) -> Result<()> {
    let ws = open_workspace()?;
    let backend = ShellGit;
    let lockfile = ws.make_lock(overlays, &backend)?;
    lockfile.save(&ws.lock_path())?;
    println!("wrote keel.lock ({} repos pinned)", lockfile.repos.len());
    for repo in &lockfile.repos {
        println!(
            "  {}  {}  <- {}",
            repo.name,
            &repo.rev[..12.min(repo.rev.len())],
            repo.source_rev
        );
    }
    Ok(())
}

fn pin() -> Result<()> {
    let ws = open_workspace()?;
    let lockfile = ws.pin(&ShellGit)?;
    lockfile.save(&ws.lock_path())?;
    println!(
        "pinned keel.lock to current HEADs ({} repos)",
        lockfile.repos.len()
    );
    for repo in &lockfile.repos {
        println!(
            "  {}  {}  ({})",
            repo.name,
            &repo.rev[..8.min(repo.rev.len())],
            repo.branch
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
        println!("no repos — add one with `keel repo add <name> --url <url>`");
        return Ok(());
    }
    let width = ws.manifest.repos.keys().map(String::len).max().unwrap_or(4);
    for (name, repo) in &ws.manifest.repos {
        let groups = if repo.groups.is_empty() {
            String::new()
        } else {
            format!("  [{}]", repo.groups.join(", "))
        };
        println!(
            "{name:<width$}  {}  {}{groups}",
            repo.rev,
            repo.checkout_path(name).display()
        );
    }
    Ok(())
}

fn repo_add(
    name: &str,
    url: Option<String>,
    remote: Option<String>,
    repo: Option<String>,
    rev: String,
    path: Option<String>,
    groups: Vec<String>,
) -> Result<()> {
    let ws = open_workspace()?;
    let spec = edit::NewRepo {
        name: name.to_string(),
        url,
        remote,
        repo,
        rev,
        path,
        groups,
    };
    let text = std::fs::read_to_string(ws.manifest_path())?;
    let updated = edit::add_repo(&text, &spec)?;
    std::fs::write(ws.manifest_path(), updated)?;
    println!("added repo `{name}`");
    println!("next: keel lock && keel sync");
    Ok(())
}

fn repo_remove(name: &str) -> Result<()> {
    let ws = open_workspace()?;
    let text = std::fs::read_to_string(ws.manifest_path())?;
    let updated = edit::remove_repo(&text, name)?;
    std::fs::write(ws.manifest_path(), updated)?;
    println!("removed repo `{name}` from the manifest");
    println!("note: its clone stays on disk; delete the directory if unwanted");
    Ok(())
}

fn stack_list() -> Result<()> {
    let ws = open_workspace()?;
    if ws.manifest.stacks.is_empty() {
        println!("no stacks — add one with `keel stack add <name> --repos a,b`");
        return Ok(());
    }
    let current = ws.current_stack();
    for (name, stack) in &ws.manifest.stacks {
        let marker = if current.as_deref() == Some(name) {
            "*"
        } else {
            " "
        };
        println!("{marker} {name}: {}", stack.repos.join(", "));
    }
    Ok(())
}

fn stack_add(name: &str, repos: &[String]) -> Result<()> {
    let ws = open_workspace()?;
    let text = std::fs::read_to_string(ws.manifest_path())?;
    let updated = edit::add_stack(&text, name, repos)?;
    std::fs::write(ws.manifest_path(), updated)?;
    println!("added stack `{name}` ({} repos)", repos.len());
    Ok(())
}

fn stack_remove(name: &str) -> Result<()> {
    let ws = open_workspace()?;
    let text = std::fs::read_to_string(ws.manifest_path())?;
    let updated = edit::remove_stack(&text, name)?;
    std::fs::write(ws.manifest_path(), updated)?;
    println!("removed stack `{name}`");
    Ok(())
}

fn status(groups: &[String]) -> Result<()> {
    let ws = open_workspace()?;
    let statuses = ws.status(groups, &ShellGit)?;
    if statuses.is_empty() {
        println!("no matching repos");
        return Ok(());
    }
    let width = statuses.iter().map(|s| s.name.len()).max().unwrap_or(4);
    println!(
        "{:<width$}  {:<24} {:<10} {:<6} DRIFT",
        "REPO", "BRANCH", "HEAD", "DIRTY"
    );
    for s in &statuses {
        if s.missing {
            println!("{:<width$}  (not cloned — run `keel sync`)", s.name);
            continue;
        }
        println!(
            "{:<width$}  {:<24} {:<10} {:<6} {}",
            s.name,
            s.branch.as_deref().unwrap_or("(detached)"),
            s.head
                .as_deref()
                .map(|h| &h[..8.min(h.len())])
                .unwrap_or("—"),
            if s.dirty { "yes" } else { "-" },
            if s.drift { "YES" } else { "-" },
        );
    }
    Ok(())
}

fn switch(stack: &str, jobs: Option<usize>) -> Result<()> {
    let ws = open_workspace()?;
    let stack = ws.pick_stack(Some(stack))?;
    ws.set_current_stack(&stack)?;
    println!("switched to stack `{stack}`");
    sync(Some(&stack), &[], &[], false, jobs)
}

fn graph(path: &Path, stack: Option<&str>, overlays: &[String]) -> Result<()> {
    let manifest = TomlLoader.load(path)?;

    let selected: Vec<String> = match stack {
        Some(name) => vec![name.to_string()],
        None => manifest.stacks.keys().cloned().collect(),
    };
    if selected.is_empty() {
        println!("no stacks defined in {}", path.display());
        return Ok(());
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

fn forall(command: &str, groups: &[String], jobs: Option<usize>) -> Result<()> {
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
        bail!("no cloned repos — run `keel sync` first");
    }

    let results = fan_out(&present, default_jobs(jobs), |(name, path)| {
        let output = shell_command(command).current_dir(path).output();
        (name.clone(), output)
    });

    let mut failures = 0usize;
    for (name, output) in results {
        println!("── {name} ──");
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
) -> Result<()> {
    let ws = open_workspace()?;
    let changeset = change::start(&ws, &ShellGit, id, repos, branch, skip_branch)?;
    println!(
        "changeset `{}` started across {} repo(s):",
        changeset.id,
        changeset.repos.len()
    );
    for repo in &changeset.repos {
        println!("  {}  -> {}", repo.name, repo.branch);
    }
    Ok(())
}

fn change_status(id: &str) -> Result<()> {
    let ws = open_workspace()?;
    let statuses = change::status(&ws, &ShellGit, id)?;
    let width = statuses.iter().map(|s| s.name.len()).max().unwrap_or(4);
    println!("changeset `{id}`");
    println!(
        "{:<width$}  {:<24} {:<9} {:<6} {:<10} PR",
        "REPO", "BRANCH", "ON IT", "DIRTY", "HEAD"
    );
    for s in &statuses {
        if s.missing {
            println!("{:<width$}  (repo missing — run `keel sync`)", s.name);
            continue;
        }
        println!(
            "{:<width$}  {:<24} {:<9} {:<6} {:<10} —",
            s.name,
            s.branch,
            if s.on_branch { "yes" } else { "NO" },
            if s.dirty { "yes" } else { "-" },
            s.head
                .as_deref()
                .map(|h| &h[..8.min(h.len())])
                .unwrap_or("—"),
        );
    }

    let changeset = change::Changeset::load(&ws, id)?;
    if changeset.repos.iter().any(|r| r.pr_number.is_some()) {
        println!();
        println!("PR/MRs:");
        let tokens = keel_forge::Tokens::from_env();
        for (name, status) in keel_forge::orchestrate::statuses(&ws, &tokens, id)? {
            match status {
                None => println!("  {name}  (no PR — run `keel change request`)"),
                Some(Ok(s)) => println!(
                    "  {name}  {}  approved: {}  ci: {}  {}",
                    match s.state {
                        keel_forge::PrState::Open => "open",
                        keel_forge::PrState::Draft => "draft",
                        keel_forge::PrState::Merged => "merged",
                        keel_forge::PrState::Closed => "closed",
                    },
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
        println!("(no PR/MRs yet — open them with `keel change request {id}`)");
    }
    Ok(())
}

fn change_request(id: &str, base: Option<&str>) -> Result<()> {
    let ws = open_workspace()?;
    let tokens = keel_forge::Tokens::from_env();
    let outcomes = keel_forge::orchestrate::request(&ws, &ShellGit, &tokens, id, base)?;
    let mut failures = 0usize;
    for outcome in &outcomes {
        match &outcome.result {
            Ok(url) => println!("  ✓ {}  {url}", outcome.name),
            Err(err) => {
                failures += 1;
                eprintln!("  ✗ {}  {err}", outcome.name);
            }
        }
    }
    if failures > 0 {
        bail!("{failures} repo(s) failed; fix and re-run `keel change request {id}`");
    }
    println!(
        "requested changeset `{id}` ({} PR/MRs, cross-linked)",
        outcomes.len()
    );
    Ok(())
}

fn change_land(id: &str) -> Result<()> {
    let ws = open_workspace()?;
    let tokens = keel_forge::Tokens::from_env();
    let outcomes = keel_forge::orchestrate::land(&ws, &tokens, id)?;
    let mut failed = false;
    for outcome in &outcomes {
        match &outcome.result {
            Ok(msg) => println!("  ✓ {}  {msg}", outcome.name),
            Err(err) => {
                failed = true;
                eprintln!("  ✗ {}  {err}", outcome.name);
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

fn snapshot_save(name: &str) -> Result<()> {
    let ws = open_workspace()?;
    let snap = keel_core::snapshot::save(&ws, &ShellGit, name)?;
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
    let snap = keel_core::snapshot::restore(&ws, &ShellGit, name)?;
    println!("restored snapshot `{name}` ({} repos)", snap.repos.len());
    Ok(())
}

fn snapshot_list() -> Result<()> {
    let ws = open_workspace()?;
    let names = keel_core::snapshot::Snapshot::list(&ws)?;
    if names.is_empty() {
        println!("no snapshots — save one with `keel snapshot save <name>`");
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
        println!("no changesets — start one with `keel change start <id>`");
        return Ok(());
    }
    for id in ids {
        println!("{id}");
    }
    Ok(())
}

fn tui() -> Result<()> {
    let ws = open_workspace()?;
    keel_tui::run(move || {
        let statuses = ws.status(&[], &ShellGit).map_err(std::io::Error::other)?;
        let views: Vec<keel_tui::FleetView> = ws
            .manifest
            .stacks
            .iter()
            .map(|(stack, spec)| keel_tui::FleetView {
                stack: stack.clone(),
                repos: statuses
                    .iter()
                    .filter(|s| spec.repos.contains(&s.name))
                    .cloned()
                    .collect(),
            })
            .collect();
        Ok(views)
    })?;
    Ok(())
}
