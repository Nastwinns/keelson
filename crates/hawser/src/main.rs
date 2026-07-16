use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use haw_core::git::GitBackend;
use haw_core::manifest::{ManifestLoader, TomlLoader, edit, import};
use haw_core::plugin::{self, Dispatch, ProcessRunner, RepoContext};
use haw_core::workspace::{MANIFEST_FILE, RepoStatus, SyncOutcome, Workspace, sync_repo};
use haw_core::{audit, change, hooks, resolver, snapshot};
use haw_forge::{PrState, Tokens, orchestrate};
use haw_git::ShellGit;
use haw_git::parallel::fan_out;
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
#[command(
    name = "haw",
    version,
    about = "The beam that binds the repos",
    after_help = "\
Examples:
  $ haw init haw.toml           bootstrap a workspace from a manifest
  $ haw sync                     clone/update every repo, writing haw.lock
  $ haw tree                     print the stack -> repo composition
  $ haw status                   dirty/drift/ahead-behind per repo
  $ haw change start FEAT-42     branch across every affected repo
  $ haw                          open the fleet cockpit (bare, no subcommand)

Run `haw <command> --help` for that command's own examples."
)]
struct Cli {
    /// Path to the manifest.
    #[arg(long, global = true, default_value = "haw.toml")]
    manifest: PathBuf,

    /// No subcommand opens the TUI cockpit (same as `haw dash`).
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Bootstrap a workspace from a manifest file or URL.
    #[command(after_help = "\
Examples:
  $ haw init haw.toml                                 from a local file
  $ haw init https://example.com/fleet/haw.toml       from a URL
  $ haw --manifest custom.toml init haw.toml           bootstrap under a custom filename")]
    Init {
        /// Path or http(s) URL of an existing haw.toml.
        source: String,
    },
    /// Clone/update repos to the state in haw.lock (writes it if absent).
    #[command(after_help = "\
Examples:
  $ haw sync                          clone/update every repo in the current stack
  $ haw sync --stack gateway          sync one specific stack
  $ haw sync --locked                 CI gate: fail unless haw.lock already exists
  $ haw sync --shared                 clone via a local mirror cache (git alternates)
  $ haw sync --group firmware -j 4    only `firmware`-grouped repos, 4 parallel jobs")]
    Sync {
        /// CI contract: fail unless haw.lock exists (no rev resolution).
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
    /// Resolve every repo's rev to a SHA and (re)write haw.lock.
    #[command(after_help = "\
Examples:
  $ haw lock                    resolve every repo's manifest rev -> haw.lock
  $ haw lock --overlay dev       resolve using the `dev` overlay's rev overrides")]
    Lock {
        #[arg(long)]
        overlay: Vec<String>,
    },
    /// Pin haw.lock to each repo's current HEAD (no network).
    #[command(
        alias = "freeze",
        after_help = "\
Examples:
  $ haw pin       snapshot every repo's current checkout into haw.lock (no network)"
    )]
    Pin,
    /// Restore haw.lock to the manifest revs (same as `haw lock`).
    #[command(
        alias = "unfreeze",
        after_help = "\
Examples:
  $ haw unpin                    restore haw.lock to the manifest's declared revs
  $ haw unpin --overlay dev       ...using the `dev` overlay's rev overrides"
    )]
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
    #[command(
        alias = "st",
        after_help = "\
Examples:
  $ haw status                       branch/head/dirty/drift for every repo
  $ haw status --group firmware       only `firmware`-grouped repos
  $ haw status --format json          machine-readable (schema haw.status/1)
  $ haw status --verify               exit 3 if anything is missing, dirty, or drifted (CI gate)"
    )]
    Status {
        /// Only repos in these groups (repeatable).
        #[arg(long = "group")]
        groups: Vec<String>,
        /// `text` (default) or `json` (schema haw.status/1).
        #[arg(long, default_value = "text")]
        format: String,
        /// Exit 3 when any repo is missing, dirty, or drifted (CI gate).
        #[arg(long)]
        verify: bool,
    },
    /// Record a stack as current and sync it.
    #[command(after_help = "\
Examples:
  $ haw switch sensor-node       record `sensor-node` as current, then sync it
  $ haw switch gateway -j 8       ...with 8 parallel sync jobs")]
    Switch {
        stack: String,
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
    },
    /// Print the stack -> repo tree.
    #[command(
        alias = "graph",
        after_help = "\
Examples:
  $ haw tree                       every stack -> repo composition
  $ haw tree --stack gateway        just one stack
  $ haw tree --overlay dev          composition after applying the `dev` overlay
  $ haw tree --format json          machine-readable (schema haw.tree/1)"
    )]
    Tree {
        #[arg(long = "stack", alias = "product")]
        stack: Option<String>,
        #[arg(long)]
        overlay: Vec<String>,
        /// `text` (default) or `json` (schema haw.tree/1).
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Run a command in every repo, in parallel.
    #[command(
        alias = "forall",
        after_help = "\
Examples:
  $ haw run 'git fetch --tags'          quote multi-word commands
  $ haw run -c 'git status -s'           repo-tool-style -c flag also works
  $ haw run --group firmware 'make'       only `firmware`-grouped repos"
    )]
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
    #[command(after_help = "\
Examples:
  $ haw change start FEAT-42 --repos kernel,hal    branch across two repos
  $ haw change status FEAT-42                       per-repo branch + PR/CI dashboard
  $ haw change request FEAT-42                       open cross-linked PR/MRs
  $ haw change land FEAT-42                          merge them in dependency order

Run `haw change <subcommand> --help` for that subcommand's own examples.")]
    Change {
        #[command(subcommand)]
        command: ChangeCommand,
    },
    /// Assert the on-disk tree matches haw.lock; exit 3 on drift (CI gate).
    #[command(after_help = "\
Examples:
  $ haw verify                    exit 3 if any repo drifted from haw.lock (CI gate)
  $ haw verify --format json       machine-readable (schema haw.status/1)")]
    Verify {
        /// `text` (default) or `json` (schema haw.status/1).
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Run each repo's `build` command from the manifest, in parallel.
    #[command(after_help = "\
Examples:
  $ haw build                       run every repo's declared `build =` command
  $ haw build --group firmware       only `firmware`-grouped repos")]
    Build {
        /// Only repos in these groups (repeatable).
        #[arg(long = "group")]
        groups: Vec<String>,
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
    },
    /// Run each repo's `test` command from the manifest, in parallel.
    #[command(after_help = "\
Examples:
  $ haw test                       run every repo's declared `test =` command
  $ haw test --group firmware -j 2  only `firmware`-grouped repos, 2 parallel jobs")]
    Test {
        /// Only repos in these groups (repeatable).
        #[arg(long = "group")]
        groups: Vec<String>,
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
    },
    /// Manage lifecycle hooks (.haw/hooks) and git integrity hooks.
    #[command(after_help = "\
Examples:
  $ haw hooks install       write a pre-commit hook (runs `haw verify`) in every repo
  $ haw hooks list          show the lifecycle hooks this workspace defines")]
    Hooks {
        #[command(subcommand)]
        command: HooksCommand,
    },
    /// Bundle baseline evidence (manifest, lock, audit log, status) for audits.
    #[command(after_help = "\
Examples:
  $ haw evidence                        bundle into ./haw-evidence.tar.gz
  $ haw evidence --out release.tar.gz    choose the output archive path")]
    Evidence {
        /// Output archive path.
        #[arg(long, default_value = "haw-evidence.tar.gz")]
        out: PathBuf,
    },
    /// Convert a west.yml or repo default.xml manifest to haw.toml.
    #[command(after_help = "\
Examples:
  $ haw import --from west.yml            convert a west manifest
  $ haw import --from default.xml          convert a Google `repo` manifest
  $ haw import --from west.yml --manifest new.toml   write the result to a custom filename")]
    Import {
        /// Path to the foreign manifest.
        #[arg(long)]
        from: PathBuf,
    },
    /// Parallel collaborative merge: slice one big merge into reviewable units.
    #[command(after_help = "\
Examples:
  $ haw merge plan origin/feature --repo kernel     slice a merge into per-directory units
  $ haw merge resolve src --take theirs --repo kernel   auto-resolve one slice
  $ haw merge status --repo kernel                   show slices and resolution state
  $ haw merge cleanup --repo kernel -m 'merge feature'  seal it as one merge commit
  $ haw merge abort --repo kernel                    give up and restore the target branch

Run `haw merge <subcommand> --help` for that subcommand's own examples.")]
    Merge {
        #[command(subcommand)]
        command: MergeCommand,
    },
    /// Open the fleet dashboard (same as bare `haw`).
    #[command(
        alias = "tui",
        after_help = "\
Examples:
  $ haw dash       open the cockpit (identical to running bare `haw`)"
    )]
    Dash {
        /// Drive the cockpit with canned, in-memory data (no workspace/network).
        #[arg(long, hide = true)]
        demo: bool,
    },
    /// Anything else runs a `haw-<name>` plugin from PATH.
    #[command(external_subcommand)]
    Plugin(Vec<String>),
}

#[derive(Subcommand)]
enum HooksCommand {
    /// Write a pre-commit hook in every repo that runs `haw verify`.
    #[command(after_help = "\
Examples:
  $ haw hooks install       add the pre-commit hook to every cloned repo")]
    Install,
    /// List the lifecycle hooks the workspace defines.
    #[command(after_help = "\
Examples:
  $ haw hooks list       show pre-sync/post-sync/... hooks declared in haw.toml")]
    List,
}

#[derive(Subcommand)]
enum RepoCommand {
    /// List repos with rev, path, and groups.
    #[command(after_help = "\
Examples:
  $ haw repo list       show every repo's rev, checkout path, and groups")]
    List,
    /// Add a repo to the manifest (keeps your comments and formatting).
    #[command(after_help = "\
Examples:
  $ haw repo add kernel --url git@github.com:acme/kernel.git --rev v6.1.2
  $ haw repo add hal --remote internal --slug hal.git --group firmware
  $ haw repo add app-mqtt --url git@github.com:acme/app-mqtt.git --path apps/mqtt --rev main")]
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
    #[command(after_help = "\
Examples:
  $ haw repo remove hal       fails if any stack/overlay still references `hal`")]
    Remove { name: String },
}

#[derive(Subcommand)]
enum StackCommand {
    /// List stacks and their repos.
    #[command(after_help = "\
Examples:
  $ haw stack list       show every stack and the repos it composes")]
    List,
    /// Add a stack composed of existing repos.
    #[command(after_help = "\
Examples:
  $ haw stack add gateway --repos kernel,hal,app-mqtt")]
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
    #[command(after_help = "\
Examples:
  $ haw stack remove sensor-node")]
    Remove { name: String },
}

#[derive(Subcommand)]
enum ChangeCommand {
    /// Create one branch across the affected repos.
    #[command(after_help = "\
Examples:
  $ haw change start FEAT-42 --repos kernel,hal      branch two repos
  $ haw change start FEAT-42                          branch every repo in the manifest
  $ haw change start FEAT-42 --skip-branch             adopt each repo's current branch instead
  $ haw change start FEAT-42 --label adas --label perf  labels forwarded to `change request`")]
    Start {
        id: String,
        /// Repos to include (default: all repos in the manifest).
        #[arg(long = "repos", alias = "bricks", value_delimiter = ',')]
        repos: Option<Vec<String>>,
        /// Branch name (default: `change/<id>`).
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
    #[command(after_help = "\
Examples:
  $ haw change status FEAT-42       branches, dirty state, and PR/MR + CI status")]
    Status { id: String },
    /// Push the changeset branches and open cross-linked PR/MRs.
    #[command(after_help = "\
Examples:
  $ haw change request FEAT-42                base branch: the locked branch, else main
  $ haw change request FEAT-42 --base develop   target a specific base branch")]
    Request {
        id: String,
        /// Target branch for the PR/MRs (default: the locked branch, else main).
        #[arg(long)]
        base: Option<String>,
    },
    /// Merge the PR/MRs in dependency order; stops at the first failure.
    #[command(after_help = "\
Examples:
  $ haw change land FEAT-42       merge every repo's PR/MR, in manifest `deps` order")]
    Land { id: String },
    /// Print a changeset repo's path (usable as: cd "$(haw change goto ID REPO)").
    #[command(after_help = "\
Examples:
  $ haw change goto FEAT-42 kernel            print kernel's checkout path
  $ cd \"$(haw change goto FEAT-42 kernel)\"     cd straight into it
  $ haw change goto FEAT-42                    interactive picker (needs a terminal)")]
    Goto {
        id: String,
        /// Repo name; omit for an interactive picker.
        repo: Option<String>,
    },
    /// Save/restore the multi-repo state of a changeset.
    #[command(after_help = "\
Examples:
  $ haw change snapshot save before-refactor       record every repo's branch + HEAD
  $ haw change snapshot restore before-refactor     check every repo back out
  $ haw change snapshot list                        show saved snapshots")]
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
    /// List recorded changesets.
    #[command(after_help = "\
Examples:
  $ haw change list       show every changeset id recorded in .haw/changesets")]
    List,
}

#[derive(Subcommand)]
enum MergeCommand {
    /// Start merging <source> into the current branch; slice the conflicts.
    #[command(after_help = "\
Examples:
  $ haw merge plan origin/feature --repo kernel                merge feature into kernel
  $ haw merge plan release/2.x --repo kernel --into custom-branch   name the integration branch")]
    Plan {
        /// Branch/tag/SHA to merge in.
        source: String,
        /// Repo to merge in (default: the only repo, else required).
        #[arg(long)]
        repo: Option<String>,
        /// Integration branch name (default: haw/merge/<source>).
        #[arg(long)]
        into: Option<String>,
    },
    /// Resolve one slice of the in-progress merge.
    #[command(after_help = "\
Examples:
  $ haw merge resolve src --take theirs --repo kernel    accept the incoming side for `src`
  $ haw merge resolve docs --take ours --repo kernel      keep the current side for `docs`
  $ haw merge resolve src --repo kernel                    stage `src` as you edited it by hand")]
    Resolve {
        slice: String,
        #[arg(long)]
        repo: Option<String>,
        /// Auto-resolve the whole slice to `ours` or `theirs` (else stage as edited).
        #[arg(long)]
        take: Option<TakeSide>,
    },
    /// Show the planned slices and their resolution state.
    #[command(after_help = "\
Examples:
  $ haw merge status --repo kernel       which slices are resolved, which remain")]
    Status {
        #[arg(long)]
        repo: Option<String>,
    },
    /// Seal the merge: commit it, fast-forward the target, drop temp branches.
    #[command(after_help = "\
Examples:
  $ haw merge cleanup --repo kernel                        refuses if any slice is unresolved
  $ haw merge cleanup --repo kernel -m 'merge feature'      custom merge commit message")]
    Cleanup {
        #[arg(long)]
        repo: Option<String>,
        /// Merge commit message (default: git's merge message).
        #[arg(long, short = 'm')]
        message: Option<String>,
    },
    /// Abort the planned merge and restore the target branch.
    #[command(after_help = "\
Examples:
  $ haw merge abort --repo kernel       undo the merge, drop the integration branch")]
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
    #[command(after_help = "\
Examples:
  $ haw change snapshot save before-refactor")]
    Save { name: String },
    /// Check every repo back out to a saved state (refuses on dirty repos).
    #[command(after_help = "\
Examples:
  $ haw change snapshot restore before-refactor")]
    Restore { name: String },
    /// List saved snapshots.
    #[command(after_help = "\
Examples:
  $ haw change snapshot list")]
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
    let _ = MANIFEST_ARG.set(cli.manifest.clone());
    let Some(command) = cli.command else {
        dash(false)?;
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
        Command::Dash { demo } => dash(demo)?,
        Command::Plugin(args) => return plugin(&args),
    }
    Ok(ExitCode::SUCCESS)
}

/// The `--manifest` flag, captured once in `run()` so every command —
/// including the bare `dash`/TUI entrypoint — honors it, not just `tree`.
static MANIFEST_ARG: OnceLock<PathBuf> = OnceLock::new();

/// Resolve `--manifest` (default `haw.toml`) against the current directory.
fn manifest_path() -> Result<PathBuf> {
    let manifest = MANIFEST_ARG
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(MANIFEST_FILE));
    Ok(if manifest.is_absolute() {
        manifest
    } else {
        std::env::current_dir()?.join(manifest)
    })
}

fn open_workspace() -> Result<Workspace> {
    Ok(Workspace::open_manifest(manifest_path()?)?)
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
    let dest = manifest_path()?;
    if dest.exists() {
        bail!("{} already exists here", dest.display());
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
    text.parse::<haw_core::manifest::Manifest>()
        .with_context(|| format!("{source} is not a valid manifest"))?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
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
        bail!("--locked: no haw.lock — commit one (haw lock) before running CI syncs");
    }
    hooks::fire(&ws, hooks::Hook::PreSync, &json!({"stack": stack}))?;
    let backend = ShellGit;
    let cache_root = if shared {
        let root = haw_git::default_cache_root().context("no cache directory on this platform")?;
        println!("sharing objects via {}", root.display());
        Some(root)
    } else {
        None
    };
    let plan = ws.plan_sync(&stack, overlays, groups, cache_root.as_deref(), &backend)?;
    if plan.wrote_lock {
        println!("wrote haw.lock ({} repos pinned)", plan.tasks.len());
        record(&ws, "lock.write", None, None, None);
    } else if !overlays.is_empty() {
        println!("note: haw.lock exists — overlays ignored (run `haw lock` to re-resolve)");
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
            "wrote haw.lock ({} repos pinned)",
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
            "pinned haw.lock to current HEADs ({} repos)",
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
    println!("restored haw.lock to the manifest revs");
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
        "schema": "haw.status/1",
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
            "groups": s.groups,
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
            serde_json::to_string_pretty(&json!({"schema": "haw.tree/1", "stacks": stacks}))?
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

fn render_ci_status(status: haw_forge::CiStatus) -> &'static str {
    match status {
        haw_forge::CiStatus::Passed => "passed",
        haw_forge::CiStatus::Failed => "failed",
        haw_forge::CiStatus::Running => "running",
        haw_forge::CiStatus::Queued => "queued",
        haw_forge::CiStatus::Cancelled => "cancelled",
    }
}

/// `github`/`gitlab`/`—` for a manifest repo, from its remote URL.
fn forge_label(ws: &Workspace, name: &str) -> String {
    ws.manifest
        .repos
        .get(name)
        .and_then(|repo| repo.clone_url(&ws.manifest.remotes))
        .map(|url| match haw_forge::detect(&url) {
            haw_forge::ForgeKind::GitHub => "github".to_string(),
            haw_forge::ForgeKind::GitLab => "gitlab".to_string(),
            haw_forge::ForgeKind::Unknown => "—".to_string(),
        })
        .unwrap_or_else(|| "—".to_string())
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
    fire_phase(
        &ws,
        hooks::Hook::PreRequest,
        json!({"id": id, "base": base}),
    )?;
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
    fire_phase(
        &ws,
        hooks::Hook::PostLand,
        json!({"id": id, "repos": outcomes.len()}),
    )?;
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
    let plan = haw_merge::plan(
        &haw_merge::git::GitMerge,
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
        TakeSide::Ours => haw_merge::Side::Ours,
        TakeSide::Theirs => haw_merge::Side::Theirs,
    });
    let plan = haw_merge::resolve(
        &haw_merge::git::GitMerge,
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
    let Some(plan) = haw_merge::load_plan(&ws.state_dir(), &name)? else {
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
    let report = haw_merge::cleanup(
        &haw_merge::git::GitMerge,
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
    let plan = haw_merge::abort(&haw_merge::git::GitMerge, &path, &ws.state_dir(), &name)?;
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
        bail!("no haw.lock to verify against — run `haw lock` first");
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
            println!("verified: tree matches haw.lock ({} repos)", statuses.len());
        }
        Ok(ExitCode::SUCCESS)
    } else {
        if format != "json" {
            eprintln!(
                "verify failed: {} repo(s) diverge from haw.lock",
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
    let (pre, post) = if build {
        (hooks::Hook::PreBuild, hooks::Hook::PostBuild)
    } else {
        (hooks::Hook::PreTest, hooks::Hook::PostTest)
    };
    fire_phase(&ws, pre, json!({"groups": groups}))?;
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
    fire_phase(&ws, post, json!({"failures": failures, "total": total}))?;
    if failures > 0 {
        bail!("{verb} failed in {failures} repo(s)");
    }
    Ok(())
}

fn hooks_install() -> Result<()> {
    let ws = open_workspace()?;
    let backend = ShellGit;
    let script = "#!/bin/sh\n# installed by `haw hooks install`\nhaw verify || {\n  echo 'haw: tree diverges from haw.lock (run haw sync or haw pin)' >&2\n  exit 1\n}\n";
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
    let mut any = false;
    for hook in hooks::Hook::ALL {
        let name = hook.name();
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
        std::fs::copy(ws.lock_path(), staging.join("haw.lock"))?;
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
            "schema": "haw.evidence/1",
            "tool": "haw",
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

/// Fire a lifecycle phase: run the `.haw/hooks/<phase>` script (if any) and
/// dispatch every `[plugins]` entry subscribed to it.
///
/// `pre-*` failures return `Err` (the caller aborts); `post-*` failures are
/// printed as warnings and swallowed. Missing plugin binaries are skipped
/// (fail-open). `extra` is merged into the plugin context for diagnostics.
fn fire_phase(ws: &Workspace, hook: hooks::Hook, extra: serde_json::Value) -> Result<()> {
    let is_pre = hook.is_pre();

    match hooks::fire(ws, hook, &extra) {
        Ok(()) => {}
        Err(err) if is_pre => return Err(err.into()),
        Err(err) => eprintln!("  ! {} hook: {err} (continuing)", hook.name()),
    }

    let subscriptions = &ws.manifest.plugins;
    if subscriptions.is_empty() {
        return Ok(());
    }

    let repos: Vec<RepoContext> = ws
        .manifest
        .repos
        .iter()
        .map(|(name, repo)| RepoContext {
            name: name.clone(),
            path: ws.root.join(repo.checkout_path(name)),
            rev: repo.rev.clone(),
            groups: repo.groups.clone(),
        })
        .collect();
    let mut context =
        plugin::phase_context(&ws.root, ws.current_stack().as_deref(), &repos, hook.name());
    if let (Some(obj), serde_json::Value::Object(extra)) = (context.as_object_mut(), extra) {
        for (key, value) in extra {
            obj.entry(key).or_insert(value);
        }
    }

    let c = Palette::new();
    let dispatches = plugin::dispatch(&ProcessRunner, subscriptions, hook.name(), &context);
    let mut blocked: Vec<String> = Vec::new();
    for dispatch in dispatches {
        match dispatch {
            Dispatch::Ran(report) => {
                let mark = if report.ok { c.ok("✓") } else { c.err("✗") };
                println!(
                    "  {mark} {} {}",
                    c.name(&report.plugin),
                    c.dim(&report.summary)
                );
                for finding in &report.findings {
                    println!("      [{}] {}", finding.level, finding.message);
                }
                if !report.ok && is_pre {
                    blocked.push(report.plugin);
                }
            }
            Dispatch::Missing { plugin } => {
                eprintln!(
                    "  {} {plugin} (no haw-{plugin} on PATH — skipped)",
                    c.dim("·")
                );
            }
            Dispatch::Unparseable { plugin, detail } => {
                eprintln!("  {} {plugin}: {detail}", c.err("!"));
            }
        }
    }
    if !blocked.is_empty() {
        bail!(
            "{} plugin(s) vetoed `{}`: {}",
            blocked.len(),
            hook.name(),
            blocked.join(", ")
        );
    }
    Ok(())
}

fn plugin(args: &[String]) -> Result<ExitCode> {
    let Some((name, rest)) = args.split_first() else {
        bail!("empty plugin invocation");
    };
    let binary = format!("haw-{name}");
    let context = match open_workspace() {
        Ok(ws) => json!({
            "schema": "haw.plugin/1",
            "root": ws.root.to_string_lossy(),
            "stack": ws.current_stack(),
            "repos": ws.manifest.repos.iter().map(|(repo_name, repo)| json!({
                "name": repo_name,
                "path": ws.root.join(repo.checkout_path(repo_name)).to_string_lossy(),
                "rev": repo.rev,
                "groups": repo.groups,
            })).collect::<Vec<_>>(),
        }),
        Err(_) => json!({"schema": "haw.plugin/1"}),
    };

    use std::io::Write;
    let mut child = std::process::Command::new(&binary)
        .args(rest)
        .env("HAW_JSON", context.to_string())
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

/// TUI controller: adapts cockpit actions to `haw-core`/`haw-forge`.
/// Runs on the TUI worker thread.
struct CliController;

impl CliController {
    fn workspace(&self) -> std::io::Result<Workspace> {
        open_workspace().map_err(std::io::Error::other)
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
        let ws = self.workspace()?;
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

    fn fleet_ci(&mut self) -> std::io::Result<Vec<haw_tui::FleetCiRun>> {
        let ws = self.workspace()?;
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
}

/// Resolve a repo's clone URL and a ready-to-call forge client, honoring the
/// manifest's explicit `forge =` key (mirrors `orchestrate::forge_hint`).
fn forge_for_repo(
    ws: &Workspace,
    repo: &str,
) -> std::io::Result<(Box<dyn haw_forge::Forge>, String)> {
    use haw_forge::ForgeFactory;
    let spec =
        ws.manifest.repos.get(repo).ok_or_else(|| {
            std::io::Error::other(format!("repo `{repo}` is not in the manifest"))
        })?;
    let url = spec.clone_url(&ws.manifest.remotes).ok_or_else(|| {
        std::io::Error::other(format!("repo `{repo}` has no resolvable clone URL"))
    })?;
    let hint = spec
        .remote
        .as_deref()
        .and_then(|name| ws.manifest.remotes.get(name))
        .and_then(|remote| remote.forge.as_deref())
        .and_then(haw_forge::kind_from_key);
    let tokens = Tokens::from_env();
    let forge = tokens
        .client_for(&url, hint)
        .map_err(std::io::Error::other)?;
    Ok((forge, url))
}

/// Run `git -C <path> <args...>`, returning trimmed stdout or an error note.
fn git_capture(path: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output();
    match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim_end().to_string()
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            format!("(git {}: {})", args.join(" "), err.trim())
        }
        Err(err) => format!("(git {}: {err})", args.join(" ")),
    }
}

/// Compose a readable, plain-text git report for one repo's checkout. Returns a
/// "not cloned" note (not an error) when the path holds no git repository.
fn git_detail_report(repo: &str, path: &Path) -> String {
    if !ShellGit.is_repo(path) {
        return format!(
            "== {repo} ==\n\nnot cloned at {}\n\nrun `haw sync` to clone it.",
            path.display()
        );
    }
    let sha = git_capture(path, &["rev-parse", "--short", "HEAD"]);
    let branch = git_capture(path, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let status = git_capture(path, &["status", "-sb"]);
    let log = git_capture(path, &["log", "--oneline", "--decorate", "-20"]);
    let show = git_capture(path, &["show", "--stat", "--oneline", "HEAD"]);
    let show: String = show.lines().take(40).collect::<Vec<_>>().join("\n");
    let remotes = git_capture(path, &["remote", "-v"]);

    let mut report = String::new();
    report.push_str(&format!("== {repo} ==\n"));
    report.push_str(&format!("branch {branch}  @ {sha}\n\n"));
    report.push_str("-- status --\n");
    report.push_str(&status);
    report.push_str("\n\n-- recent commits --\n");
    report.push_str(&log);
    report.push_str("\n\n-- last commit --\n");
    report.push_str(&show);
    report.push_str("\n\n-- remotes --\n");
    report.push_str(&remotes);
    report.push('\n');
    report
}

fn dash(demo: bool) -> Result<()> {
    let controller: Box<dyn haw_tui::Controller> = if demo {
        Box::new(DemoController)
    } else {
        open_workspace()?;
        Box::new(CliController)
    };
    if let Some(path) = haw_tui::run(controller)? {
        println!("{}", path.display());
    }
    Ok(())
}

/// A cockpit controller backed entirely by canned, in-memory data. It reaches
/// no workspace, git, or network, so `haw dash --demo` renders every view —
/// fleet, PR/MRs, CI, changesets, merges — deterministically for recordings.
struct DemoController;

impl DemoController {
    #[allow(clippy::too_many_arguments)]
    fn repo(
        name: &str,
        groups: &[&str],
        branch: Option<&str>,
        head: Option<&str>,
        dirty: bool,
        drift: bool,
        locked_rev: Option<&str>,
        ahead_behind: Option<(u64, u64)>,
        missing: bool,
    ) -> RepoStatus {
        RepoStatus {
            name: name.to_string(),
            path: PathBuf::from("repos").join(name),
            missing,
            branch: branch.map(str::to_string),
            head: head.map(str::to_string),
            dirty,
            locked_rev: locked_rev.map(str::to_string),
            drift,
            ahead_behind,
            groups: groups.iter().map(|g| g.to_string()).collect(),
        }
    }

    fn gateway_fleet() -> Vec<RepoStatus> {
        vec![
            Self::repo(
                "kernel",
                &["firmware"],
                Some("release/6.1"),
                Some("a1c9f4e2b7d80516"),
                false,
                false,
                Some("a1c9f4e2b7d80516"),
                Some((0, 0)),
                false,
            ),
            Self::repo(
                "hal",
                &["firmware"],
                Some("feature/i2c-dma"),
                Some("7f3b21d0e5a4c9b6"),
                true,
                false,
                Some("7f3b21d0e5a4c9b6"),
                Some((3, 1)),
                false,
            ),
            Self::repo(
                "app-mqtt",
                &["ci", "apps"],
                Some("main"),
                Some("d4e88a1c60b3f279"),
                false,
                true,
                Some("22aa77bc11ee9930"),
                Some((0, 4)),
                false,
            ),
            Self::repo(
                "telemetry",
                &["apps"],
                None,
                None,
                false,
                false,
                Some("55cc33ee11aa8842"),
                None,
                true,
            ),
        ]
    }

    fn sensor_fleet() -> Vec<RepoStatus> {
        vec![
            Self::repo(
                "kernel",
                &["firmware"],
                Some("release/6.1"),
                Some("a1c9f4e2b7d80516"),
                false,
                false,
                Some("a1c9f4e2b7d80516"),
                Some((0, 0)),
                false,
            ),
            Self::repo(
                "sensor-drv",
                &["firmware", "ci"],
                Some("main"),
                Some("9b0a1c2d3e4f5061"),
                false,
                false,
                Some("9b0a1c2d3e4f5061"),
                Some((0, 0)),
                false,
            ),
        ]
    }
}

impl haw_tui::Controller for DemoController {
    fn snapshot(&mut self) -> std::io::Result<haw_tui::Snapshot> {
        let paths = [
            "kernel",
            "hal",
            "app-mqtt",
            "telemetry",
            "sensor-drv",
            "edge-daemon",
        ]
        .iter()
        .map(|name| {
            (
                name.to_string(),
                PathBuf::from("/home/you/work/gateway").join(name),
            )
        })
        .collect();

        let changesets = vec![
            haw_tui::ChangesetSummary {
                id: "FEAT-42".to_string(),
                repos: vec![
                    haw_tui::ChangeRepoRow {
                        name: "kernel".to_string(),
                        branch: "change/FEAT-42".to_string(),
                        on_branch: true,
                        dirty: false,
                        head: Some("a1c9f4e2b7d80516".to_string()),
                        forge: "github".to_string(),
                        pr: "#128 ● open".to_string(),
                        ci: "✓ passed".to_string(),
                    },
                    haw_tui::ChangeRepoRow {
                        name: "hal".to_string(),
                        branch: "change/FEAT-42".to_string(),
                        on_branch: true,
                        dirty: true,
                        head: Some("7f3b21d0e5a4c9b6".to_string()),
                        forge: "gitlab".to_string(),
                        pr: "!47 ◐ review".to_string(),
                        ci: "⏳ running".to_string(),
                    },
                    haw_tui::ChangeRepoRow {
                        name: "app-mqtt".to_string(),
                        branch: "change/FEAT-42".to_string(),
                        on_branch: false,
                        dirty: false,
                        head: Some("d4e88a1c60b3f279".to_string()),
                        forge: "github".to_string(),
                        pr: "—".to_string(),
                        ci: "—".to_string(),
                    },
                ],
            },
            haw_tui::ChangesetSummary {
                id: "BUG-1187".to_string(),
                repos: vec![
                    haw_tui::ChangeRepoRow {
                        name: "sensor-drv".to_string(),
                        branch: "change/BUG-1187".to_string(),
                        on_branch: true,
                        dirty: false,
                        head: Some("9b0a1c2d3e4f5061".to_string()),
                        forge: "github".to_string(),
                        pr: "#91 ● merged".to_string(),
                        ci: "✓ passed".to_string(),
                    },
                    haw_tui::ChangeRepoRow {
                        name: "telemetry".to_string(),
                        branch: "change/BUG-1187".to_string(),
                        on_branch: true,
                        dirty: false,
                        head: Some("c0ffee1234567890".to_string()),
                        forge: "gitlab".to_string(),
                        pr: "!12 ✗ closed".to_string(),
                        ci: "✗ failed".to_string(),
                    },
                ],
            },
        ];

        let tree = "\
└─ gateway
   ├─ kernel      release/6.1
   ├─ hal         feature/i2c-dma
   ├─ app-mqtt    main
   └─ telemetry   main
├─ sensor-node
   ├─ kernel      release/6.1
   └─ sensor-drv  main"
            .to_string();

        Ok(haw_tui::Snapshot {
            root_label: "~/work/gateway".to_string(),
            stacks: vec!["gateway".to_string(), "sensor-node".to_string()],
            current_stack: Some("gateway".to_string()),
            fleet: vec![
                ("gateway".to_string(), Self::gateway_fleet()),
                ("sensor-node".to_string(), Self::sensor_fleet()),
            ],
            changesets,
            lock_present: true,
            paths,
            tree,
            merges: vec![(
                "hal".to_string(),
                haw_tui::MergeBadge {
                    source: "origin/feature/i2c-dma".to_string(),
                    resolved: 2,
                    total: 3,
                },
            )],
        })
    }

    fn changeset_prs(&mut self, id: &str) -> std::io::Result<haw_tui::ChangesetSummary> {
        self.snapshot()?
            .changesets
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| std::io::Error::other(format!("no changeset `{id}`")))
    }

    fn sync_stack(&mut self, stack: &str) -> std::io::Result<String> {
        Ok(format!("synced stack `{stack}` (4 repos, 0 failed)"))
    }

    fn sync_repo(&mut self, repo: &str) -> std::io::Result<String> {
        Ok(format!("synced `{repo}` — up to date"))
    }

    fn switch(&mut self, stack: &str) -> std::io::Result<String> {
        Ok(format!("switched to `{stack}` — synced (4 repos)"))
    }

    fn pin(&mut self) -> std::io::Result<String> {
        Ok("pinned haw.lock to current HEADs (6 repos)".to_string())
    }

    fn lock(&mut self) -> std::io::Result<String> {
        Ok("wrote haw.lock (6 repos pinned)".to_string())
    }

    fn run_cmd(&mut self, cmd: &str) -> std::io::Result<String> {
        Ok(format!(
            "$ {cmd}\n── kernel ──\nOK\n── hal ──\nOK\n── app-mqtt ──\nOK\nran in 3/3 repos"
        ))
    }

    fn change_start(&mut self, id: &str) -> std::io::Result<String> {
        Ok(format!("changeset `{id}` started across 3 repos"))
    }

    fn change_request(&mut self, id: &str, only: Option<Vec<String>>) -> std::io::Result<String> {
        let count = only.map_or(3, |repos| repos.len());
        Ok(format!("requested `{id}` ({count} PR/MRs, cross-linked)"))
    }

    fn change_land(&mut self, id: &str) -> std::io::Result<String> {
        Ok(format!("landed `{id}` (3 repos)"))
    }

    fn pr_merge(&mut self, repo: &str, number: u64) -> std::io::Result<String> {
        Ok(format!("merged {repo}#{number}"))
    }

    fn pr_approve(&mut self, repo: &str, number: u64) -> std::io::Result<String> {
        Ok(format!("approved {repo}#{number}"))
    }

    fn merge_cleanup(&mut self, repo: &str) -> std::io::Result<String> {
        Ok(format!(
            "merged 3 slice(s) into `main` on `{repo}` (e91f0a4c); dropped haw/merge branch"
        ))
    }

    fn merge_abort(&mut self, repo: &str) -> std::io::Result<String> {
        Ok(format!("aborted merge of `{repo}`; back on `main`"))
    }

    fn fleet_prs(&mut self) -> std::io::Result<Vec<haw_tui::FleetPr>> {
        let pr = |repo: &str,
                  forge: &str,
                  number: u64,
                  title: &str,
                  state: &str,
                  approved: bool,
                  ci: Option<bool>| haw_tui::FleetPr {
            repo: repo.to_string(),
            forge: forge.to_string(),
            number,
            title: title.to_string(),
            url: format!("https://{forge}.com/acme/{repo}/pull/{number}"),
            state: state.to_string(),
            approved,
            ci,
        };
        Ok(vec![
            pr(
                "kernel",
                "github",
                128,
                "i2c: add DMA-backed transfers",
                "open",
                true,
                Some(true),
            ),
            pr(
                "hal",
                "gitlab",
                47,
                "hal: wire i2c DMA descriptors",
                "draft",
                false,
                Some(false),
            ),
            pr(
                "app-mqtt",
                "github",
                214,
                "mqtt: reconnect backoff + jitter",
                "open",
                false,
                None,
            ),
            pr(
                "sensor-drv",
                "github",
                91,
                "drv: calibrate on cold boot",
                "merged",
                true,
                Some(true),
            ),
            pr(
                "telemetry",
                "gitlab",
                12,
                "telemetry: batch OTLP exports",
                "open",
                true,
                Some(true),
            ),
            pr(
                "edge-daemon",
                "github",
                8,
                "edge: graceful shutdown on SIGTERM",
                "draft",
                false,
                None,
            ),
        ])
    }

    fn fleet_ci(&mut self) -> std::io::Result<Vec<haw_tui::FleetCiRun>> {
        let run = |repo: &str, id: u64, name: &str, branch: &str, event: &str, status: &str| {
            haw_tui::FleetCiRun {
                repo: repo.to_string(),
                id,
                name: name.to_string(),
                branch: branch.to_string(),
                event: event.to_string(),
                status: status.to_string(),
                url: format!("https://github.com/acme/{repo}/actions/runs/{id}"),
            }
        };
        Ok(vec![
            run(
                "kernel",
                9001,
                "build-and-test",
                "release/6.1",
                "push",
                "passed",
            ),
            run(
                "hal",
                9002,
                "firmware-ci",
                "feature/i2c-dma",
                "pull_request",
                "running",
            ),
            run("app-mqtt", 9003, "integration", "main", "push", "failed"),
            run("telemetry", 9004, "lint", "main", "pull_request", "queued"),
            run(
                "sensor-drv",
                9005,
                "nightly",
                "main",
                "schedule",
                "cancelled",
            ),
            run("edge-daemon", 9006, "build", "main", "push", "passed"),
        ])
    }

    fn governance(&mut self) -> std::io::Result<haw_tui::Governance> {
        let plugin = |name: &str, phases: &[&str]| haw_tui::GovPlugin {
            name: name.to_string(),
            phases: phases.iter().map(|p| p.to_string()).collect(),
        };
        let artifact = |plugin: &str, kind: &str, path: &str, exists: bool| haw_tui::GovArtifact {
            plugin: plugin.to_string(),
            kind: kind.to_string(),
            path: path.to_string(),
            exists,
        };
        let finding = |plugin: &str, level: &str, message: &str| haw_tui::GovFinding {
            plugin: plugin.to_string(),
            level: level.to_string(),
            message: message.to_string(),
        };
        Ok(haw_tui::Governance {
            plugins: vec![
                plugin("haw-compliance", &["post-build"]),
                plugin("haw-artifact", &["post-land"]),
                plugin("haw-git-gate", &["pre-request"]),
            ],
            artifacts: vec![
                artifact("haw-compliance", "sbom", ".haw/sbom/kernel.cdx.json", true),
                artifact("haw-compliance", "sbom", ".haw/sbom/kernel.spdx.json", true),
                artifact(
                    "haw-artifact",
                    "provenance",
                    ".haw/provenance/kernel.intoto.jsonl",
                    true,
                ),
                artifact(
                    "haw-artifact",
                    "signature",
                    ".haw/provenance/kernel.sig",
                    false,
                ),
            ],
            findings: vec![
                finding("haw-compliance", "info", "SBOM generated for 4 repos"),
                finding("haw-git-gate", "warn", "no signer on PATH"),
            ],
        })
    }

    fn repo_detail(&mut self, repo: &str) -> std::io::Result<String> {
        Ok(format!(
            "== {repo} ==\n\
branch release/6.1  @ a1c9f4e\n\
\n\
-- status --\n\
## release/6.1...origin/release/6.1\n\
\n\
-- recent commits --\n\
a1c9f4e (HEAD -> release/6.1, origin/release/6.1) i2c: add DMA-backed transfers\n\
7f3b21d hal: wire i2c DMA descriptors\n\
d4e88a1 mqtt: reconnect backoff + jitter\n\
9b0a1c2 drv: calibrate on cold boot\n\
c0ffee1 build: bump toolchain to 1.79\n\
\n\
-- last commit --\n\
a1c9f4e i2c: add DMA-backed transfers\n\
 drivers/i2c/dma.c | 142 ++++++++++++++++++++++++++++\n\
 drivers/i2c/i2c.h |  12 +++\n\
 2 files changed, 154 insertions(+)\n\
\n\
-- remotes --\n\
origin\tgit@github.com:acme/{repo}.git (fetch)\n\
origin\tgit@github.com:acme/{repo}.git (push)\n"
        ))
    }

    fn pr_detail(&mut self, repo: &str, number: u64) -> std::io::Result<String> {
        Ok(format!(
            "#{number} i2c: add DMA-backed transfers — open\n\
head feature/i2c-dma @ a1c9f4e  ->  base release/6.1\n\
mergeable: yes\n\
\n\
-- reviewers --\n\
  octavia: APPROVED\n\
  rui: CHANGES_REQUESTED\n\
\n\
-- checks --\n\
  build-and-test: completed/success\n\
  clippy: completed/success\n\
  integration: completed/failure\n\
\n\
-- body --\n\
Adds DMA-backed transfers to the {repo} i2c driver.\n\
\n\
- new descriptor ring in drivers/i2c/dma.c\n\
- falls back to PIO when no channel is free\n\
- part of changeset FEAT-42\n\
\n\
url: https://github.com/acme/{repo}/pull/{number}\n"
        ))
    }

    fn ci_detail(&mut self, repo: &str, run_id: u64) -> std::io::Result<String> {
        Ok(format!(
            "build-and-test — completed/success\n\
branch release/6.1  event push  @ a1c9f4e\n\
\n\
-- jobs --\n\
  build: completed/success\n\
    - checkout: success\n\
    - configure: success\n\
    - compile: success\n\
  test: completed/success\n\
    - checkout: success\n\
    - unit: success\n\
    - integration: success\n\
  lint: completed/success\n\
    - clippy: success\n\
    - fmt: success\n\
\n\
url: https://github.com/acme/{repo}/actions/runs/{run_id}\n"
        ))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod demo_controller_tests {
    use super::*;
    use haw_tui::Controller;

    #[test]
    fn snapshot_has_expected_shape() {
        let mut controller = DemoController;
        let snapshot = controller.snapshot().expect("demo snapshot");
        assert_eq!(snapshot.stacks.len(), 2);
        assert_eq!(snapshot.fleet.len(), 2);
        let gateway = &snapshot
            .fleet
            .iter()
            .find(|(name, _)| name == "gateway")
            .expect("gateway stack")
            .1;
        assert_eq!(gateway.len(), 4);
        assert!(gateway.iter().any(|r| r.missing));
        assert!(gateway.iter().any(|r| r.drift));
        assert!(gateway.iter().any(|r| r.dirty));
        assert_eq!(snapshot.changesets.len(), 2);
        assert_eq!(snapshot.merges.len(), 1);
        assert!(snapshot.lock_present);
    }

    #[test]
    fn fleet_views_are_populated() {
        let mut controller = DemoController;
        assert!(!controller.fleet_prs().expect("prs").is_empty());
        assert!(!controller.fleet_ci().expect("ci").is_empty());
    }
}
