//! The haw cockpit: a k9s-style, keyboard-first ratatui dashboard.
//!
//! Views: stacks -> fleet grid -> repo detail, changesets -> changeset grid,
//! tree, help overlay. `/` filters the grid, `:` opens a command bar whose
//! verbs mirror the CLI. Actions run on a worker thread so the UI never
//! freezes; a spinner shows progress.
//!
//! All domain work goes through the [`Controller`] trait — this crate renders
//! and dispatches, nothing more.

use std::cell::RefCell;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use haw_core::workspace::RepoStatus;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table,
    TableState,
};

/// The cockpit skin: k9s-style selectable palettes.
///
/// The default is Catppuccin-Mocha-leaning, chosen to read on dark terms.
/// Rendering runs single-threaded on the main loop, so a `thread_local`
/// current theme is both correct and cheap. Every `draw_*` helper reads the
/// active palette through the accessor fns (`theme::accent()`, ...).
mod theme {
    use ratatui::style::Color;
    use std::cell::RefCell;

    /// A full cockpit palette. All fields are plain [`Color`]s so a palette can
    /// mix RGB (rich terminals) and named ANSI (NO_COLOR / light terms).
    #[derive(Debug, Clone, Copy)]
    pub struct Theme {
        pub accent: Color,
        pub mauve: Color,
        pub green: Color,
        pub yellow: Color,
        pub red: Color,
        pub teal: Color,
        pub peach: Color,
        pub text: Color,
        pub dim: Color,
        pub surface: Color,
        pub surface0: Color,
        pub crust: Color,
    }

    impl Theme {
        /// Catppuccin Mocha — the historical cockpit palette and the default.
        pub const fn catppuccin() -> Self {
            Self {
                accent: Color::Rgb(137, 180, 250),
                mauve: Color::Rgb(203, 166, 247),
                green: Color::Rgb(166, 227, 161),
                yellow: Color::Rgb(249, 226, 175),
                red: Color::Rgb(243, 139, 168),
                teal: Color::Rgb(148, 226, 213),
                peach: Color::Rgb(250, 179, 135),
                text: Color::Rgb(205, 214, 244),
                dim: Color::Rgb(127, 132, 156),
                surface: Color::Rgb(69, 71, 90),
                surface0: Color::Rgb(49, 50, 68),
                crust: Color::Rgb(17, 17, 27),
            }
        }

        /// Dracula.
        pub const fn dracula() -> Self {
            Self {
                accent: Color::Rgb(139, 233, 253),
                mauve: Color::Rgb(189, 147, 249),
                green: Color::Rgb(80, 250, 123),
                yellow: Color::Rgb(241, 250, 140),
                red: Color::Rgb(255, 85, 85),
                teal: Color::Rgb(139, 233, 253),
                peach: Color::Rgb(255, 184, 108),
                text: Color::Rgb(248, 248, 242),
                dim: Color::Rgb(98, 114, 164),
                surface: Color::Rgb(68, 71, 90),
                surface0: Color::Rgb(40, 42, 54),
                crust: Color::Rgb(30, 31, 41),
            }
        }

        /// Nord.
        pub const fn nord() -> Self {
            Self {
                accent: Color::Rgb(136, 192, 208),
                mauve: Color::Rgb(180, 142, 173),
                green: Color::Rgb(163, 190, 140),
                yellow: Color::Rgb(235, 203, 139),
                red: Color::Rgb(191, 97, 106),
                teal: Color::Rgb(143, 188, 187),
                peach: Color::Rgb(208, 135, 112),
                text: Color::Rgb(216, 222, 233),
                dim: Color::Rgb(97, 110, 136),
                surface: Color::Rgb(67, 76, 94),
                surface0: Color::Rgb(59, 66, 82),
                crust: Color::Rgb(46, 52, 64),
            }
        }

        /// Gruvbox (dark).
        pub const fn gruvbox() -> Self {
            Self {
                accent: Color::Rgb(131, 165, 152),
                mauve: Color::Rgb(211, 134, 155),
                green: Color::Rgb(184, 187, 38),
                yellow: Color::Rgb(250, 189, 47),
                red: Color::Rgb(251, 73, 52),
                teal: Color::Rgb(142, 192, 124),
                peach: Color::Rgb(254, 128, 25),
                text: Color::Rgb(235, 219, 178),
                dim: Color::Rgb(146, 131, 116),
                surface: Color::Rgb(80, 73, 69),
                surface0: Color::Rgb(60, 56, 54),
                crust: Color::Rgb(40, 40, 40),
            }
        }

        /// Solarized (dark).
        pub const fn solarized() -> Self {
            Self {
                accent: Color::Rgb(38, 139, 210),
                mauve: Color::Rgb(108, 113, 196),
                green: Color::Rgb(133, 153, 0),
                yellow: Color::Rgb(181, 137, 0),
                red: Color::Rgb(220, 50, 47),
                teal: Color::Rgb(42, 161, 152),
                peach: Color::Rgb(203, 75, 22),
                text: Color::Rgb(147, 161, 161),
                dim: Color::Rgb(88, 110, 117),
                surface: Color::Rgb(7, 54, 66),
                surface0: Color::Rgb(0, 43, 54),
                crust: Color::Rgb(0, 33, 43),
            }
        }

        /// Monochrome — no RGB reliance. Uses `Color::Reset` and named ANSI so
        /// it reads on `NO_COLOR` and light terminals. Pass/fail status still
        /// maps to named green/yellow/red for legibility.
        pub const fn monochrome() -> Self {
            Self {
                accent: Color::White,
                mauve: Color::White,
                green: Color::Green,
                yellow: Color::Yellow,
                red: Color::Red,
                teal: Color::White,
                peach: Color::White,
                text: Color::Reset,
                dim: Color::Gray,
                surface: Color::Gray,
                surface0: Color::Reset,
                crust: Color::Reset,
            }
        }

        /// Look up a built-in theme by name (case-insensitive).
        pub fn by_name(name: &str) -> Option<Self> {
            match name.trim().to_ascii_lowercase().as_str() {
                "catppuccin" => Some(Self::catppuccin()),
                "dracula" => Some(Self::dracula()),
                "nord" => Some(Self::nord()),
                "gruvbox" => Some(Self::gruvbox()),
                "solarized" => Some(Self::solarized()),
                "monochrome" => Some(Self::monochrome()),
                _ => None,
            }
        }
    }

    /// Names of all built-in themes, in listing order.
    pub const THEMES: &[&str] = &[
        "catppuccin",
        "dracula",
        "nord",
        "gruvbox",
        "solarized",
        "monochrome",
    ];

    thread_local! {
        static CURRENT: RefCell<Theme> = const { RefCell::new(Theme::catppuccin()) };
    }

    /// Replace the active palette. Takes effect on the next render.
    pub fn set(t: Theme) {
        CURRENT.with(|c| *c.borrow_mut() = t);
    }

    macro_rules! accessor {
        ($($f:ident),* $(,)?) => {
            $(pub fn $f() -> Color { CURRENT.with(|t| t.borrow().$f) })*
        };
    }

    accessor!(
        accent, mauve, green, yellow, red, teal, peach, text, dim, surface, surface0, crust,
    );
}

pub use theme::{THEMES, Theme};

/// Why the cockpit exited with a repo path in hand: the caller either prints
/// the path (`cd "$(haw dash)"`) or drops the user into a shell there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exit {
    /// The user asked to `goto` a repo — print the path.
    Goto(PathBuf),
    /// The user asked to drop into a shell in a repo.
    Shell(PathBuf),
}

/// One entry in the file browser's current directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
}

/// One file changed by a PR/MR, for the `View::PrFiles` list. `status` is a
/// short change label (`added`/`modified`/`removed`/`renamed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrFileEntry {
    pub path: String,
    pub status: String,
}

/// Snapshot restored when returning from a PR file's content into the PR files
/// browser: the PR context (`repo`, `number`, `title`) plus the cached listing.
type PrFilesReturn = ((String, u64, String), Option<Vec<PrFileEntry>>);

/// One repo of a changeset, with its rendered PR/CI cells.
#[derive(Debug, Clone)]
pub struct ChangeRepoRow {
    pub name: String,
    pub branch: String,
    pub on_branch: bool,
    pub dirty: bool,
    pub head: Option<String>,
    /// `github`/`gitlab`/`—`, detected from the repo's remote URL.
    pub forge: String,
    /// Rendered PR/MR cell (`#128 ● open`), `—` before `change request`.
    pub pr: String,
    /// Rendered CI cell (`✓ passed`, `⏳ running`, `—`).
    pub ci: String,
}

/// One changeset and its repos.
#[derive(Debug, Clone)]
pub struct ChangesetSummary {
    pub id: String,
    pub repos: Vec<ChangeRepoRow>,
}

/// Full data refresh for the cockpit.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub root_label: String,
    pub stacks: Vec<String>,
    pub current_stack: Option<String>,
    /// stack -> repo statuses.
    pub fleet: Vec<(String, Vec<RepoStatus>)>,
    pub changesets: Vec<ChangesetSummary>,
    pub lock_present: bool,
    /// repo name -> absolute checkout path (for goto).
    pub paths: Vec<(String, PathBuf)>,
    /// Rendered `haw tree` output for the tree view.
    pub tree: String,
    /// repo name -> its planned collaborative merge, if any (Phase 6).
    pub merges: Vec<(String, MergeBadge)>,
}

/// A repo's in-progress `haw merge` (see `haw-merge`), just enough to
/// render a badge — the TUI stays free of a `haw-merge` dependency.
#[derive(Debug, Clone)]
pub struct MergeBadge {
    pub source: String,
    pub resolved: usize,
    pub total: usize,
}

/// One open PR/MR for the fleet-wide PR/MR view (`m`).
#[derive(Debug, Clone)]
pub struct FleetPr {
    pub repo: String,
    /// `github`/`gitlab`.
    pub forge: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    /// Rendered state: `open`/`draft`/`merged`/`closed`.
    pub state: String,
    pub approved: bool,
    /// `None` while CI is pending or absent.
    pub ci: Option<bool>,
}

/// One registered plugin for the governance view (`v`).
#[derive(Debug, Clone)]
pub struct GovPlugin {
    pub name: String,
    /// Lifecycle phases the plugin subscribes to (e.g. `post-build`).
    pub phases: Vec<String>,
}

/// One artifact a plugin produced or is expected to produce.
#[derive(Debug, Clone)]
pub struct GovArtifact {
    pub plugin: String,
    /// `sbom`/`provenance`/`signature`/…
    pub kind: String,
    /// Path to the artifact, relative to the workspace root.
    pub path: String,
    /// Whether the artifact currently exists on disk.
    pub exists: bool,
}

/// One finding a plugin surfaced.
#[derive(Debug, Clone)]
pub struct GovFinding {
    pub plugin: String,
    /// `info`/`warn`/`error`.
    pub level: String,
    pub message: String,
}

/// The plugin/governance surface for the governance view (`v`).
#[derive(Debug, Clone, Default)]
pub struct Governance {
    pub plugins: Vec<GovPlugin>,
    pub artifacts: Vec<GovArtifact>,
    pub findings: Vec<GovFinding>,
}

/// One available plugin panel for the `View::Plugins` list (`P`). Sourced from
/// the manifest `[plugins]` keys unioned with `haw-*` executables on PATH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPanel {
    /// The plugin's bare name (`sbom` for `haw-sbom`).
    pub name: String,
    /// Lifecycle phases the plugin subscribes to per the manifest (empty when
    /// the plugin is only discovered on PATH, not registered).
    pub phases: Vec<String>,
}

/// One failed operation, kept in the rolling session error log (`E`). Wall-clock
/// time is unavailable here, so `when_seq` is a monotonic counter that also
/// orders the log newest-first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorEntry {
    /// Monotonic sequence number; higher is newer.
    pub when_seq: u64,
    /// What was being attempted (e.g. `PR/MR fetch`, `sync`).
    pub context: String,
    /// The failure message.
    pub message: String,
}

/// One cross-repo grep hit for the `:grep` command / `View::Grep` list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepHit {
    pub repo: String,
    /// Repo-relative path of the matching file.
    pub path: String,
    pub line: u32,
    pub text: String,
}

/// Parse one `git grep -n` output line — `<relpath>:<line>:<text>` — into a
/// [`GrepHit`] for `repo`. Returns `None` when the line lacks the expected
/// `path:line:` prefix (the text field itself may contain further colons).
pub fn parse_grep_line(repo: &str, raw: &str) -> Option<GrepHit> {
    let (path, rest) = raw.split_once(':')?;
    let (line, text) = rest.split_once(':')?;
    let line: u32 = line.parse().ok()?;
    Some(GrepHit {
        repo: repo.to_string(),
        path: path.to_string(),
        line,
        text: text.to_string(),
    })
}

/// One CI run/pipeline for the fleet-wide CI view (`i`).
#[derive(Debug, Clone)]
pub struct FleetCiRun {
    pub repo: String,
    /// Run/pipeline id, used to fetch its drill-in detail.
    pub id: u64,
    pub name: String,
    pub branch: String,
    pub event: String,
    /// Rendered status: `passed`/`failed`/`running`/`queued`/`cancelled`.
    pub status: String,
    pub url: String,
}

/// Everything the cockpit can ask the application to do. Implementations run
/// on a worker thread, so they must be `Send`.
pub trait Controller: Send {
    fn snapshot(&mut self) -> io::Result<Snapshot>;
    /// PR/CI cells for one changeset (network; fetched on drill-in).
    fn changeset_prs(&mut self, id: &str) -> io::Result<ChangesetSummary>;
    fn sync_stack(&mut self, stack: &str) -> io::Result<String>;
    fn sync_repo(&mut self, repo: &str) -> io::Result<String>;
    /// Sync a specific marked set of repos (fleet bulk action).
    fn sync_repos(&mut self, repos: &[String]) -> io::Result<String>;
    fn switch(&mut self, stack: &str) -> io::Result<String>;
    fn pin(&mut self) -> io::Result<String>;
    fn lock(&mut self) -> io::Result<String>;
    fn run_cmd(&mut self, cmd: &str) -> io::Result<String>;
    /// Run `cmd` across a marked set of repos only (fleet bulk action).
    fn run_cmd_in(&mut self, cmd: &str, repos: &[String]) -> io::Result<String>;
    fn change_start(&mut self, id: &str) -> io::Result<String>;
    fn change_request(&mut self, id: &str, only: Option<Vec<String>>) -> io::Result<String>;
    fn change_land(&mut self, id: &str) -> io::Result<String>;
    /// Merge one open PR/MR on its forge (fleet PR view / PR drill-in).
    fn pr_merge(&mut self, repo: &str, number: u64) -> io::Result<String>;
    /// Approve one open PR/MR on its forge (fleet PR view / PR drill-in).
    fn pr_approve(&mut self, repo: &str, number: u64) -> io::Result<String>;
    /// Fetch a PR/MR's branch into the repo worktree and check it out locally.
    fn pr_checkout(&mut self, repo: &str, number: u64) -> io::Result<String>;
    /// Seal a fully-resolved merge plan for `repo` (see `haw merge cleanup`).
    fn merge_cleanup(&mut self, repo: &str) -> io::Result<String>;
    /// Abort a planned merge for `repo` (see `haw merge abort`).
    fn merge_abort(&mut self, repo: &str) -> io::Result<String>;
    /// Every open PR/MR across the fleet (network; fetched on entering `m`).
    fn fleet_prs(&mut self) -> io::Result<Vec<FleetPr>>;
    /// Recent CI runs/pipelines across the fleet (network; fetched on `i`).
    fn fleet_ci(&mut self) -> io::Result<Vec<FleetCiRun>>;
    /// Fleet PR/MRs, honoring a controller-side cache. `force` (a manual `m`
    /// refetch) bypasses the cache; the default ignores it and always fetches.
    fn fleet_prs_refresh(&mut self, _force: bool) -> io::Result<Vec<FleetPr>> {
        self.fleet_prs()
    }
    /// Fleet CI runs, honoring a controller-side cache. `force` (a manual `i`
    /// refetch) bypasses the cache; the default ignores it and always fetches.
    fn fleet_ci_refresh(&mut self, _force: bool) -> io::Result<Vec<FleetCiRun>> {
        self.fleet_ci()
    }
    /// The plugin/governance surface (read-only; fetched on entering `v`).
    fn governance(&mut self) -> io::Result<Governance>;
    /// Available plugin panels: manifest `[plugins]` keys unioned with `haw-*`
    /// executables on PATH, deduped (fetched on entering `View::Plugins`).
    fn plugin_panels(&mut self) -> io::Result<Vec<PluginPanel>>;
    /// Run one plugin in a render intent (`HAW_RENDER=1`, `"intent":"render"`)
    /// and return the text panel to show in the shared detail view.
    fn plugin_render(&mut self, name: &str) -> io::Result<String>;
    /// A live, plain-text git detail report for one repo (drill-in on `Enter`).
    fn repo_detail(&mut self, repo: &str) -> io::Result<String>;
    /// A plain-text drill-in report for one PR/MR (reviewers, checks, body).
    fn pr_detail(&mut self, repo: &str, number: u64) -> io::Result<String>;
    /// A plain-text drill-in report for one CI run/pipeline (jobs, steps).
    fn ci_detail(&mut self, repo: &str, run_id: u64) -> io::Result<String>;
    /// The unified diff for one PR/MR as plain text (scrollable detail view).
    fn pr_diff(&mut self, repo: &str, number: u64) -> io::Result<String>;
    /// The files changed by one PR/MR (path + status), for the PR files browser.
    fn pr_files(&mut self, repo: &str, number: u64) -> io::Result<Vec<PrFileEntry>>;
    /// The full content of one changed file, AT THE PR's head ref (detail view).
    fn pr_file_content(&mut self, repo: &str, number: u64, path: &str) -> io::Result<String>;
    /// The CI run/pipeline's job logs as plain text (scrollable detail view).
    fn ci_logs(&mut self, repo: &str, run_id: u64) -> io::Result<String>;
    /// Cross-repo grep: run `git grep -n <pattern>` in each cloned repo of the
    /// stack (or the whole fleet when `stack` is `None`), returning every hit.
    fn grep(&mut self, pattern: &str, stack: Option<&str>) -> io::Result<Vec<GrepHit>>;
    /// `git fetch` one repo's default remote (distinct from `sync`).
    fn repo_fetch(&mut self, repo: &str) -> io::Result<String>;
    /// Run a shell command in one repo's checkout dir (combined stdout+stderr).
    fn exec_in(&mut self, repo: &str, cmd: &str) -> io::Result<String>;
    /// List `subpath` ("" = root) of `repo`'s tree, on local disk or the forge.
    fn repo_tree(&mut self, repo: &str, subpath: &str, remote: bool) -> io::Result<Vec<FileEntry>>;
    /// The text content of `path` in `repo`, from local disk or the forge.
    fn file_content(&mut self, repo: &str, path: &str, remote: bool) -> io::Result<String>;
}

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The single source of truth mapping each sortable column (the index the
/// row comparators in `fleet_rows`/`pr_rows`/`ci_rows` switch on) to the
/// rendered header-cell index the caret must sit on. The slice length is the
/// sortable-column count for the view; an empty slice means "not sortable".
///
/// Fleet comparator cols: name, branch, head, dirty, drift, ahead_behind.
/// Fleet header cells:     "", REPO, GROUPS, BRANCH, HEAD, DIRTY, DRIFT, ↑/↓, MERGE.
/// PR comparator cols:     repo, number, title, state.
/// PR header cells:        REPO, FORGE, #, TITLE, STATE, APPR, CI.
/// CI comparator cols:     repo, name, branch, status.
/// CI header cells:        REPO, WORKFLOW, BRANCH, EVENT, STATUS.
fn sort_header_map(view: View) -> &'static [usize] {
    match view {
        View::Fleet => &[1, 3, 4, 5, 6, 7],
        View::Prs => &[0, 2, 3, 4],
        View::Ci => &[0, 1, 2, 4],
        _ => &[],
    }
}

/// The header-cell index + direction the sort caret should render on for the
/// current `sort` state, or `None` when nothing is sorted / the view is not
/// sortable. Derived from [`sort_header_map`] so the caret can never drift from
/// the comparator.
fn sort_caret(view: View, sort: Option<(u16, bool)>) -> Option<(usize, bool)> {
    let map = sort_header_map(view);
    sort.and_then(|(col, desc)| map.get(col as usize).map(|&idx| (idx, desc)))
}

/// Whether a view is a top-level peer reachable by a bare letter switch
/// (`t`/`c`/`m`/`i`/`v`/`P`/`E`/`S`). Switching between peers resets the
/// back-stack to root (Fleet); it is not a drill-in.
fn is_top_level(view: View) -> bool {
    matches!(
        view,
        View::Fleet
            | View::Stacks
            | View::Changesets
            | View::Prs
            | View::Ci
            | View::Governance
            | View::Plugins
            | View::Tree
            | View::Errors
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum View {
    Stacks,
    Fleet,
    Changesets,
    Changeset,
    Tree,
    Prs,
    Ci,
    Governance,
    Plugins,
    Errors,
    RepoDetail,
    PrDetail,
    CiDetail,
    Files,
    PrFiles,
    Grep,
}

#[derive(Debug, Clone, PartialEq)]
enum InputMode {
    None,
    Filter(String),
    Command(String),
    NewChangeset(String),
    /// The `!` single-repo shell prompt; carries the target repo + typed cmd.
    Exec(String, String),
}

enum Job {
    Refresh,
    ChangesetPrs(String),
    /// `force` bypasses the controller's TTL cache (a manual `m` refetch).
    FleetPrs {
        force: bool,
    },
    /// `force` bypasses the controller's TTL cache (a manual `i` refetch).
    FleetCi {
        force: bool,
    },
    Governance,
    /// List the available plugin panels (manifest ∪ PATH).
    PluginPanels,
    /// Render one plugin's panel into the shared detail view.
    PluginRender(String),
    RepoDetail(String),
    PrDetail(String, u64),
    CiDetail(String, u64),
    PrDiff(String, u64),
    CiLogs(String, u64),
    /// (repo, number) — list the files a PR/MR changed.
    PrFiles(String, u64),
    /// (repo, number, path, title) — fetch one changed file's content at the PR ref.
    PrFileContent(String, u64, String, String),
    /// (repo, subpath, remote) — list a directory in the file browser.
    RepoTree(String, String, bool),
    /// (repo, path, remote, title) — fetch one file's content into the detail view.
    FileContent(String, String, bool, String),
    /// (pattern, stack) — cross-repo grep on the worker thread.
    Grep(String, Option<String>),
    Action(&'static str, ActionKind),
}

enum ActionKind {
    SyncStack(String),
    SyncRepo(String),
    SyncRepos(Vec<String>),
    Switch(String),
    Pin,
    Lock,
    Run(String),
    RunRepos(String, Vec<String>),
    ChangeStart(String),
    ChangeRequest(String, Option<Vec<String>>),
    ChangeLand(String),
    MergePr(String, u64),
    ApprovePr(String, u64),
    CheckoutPr(String, u64),
    MergeCleanup(String),
    MergeAbort(String),
    /// `git fetch` one repo, then the outcome loop refreshes.
    RepoFetch(String),
    /// Run a shell command in one repo, showing output in the detail view.
    Exec(String, String),
}

enum Outcome {
    Snapshot(Box<io::Result<Snapshot>>),
    ChangesetPrs(Box<io::Result<ChangesetSummary>>),
    FleetPrs(Box<io::Result<Vec<FleetPr>>>),
    FleetCi(Box<io::Result<Vec<FleetCiRun>>>),
    Governance(Box<io::Result<Governance>>),
    /// The available plugin panels list.
    PluginPanels(Box<io::Result<Vec<PluginPanel>>>),
    /// A shared drill-in detail (repo git / PR / CI); carries its panel title.
    Detail(String, Box<io::Result<String>>),
    /// A file browser directory listing.
    Tree(Box<io::Result<Vec<FileEntry>>>),
    /// A PR/MR's changed-files listing.
    PrFiles(Box<io::Result<Vec<PrFileEntry>>>),
    /// Cross-repo grep results.
    Grep(Box<io::Result<Vec<GrepHit>>>),
    Action(&'static str, io::Result<String>),
}

struct App {
    view: View,
    back: Vec<View>,
    snapshot: Snapshot,
    stack: Option<String>,
    changeset: Option<String>,
    selected_repos: Vec<String>,
    /// Active column sort for the Fleet/Prs/Ci tables: `(column index, descending)`.
    /// Reset on view change / back.
    sort: Option<(u16, bool)>,
    cursor: ListState,
    input: InputMode,
    filter: String,
    message: String,
    busy: Option<&'static str>,
    spinner: usize,
    /// Free-running frame counter; paces the input cursor blink.
    tick: u64,
    help: bool,
    /// Scroll offset for the `?` help overlay (j/k/PgUp/PgDn).
    help_scroll: u16,
    /// Set when the user asked to leave the cockpit into a repo (goto/shell).
    exit: Option<Exit>,
    /// File browser: the repo whose tree is open, `None` outside `View::Files`.
    files_repo: Option<String>,
    /// File browser: the current subpath under the repo root ("" = root).
    files_subpath: String,
    /// File browser: whether the tree is the forge view (else local disk).
    files_remote: bool,
    /// File browser: the current directory listing; `None` while loading.
    files_entries: Option<Vec<FileEntry>>,
    /// Set when an action with real side effects (land, request) awaits y/n.
    pending_confirm: Option<Confirm>,
    /// Full multi-repo output from the last `r`/`:run`, shown as a dismissable overlay.
    output: Option<String>,
    /// Fleet-wide open PR/MRs; `None` until first fetched (`m` view).
    prs: Option<Vec<FleetPr>>,
    /// Fleet-wide recent CI runs; `None` until first fetched (`i` view).
    ci: Option<Vec<FleetCiRun>>,
    /// Plugin/governance surface; `None` until first fetched (`v` view).
    gov: Option<Governance>,
    /// Plain-text report for the shared scrollable detail view (repo git / PR /
    /// CI); `None` while loading.
    detail_text: Option<String>,
    /// Panel title + crumb label for the shared detail view.
    detail_title: String,
    /// Scroll offset for the shared detail view.
    detail_scroll: u16,
    /// Scroll offset to apply once the next `Outcome::Detail` text loads
    /// (grep jump-to-line). Cleared after it is consumed. `None` = scroll to 0.
    pending_scroll: Option<usize>,
    /// A snapshot of the file browser (`repo`, `subpath`, `entries`) captured
    /// when drilling from Files into the detail viewer, so `go_back` into Files
    /// restores the exact listing instead of wiping it.
    files_return: Option<(String, String, Option<Vec<FileEntry>>)>,
    /// The PR currently drilled into (`repo`, `number`, `title`), so the
    /// PR detail view can merge/approve it; `None` outside a PR drill-in.
    detail_pr: Option<(String, u64, String)>,
    /// The CI run currently drilled into (`repo`, `run_id`, `name`), so the CI
    /// detail view can open its logs; `None` outside a CI drill-in.
    detail_ci: Option<(String, u64, String)>,
    /// Rows visible in the last-drawn table body — the page step for
    /// PageDown/PageUp/Ctrl-d/Ctrl-u. Updated each frame; defaults to a sane
    /// fallback before the first draw.
    page_size: usize,
    /// Fleet "problems only" filter (`p`): show only rows with `has_problem()`.
    problems_only: bool,
    /// Cross-repo grep results for `View::Grep`; `None` while loading.
    grep_hits: Option<Vec<GrepHit>>,
    /// The pattern of the last `:grep`, for the panel title.
    grep_pattern: String,
    /// Available plugin panels for `View::Plugins`; `None` until first fetched.
    panels: Option<Vec<PluginPanel>>,
    /// Changed files of the PR the `View::PrFiles` browser is showing; `None`
    /// while loading.
    pr_file_entries: Option<Vec<PrFileEntry>>,
    /// The PR whose changed files are open in `View::PrFiles` (`repo`, `number`,
    /// `title`); `None` outside that view.
    pr_files_pr: Option<(String, u64, String)>,
    /// Snapshot of the PR files browser (`pr` tuple + entries) captured when
    /// drilling into a file's content, so `go_back` into `View::PrFiles` restores
    /// the PR context and the exact listing.
    pr_files_return: Option<PrFilesReturn>,
    /// Rolling session error log (newest last); capped at [`ERROR_LOG_CAP`].
    errors: Vec<ErrorEntry>,
    /// Monotonic counter stamping each [`ErrorEntry`] (wall-clock unavailable).
    error_seq: u64,
}

/// Cap on the rolling session error log — the last N failures are kept.
const ERROR_LOG_CAP: usize = 100;

thread_local! {
    /// A reusable fuzzy matcher; kept per-thread so filtering never re-allocates
    /// its scratch buffers on each keystroke.
    static MATCHER: RefCell<Matcher> = RefCell::new(Matcher::new(Config::DEFAULT));
}

/// Fuzzy match, case-insensitive; an empty needle matches everything. A
/// superset of substring matching, so plain prefixes/substrings still hit.
fn hit(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    MATCHER.with(|cell| {
        let mut matcher = cell.borrow_mut();
        let pattern = Pattern::parse(needle, CaseMatching::Ignore, Normalization::Smart);
        let mut buf = Vec::new();
        let haystack = Utf32Str::new(haystack, &mut buf);
        pattern
            .score(haystack, &mut matcher)
            .is_some_and(|score| score > 0)
    })
}

/// A repo matches a filter if its name or any of its groups contains it.
fn repo_matches(name: &str, groups: &[String], filter: &str) -> bool {
    filter.is_empty() || hit(name, filter) || groups.iter().any(|g| hit(g, filter))
}

impl App {
    fn fleet_rows(&self) -> Vec<&RepoStatus> {
        let stack = self.stack.as_deref().unwrap_or_default();
        let mut rows: Vec<&RepoStatus> = self
            .snapshot
            .fleet
            .iter()
            .find(|(name, _)| name == stack)
            .map(|(_, repos)| {
                repos
                    .iter()
                    .filter(|r| repo_matches(&r.name, &r.groups, &self.filter))
                    .filter(|r| !self.problems_only || has_problem(r))
                    .collect()
            })
            .unwrap_or_default();
        if let Some((col, desc)) = self.sort {
            rows.sort_by(|a, b| {
                let ord = match col {
                    0 => a.name.cmp(&b.name),
                    1 => a.branch.cmp(&b.branch),
                    2 => a.head.cmp(&b.head),
                    3 => a.dirty.cmp(&b.dirty),
                    4 => a.drift.cmp(&b.drift),
                    _ => a.ahead_behind.cmp(&b.ahead_behind),
                };
                if desc { ord.reverse() } else { ord }
            });
        }
        rows
    }

    fn stack_rows(&self) -> Vec<&str> {
        self.snapshot
            .stacks
            .iter()
            .map(String::as_str)
            .filter(|s| hit(s, &self.filter))
            .collect()
    }

    fn changeset_rows(&self) -> Vec<&ChangesetSummary> {
        self.snapshot
            .changesets
            .iter()
            .filter(|c| hit(&c.id, &self.filter))
            .collect()
    }

    fn current_changeset(&self) -> Option<&ChangesetSummary> {
        let id = self.changeset.as_deref()?;
        self.snapshot.changesets.iter().find(|c| c.id == id)
    }

    fn change_repo_rows(&self) -> Vec<&ChangeRepoRow> {
        self.current_changeset()
            .map(|c| {
                c.repos
                    .iter()
                    .filter(|r| hit(&r.name, &self.filter))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn pr_rows(&self) -> Vec<&FleetPr> {
        let mut rows: Vec<&FleetPr> = self
            .prs
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|p| hit(&p.repo, &self.filter) || hit(&p.title, &self.filter))
            .collect();
        if let Some((col, desc)) = self.sort {
            rows.sort_by(|a, b| {
                let ord = match col {
                    0 => a.repo.cmp(&b.repo),
                    1 => a.number.cmp(&b.number),
                    2 => a.title.cmp(&b.title),
                    _ => a.state.cmp(&b.state),
                };
                if desc { ord.reverse() } else { ord }
            });
        }
        rows
    }

    fn ci_rows(&self) -> Vec<&FleetCiRun> {
        let mut rows: Vec<&FleetCiRun> = self
            .ci
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|r| {
                hit(&r.repo, &self.filter)
                    || hit(&r.branch, &self.filter)
                    || hit(&r.name, &self.filter)
            })
            .collect();
        if let Some((col, desc)) = self.sort {
            rows.sort_by(|a, b| {
                let ord = match col {
                    0 => a.repo.cmp(&b.repo),
                    1 => a.name.cmp(&b.name),
                    2 => a.branch.cmp(&b.branch),
                    _ => a.status.cmp(&b.status),
                };
                if desc { ord.reverse() } else { ord }
            });
        }
        rows
    }

    fn gov_rows(&self) -> Vec<&GovPlugin> {
        self.gov
            .as_ref()
            .map(|g| {
                g.plugins
                    .iter()
                    .filter(|p| {
                        hit(&p.name, &self.filter)
                            || p.phases.iter().any(|ph| hit(ph, &self.filter))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn rows_len(&self) -> usize {
        match self.view {
            View::Stacks => self.stack_rows().len(),
            View::Fleet => self.fleet_rows().len(),
            View::Changesets => self.changeset_rows().len(),
            View::Changeset => self.change_repo_rows().len(),
            View::Tree => 0,
            View::Prs => self.pr_rows().len(),
            View::Ci => self.ci_rows().len(),
            View::Governance => self.gov_rows().len(),
            View::Plugins => self.panel_rows().len(),
            View::Errors => self.error_rows().len(),
            View::Files => self.file_rows().len(),
            View::PrFiles => self.pr_file_rows().len(),
            View::Grep => self.grep_rows().len(),
            View::RepoDetail | View::PrDetail | View::CiDetail => 0,
        }
    }

    /// The file browser's current listing, filtered and sorted dirs-first then
    /// lexicographically.
    fn file_rows(&self) -> Vec<&FileEntry> {
        let mut rows: Vec<&FileEntry> = self
            .files_entries
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|e| hit(&e.name, &self.filter))
            .collect();
        rows.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
        rows
    }

    /// A PR/MR's changed files, filtered by the live filter (path/status).
    fn pr_file_rows(&self) -> Vec<&PrFileEntry> {
        self.pr_file_entries
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|e| hit(&e.path, &self.filter) || hit(&e.status, &self.filter))
            .collect()
    }

    /// Cross-repo grep hits, filtered by the live filter (repo/path/text).
    fn grep_rows(&self) -> Vec<&GrepHit> {
        self.grep_hits
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|h| {
                self.filter.is_empty()
                    || hit(&h.repo, &self.filter)
                    || hit(&h.path, &self.filter)
                    || hit(&h.text, &self.filter)
            })
            .collect()
    }

    /// Total repos in the current stack, ignoring filters — the denominator of
    /// the `problems (3/40)` title.
    fn fleet_total(&self) -> usize {
        let stack = self.stack.as_deref().unwrap_or_default();
        self.snapshot
            .fleet
            .iter()
            .find(|(name, _)| name == stack)
            .map_or(0, |(_, repos)| repos.len())
    }

    /// URL under the cursor in the PR/CI views, for `o` (open in browser).
    fn cursor_url(&self) -> Option<String> {
        let index = self.cursor.selected()?;
        match self.view {
            View::Prs => self.pr_rows().get(index).map(|p| p.url.clone()),
            View::Ci => self.ci_rows().get(index).map(|r| r.url.clone()),
            _ => None,
        }
    }

    /// Path of the first existing artifact for the plugin under the cursor in
    /// the governance view, for `o` (open the artifact).
    fn cursor_path(&self) -> Option<String> {
        if self.view != View::Governance {
            return None;
        }
        let index = self.cursor.selected()?;
        let plugin = self.gov_rows().get(index).map(|p| p.name.clone())?;
        let gov = self.gov.as_ref()?;
        gov.artifacts
            .iter()
            .find(|a| a.plugin == plugin && a.exists)
            .or_else(|| gov.artifacts.iter().find(|a| a.plugin == plugin))
            .map(|a| a.path.clone())
    }

    fn cursor_repo(&self) -> Option<String> {
        let index = self.cursor.selected()?;
        match self.view {
            View::Fleet => self.fleet_rows().get(index).map(|r| r.name.clone()),
            View::Changeset => self.change_repo_rows().get(index).map(|r| r.name.clone()),
            View::Prs => self.pr_rows().get(index).map(|p| p.repo.clone()),
            View::Ci => self.ci_rows().get(index).map(|r| r.repo.clone()),
            View::Grep => self.grep_rows().get(index).map(|h| h.repo.clone()),
            View::Files | View::RepoDetail => self.files_repo.clone(),
            _ => None,
        }
    }

    /// The PR to act on (merge/approve): the cursor row in the `Prs` list, or
    /// the drilled-in PR in `PrDetail`. `(repo, number, title)`.
    fn current_pr(&self) -> Option<(String, u64, String)> {
        match self.view {
            View::Prs => {
                let index = self.cursor.selected()?;
                self.pr_rows()
                    .get(index)
                    .map(|p| (p.repo.clone(), p.number, p.title.clone()))
            }
            View::PrDetail => self.detail_pr.clone(),
            _ => None,
        }
    }

    /// The rendered state (`open`/`draft`/`merged`/`closed`) of the PR the
    /// cursor/drill-in points at, so write actions can be gated. `None` outside
    /// a PR context or when the PR is not in the fetched list.
    fn current_pr_state(&self) -> Option<String> {
        let (repo, number, _) = self.current_pr()?;
        self.prs
            .as_deref()?
            .iter()
            .find(|p| p.repo == repo && p.number == number)
            .map(|p| p.state.clone())
    }

    /// Whether the current PR accepts write actions (merge/approve/checkout):
    /// only open or draft PRs. A missing state (list not fetched) is permissive
    /// so the drill-in path still works.
    fn current_pr_writable(&self) -> bool {
        match self.current_pr_state() {
            Some(state) => state.contains("open") || state.contains("draft"),
            None => true,
        }
    }

    fn repo_path(&self, repo: &str) -> Option<PathBuf> {
        self.snapshot
            .paths
            .iter()
            .find(|(name, _)| name == repo)
            .map(|(_, path)| path.clone())
    }

    fn merge_badge(&self, repo: &str) -> Option<&MergeBadge> {
        self.snapshot
            .merges
            .iter()
            .find(|(name, _)| name == repo)
            .map(|(_, badge)| badge)
    }

    fn clamp_cursor(&mut self) {
        let last = self.rows_len().saturating_sub(1);
        self.cursor
            .select(Some(self.cursor.selected().unwrap_or(0).min(last)));
    }

    /// Append a failure to the rolling error log, stamping it with the next
    /// sequence number and trimming the log to [`ERROR_LOG_CAP`]. Keeps the
    /// transient `message` behavior separate — callers still set that too.
    fn push_error(&mut self, context: &str, message: impl Into<String>) {
        self.error_seq += 1;
        self.errors.push(ErrorEntry {
            when_seq: self.error_seq,
            context: context.to_string(),
            message: message.into(),
        });
        let overflow = self.errors.len().saturating_sub(ERROR_LOG_CAP);
        if overflow > 0 {
            self.errors.drain(0..overflow);
        }
    }

    /// The error log newest-first, for the `View::Errors` list, honoring the
    /// live filter (context or message).
    fn error_rows(&self) -> Vec<&ErrorEntry> {
        self.errors
            .iter()
            .rev()
            .filter(|e| {
                self.filter.is_empty()
                    || hit(&e.context, &self.filter)
                    || hit(&e.message, &self.filter)
            })
            .collect()
    }

    /// Available plugin panels, filtered by the live filter (name/phases).
    fn panel_rows(&self) -> Vec<&PluginPanel> {
        self.panels
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|p| {
                self.filter.is_empty()
                    || hit(&p.name, &self.filter)
                    || p.phases.iter().any(|ph| hit(ph, &self.filter))
            })
            .collect()
    }

    /// Move the row cursor by roughly one visible page, clamped to the row
    /// range. `down` picks direction; the step is the last-drawn body height.
    fn move_page(&mut self, down: bool) {
        let step = self.page_size.max(1);
        let last = self.rows_len().saturating_sub(1);
        let current = self.cursor.selected().unwrap_or(0);
        let next = if down {
            current.saturating_add(step).min(last)
        } else {
            current.saturating_sub(step)
        };
        self.cursor.select(Some(next));
    }

    fn goto_view(&mut self, view: View) {
        if self.view != view {
            // Top-level peer switches (Fleet/Stacks/Changesets/Prs/Ci/…) reset
            // the back-stack to root rather than piling on: a lateral move is not
            // a drill-in, so `b` should return to the fleet, not replay a chain of
            // peers. Real drill-ins (Changeset/detail/Files/Grep) still push.
            if is_top_level(view) {
                self.back.clear();
                if view != View::Fleet {
                    self.back.push(View::Fleet);
                }
            } else if self.back.last() != Some(&self.view) {
                self.back.push(self.view);
            }
            self.view = view;
            self.cursor.select(Some(0));
            self.filter.clear();
            self.sort = None;
            self.selected_repos.clear();
            if view != View::Files {
                self.files_repo = None;
                self.files_entries = None;
                self.files_subpath.clear();
            }
            if view != View::PrFiles {
                self.pr_file_entries = None;
                self.pr_files_pr = None;
            }
        }
    }

    /// Whether the current view is one of the shared scrollable detail views.
    fn is_detail_view(&self) -> bool {
        matches!(
            self.view,
            View::RepoDetail | View::PrDetail | View::CiDetail
        )
    }

    fn go_back(&mut self) {
        if let Some(previous) = self.back.pop() {
            self.view = previous;
            self.filter.clear();
            self.sort = None;
            self.selected_repos.clear();
            // Returning into the file browser: restore the snapshot captured on
            // the way into the detail viewer, so the listing is not wiped.
            if previous == View::Files
                && let Some((repo, subpath, entries)) = self.files_return.take()
            {
                self.files_repo = Some(repo);
                self.files_subpath = subpath;
                self.files_entries = entries;
            }
            // Returning into the PR files browser: restore the PR context and the
            // captured listing, so `b` from a file's content lands back on it.
            if previous == View::PrFiles
                && let Some((pr, entries)) = self.pr_files_return.take()
            {
                self.pr_files_pr = Some(pr);
                self.pr_file_entries = entries;
            }
            self.clamp_cursor();
        }
    }

    /// Sortable column count for the current view (0 = not sortable).
    fn sortable_cols(&self) -> u16 {
        u16::try_from(sort_header_map(self.view).len()).unwrap_or(0)
    }

    /// Move the active sort column by `delta` (wrapping), starting from the
    /// first column when unset. No-op on non-sortable views.
    fn cycle_sort(&mut self, forward: bool) {
        let cols = self.sortable_cols();
        if cols == 0 {
            return;
        }
        let next = match self.sort {
            None => 0,
            Some((col, _)) => {
                if forward {
                    (col + 1) % cols
                } else {
                    (col + cols - 1) % cols
                }
            }
        };
        let desc = self.sort.map(|(_, d)| d).unwrap_or(false);
        self.sort = Some((next, desc));
        self.clamp_cursor();
    }

    /// Toggle ascending/descending on the active sort column, defaulting the
    /// column to the first when unset. No-op on non-sortable views.
    fn toggle_sort_dir(&mut self) {
        if self.sortable_cols() == 0 {
            return;
        }
        self.sort = Some(match self.sort {
            None => (0, true),
            Some((col, desc)) => (col, !desc),
        });
        self.clamp_cursor();
    }
}

/// Run the cockpit until quit. Returns an [`Exit`] when the user asked to
/// leave into a repo — either to `goto` it (print the path) or to open a shell.
pub fn run(controller: Box<dyn Controller>) -> io::Result<Option<Exit>> {
    let (job_tx, job_rx) = channel::<Job>();
    let (out_tx, out_rx) = channel::<Outcome>();
    spawn_worker(controller, job_rx, out_tx);

    theme::set(startup_theme());

    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &job_tx, &out_rx);
    ratatui::restore();
    result
}

/// Pick the startup palette from the environment.
///
/// Per the NO_COLOR spec, presence of a non-empty `NO_COLOR` selects the
/// `monochrome` skin. Otherwise `HAW_THEME` names a built-in theme; anything
/// unset or unrecognized falls back to the default catppuccin palette.
fn startup_theme() -> Theme {
    if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        return Theme::monochrome();
    }
    std::env::var("HAW_THEME")
        .ok()
        .and_then(|name| Theme::by_name(&name))
        .unwrap_or_else(Theme::catppuccin)
}

fn spawn_worker(controller: Box<dyn Controller>, jobs: Receiver<Job>, outcomes: Sender<Outcome>) {
    std::thread::spawn(move || {
        let mut controller = controller;
        while let Ok(job) = jobs.recv() {
            let outcome = match job {
                Job::Refresh => Outcome::Snapshot(Box::new(controller.snapshot())),
                Job::ChangesetPrs(id) => {
                    Outcome::ChangesetPrs(Box::new(controller.changeset_prs(&id)))
                }
                Job::FleetPrs { force } => {
                    Outcome::FleetPrs(Box::new(controller.fleet_prs_refresh(force)))
                }
                Job::FleetCi { force } => {
                    Outcome::FleetCi(Box::new(controller.fleet_ci_refresh(force)))
                }
                Job::Governance => Outcome::Governance(Box::new(controller.governance())),
                Job::PluginPanels => Outcome::PluginPanels(Box::new(controller.plugin_panels())),
                Job::PluginRender(name) => Outcome::Detail(
                    format!("plugin: {name}"),
                    Box::new(controller.plugin_render(&name)),
                ),
                Job::RepoDetail(name) => Outcome::Detail(
                    format!("repo {name}"),
                    Box::new(controller.repo_detail(&name)),
                ),
                Job::PrDetail(repo, number) => Outcome::Detail(
                    format!("PR {repo}#{number}"),
                    Box::new(controller.pr_detail(&repo, number)),
                ),
                Job::CiDetail(repo, run_id) => Outcome::Detail(
                    format!("CI {repo} #{run_id}"),
                    Box::new(controller.ci_detail(&repo, run_id)),
                ),
                Job::PrDiff(repo, number) => Outcome::Detail(
                    format!("diff {repo}#{number}"),
                    Box::new(controller.pr_diff(&repo, number)),
                ),
                Job::CiLogs(repo, run_id) => Outcome::Detail(
                    format!("logs {repo} #{run_id}"),
                    Box::new(controller.ci_logs(&repo, run_id)),
                ),
                Job::RepoTree(repo, subpath, remote) => {
                    Outcome::Tree(Box::new(controller.repo_tree(&repo, &subpath, remote)))
                }
                Job::PrFiles(repo, number) => {
                    Outcome::PrFiles(Box::new(controller.pr_files(&repo, number)))
                }
                Job::PrFileContent(repo, number, path, title) => Outcome::Detail(
                    title,
                    Box::new(controller.pr_file_content(&repo, number, &path)),
                ),
                Job::FileContent(repo, path, remote, title) => Outcome::Detail(
                    title,
                    Box::new(controller.file_content(&repo, &path, remote)),
                ),
                Job::Grep(pattern, stack) => {
                    Outcome::Grep(Box::new(controller.grep(&pattern, stack.as_deref())))
                }
                Job::Action(label, kind) => {
                    let result = match kind {
                        ActionKind::SyncStack(stack) => controller.sync_stack(&stack),
                        ActionKind::SyncRepo(repo) => controller.sync_repo(&repo),
                        ActionKind::SyncRepos(repos) => controller.sync_repos(&repos),
                        ActionKind::Switch(stack) => controller.switch(&stack),
                        ActionKind::Pin => controller.pin(),
                        ActionKind::Lock => controller.lock(),
                        ActionKind::Run(cmd) => controller.run_cmd(&cmd),
                        ActionKind::RunRepos(cmd, repos) => controller.run_cmd_in(&cmd, &repos),
                        ActionKind::ChangeStart(id) => controller.change_start(&id),
                        ActionKind::ChangeRequest(id, only) => controller.change_request(&id, only),
                        ActionKind::ChangeLand(id) => controller.change_land(&id),
                        ActionKind::MergePr(repo, number) => controller.pr_merge(&repo, number),
                        ActionKind::ApprovePr(repo, number) => controller.pr_approve(&repo, number),
                        ActionKind::CheckoutPr(repo, number) => {
                            controller.pr_checkout(&repo, number)
                        }
                        ActionKind::MergeCleanup(repo) => controller.merge_cleanup(&repo),
                        ActionKind::MergeAbort(repo) => controller.merge_abort(&repo),
                        ActionKind::RepoFetch(repo) => controller.repo_fetch(&repo),
                        ActionKind::Exec(repo, cmd) => controller.exec_in(&repo, &cmd),
                    };
                    Outcome::Action(label, result)
                }
            };
            if outcomes.send(outcome).is_err() {
                return;
            }
        }
    });
}

fn dispatch(app: &mut App, jobs: &Sender<Job>, label: &'static str, kind: ActionKind) {
    if app.busy.is_some() {
        app.message = "busy — wait for the current operation".to_string();
        return;
    }
    app.busy = Some(label);
    let _ = jobs.send(Job::Action(label, kind));
}

fn request_refresh(app: &mut App, jobs: &Sender<Job>) {
    if app.busy.is_none() {
        app.busy = Some("refresh");
        let _ = jobs.send(Job::Refresh);
    }
}

/// Navigate into a changeset's view and fetch its live PR/CI status.
fn open_changeset(app: &mut App, jobs: &Sender<Job>, id: &str) {
    app.changeset = Some(id.to_string());
    app.selected_repos.clear();
    app.goto_view(View::Changeset);
    // Always enqueue the read; the serial worker runs it after any in-flight
    // job, so entering the view while busy still loads (never refused).
    app.busy = Some("PR status");
    let _ = jobs.send(Job::ChangesetPrs(id.to_string()));
}

/// Navigate into the shared scrollable detail view and fetch its report.
/// `title` seeds the panel/crumb label; `busy` labels the spinner.
fn open_detail(
    app: &mut App,
    jobs: &Sender<Job>,
    view: View,
    title: String,
    busy: &'static str,
    job: Job,
) {
    app.detail_title = title;
    app.detail_text = None;
    app.detail_scroll = 0;
    app.pending_scroll = None;
    app.goto_view(view);
    // Always enqueue the read; the serial worker runs it after any in-flight
    // job, so drilling in while busy still loads (never refused).
    app.busy = Some(busy);
    let _ = jobs.send(job);
}

/// Navigate into a repo's live git detail view and fetch its report.
fn open_repo_detail(app: &mut App, jobs: &Sender<Job>, repo: &str) {
    open_detail(
        app,
        jobs,
        View::RepoDetail,
        format!("repo {repo}"),
        "git detail",
        Job::RepoDetail(repo.to_string()),
    );
    // Track the repo so `x`/`!`/`F` in the detail view act on it. Set after
    // `open_detail` — `goto_view` clears `files_repo` on non-Files views.
    app.files_repo = Some(repo.to_string());
}

/// Navigate into a PR/MR's drill-in detail view and fetch its report.
/// `title` seeds `detail_pr` so the view can merge/approve the current PR.
fn open_pr_detail(app: &mut App, jobs: &Sender<Job>, repo: &str, number: u64, title: &str) {
    app.detail_pr = Some((repo.to_string(), number, title.to_string()));
    open_detail(
        app,
        jobs,
        View::PrDetail,
        format!("PR {repo}#{number}"),
        "PR detail",
        Job::PrDetail(repo.to_string(), number),
    );
}

/// Navigate into a CI run's drill-in detail view and fetch its report.
fn open_ci_detail(app: &mut App, jobs: &Sender<Job>, repo: &str, run_id: u64, name: &str) {
    app.detail_ci = Some((repo.to_string(), run_id, name.to_string()));
    open_detail(
        app,
        jobs,
        View::CiDetail,
        format!("CI {repo} {name}"),
        "CI detail",
        Job::CiDetail(repo.to_string(), run_id),
    );
}

/// Navigate into a PR/MR's diff (the "read the code" ask) — the shared
/// scrollable detail view, titled `diff <repo>#<number>`.
fn open_pr_diff(app: &mut App, jobs: &Sender<Job>, repo: &str, number: u64) {
    open_detail(
        app,
        jobs,
        View::RepoDetail,
        format!("diff {repo}#{number}"),
        "PR diff",
        Job::PrDiff(repo.to_string(), number),
    );
}

/// Open the PR/MR files browser: a list of the files this PR changed, each
/// selectable to read its whole content at the PR's head ref. Mirrors the
/// `View::Files` machinery but scoped to a PR.
fn open_pr_files(app: &mut App, jobs: &Sender<Job>, repo: &str, number: u64, title: &str) {
    app.goto_view(View::PrFiles);
    app.pr_files_pr = Some((repo.to_string(), number, title.to_string()));
    app.pr_file_entries = None;
    app.cursor.select(Some(0));
    // Always enqueue the read; the serial worker runs it after any in-flight job.
    app.busy = Some("PR files");
    let _ = jobs.send(Job::PrFiles(repo.to_string(), number));
}

/// Fetch one changed file's whole content at the PR's head ref into the shared
/// scrollable detail view (`View::PrDetail`), titled `PR #<n> <path>`.
fn open_pr_file_content(app: &mut App, jobs: &Sender<Job>, path: &str) {
    let Some((repo, number, title)) = app.pr_files_pr.clone() else {
        return;
    };
    let panel_title = format!("PR #{number} {path}");
    app.detail_title = panel_title.clone();
    app.detail_text = None;
    app.detail_scroll = 0;
    app.pending_scroll = None;
    // Seed the PR drill-in context so PrDetail's merge/approve/f keys still act
    // on this PR while its file content is shown.
    app.detail_pr = Some((repo.clone(), number, title.clone()));
    // Snapshot the browser so `go_back` into PrFiles restores it; `goto_view`
    // clears the `pr_files_*` fields on the way into the detail view.
    app.pr_files_return = Some(((repo.clone(), number, title), app.pr_file_entries.clone()));
    app.goto_view(View::PrDetail);
    app.busy = Some("PR file");
    let _ = jobs.send(Job::PrFileContent(
        repo,
        number,
        path.to_string(),
        panel_title,
    ));
}

/// Navigate into a CI run's logs — the shared scrollable detail view, titled
/// `logs <repo> <name>`.
fn open_ci_logs(app: &mut App, jobs: &Sender<Job>, repo: &str, run_id: u64, name: &str) {
    open_detail(
        app,
        jobs,
        View::RepoDetail,
        format!("logs {repo} {name}"),
        "CI logs",
        Job::CiLogs(repo.to_string(), run_id),
    );
}

/// Open the file browser on `repo` at its root. `remote` picks the forge view
/// (true when the repo isn't cloned locally).
fn open_files(app: &mut App, jobs: &Sender<Job>, repo: &str, remote: bool) {
    app.goto_view(View::Files);
    app.files_repo = Some(repo.to_string());
    app.files_subpath = String::new();
    app.files_remote = remote;
    reload_files(app, jobs);
}

/// (Re)fetch the current file-browser directory listing.
fn reload_files(app: &mut App, jobs: &Sender<Job>) {
    let Some(repo) = app.files_repo.clone() else {
        return;
    };
    app.files_entries = None;
    app.cursor.select(Some(0));
    app.busy = Some("files");
    let _ = jobs.send(Job::RepoTree(
        repo,
        app.files_subpath.clone(),
        app.files_remote,
    ));
}

/// Fetch one file's content into the shared scrollable detail view. The detail
/// view lives under `View::RepoDetail`, titled `<repo>:/<path>`.
fn open_file_content(app: &mut App, jobs: &Sender<Job>, name: &str) {
    let Some(repo) = app.files_repo.clone() else {
        return;
    };
    let path = if app.files_subpath.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", app.files_subpath, name)
    };
    let title = format!("{repo}:/{path}");
    app.detail_title = title.clone();
    app.detail_text = None;
    app.detail_scroll = 0;
    app.pending_scroll = None;
    // Snapshot the browser so `go_back` into Files restores the same listing;
    // `goto_view` clears `files_*` on the way into the detail view.
    app.files_return = Some((
        repo.clone(),
        app.files_subpath.clone(),
        app.files_entries.clone(),
    ));
    let remote = app.files_remote;
    app.goto_view(View::RepoDetail);
    app.busy = Some("file");
    let _ = jobs.send(Job::FileContent(repo, path, remote, title));
}

/// Run a cross-repo grep on the worker thread and open the results list.
fn run_grep(app: &mut App, jobs: &Sender<Job>, pattern: &str) {
    let pattern = pattern.trim().to_string();
    if pattern.is_empty() {
        app.message = "grep: give a pattern — :grep <pattern>".to_string();
        return;
    }
    app.grep_pattern = pattern.clone();
    app.grep_hits = None;
    app.goto_view(View::Grep);
    // Always enqueue: the serial worker runs it after any in-flight job.
    app.busy = Some("grep");
    let _ = jobs.send(Job::Grep(pattern, app.stack.clone()));
}

/// Open the file that a grep hit points at, scrolled to the hit's line.
fn open_grep_hit(app: &mut App, jobs: &Sender<Job>, hit: &GrepHit) {
    let title = format!("{}:/{}", hit.repo, hit.path);
    app.detail_title = title.clone();
    app.detail_text = None;
    app.detail_scroll = 0;
    // A 1-based line maps to a 0-based scroll offset. Stashed as `pending_scroll`
    // and applied (clamped) when the file text lands, since the `Outcome::Detail`
    // arm resets `detail_scroll` to 0 on load.
    app.pending_scroll = Some(hit.line.saturating_sub(1) as usize);
    app.goto_view(View::RepoDetail);
    app.busy = Some("file");
    let _ = jobs.send(Job::FileContent(
        hit.repo.clone(),
        hit.path.clone(),
        false,
        title,
    ));
}

/// Run a shell command in one repo on the worker thread; its output lands in
/// the shared detail view titled `$ <cmd> @ <repo>`.
fn run_exec(app: &mut App, jobs: &Sender<Job>, repo: &str, cmd: &str) {
    let cmd = cmd.trim().to_string();
    if cmd.is_empty() {
        app.message = "exec: give a command — !<cmd>".to_string();
        return;
    }
    if app.busy.is_some() {
        app.message = "busy — wait for the current operation".to_string();
        return;
    }
    app.detail_title = format!("$ {cmd} @ {repo}");
    app.detail_text = None;
    app.detail_scroll = 0;
    app.goto_view(View::RepoDetail);
    app.message = format!("→ {cmd} @ {repo}");
    app.busy = Some("exec");
    let _ = jobs.send(Job::Action("exec", ActionKind::Exec(repo.to_string(), cmd)));
}

/// Navigate to a fleet-wide network view (`m`/`i`) and (re)fetch its rows.
/// Pressing the key while already on the view is a manual refetch, which
/// `force`s the controller's TTL cache to refresh; merely entering the view
/// reuses a fresh cached fetch.
fn open_fleet_view(app: &mut App, jobs: &Sender<Job>, view: View) {
    let force = app.view == view;
    app.goto_view(view);
    // Read-only fetches always enqueue — the single worker runs them serially,
    // so navigating while busy is never refused (it just queues behind the
    // current job). `busy` only labels the spinner.
    match view {
        View::Prs => {
            app.busy = Some("PR/MRs");
            let _ = jobs.send(Job::FleetPrs { force });
        }
        View::Ci => {
            app.busy = Some("CI runs");
            let _ = jobs.send(Job::FleetCi { force });
        }
        View::Governance => {
            app.busy = Some("governance");
            let _ = jobs.send(Job::Governance);
        }
        View::Plugins => {
            app.busy = Some("plugins");
            let _ = jobs.send(Job::PluginPanels);
        }
        _ => {}
    }
}

/// Render one plugin's panel into the shared scrollable detail view, titled
/// `plugin: <name>`.
fn open_plugin_render(app: &mut App, jobs: &Sender<Job>, name: &str) {
    open_detail(
        app,
        jobs,
        View::RepoDetail,
        format!("plugin: {name}"),
        "plugin render",
        Job::PluginRender(name.to_string()),
    );
}

/// Open `url` with the platform's default browser, detached (best effort).
fn open_in_browser(url: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", ""]);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = std::process::Command::new("xdg-open");
    command
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

/// A confirmation gate for actions with real side effects (opens/merges PRs).
/// `Some` describes the pending action; `y`/`n` (or Enter/Esc) resolve it.
#[derive(Debug, Clone)]
enum Confirm {
    Land(String),
    Request(String, Option<Vec<String>>),
    MergePr {
        repo: String,
        number: u64,
        title: String,
    },
    ApprovePr {
        repo: String,
        number: u64,
        title: String,
    },
    CheckoutPr {
        repo: String,
        number: u64,
        title: String,
    },
    MergeCleanup(String),
}

fn run_command_bar(app: &mut App, jobs: &Sender<Job>, line: &str) {
    let (verb, rest) = line
        .trim()
        .split_once(' ')
        .map_or((line.trim(), ""), |(v, r)| (v, r.trim()));
    let (sub, arg) = rest
        .split_once(' ')
        .map_or((rest, ""), |(v, r)| (v, r.trim()));
    match (verb, rest) {
        ("sync", "") => {
            if let Some(stack) = app.stack.clone() {
                app.message = format!("→ haw sync --stack {stack}");
                dispatch(app, jobs, "sync", ActionKind::SyncStack(stack));
            }
        }
        ("stack" | "switch", name) if !name.is_empty() => {
            app.message = format!("→ haw switch {name}");
            dispatch(app, jobs, "switch", ActionKind::Switch(name.to_string()));
        }
        ("run", cmd) if !cmd.is_empty() => {
            if app.view == View::Fleet && !app.selected_repos.is_empty() {
                let repos = app.selected_repos.clone();
                app.message = format!("→ haw run '{cmd}' ({} marked repos)", repos.len());
                dispatch(
                    app,
                    jobs,
                    "run",
                    ActionKind::RunRepos(cmd.to_string(), repos),
                );
            } else {
                app.message = format!("→ haw run '{cmd}'");
                dispatch(app, jobs, "run", ActionKind::Run(cmd.to_string()));
            }
        }
        ("change", "") => app.goto_view(View::Changesets),
        ("change", "start") => app.input = InputMode::NewChangeset(String::new()),
        ("change", _) if sub == "start" && !arg.is_empty() => {
            app.message = format!("→ haw change start {arg}");
            dispatch(
                app,
                jobs,
                "change start",
                ActionKind::ChangeStart(arg.to_string()),
            );
        }
        ("change", _) if sub == "land" && !arg.is_empty() => {
            app.pending_confirm = Some(Confirm::Land(arg.to_string()));
        }
        ("change", _) if sub == "request" && !arg.is_empty() => {
            app.pending_confirm = Some(Confirm::Request(arg.to_string(), None));
        }
        ("change", id) => open_changeset(app, jobs, id),
        ("pin", "") => {
            app.message = "→ haw pin".to_string();
            dispatch(app, jobs, "pin", ActionKind::Pin);
        }
        ("lock", "") => {
            app.message = "→ haw lock".to_string();
            dispatch(app, jobs, "lock", ActionKind::Lock);
        }
        ("tree", "") => app.goto_view(View::Tree),
        ("prs", "") => open_fleet_view(app, jobs, View::Prs),
        ("ci", "") => open_fleet_view(app, jobs, View::Ci),
        ("governance", "") => open_fleet_view(app, jobs, View::Governance),
        ("plugins", "") => open_fleet_view(app, jobs, View::Plugins),
        ("errors", "") => app.goto_view(View::Errors),
        ("theme", "") => {
            app.message = format!("themes: {}", theme::THEMES.join(", "));
        }
        ("theme", name) => match Theme::by_name(name) {
            Some(t) => {
                theme::set(t);
                app.message = format!("theme → {name}");
            }
            None => {
                app.message = format!("unknown theme `{name}`; try: {}", theme::THEMES.join(", "));
            }
        },
        ("help", "") => app.help = true,
        ("grep", pat) if !pat.is_empty() => run_grep(app, jobs, pat),
        ("grep", "") => app.message = "grep: give a pattern — :grep <pattern>".to_string(),
        ("sh", cmd) if !cmd.is_empty() => match app.cursor_repo() {
            Some(repo) => run_exec(app, jobs, &repo, cmd),
            None => app.message = "sh: put the cursor on a repo row".to_string(),
        },
        ("problems", "") => {
            app.problems_only = !app.problems_only;
            app.clamp_cursor();
            app.message = if app.problems_only {
                "problems-only filter on".to_string()
            } else {
                "problems-only filter off".to_string()
            };
        }
        ("fetch", "") => match app.cursor_repo() {
            Some(repo) => {
                app.message = format!("→ git fetch ({repo})");
                dispatch(app, jobs, "fetch", ActionKind::RepoFetch(repo));
            }
            None => app.message = "fetch: put the cursor on a repo row".to_string(),
        },
        ("merge", "") => {
            app.message = match app.snapshot.merges.len() {
                0 => "no merges in progress".to_string(),
                n => format!(
                    "{n} repo(s) mid-merge — see the fleet's MERGE column; \
                     :merge cleanup <repo> · :merge abort <repo>"
                ),
            };
        }
        ("merge", _) if sub == "cleanup" && !arg.is_empty() => {
            if app.merge_badge(arg).is_some() {
                app.pending_confirm = Some(Confirm::MergeCleanup(arg.to_string()));
            } else {
                app.message = format!("no merge planned for `{arg}`");
            }
        }
        ("merge", _) if sub == "abort" && !arg.is_empty() => {
            if app.merge_badge(arg).is_some() {
                app.message = format!("→ haw merge abort --repo {arg}");
                dispatch(
                    app,
                    jobs,
                    "merge abort",
                    ActionKind::MergeAbort(arg.to_string()),
                );
            } else {
                app.message = format!("no merge planned for `{arg}`");
            }
        }
        ("q" | "quit", _) => app.message = "use q outside the command bar".to_string(),
        // `:<repo-substring>` (single token) → jump the fleet cursor to the
        // first matching repo. Only a bare token with no args is treated so.
        (token, "") if !token.is_empty() && jump_to_repo(app, token) => {}
        _ => app.message = format!("unknown command `{line}`"),
    }
}

/// Move the fleet cursor to the first repo whose name fuzzy-matches `needle`.
/// Returns `false` (and does nothing) when no repo matches, so the caller can
/// fall through to an "unknown command" message.
fn jump_to_repo(app: &mut App, needle: &str) -> bool {
    let stack = app.stack.as_deref().unwrap_or_default();
    let matched = app
        .snapshot
        .fleet
        .iter()
        .find(|(name, _)| name == stack)
        .and_then(|(_, repos)| repos.iter().find(|r| hit(&r.name, needle)))
        .map(|r| r.name.clone());
    match matched {
        Some(name) => {
            app.problems_only = false;
            app.filter.clear();
            if app.view != View::Fleet {
                app.goto_view(View::Fleet);
            }
            if let Some(index) = app.fleet_rows().iter().position(|r| r.name == name) {
                app.cursor.select(Some(index));
            }
            app.message = format!("→ {name}");
            true
        }
        None => false,
    }
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    jobs: &Sender<Job>,
    outcomes: &Receiver<Outcome>,
) -> io::Result<Option<Exit>> {
    let mut app = App {
        view: View::Fleet,
        back: Vec::new(),
        snapshot: Snapshot::default(),
        stack: None,
        changeset: None,
        selected_repos: Vec::new(),
        sort: None,
        cursor: ListState::default(),
        input: InputMode::None,
        filter: String::new(),
        message: "loading…".to_string(),
        busy: None,
        spinner: 0,
        tick: 0,
        help: false,
        help_scroll: 0,
        exit: None,
        files_repo: None,
        files_subpath: String::new(),
        files_remote: false,
        files_entries: None,
        pending_confirm: None,
        output: None,
        prs: None,
        ci: None,
        gov: None,
        detail_text: None,
        detail_title: String::new(),
        detail_scroll: 0,
        pending_scroll: None,
        files_return: None,
        detail_pr: None,
        detail_ci: None,
        page_size: 10,
        problems_only: false,
        grep_hits: None,
        grep_pattern: String::new(),
        panels: None,
        pr_file_entries: None,
        pr_files_pr: None,
        pr_files_return: None,
        errors: Vec::new(),
        error_seq: 0,
    };
    app.cursor.select(Some(0));
    request_refresh(&mut app, jobs);
    let mut last_refresh = Instant::now();

    loop {
        while let Ok(outcome) = outcomes.try_recv() {
            match outcome {
                Outcome::Snapshot(result) => {
                    app.busy = None;
                    match *result {
                        Ok(snapshot) => {
                            if app.stack.is_none() {
                                app.stack = snapshot
                                    .current_stack
                                    .clone()
                                    .or_else(|| snapshot.stacks.first().cloned());
                            }
                            app.snapshot = snapshot;
                            app.clamp_cursor();
                            if app.message == "loading…" {
                                app.message = "ready — press ? for help".to_string();
                            }
                        }
                        Err(err) => {
                            app.message = format!("refresh failed: {err}");
                            app.push_error("refresh", err.to_string());
                        }
                    }
                }
                Outcome::ChangesetPrs(result) => {
                    app.busy = None;
                    match *result {
                        Ok(summary) => {
                            if let Some(slot) = app
                                .snapshot
                                .changesets
                                .iter_mut()
                                .find(|c| c.id == summary.id)
                            {
                                *slot = summary;
                            }
                            app.message = "PR/MR status refreshed".to_string();
                        }
                        Err(err) => {
                            app.message = format!("PR status failed: {err}");
                            app.push_error("PR status", err.to_string());
                        }
                    }
                }
                Outcome::FleetPrs(result) => {
                    app.busy = None;
                    match *result {
                        Ok(prs) => {
                            app.message = format!("{} open PR/MR(s) across the fleet", prs.len());
                            app.prs = Some(prs);
                            app.clamp_cursor();
                        }
                        Err(err) => {
                            app.message = format!("PR/MR fetch failed: {err}");
                            app.push_error("PR/MR fetch", err.to_string());
                        }
                    }
                }
                Outcome::FleetCi(result) => {
                    app.busy = None;
                    match *result {
                        Ok(runs) => {
                            app.message =
                                format!("{} recent CI run(s) across the fleet", runs.len());
                            app.ci = Some(runs);
                            app.clamp_cursor();
                        }
                        Err(err) => {
                            app.message = format!("CI fetch failed: {err}");
                            app.push_error("CI fetch", err.to_string());
                        }
                    }
                }
                Outcome::Governance(result) => {
                    app.busy = None;
                    match *result {
                        Ok(gov) => {
                            app.message = format!(
                                "{} plugin(s) · {} artifact(s) · {} finding(s)",
                                gov.plugins.len(),
                                gov.artifacts.len(),
                                gov.findings.len()
                            );
                            app.gov = Some(gov);
                            app.clamp_cursor();
                        }
                        Err(err) => {
                            app.message = format!("governance fetch failed: {err}");
                            app.push_error("governance fetch", err.to_string());
                        }
                    }
                }
                Outcome::PluginPanels(result) => {
                    app.busy = None;
                    match *result {
                        Ok(panels) => {
                            app.message = format!("{} plugin panel(s) available", panels.len());
                            app.panels = Some(panels);
                            app.clamp_cursor();
                        }
                        Err(err) => {
                            app.message = format!("plugin panels failed: {err}");
                            app.push_error("plugin panels", err.to_string());
                        }
                    }
                }
                Outcome::Detail(title, result) => {
                    app.busy = None;
                    app.detail_scroll = 0;
                    app.detail_title = title;
                    match *result {
                        Ok(report) => {
                            app.detail_text = Some(report);
                            // Apply any pending jump-to-line, clamped to the now-
                            // known content height (grep hit → file line).
                            if let Some(target) = app.pending_scroll.take() {
                                let max = usize::from(detail_max_scroll(&app));
                                app.detail_scroll =
                                    u16::try_from(target.min(max)).unwrap_or(u16::MAX);
                            }
                            app.message = "detail loaded".to_string();
                        }
                        Err(err) => {
                            app.detail_text = Some(format!("failed to load detail: {err}"));
                            app.message = format!("detail failed: {err}");
                            app.push_error(&app.detail_title.clone(), err.to_string());
                        }
                    }
                }
                Outcome::Tree(result) => {
                    app.busy = None;
                    match *result {
                        Ok(entries) => {
                            let count = entries.len();
                            app.files_entries = Some(entries);
                            app.cursor.select(Some(0));
                            app.clamp_cursor();
                            app.message = format!("{count} entr(y/ies)");
                        }
                        Err(err) => {
                            app.files_entries = Some(Vec::new());
                            app.message = format!("files failed: {err}");
                            app.push_error("files", err.to_string());
                        }
                    }
                }
                Outcome::PrFiles(result) => {
                    app.busy = None;
                    match *result {
                        Ok(entries) => {
                            let count = entries.len();
                            app.pr_file_entries = Some(entries);
                            app.cursor.select(Some(0));
                            app.clamp_cursor();
                            app.message = format!("{count} changed file(s)");
                        }
                        Err(err) => {
                            app.pr_file_entries = Some(Vec::new());
                            app.message = format!("PR files failed: {err}");
                            app.push_error("PR files", err.to_string());
                        }
                    }
                }
                Outcome::Grep(result) => {
                    app.busy = None;
                    match *result {
                        Ok(hits) => {
                            app.message = format!(
                                "{} hit(s) for `{}` across the fleet",
                                hits.len(),
                                app.grep_pattern
                            );
                            app.grep_hits = Some(hits);
                            app.cursor.select(Some(0));
                            app.clamp_cursor();
                        }
                        Err(err) => {
                            app.grep_hits = Some(Vec::new());
                            app.message = format!("grep failed: {err}");
                            app.push_error("grep", err.to_string());
                        }
                    }
                }
                Outcome::Action(label, result) => {
                    app.busy = None;
                    // `!`/`:sh` exec output lands in the shared detail view.
                    if label == "exec" {
                        app.detail_scroll = 0;
                        match result {
                            Ok(output) => {
                                app.detail_text = Some(output);
                                app.message = "command finished".to_string();
                            }
                            Err(err) => {
                                app.detail_text = Some(format!("failed to run: {err}"));
                                app.message = format!("exec failed: {err}");
                                app.push_error("exec", err.to_string());
                            }
                        }
                        continue;
                    }
                    match result {
                        Ok(message) if label == "run" => {
                            app.message = "ran — press any key to dismiss the output".to_string();
                            app.output = Some(message);
                        }
                        Ok(message) => app.message = message,
                        Err(err) => {
                            app.message = format!("{label} failed: {err}");
                            app.push_error(label, err.to_string());
                        }
                    }
                    // A merge/approve changes the fleet PR list — re-fetch it so a
                    // merged PR disappears and approvals show, when we're on it.
                    if (label == "merge PR" || label == "approve PR")
                        && matches!(app.view, View::Prs | View::PrDetail)
                        && app.busy.is_none()
                    {
                        app.busy = Some("PR/MRs");
                        // The list just changed on the forge — bypass the cache.
                        let _ = jobs.send(Job::FleetPrs { force: true });
                    } else {
                        request_refresh(&mut app, jobs);
                    }
                    last_refresh = Instant::now();
                }
            }
        }

        // Auto-refresh the fleet/status snapshot when idle and safe, k9s-style.
        // Never disturbs input, overlays, or in-flight work; network views
        // (Prs/Ci/Governance) stay strictly on-demand.
        if app.busy.is_none()
            && app.input == InputMode::None
            && !app.help
            && app.output.is_none()
            && app.pending_confirm.is_none()
            && last_refresh.elapsed() >= Duration::from_secs(5)
        {
            request_refresh(&mut app, jobs);
            last_refresh = Instant::now();
        }

        app.tick = app.tick.wrapping_add(1);
        if app.busy.is_some() {
            app.spinner = (app.spinner + 1) % SPINNER.len();
        }
        terminal.draw(|frame| draw(frame, &mut app))?;

        if !event::poll(Duration::from_millis(120))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(app.exit);
        }
        if key.code == KeyCode::F(5)
            || (key.code == KeyCode::Char('r') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            app.message = "refreshing…".to_string();
            request_refresh(&mut app, jobs);
            last_refresh = Instant::now();
            continue;
        }

        if app.help {
            match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    app.help_scroll = app
                        .help_scroll
                        .saturating_add(1)
                        .min(help_max_scroll(frame_height(terminal)));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    app.help_scroll = app.help_scroll.saturating_sub(1);
                }
                KeyCode::PageDown => {
                    app.help_scroll = app
                        .help_scroll
                        .saturating_add(10)
                        .min(help_max_scroll(frame_height(terminal)));
                }
                KeyCode::PageUp => app.help_scroll = app.help_scroll.saturating_sub(10),
                _ => {
                    app.help = false;
                    app.help_scroll = 0;
                }
            }
            continue;
        }

        if app.output.is_some() {
            app.output = None;
            continue;
        }

        if let Some(confirm) = app.pending_confirm.take() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => match confirm {
                    Confirm::Land(id) => {
                        app.message = format!("→ haw change land {id}");
                        dispatch(&mut app, jobs, "change land", ActionKind::ChangeLand(id));
                    }
                    Confirm::Request(id, only) => {
                        app.message = format!("→ haw change request {id}");
                        dispatch(
                            &mut app,
                            jobs,
                            "change request",
                            ActionKind::ChangeRequest(id, only),
                        );
                    }
                    Confirm::MergePr { repo, number, .. } => {
                        app.message = format!("→ haw merge PR {repo}#{number}");
                        dispatch(
                            &mut app,
                            jobs,
                            "merge PR",
                            ActionKind::MergePr(repo, number),
                        );
                    }
                    Confirm::ApprovePr { repo, number, .. } => {
                        app.message = format!("→ haw approve PR {repo}#{number}");
                        dispatch(
                            &mut app,
                            jobs,
                            "approve PR",
                            ActionKind::ApprovePr(repo, number),
                        );
                    }
                    Confirm::CheckoutPr { repo, number, .. } => {
                        app.message = format!("→ haw checkout PR {repo}#{number}");
                        dispatch(
                            &mut app,
                            jobs,
                            "checkout PR",
                            ActionKind::CheckoutPr(repo, number),
                        );
                    }
                    Confirm::MergeCleanup(repo) => {
                        app.message = format!("→ haw merge cleanup --repo {repo}");
                        dispatch(
                            &mut app,
                            jobs,
                            "merge cleanup",
                            ActionKind::MergeCleanup(repo),
                        );
                    }
                },
                _ => app.message = "cancelled".to_string(),
            }
            continue;
        }

        match &mut app.input {
            InputMode::Filter(buffer)
            | InputMode::Command(buffer)
            | InputMode::NewChangeset(buffer)
            | InputMode::Exec(_, buffer) => {
                match key.code {
                    KeyCode::Esc => app.input = InputMode::None,
                    KeyCode::Backspace => {
                        buffer.pop();
                        if let InputMode::Filter(b) = &app.input {
                            app.filter = b.clone();
                            app.clamp_cursor();
                        }
                    }
                    KeyCode::Char(c) => {
                        buffer.push(c);
                        if let InputMode::Filter(b) = &app.input {
                            app.filter = b.clone();
                            app.clamp_cursor();
                        }
                    }
                    KeyCode::Enter => {
                        let mode = std::mem::replace(&mut app.input, InputMode::None);
                        match mode {
                            InputMode::Filter(_) => {}
                            InputMode::Command(line) => run_command_bar(&mut app, jobs, &line),
                            InputMode::Exec(repo, cmd) => {
                                run_exec(&mut app, jobs, &repo, &cmd);
                            }
                            InputMode::NewChangeset(id) => {
                                let id = id.trim().to_string();
                                if !id.is_empty() {
                                    app.message = format!("→ haw change start {id}");
                                    dispatch(
                                        &mut app,
                                        jobs,
                                        "change start",
                                        ActionKind::ChangeStart(id),
                                    );
                                }
                            }
                            InputMode::None => {}
                        }
                    }
                    _ => {}
                }
                continue;
            }
            InputMode::None => {}
        }

        let selected = app.cursor.selected().unwrap_or(0);
        match key.code {
            KeyCode::Char('q') => return Ok(app.exit),
            KeyCode::Char('?') => {
                app.help = true;
                app.help_scroll = 0;
            }
            KeyCode::Char('/') => app.input = InputMode::Filter(app.filter.clone()),
            KeyCode::Char(':') => app.input = InputMode::Command(String::new()),
            KeyCode::Esc | KeyCode::Char('b') | KeyCode::Backspace if app.view == View::Files => {
                if !app.filter.is_empty() {
                    app.filter.clear();
                } else if let Some((parent, _)) = app.files_subpath.rsplit_once('/') {
                    app.files_subpath = parent.to_string();
                    reload_files(&mut app, jobs);
                } else if !app.files_subpath.is_empty() {
                    app.files_subpath.clear();
                    reload_files(&mut app, jobs);
                } else {
                    app.go_back();
                }
            }
            KeyCode::Esc | KeyCode::Char('b') => {
                if !app.filter.is_empty() {
                    app.filter.clear();
                } else {
                    app.go_back();
                }
            }
            KeyCode::Char('R') if app.view == View::Files => {
                app.files_remote = !app.files_remote;
                reload_files(&mut app, jobs);
            }
            KeyCode::Down | KeyCode::Char('j') if app.is_detail_view() => {
                app.detail_scroll = app
                    .detail_scroll
                    .saturating_add(1)
                    .min(detail_max_scroll(&app));
            }
            KeyCode::Up | KeyCode::Char('k') if app.is_detail_view() => {
                app.detail_scroll = app.detail_scroll.saturating_sub(1);
            }
            KeyCode::PageDown if app.is_detail_view() => {
                app.detail_scroll = app
                    .detail_scroll
                    .saturating_add(10)
                    .min(detail_max_scroll(&app));
            }
            KeyCode::PageUp if app.is_detail_view() => {
                app.detail_scroll = app.detail_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => app.move_page(true),
            KeyCode::PageUp => app.move_page(false),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.move_page(true);
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.move_page(false);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.cursor
                    .select(Some((selected + 1).min(app.rows_len().saturating_sub(1))));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.cursor.select(Some(selected.saturating_sub(1)));
            }
            KeyCode::Char('>') => app.cycle_sort(true),
            KeyCode::Char('<') => app.cycle_sort(false),
            KeyCode::Char('.') => app.toggle_sort_dir(),
            KeyCode::Char('t') => app.goto_view(View::Tree),
            KeyCode::Char('c') => app.goto_view(View::Changesets),
            KeyCode::Char('m') => open_fleet_view(&mut app, jobs, View::Prs),
            KeyCode::Char('i') => open_fleet_view(&mut app, jobs, View::Ci),
            KeyCode::Char('v') => open_fleet_view(&mut app, jobs, View::Governance),
            KeyCode::Char('P') => open_fleet_view(&mut app, jobs, View::Plugins),
            KeyCode::Char('E') => app.goto_view(View::Errors),
            KeyCode::Char('o') if app.view == View::Prs || app.view == View::Ci => {
                match app.cursor_url() {
                    Some(url) if !url.is_empty() => match open_in_browser(&url) {
                        Ok(()) => app.message = format!("→ opened {url}"),
                        Err(err) => app.message = format!("open failed: {err}"),
                    },
                    _ => app.message = "open: put the cursor on a row".to_string(),
                }
            }
            KeyCode::Char('o') if app.view == View::Governance => match app.cursor_path() {
                Some(path) if !path.is_empty() => match open_in_browser(&path) {
                    Ok(()) => app.message = format!("→ opened {path}"),
                    Err(err) => app.message = format!("open failed: {err}"),
                },
                _ => app.message = "open: no artifact for this plugin".to_string(),
            },
            KeyCode::Char('g') => {
                if let Some(repo) = app.cursor_repo()
                    && let Some(path) = app.repo_path(&repo)
                {
                    app.exit = Some(Exit::Goto(path));
                    return Ok(app.exit);
                }
                app.message = "goto: put the cursor on a repo row".to_string();
            }
            KeyCode::Char('x')
                if matches!(
                    app.view,
                    View::Fleet
                        | View::RepoDetail
                        | View::Files
                        | View::Changeset
                        | View::Prs
                        | View::Ci
                        | View::Grep
                ) =>
            {
                match app.cursor_repo().as_deref().and_then(|r| app.repo_path(r)) {
                    Some(path) if path.exists() => {
                        app.exit = Some(Exit::Shell(path));
                        return Ok(app.exit);
                    }
                    Some(_) => {
                        app.message = "not cloned — press s to sync".to_string();
                    }
                    None => app.message = "shell: put the cursor on a repo row".to_string(),
                }
            }
            KeyCode::Char('f')
                if matches!(
                    app.view,
                    View::Fleet | View::Changeset | View::Ci | View::Grep
                ) =>
            {
                match app.cursor_repo() {
                    Some(repo) => {
                        let cloned = app
                            .repo_path(&repo)
                            .is_some_and(|p| p.join(".git").exists());
                        if !cloned {
                            app.message = "not cloned — showing forge view".to_string();
                        }
                        open_files(&mut app, jobs, &repo, !cloned);
                    }
                    None => app.message = "files: put the cursor on a repo row".to_string(),
                }
            }
            KeyCode::Enter => match app.view {
                View::Stacks => {
                    if let Some(stack) = app.stack_rows().get(selected).map(|s| s.to_string()) {
                        app.stack = Some(stack);
                        app.goto_view(View::Fleet);
                    }
                }
                View::Changesets => {
                    if let Some(id) = app.changeset_rows().get(selected).map(|c| c.id.clone()) {
                        open_changeset(&mut app, jobs, &id);
                    }
                }
                View::Fleet => {
                    if let Some(repo) = app.cursor_repo() {
                        open_repo_detail(&mut app, jobs, &repo);
                    }
                }
                View::Files => {
                    if let Some(entry) = app.file_rows().get(selected).map(|e| (*e).clone()) {
                        if entry.is_dir {
                            if app.files_subpath.is_empty() {
                                app.files_subpath = entry.name;
                            } else {
                                app.files_subpath = format!("{}/{}", app.files_subpath, entry.name);
                            }
                            reload_files(&mut app, jobs);
                        } else {
                            open_file_content(&mut app, jobs, &entry.name);
                        }
                    }
                }
                View::Prs => {
                    if let Some((repo, number, title)) = app
                        .pr_rows()
                        .get(selected)
                        .map(|p| (p.repo.clone(), p.number, p.title.clone()))
                    {
                        open_pr_detail(&mut app, jobs, &repo, number, &title);
                    }
                }
                View::Ci => {
                    if let Some((repo, id, name)) = app
                        .ci_rows()
                        .get(selected)
                        .map(|r| (r.repo.clone(), r.id, r.name.clone()))
                    {
                        open_ci_detail(&mut app, jobs, &repo, id, &name);
                    }
                }
                View::PrFiles => {
                    if let Some(path) = app.pr_file_rows().get(selected).map(|e| e.path.clone()) {
                        open_pr_file_content(&mut app, jobs, &path);
                    }
                }
                View::Grep => {
                    if let Some(hit) = app.grep_rows().get(selected).map(|h| (*h).clone()) {
                        open_grep_hit(&mut app, jobs, &hit);
                    }
                }
                View::Plugins => {
                    if let Some(name) = app.panel_rows().get(selected).map(|p| p.name.clone()) {
                        open_plugin_render(&mut app, jobs, &name);
                    }
                }
                _ => {}
            },
            KeyCode::Char('s') if app.view == View::Fleet => {
                if !app.selected_repos.is_empty() {
                    let repos = app.selected_repos.clone();
                    app.message = format!("→ haw sync ({} marked repos)", repos.len());
                    dispatch(&mut app, jobs, "sync", ActionKind::SyncRepos(repos));
                } else if let Some(repo) = app.cursor_repo() {
                    app.message = format!("→ haw sync ({repo})");
                    dispatch(&mut app, jobs, "sync", ActionKind::SyncRepo(repo));
                } else if let Some(stack) = app.stack.clone() {
                    app.message = format!("→ haw sync --stack {stack}");
                    dispatch(&mut app, jobs, "sync", ActionKind::SyncStack(stack));
                }
            }
            KeyCode::Char('s') if app.view == View::Stacks => {
                if let Some(stack) = app.stack_rows().get(selected).map(|s| s.to_string()) {
                    app.message = format!("→ haw sync --stack {stack}");
                    dispatch(&mut app, jobs, "sync", ActionKind::SyncStack(stack));
                }
            }
            KeyCode::Char('S') => {
                let target = match app.view {
                    View::Stacks => app.stack_rows().get(selected).map(|s| s.to_string()),
                    _ => None,
                };
                match target {
                    Some(stack) => {
                        app.message = format!("→ haw switch {stack}");
                        app.stack = Some(stack.clone());
                        dispatch(&mut app, jobs, "switch", ActionKind::Switch(stack));
                    }
                    None => app.goto_view(View::Stacks),
                }
            }
            KeyCode::Char('p') if app.view == View::Fleet => {
                app.problems_only = !app.problems_only;
                app.clamp_cursor();
                app.message = if app.problems_only {
                    "problems-only filter on".to_string()
                } else {
                    "problems-only filter off".to_string()
                };
            }
            KeyCode::Char('p') if app.view == View::Stacks => {
                app.message = "→ haw pin".to_string();
                dispatch(&mut app, jobs, "pin", ActionKind::Pin);
            }
            KeyCode::Char('!') if matches!(app.view, View::Fleet | View::RepoDetail) => {
                let repo = match app.view {
                    View::RepoDetail => app.files_repo.clone(),
                    _ => app.cursor_repo(),
                };
                match repo {
                    Some(repo) => app.input = InputMode::Exec(repo, String::new()),
                    None => app.message = "exec: put the cursor on a repo row".to_string(),
                }
            }
            KeyCode::Char('F') if matches!(app.view, View::Fleet | View::RepoDetail) => {
                let repo = match app.view {
                    View::RepoDetail => app.files_repo.clone(),
                    _ => app.cursor_repo(),
                };
                match repo {
                    Some(repo) => {
                        app.message = format!("→ git fetch ({repo})");
                        dispatch(&mut app, jobs, "fetch", ActionKind::RepoFetch(repo));
                    }
                    None => app.message = "fetch: put the cursor on a repo row".to_string(),
                }
            }
            KeyCode::Char('l') if app.view == View::Fleet || app.view == View::Stacks => {
                app.message = "→ haw lock".to_string();
                dispatch(&mut app, jobs, "lock", ActionKind::Lock);
            }
            KeyCode::Char('r') => {
                app.input = InputMode::Command("run ".to_string());
            }
            KeyCode::Char('n') if app.view == View::Changesets || app.view == View::Changeset => {
                app.input = InputMode::NewChangeset(String::new());
            }
            KeyCode::Char(' ') if app.view == View::Changeset || app.view == View::Fleet => {
                if let Some(repo) = app.cursor_repo() {
                    if let Some(found) = app.selected_repos.iter().position(|r| r == &repo) {
                        app.selected_repos.remove(found);
                    } else {
                        app.selected_repos.push(repo);
                    }
                }
            }
            KeyCode::Char('R') if app.view == View::Changeset => {
                if let Some(id) = app.changeset.clone() {
                    let only = if app.selected_repos.is_empty() {
                        None
                    } else {
                        Some(app.selected_repos.clone())
                    };
                    app.pending_confirm = Some(Confirm::Request(id, only));
                }
            }
            KeyCode::Char('L') if app.view == View::Changeset => {
                if let Some(id) = app.changeset.clone() {
                    app.pending_confirm = Some(Confirm::Land(id));
                }
            }
            KeyCode::Char('M') if app.view == View::Prs || app.view == View::PrDetail => {
                match app.current_pr() {
                    Some(_) if !app.current_pr_writable() => {
                        if let Some((_, number, _)) = app.current_pr() {
                            let state = app.current_pr_state().unwrap_or_default();
                            app.message = format!("PR #{number} is already {state}");
                        }
                    }
                    Some((repo, number, title)) => {
                        app.pending_confirm = Some(Confirm::MergePr {
                            repo,
                            number,
                            title,
                        });
                    }
                    None => app.message = "merge: put the cursor on a PR row".to_string(),
                }
            }
            KeyCode::Char('A') if app.view == View::Prs || app.view == View::PrDetail => {
                match app.current_pr() {
                    Some(_) if !app.current_pr_writable() => {
                        if let Some((_, number, _)) = app.current_pr() {
                            let state = app.current_pr_state().unwrap_or_default();
                            app.message = format!("PR #{number} is already {state}");
                        }
                    }
                    Some((repo, number, title)) => {
                        app.pending_confirm = Some(Confirm::ApprovePr {
                            repo,
                            number,
                            title,
                        });
                    }
                    None => app.message = "approve: put the cursor on a PR row".to_string(),
                }
            }
            KeyCode::Char('C') if app.view == View::Prs || app.view == View::PrDetail => {
                match app.current_pr() {
                    Some(_) if !app.current_pr_writable() => {
                        if let Some((_, number, _)) = app.current_pr() {
                            let state = app.current_pr_state().unwrap_or_default();
                            app.message = format!("PR #{number} is already {state}");
                        }
                    }
                    Some((repo, number, title)) => {
                        app.pending_confirm = Some(Confirm::CheckoutPr {
                            repo,
                            number,
                            title,
                        });
                    }
                    None => app.message = "checkout: put the cursor on a PR row".to_string(),
                }
            }
            KeyCode::Char('d') if app.view == View::Prs || app.view == View::PrDetail => {
                match app.current_pr() {
                    Some((repo, number, _)) => open_pr_diff(&mut app, jobs, &repo, number),
                    None => app.message = "diff: put the cursor on a PR row".to_string(),
                }
            }
            KeyCode::Char('f') if app.view == View::Prs || app.view == View::PrDetail => {
                match app.current_pr() {
                    Some((repo, number, title)) => {
                        open_pr_files(&mut app, jobs, &repo, number, &title)
                    }
                    None => app.message = "files: put the cursor on a PR row".to_string(),
                }
            }
            KeyCode::Char('l') if app.view == View::Ci => {
                let run = app
                    .ci_rows()
                    .get(selected)
                    .map(|r| (r.repo.clone(), r.id, r.name.clone()));
                match run {
                    Some((repo, id, name)) => open_ci_logs(&mut app, jobs, &repo, id, &name),
                    None => app.message = "logs: put the cursor on a CI row".to_string(),
                }
            }
            KeyCode::Char('l') if app.view == View::CiDetail => match &app.detail_ci {
                Some((repo, id, name)) => {
                    let (repo, id, name) = (repo.clone(), *id, name.clone());
                    open_ci_logs(&mut app, jobs, &repo, id, &name);
                }
                None => app.message = "logs: no CI run in view".to_string(),
            },
            // A hinted key that fell through every guard is advertised somewhere
            // but does nothing here — tell the user rather than no-op silently.
            // Truly unbound keys stay quiet.
            KeyCode::Char(c) if key_hinted_elsewhere(app.view, c) => {
                app.message = "not available in this view — press ? for all keys".to_string();
            }
            _ => {}
        }
    }
}

/// Whether `c` is a key the cockpit advertises in some view but not the current
/// one — used to give feedback when a hinted-but-inapplicable key is pressed.
/// Keeps global keys (handled everywhere) out so they never trip this path.
fn key_hinted_elsewhere(current: View, c: char) -> bool {
    // Keys wired globally in the event loop, handled in every view.
    const GLOBAL: &[char] = &[
        'q', '?', '/', ':', 'b', 'j', 'k', 'g', 'S', 'r', 't', 'c', 'm', 'i', 'v', 'P', 'E',
    ];
    if GLOBAL.contains(&c) {
        return false;
    }
    let key = c.to_string();
    let hinted_here = key_hints(current)
        .iter()
        .any(|(k, _)| hint_key_tokens(k).contains(&key.as_str()));
    if hinted_here {
        return false;
    }
    ALL_HINTED_VIEWS.iter().any(|&v| {
        key_hints(v)
            .iter()
            .any(|(k, _)| hint_key_tokens(k).contains(&key.as_str()))
    })
}

/// All views, for scanning hint labels. Mirrors the test's `ALL_VIEWS`.
const ALL_HINTED_VIEWS: [View; 16] = [
    View::Stacks,
    View::Fleet,
    View::Changesets,
    View::Changeset,
    View::Tree,
    View::Prs,
    View::Ci,
    View::Governance,
    View::Plugins,
    View::Errors,
    View::RepoDetail,
    View::PrDetail,
    View::CiDetail,
    View::Files,
    View::PrFiles,
    View::Grep,
];

/// Split a hint's key label into its individual key tokens. A label may pack
/// several keys — `"enter open · R remote · b up"` advertises `enter`, `R`, `b`;
/// `"j/k"` advertises `j` and `k`; `"<>"` advertises `<` and `>`. Each `·`-part's
/// leading whitespace-delimited token is the key.
fn hint_key_tokens(label: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    for part in label.split('·') {
        let Some(first) = part.split_whitespace().next() else {
            continue;
        };
        // Split combined single-char labels like `j/k` and `<>` into keys.
        if first == "j/k" {
            tokens.push("j");
            tokens.push("k");
        } else if first == "<>" {
            tokens.push("<");
            tokens.push(">");
        } else if first == "PgUp/PgDn" {
            tokens.push("PgUp/PgDn");
        } else {
            tokens.push(first);
        }
    }
    tokens
}

fn view_name(app: &App, view: View) -> String {
    match view {
        View::Stacks => "stacks".to_string(),
        View::Fleet => "fleet".to_string(),
        View::Changesets => "changesets".to_string(),
        View::Changeset => format!("change {}", app.changeset.as_deref().unwrap_or("—")),
        View::Tree => "tree".to_string(),
        View::Prs => "pr/mr".to_string(),
        View::Ci => "ci".to_string(),
        View::Governance => "governance".to_string(),
        View::Plugins => "plugins".to_string(),
        View::Errors => "errors".to_string(),
        View::Files => {
            let repo = app.files_repo.as_deref().unwrap_or("—");
            let scope = if app.files_remote { "forge" } else { "local" };
            if app.files_subpath.is_empty() {
                format!("files {repo} ({scope})")
            } else {
                format!("files {repo}:/{} ({scope})", app.files_subpath)
            }
        }
        View::RepoDetail | View::PrDetail | View::CiDetail => app.detail_title.clone(),
        View::PrFiles => match &app.pr_files_pr {
            Some((repo, number, _)) => format!("PR files {repo}#{number}"),
            None => "PR files".to_string(),
        },
        View::Grep => {
            if app.grep_pattern.is_empty() {
                "grep".to_string()
            } else {
                format!("grep {}", app.grep_pattern)
            }
        }
    }
}

/// Max scroll offset for the shared detail view: total lines minus one.
fn detail_max_scroll(app: &App) -> u16 {
    let lines = app
        .detail_text
        .as_deref()
        .map_or(0, |report| report.lines().count());
    u16::try_from(lines.saturating_sub(1)).unwrap_or(u16::MAX)
}

fn key_hints(view: View) -> &'static [(&'static str, &'static str)] {
    match view {
        View::Stacks => &[
            ("enter", "open fleet"),
            ("s", "sync stack"),
            ("S", "switch stack"),
            ("p", "pin"),
            ("l", "lock"),
            ("c", "changesets"),
            ("t", "tree"),
            ("/", "filter"),
            (":", "cmd"),
            ("?", "help"),
        ],
        View::Fleet => &[
            ("s", "sync"),
            ("F", "fetch"),
            ("S", "switch stack"),
            ("space", "mark"),
            ("p", "problems"),
            ("l", "lock"),
            ("!", "exec"),
            ("c", "changesets"),
            ("m", "PRs"),
            ("i", "CI"),
            ("v", "governance"),
            ("P", "plugins"),
            ("E", "errors"),
            ("r", "run"),
            ("g", "goto"),
            ("x", "shell"),
            ("f", "files"),
            ("t", "tree"),
            ("<>", "sort"),
            (".", "dir"),
            ("PgUp/PgDn", "page"),
            ("/", "filter"),
            (":", "cmd"),
        ],
        View::Changesets => &[
            ("enter", "open"),
            ("n", "new"),
            ("b", "back"),
            ("/", "filter"),
            ("?", "help"),
            ("q", "quit"),
        ],
        View::Changeset => &[
            ("space", "select"),
            ("R", "request PR"),
            ("L", "land"),
            ("n", "new"),
            ("g", "goto"),
            ("f", "files"),
            ("PgUp/PgDn", "page"),
            ("b", "back"),
            ("/", "filter"),
            (":", "cmd"),
        ],
        View::Tree => &[("b", "back"), ("q", "quit")],
        View::Prs => &[
            ("enter", "detail"),
            ("d", "diff"),
            ("M", "merge"),
            ("A", "approve"),
            ("C", "checkout"),
            ("o", "open in browser"),
            ("m", "refetch"),
            ("i", "CI runs"),
            ("g", "goto"),
            ("x", "shell"),
            ("f", "files"),
            ("<>", "sort"),
            (".", "dir"),
            ("PgUp/PgDn", "page"),
            ("b", "back"),
            ("/", "filter"),
            ("?", "help"),
        ],
        View::Ci => &[
            ("enter", "detail"),
            ("l", "logs"),
            ("o", "open in browser"),
            ("i", "refetch"),
            ("m", "PR/MRs"),
            ("g", "goto"),
            ("x", "shell"),
            ("f", "files"),
            ("<>", "sort"),
            (".", "dir"),
            ("PgUp/PgDn", "page"),
            ("b", "back"),
            ("/", "filter"),
            ("?", "help"),
        ],
        View::Governance => &[
            ("o", "open artifact"),
            ("v", "refetch"),
            ("b", "back"),
            ("/", "filter"),
            ("?", "help"),
        ],
        View::Plugins => &[
            ("enter", "render panel"),
            ("P", "refetch"),
            ("b", "back"),
            ("/", "filter"),
            ("?", "help"),
        ],
        View::Errors => &[("b", "back"), ("/", "filter"), ("?", "help"), ("q", "quit")],
        View::PrDetail => &[
            ("j/k", "scroll"),
            ("d", "diff"),
            ("f", "files"),
            ("M", "merge"),
            ("A", "approve"),
            ("C", "checkout"),
            ("b", "back"),
            ("q", "quit"),
        ],
        View::CiDetail => &[
            ("j/k", "scroll"),
            ("l", "logs"),
            ("b", "back"),
            ("q", "quit"),
        ],
        View::RepoDetail => &[
            ("j/k", "scroll"),
            ("x", "shell"),
            ("!", "exec"),
            ("F", "fetch"),
            ("b", "back"),
            ("q", "quit"),
        ],
        View::Grep => &[
            ("enter", "open hit"),
            ("g", "goto"),
            ("x", "shell"),
            ("f", "files"),
            ("PgUp/PgDn", "page"),
            ("b", "back"),
            ("/", "filter"),
            (":", "cmd"),
            ("?", "help"),
        ],
        View::Files => &[
            ("enter", "open"),
            ("R", "remote"),
            ("b", "up / back"),
            ("x", "shell"),
            ("/", "filter"),
            ("q", "quit"),
        ],
        View::PrFiles => &[
            ("enter", "read file"),
            ("b", "back"),
            ("/", "filter"),
            ("?", "help"),
            ("q", "quit"),
        ],
    }
}

fn draw(frame: &mut Frame, app: &mut App) {
    let zones = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    // The page step for PageDown/PageUp: the table body height (content area
    // minus the panel's top/bottom border and the header row). A floor keeps it
    // useful on tiny terminals.
    app.page_size = usize::from(zones[1].height).saturating_sub(3).max(1);

    draw_header(frame, app, zones[0]);
    match app.view {
        View::Stacks => draw_stacks(frame, app, zones[1]),
        View::Fleet => draw_fleet(frame, app, zones[1]),
        View::Changesets => draw_changesets(frame, app, zones[1]),
        View::Changeset => draw_changeset(frame, app, zones[1]),
        View::Tree => draw_tree(frame, app, zones[1]),
        View::Prs => draw_prs(frame, app, zones[1]),
        View::Ci => draw_ci(frame, app, zones[1]),
        View::Governance => draw_governance(frame, app, zones[1]),
        View::Plugins => draw_plugins(frame, app, zones[1]),
        View::Errors => draw_errors(frame, app, zones[1]),
        View::Files => draw_files(frame, app, zones[1]),
        View::PrFiles => draw_pr_files(frame, app, zones[1]),
        View::Grep => draw_grep(frame, app, zones[1]),
        View::RepoDetail | View::PrDetail | View::CiDetail => draw_detail(frame, app, zones[1]),
    }
    draw_status(frame, app, zones[2]);
    draw_crumbs(frame, app, zones[3]);

    if let Some(output) = &app.output {
        draw_output(frame, output);
    }
    if let Some(confirm) = &app.pending_confirm {
        draw_confirm(frame, confirm);
    }
    if app.help {
        draw_help(frame, app.help_scroll);
    }
}

fn draw_output(frame: &mut Frame, output: &str) {
    let area = frame.area();
    let width = area.width.saturating_sub(8).max(20);
    let height = area.height.saturating_sub(6).max(6);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    let visible = height.saturating_sub(2) as usize;
    let all_lines: Vec<&str> = output.lines().collect();
    let shown = if all_lines.len() > visible {
        &all_lines[all_lines.len() - visible..]
    } else {
        &all_lines[..]
    };
    let text: Vec<Line> = shown
        .iter()
        .map(|l| Line::styled((*l).to_string(), Style::default().fg(theme::text())))
        .collect();
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(text)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::teal()))
                .title(Span::styled(
                    format!(" output ({} lines) — any key closes ", all_lines.len()),
                    Style::default()
                        .fg(theme::mauve())
                        .add_modifier(Modifier::BOLD),
                )),
        ),
        popup,
    );
}

fn draw_confirm(frame: &mut Frame, confirm: &Confirm) {
    let (command, reach, detail) = match confirm {
        Confirm::Land(id) => (
            format!("haw change land {id}"),
            "this reaches the network:",
            "merge the PR/MRs in dependency order".to_string(),
        ),
        Confirm::Request(id, only) => (
            format!("haw change request {id}"),
            "this reaches the network:",
            match only {
                Some(repos) => format!(
                    "open PR/MRs for {} repo(s): {}",
                    repos.len(),
                    repos.join(", ")
                ),
                None => "open PR/MRs for every repo in the changeset".to_string(),
            },
        ),
        Confirm::MergePr {
            repo,
            number,
            title,
        } => (
            format!("haw merge PR #{number} ({repo})"),
            "this reaches the network:",
            format!("merge the PR/MR on its forge — {title}"),
        ),
        Confirm::ApprovePr {
            repo,
            number,
            title,
        } => (
            format!("haw approve PR #{number} ({repo})"),
            "this reaches the network:",
            format!("approve the PR/MR on its forge — {title}"),
        ),
        Confirm::CheckoutPr {
            repo,
            number,
            title,
        } => (
            format!("haw checkout PR #{number} ({repo})"),
            "this fetches and switches the worktree:",
            format!("check out the PR/MR branch locally as haw-pr-{number} — {title}"),
        ),
        Confirm::MergeCleanup(repo) => (
            format!("haw merge cleanup --repo {repo}"),
            "this commits and rewrites branches:",
            "seal the merge and fast-forward its target branch".to_string(),
        ),
    };
    let area = frame.area();
    let width = area.width.min(64);
    let height = 7.min(area.height);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    let text = vec![
        Line::from(vec![
            Span::styled(
                command,
                Style::default()
                    .fg(theme::yellow())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" — {reach}"), Style::default().fg(theme::text())),
        ]),
        Line::raw(""),
        Line::styled(format!(" {detail}"), Style::default().fg(theme::text())),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "y",
                Style::default()
                    .fg(theme::green())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("/enter confirm   ", Style::default().fg(theme::dim())),
            Span::styled(
                "any other key",
                Style::default()
                    .fg(theme::red())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" cancels", Style::default().fg(theme::dim())),
        ]),
    ];
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(text)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::yellow()))
                .title(Span::styled(
                    " confirm ",
                    Style::default()
                        .fg(theme::yellow())
                        .add_modifier(Modifier::BOLD),
                )),
        ),
        popup,
    );
}

fn kv(key: &str, value: Span<'static>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {key:<12}"), Style::default().fg(theme::dim())),
        value,
    ])
}

/// The width one hint cell occupies: `<key>` in a fixed field plus a
/// left-padded label field. Both fields are sized to the widest entry so the
/// cells align into a tidy grid, k9s-style.
fn hint_cell_widths(hints: &[(&str, &str)]) -> (usize, usize) {
    let key_w = hints
        .iter()
        .map(|(k, _)| k.chars().count() + 2) // +2 for the surrounding <>
        .max()
        .unwrap_or(3);
    let label_w = hints
        .iter()
        .map(|(_, l)| l.chars().count())
        .max()
        .unwrap_or(6);
    (key_w, label_w)
}

/// Lay the key hints out as an aligned grid that fits `width`. Columns are
/// even and sized to the widest cell; on narrow terminals the grid degrades to
/// fewer columns (down to one). A blank in each cell keeps `<key> label`
/// pairs from touching their neighbor.
fn hint_grid(hints: &[(&'static str, &'static str)], width: u16) -> Vec<Line<'static>> {
    let (key_w, label_w) = hint_cell_widths(hints);
    // One extra leading space + one trailing gutter space keeps cells apart.
    let cell_w = key_w + 1 + label_w + 1;
    let cols = (usize::from(width).saturating_sub(1) / cell_w.max(1)).max(1);

    let mut lines: Vec<Line> = Vec::new();
    for row in hints.chunks(cols) {
        let mut spans = Vec::new();
        for (key, label) in row {
            let padded_key = format!("{:<key_w$}", format!("<{key}>"));
            spans.push(Span::styled(
                format!(" {padded_key}"),
                Style::default()
                    .fg(theme::accent())
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!(" {label:<label_w$} "),
                Style::default().fg(theme::dim()),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(34),
            Constraint::Min(28),
            Constraint::Length(14),
        ])
        .split(area);

    let lock = if app.snapshot.lock_present {
        Span::styled("✓ committed", Style::default().fg(theme::green()))
    } else {
        Span::styled("✗ absent — run haw lock", Style::default().fg(theme::red()))
    };
    let mut info = vec![
        kv(
            "context:",
            Span::styled(
                app.snapshot.root_label.clone(),
                Style::default()
                    .fg(theme::text())
                    .add_modifier(Modifier::BOLD),
            ),
        ),
        kv(
            "stack:",
            Span::styled(
                app.stack.clone().unwrap_or_else(|| "—".to_string()),
                Style::default()
                    .fg(theme::accent())
                    .add_modifier(Modifier::BOLD),
            ),
        ),
        kv("lock:", lock),
        kv(
            "repos:",
            Span::styled(
                format!("{}", app.fleet_rows().len()),
                Style::default().fg(theme::text()),
            ),
        ),
        kv(
            "changesets:",
            Span::styled(
                format!("{}", app.snapshot.changesets.len()),
                Style::default().fg(theme::text()),
            ),
        ),
    ];
    if !app.errors.is_empty() {
        info.push(kv(
            "errors:",
            Span::styled(
                format!("⚠ {} — press E", app.errors.len()),
                Style::default()
                    .fg(theme::red())
                    .add_modifier(Modifier::BOLD),
            ),
        ));
    }
    frame.render_widget(Paragraph::new(Text::from(info)), columns[0]);

    let mut key_lines = hint_grid(key_hints(app.view), columns[1].width);
    key_lines.push(Line::from(vec![
        Span::styled(
            "<q>",
            Style::default()
                .fg(theme::red())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" quit ", Style::default().fg(theme::dim())),
        Span::styled(
            "<:>",
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cmd ", Style::default().fg(theme::dim())),
        Span::styled(
            "<?>",
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" all keys (scrollable)", Style::default().fg(theme::dim())),
    ]));
    frame.render_widget(Paragraph::new(Text::from(key_lines)), columns[1]);

    let logo = vec![
        Line::styled("┬ ┬┌─┐┬ ┬", Style::default().fg(theme::mauve())),
        Line::styled("├─┤├─┤│││", Style::default().fg(theme::mauve())),
        Line::styled("┴ ┴┴ ┴└┴┘", Style::default().fg(theme::mauve())),
        Line::styled(" cockpit ⚓", Style::default().fg(theme::dim())),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(logo)).alignment(Alignment::Right),
        columns[2],
    );
}

/// A `row N/T` cursor-position suffix for a row-based panel title, so the user
/// knows where they are in a long list. Empty when there are no rows.
fn row_indicator(app: &App, total: usize) -> String {
    if total == 0 {
        return String::new();
    }
    let n = app.cursor.selected().unwrap_or(0).min(total - 1) + 1;
    format!(" · row {n}/{total}")
}

fn panel(title: String) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::surface()))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(theme::mauve())
                .add_modifier(Modifier::BOLD),
        ))
}

fn header_row(cells: &[&'static str]) -> Row<'static> {
    sorted_header_row(cells, None)
}

/// A header row that shows a `▲`/`▼` caret on the header cell at the given
/// index. `active` is `(header-column-index, descending)`.
fn sorted_header_row(cells: &[&'static str], active: Option<(usize, bool)>) -> Row<'static> {
    Row::new(
        cells
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let text = match active {
                    Some((idx, desc)) if idx == i => {
                        format!("{c} {}", if desc { "▼" } else { "▲" })
                    }
                    _ => (*c).to_string(),
                };
                Cell::from(Span::styled(
                    text,
                    Style::default()
                        .fg(theme::accent())
                        .add_modifier(Modifier::BOLD),
                ))
            })
            .collect::<Vec<_>>(),
    )
}

fn cursor_style() -> Style {
    Style::default()
        .bg(theme::surface0())
        .add_modifier(Modifier::BOLD)
}

fn short(sha: &str) -> &str {
    sha.get(..8).unwrap_or(sha)
}

/// Whether a repo row is a problem the fleet grid should flag loudly: not
/// cloned, drifted from the lock, a dirty worktree, or behind its upstream.
fn has_problem(repo: &RepoStatus) -> bool {
    repo.missing
        || repo.drift
        || repo.dirty
        || repo.ahead_behind.is_some_and(|(_, behind)| behind > 0)
}

fn state_dot(repo: &RepoStatus) -> Span<'static> {
    let (dot, color) = if repo.missing || repo.drift {
        ("⚠", theme::red())
    } else if repo.dirty {
        ("●", theme::yellow())
    } else {
        ("●", theme::green())
    };
    Span::styled(dot, Style::default().fg(color))
}

/// Spans for `↑N ↓N`, green ahead / red behind; `—` without an upstream.
fn ahead_behind_spans(ahead_behind: Option<(u64, u64)>) -> Vec<Span<'static>> {
    match ahead_behind {
        None => vec![Span::styled("—", Style::default().fg(theme::dim()))],
        Some((0, 0)) => vec![Span::styled(
            "up to date",
            Style::default().fg(theme::dim()),
        )],
        Some((ahead, behind)) => {
            let mut spans = Vec::new();
            if ahead > 0 {
                spans.push(Span::styled(
                    format!("↑{ahead} "),
                    Style::default().fg(theme::green()),
                ));
            }
            if behind > 0 {
                spans.push(Span::styled(
                    format!("↓{behind}"),
                    Style::default().fg(theme::red()),
                ));
            }
            spans
        }
    }
}

fn ahead_behind_cell(ahead_behind: Option<(u64, u64)>) -> Line<'static> {
    match ahead_behind {
        Some((0, 0)) => Line::styled("·", Style::default().fg(theme::dim())),
        other => Line::from(ahead_behind_spans(other)),
    }
}

fn groups_label(groups: &[String]) -> (String, ratatui::style::Color) {
    if groups.is_empty() {
        ("—".to_string(), theme::dim())
    } else {
        (groups.join(","), theme::teal())
    }
}

fn draw_fleet(frame: &mut Frame, app: &mut App, area: Rect) {
    let zones = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(5)])
        .split(area);

    let rows: Vec<Row> = app
        .fleet_rows()
        .iter()
        .map(|repo| {
            let (groups, groups_color) = groups_label(&repo.groups);
            let marked = app.selected_repos.contains(&repo.name);
            let problem = has_problem(repo);
            // A problem row leads with a red `⚠`; a clean row keeps its dot.
            let mark_cell = || {
                if marked {
                    Cell::from(Span::styled("◉", Style::default().fg(theme::teal())))
                } else if problem && !repo.missing && !repo.drift {
                    // dirty / behind: state_dot renders a colored dot, but a
                    // problem must be loud — override with a red ⚠.
                    Cell::from(Span::styled("⚠", Style::default().fg(theme::red())))
                } else {
                    Cell::from(state_dot(repo))
                }
            };
            // The repo-name style: red+bold on any problem, else normal bold.
            let name_style = if problem {
                Style::default()
                    .fg(theme::red())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme::text())
                    .add_modifier(Modifier::BOLD)
            };
            let merge_cell = match app.merge_badge(&repo.name) {
                Some(badge) => Cell::from(Span::styled(
                    format!("{}/{}", badge.resolved, badge.total),
                    Style::default().fg(theme::yellow()),
                )),
                None => Cell::from(Span::styled("—", Style::default().fg(theme::dim()))),
            };
            if repo.missing {
                return Row::new(vec![
                    mark_cell(),
                    Cell::from(Span::styled(
                        repo.name.clone(),
                        Style::default().fg(theme::red()),
                    )),
                    Cell::from(Span::styled(groups, Style::default().fg(groups_color))),
                    Cell::from(Span::styled(
                        "not cloned — press s",
                        Style::default().fg(theme::dim()),
                    )),
                ]);
            }
            Row::new(vec![
                mark_cell(),
                Cell::from(Span::styled(repo.name.clone(), name_style)),
                Cell::from(Span::styled(groups, Style::default().fg(groups_color))),
                Cell::from(Span::styled(
                    repo.branch.clone().unwrap_or_else(|| "(detached)".into()),
                    Style::default().fg(theme::yellow()),
                )),
                Cell::from(Span::styled(
                    repo.head.as_deref().map_or("—", short).to_string(),
                    Style::default().fg(theme::dim()),
                )),
                Cell::from(if repo.dirty {
                    Span::styled("yes", Style::default().fg(theme::yellow()))
                } else {
                    Span::styled("·", Style::default().fg(theme::dim()))
                }),
                Cell::from(if repo.drift {
                    Span::styled("DRIFT", Style::default().fg(theme::red()))
                } else {
                    Span::styled("·", Style::default().fg(theme::dim()))
                }),
                Cell::from(ahead_behind_cell(repo.ahead_behind)),
                merge_cell,
            ])
        })
        .collect();

    let count = rows.len();
    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Min(10),
            Constraint::Min(12),
            Constraint::Min(14),
            Constraint::Length(9),
            // DIRTY/DRIFT hold a "▲"/"▼" sort caret when active — width 7.
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(9),
            Constraint::Length(7),
        ],
    )
    .header(sorted_header_row(
        &[
            "",
            "REPO",
            "GROUPS",
            "BRANCH",
            "HEAD",
            "DIRTY",
            "DRIFT",
            "↑ / ↓",
            "MERGE",
        ],
        sort_caret(View::Fleet, app.sort),
    ))
    .block(panel(if app.problems_only {
        format!(
            "fleet ⚠ problems ({count}/{}){}",
            app.fleet_total(),
            row_indicator(app, count)
        )
    } else {
        format!("fleet({count}){}", row_indicator(app, count))
    }))
    .row_highlight_style(cursor_style())
    .highlight_symbol(Span::styled("▍", Style::default().fg(theme::accent())));

    let mut state = TableState::default();
    state.select(app.cursor.selected());
    frame.render_stateful_widget(table, zones[0], &mut state);

    let detail = app
        .cursor
        .selected()
        .and_then(|i| app.fleet_rows().get(i).copied().cloned());
    let lines = match detail {
        Some(repo) => {
            let (groups, groups_color) = groups_label(&repo.groups);
            vec![
                Line::from(vec![
                    Span::styled(
                        format!(" {} ", repo.name),
                        Style::default()
                            .fg(theme::accent())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("groups ", Style::default().fg(theme::dim())),
                    Span::styled(groups, Style::default().fg(groups_color)),
                    Span::styled("  · path ", Style::default().fg(theme::dim())),
                    Span::styled(
                        repo.path.display().to_string(),
                        Style::default().fg(theme::text()),
                    ),
                ]),
                {
                    let mut spans = vec![
                        Span::styled(" locked ", Style::default().fg(theme::dim())),
                        Span::styled(
                            repo.locked_rev.as_deref().map_or("—", short).to_string(),
                            Style::default().fg(theme::text()),
                        ),
                        Span::styled("  · remote ", Style::default().fg(theme::dim())),
                    ];
                    spans.extend(ahead_behind_spans(repo.ahead_behind));
                    spans.push(Span::styled("  · ", Style::default().fg(theme::dim())));
                    spans.push(if repo.missing {
                        Span::styled("NOT CLONED", Style::default().fg(theme::red()))
                    } else if repo.drift {
                        Span::styled("DRIFT (head ≠ lock)", Style::default().fg(theme::red()))
                    } else if repo.dirty {
                        Span::styled("dirty worktree", Style::default().fg(theme::yellow()))
                    } else {
                        Span::styled("in sync ✓", Style::default().fg(theme::green()))
                    });
                    Line::from(spans)
                },
            ]
        }
        None => vec![Line::styled(
            " no repos — check haw.toml",
            Style::default().fg(theme::dim()),
        )],
    };
    let mut lines = lines;
    if let Some(repo) = app
        .cursor
        .selected()
        .and_then(|i| app.fleet_rows().get(i).map(|r| r.name.clone()))
        && let Some(badge) = app.merge_badge(&repo)
    {
        lines.push(Line::from(vec![
            Span::styled(" merge ", Style::default().fg(theme::mauve())),
            Span::styled(badge.source.clone(), Style::default().fg(theme::yellow())),
            Span::styled(
                format!("  {}/{} slices resolved", badge.resolved, badge.total),
                Style::default().fg(theme::text()),
            ),
            Span::styled(
                "  · :merge cleanup / :merge abort",
                Style::default().fg(theme::dim()),
            ),
        ]));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(panel("detail".to_string())),
        zones[1],
    );
}

fn draw_stacks(frame: &mut Frame, app: &mut App, area: Rect) {
    let current = app.stack.clone();
    let counts: Vec<(String, usize)> = app
        .snapshot
        .fleet
        .iter()
        .map(|(name, repos)| (name.clone(), repos.len()))
        .collect();
    let items: Vec<ListItem> = app
        .stack_rows()
        .iter()
        .map(|name| {
            let is_current = current.as_deref() == Some(name);
            let marker = if is_current {
                Span::styled(
                    "▸ ",
                    Style::default()
                        .fg(theme::accent())
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  ")
            };
            let count = counts
                .iter()
                .find(|(n, _)| n == name)
                .map_or(0, |(_, c)| *c);
            let style = if is_current {
                Style::default()
                    .fg(theme::text())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::text())
            };
            ListItem::new(Line::from(vec![
                marker,
                Span::styled((*name).to_string(), style),
                Span::styled(
                    format!("  · {count} repos"),
                    Style::default().fg(theme::dim()),
                ),
            ]))
        })
        .collect();
    let count = items.len();
    let list = List::new(items)
        .block(panel(format!("stacks({count})")))
        .highlight_style(cursor_style());
    frame.render_stateful_widget(list, area, &mut app.cursor);
    if count == 0 {
        draw_empty_hint(frame, area, "no stacks — check haw.toml");
    }
}

fn draw_changesets(frame: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .changeset_rows()
        .iter()
        .map(|c| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {}", c.id),
                    Style::default()
                        .fg(theme::text())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  · {} repos", c.repos.len()),
                    Style::default().fg(theme::dim()),
                ),
            ]))
        })
        .collect();
    let count = items.len();
    let list = List::new(items)
        .block(panel(format!("changesets({count})")))
        .highlight_style(cursor_style());
    frame.render_stateful_widget(list, area, &mut app.cursor);
    if count == 0 {
        draw_empty_hint(frame, area, "no changesets — :change start <id>");
    }
}

fn pr_span(text: &str) -> Span<'static> {
    let color = if text.contains("open") {
        theme::green()
    } else if text.contains("merged") {
        theme::mauve()
    } else if text.contains("draft") {
        theme::peach()
    } else if text.contains("closed") {
        theme::red()
    } else {
        theme::dim()
    };
    Span::styled(text.to_string(), Style::default().fg(color))
}

/// Status emoji for a CI status word (`passed`/`failed`/`running`/…), used in
/// the CI list STATUS cell and detail headers. 2 cells wide — callers that
/// place it in a tight table column must budget for it.
fn ci_status_emoji(status: &str) -> &'static str {
    let lower = status.to_lowercase();
    if lower.contains("pass") || status.contains('✓') {
        "✅"
    } else if lower.contains("fail") || status.contains('✗') {
        "❌"
    } else if lower.contains("cancel") {
        "⏹"
    } else if lower.contains("queue") || lower.contains("pend") {
        "⏳"
    } else if lower.contains("run") {
        "🔄"
    } else {
        "•"
    }
}

/// Status emoji for a PR/MR state word, used in the PR list STATE cell and
/// detail headers.
fn pr_state_emoji(state: &str) -> &'static str {
    if state.contains("open") {
        "🟢"
    } else if state.contains("merged") {
        "🟣"
    } else if state.contains("draft") {
        "📝"
    } else if state.contains("closed") {
        "🔴"
    } else {
        "•"
    }
}

fn ci_span(text: &str) -> Span<'static> {
    let lower = text.to_lowercase();
    let color = if text.contains('✓') || lower.contains("pass") {
        theme::green()
    } else if lower.contains("fail") || text.contains('✗') {
        theme::red()
    } else if lower.contains("run")
        || lower.contains("pend")
        || lower.contains("queue")
        || text.contains('⏳')
    {
        theme::yellow()
    } else {
        theme::dim()
    };
    Span::styled(text.to_string(), Style::default().fg(color))
}

fn draw_changeset(frame: &mut Frame, app: &mut App, area: Rect) {
    let rows: Vec<Row> = app
        .change_repo_rows()
        .iter()
        .map(|repo| {
            let selected = app.selected_repos.contains(&repo.name);
            Row::new(vec![
                Cell::from(if selected {
                    Span::styled("◉", Style::default().fg(theme::teal()))
                } else {
                    Span::styled("·", Style::default().fg(theme::dim()))
                }),
                Cell::from(Span::styled(
                    repo.name.clone(),
                    Style::default()
                        .fg(theme::text())
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    repo.branch.clone(),
                    Style::default().fg(theme::yellow()),
                )),
                Cell::from(if repo.on_branch {
                    Span::styled("yes", Style::default().fg(theme::green()))
                } else {
                    Span::styled("NO", Style::default().fg(theme::red()))
                }),
                Cell::from(if repo.dirty {
                    Span::styled("yes", Style::default().fg(theme::yellow()))
                } else {
                    Span::styled("·", Style::default().fg(theme::dim()))
                }),
                Cell::from(Span::styled(
                    repo.head.as_deref().map_or("—", short).to_string(),
                    Style::default().fg(theme::dim()),
                )),
                Cell::from(Span::styled(
                    repo.forge.clone(),
                    Style::default().fg(if repo.forge == "github" {
                        theme::accent()
                    } else if repo.forge == "gitlab" {
                        theme::peach()
                    } else {
                        theme::dim()
                    }),
                )),
                Cell::from(pr_span(&repo.pr)),
                Cell::from(ci_span(&repo.ci)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Min(10),
            Constraint::Min(14),
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Length(9),
            Constraint::Length(7),
            Constraint::Min(12),
            Constraint::Min(10),
        ],
    )
    .header(header_row(&[
        "", "REPO", "BRANCH", "ON IT", "DIRTY", "HEAD", "FORGE", "PR / MR", "CI",
    ]))
    .block(panel(format!(
        "change {}{}",
        app.changeset.as_deref().unwrap_or_default(),
        row_indicator(app, app.change_repo_rows().len())
    )))
    .row_highlight_style(cursor_style())
    .highlight_symbol(Span::styled("▍", Style::default().fg(theme::accent())));

    let mut state = TableState::default();
    state.select(app.cursor.selected());
    frame.render_stateful_widget(table, area, &mut state);
}

fn forge_span(forge: &str) -> Span<'static> {
    let color = match forge {
        "github" => theme::accent(),
        "gitlab" => theme::peach(),
        _ => theme::dim(),
    };
    Span::styled(forge.to_string(), Style::default().fg(color))
}

fn draw_prs(frame: &mut Frame, app: &mut App, area: Rect) {
    let fetched = app.prs.is_some();
    let rows: Vec<Row> = app
        .pr_rows()
        .iter()
        .map(|pr| {
            Row::new(vec![
                Cell::from(Span::styled(
                    pr.repo.clone(),
                    Style::default()
                        .fg(theme::text())
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(forge_span(&pr.forge)),
                Cell::from(Span::styled(
                    format!("#{}", pr.number),
                    Style::default().fg(theme::yellow()),
                )),
                Cell::from(Span::styled(
                    pr.title.clone(),
                    Style::default().fg(theme::text()),
                )),
                Cell::from(pr_span(&format!(
                    "{} {}",
                    pr_state_emoji(&pr.state),
                    pr.state
                ))),
                Cell::from(if pr.approved {
                    Span::styled("✓", Style::default().fg(theme::green()))
                } else {
                    Span::styled("·", Style::default().fg(theme::dim()))
                }),
                Cell::from(ci_span(match pr.ci {
                    Some(true) => "✅ passed",
                    Some(false) => "❌ failed",
                    None => "—",
                })),
            ])
        })
        .collect();

    let count = rows.len();
    let table = Table::new(
        rows,
        [
            Constraint::Min(10),
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Min(24),
            // STATE: 2-cell emoji + " merged" (7) → widen from 7.
            Constraint::Length(9),
            Constraint::Length(5),
            // CI: 2-cell emoji + " passed" → widen from 9.
            Constraint::Length(10),
        ],
    )
    .header(sorted_header_row(
        &["REPO", "FORGE", "#", "TITLE", "STATE", "APPR", "CI"],
        sort_caret(View::Prs, app.sort),
    ))
    .block(panel(format!(
        "open PR/MRs({count}){}",
        row_indicator(app, count)
    )))
    .row_highlight_style(cursor_style())
    .highlight_symbol(Span::styled("▍", Style::default().fg(theme::accent())));

    let mut state = TableState::default();
    state.select(app.cursor.selected());
    frame.render_stateful_widget(table, area, &mut state);

    if count == 0 {
        draw_empty_hint(
            frame,
            area,
            if fetched {
                "no open PR/MRs across the fleet"
            } else if app.busy.is_some() {
                "fetching PR/MRs…"
            } else {
                "press m to fetch PR/MRs"
            },
        );
    }
}

fn draw_ci(frame: &mut Frame, app: &mut App, area: Rect) {
    let fetched = app.ci.is_some();
    let rows: Vec<Row> = app
        .ci_rows()
        .iter()
        .map(|run| {
            Row::new(vec![
                Cell::from(Span::styled(
                    run.repo.clone(),
                    Style::default()
                        .fg(theme::text())
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    run.name.clone(),
                    Style::default().fg(theme::text()),
                )),
                Cell::from(Span::styled(
                    run.branch.clone(),
                    Style::default().fg(theme::yellow()),
                )),
                Cell::from(Span::styled(
                    run.event.clone(),
                    Style::default().fg(theme::teal()),
                )),
                Cell::from(ci_span(&format!(
                    "{} {}",
                    ci_status_emoji(&run.status),
                    run.status
                ))),
            ])
        })
        .collect();

    let count = rows.len();
    let table = Table::new(
        rows,
        [
            Constraint::Min(10),
            Constraint::Min(18),
            Constraint::Min(14),
            Constraint::Length(13),
            // STATUS holds a 2-cell emoji + " cancelled" (9) — widen to fit.
            Constraint::Length(13),
        ],
    )
    .header(sorted_header_row(
        &["REPO", "WORKFLOW", "BRANCH", "EVENT", "STATUS"],
        sort_caret(View::Ci, app.sort),
    ))
    .block(panel(format!(
        "CI runs({count}){}",
        row_indicator(app, count)
    )))
    .row_highlight_style(cursor_style())
    .highlight_symbol(Span::styled("▍", Style::default().fg(theme::accent())));

    let mut state = TableState::default();
    state.select(app.cursor.selected());
    frame.render_stateful_widget(table, area, &mut state);

    if count == 0 {
        draw_empty_hint(
            frame,
            area,
            if fetched {
                "no recent CI runs across the fleet"
            } else if app.busy.is_some() {
                "fetching CI runs…"
            } else {
                "press i to fetch CI runs"
            },
        );
    }
}

/// Colored ✓/warn/✗ status for a plugin, derived from its findings' worst level.
fn gov_status_span(gov: &Governance, plugin: &str) -> Span<'static> {
    let mut worst = 0u8;
    for finding in gov.findings.iter().filter(|f| f.plugin == plugin) {
        let rank = match finding.level.as_str() {
            "error" => 2,
            "warn" => 1,
            _ => 0,
        };
        worst = worst.max(rank);
    }
    match worst {
        2 => ci_span("✗ error"),
        1 => ci_span("⏳ warn"),
        _ => ci_span("✓ ok"),
    }
}

/// Color for a finding level: green info, yellow warn, red error.
fn finding_color(level: &str) -> ratatui::style::Color {
    match level {
        "error" => theme::red(),
        "warn" => theme::yellow(),
        _ => theme::green(),
    }
}

fn draw_governance(frame: &mut Frame, app: &mut App, area: Rect) {
    let zones = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(8)])
        .split(area);

    let fetched = app.gov.is_some();
    let empty = Governance::default();
    let gov = app.gov.as_ref().unwrap_or(&empty);
    let rows: Vec<Row> = app
        .gov_rows()
        .iter()
        .map(|plugin| {
            let phases = if plugin.phases.is_empty() {
                "—".to_string()
            } else {
                plugin.phases.join(", ")
            };
            Row::new(vec![
                Cell::from(Span::styled(
                    plugin.name.clone(),
                    Style::default()
                        .fg(theme::text())
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(phases, Style::default().fg(theme::teal()))),
                Cell::from(gov_status_span(gov, &plugin.name)),
            ])
        })
        .collect();

    let count = rows.len();
    let table = Table::new(
        rows,
        [
            Constraint::Min(14),
            Constraint::Min(20),
            Constraint::Length(9),
        ],
    )
    .header(header_row(&["PLUGIN", "PHASES", "STATUS"]))
    .block(panel(format!(
        "governance({count}){}",
        row_indicator(app, count)
    )))
    .row_highlight_style(cursor_style())
    .highlight_symbol(Span::styled("▍", Style::default().fg(theme::accent())));

    let mut state = TableState::default();
    state.select(app.cursor.selected());
    frame.render_stateful_widget(table, zones[0], &mut state);

    if count == 0 {
        draw_empty_hint(
            frame,
            zones[0],
            if fetched {
                "no [plugins] registered — add them to haw.toml"
            } else if app.busy.is_some() {
                "fetching governance…"
            } else {
                "press v to fetch governance"
            },
        );
    }

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::styled(
        " artifacts",
        Style::default()
            .fg(theme::mauve())
            .add_modifier(Modifier::BOLD),
    ));
    if gov.artifacts.is_empty() {
        lines.push(Line::styled("   none", Style::default().fg(theme::dim())));
    } else {
        for artifact in &gov.artifacts {
            let (mark, color) = if artifact.exists {
                ("✓", theme::green())
            } else {
                ("✗", theme::red())
            };
            lines.push(Line::from(vec![
                Span::styled(format!("   {mark} "), Style::default().fg(color)),
                Span::styled(
                    format!("{:<10}", artifact.kind),
                    Style::default().fg(theme::teal()),
                ),
                Span::styled(artifact.path.clone(), Style::default().fg(theme::text())),
            ]));
        }
    }
    lines.push(Line::styled(
        " findings",
        Style::default()
            .fg(theme::mauve())
            .add_modifier(Modifier::BOLD),
    ));
    if gov.findings.is_empty() {
        lines.push(Line::styled("   none", Style::default().fg(theme::dim())));
    } else {
        for finding in &gov.findings {
            let color = finding_color(&finding.level);
            lines.push(Line::from(vec![
                Span::styled(
                    format!("   [{}] ", finding.level),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}: ", finding.plugin),
                    Style::default().fg(theme::dim()),
                ),
                Span::styled(finding.message.clone(), Style::default().fg(theme::text())),
            ]));
        }
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(panel("artifacts & findings".to_string())),
        zones[1],
    );
}

/// Centered dim hint inside an empty table body.
fn draw_empty_hint(frame: &mut Frame, area: Rect, hint: &str) {
    let body = Rect {
        x: area.x + 1,
        y: area.y + 2,
        width: area.width.saturating_sub(2),
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(Line::styled(
            hint.to_string(),
            Style::default().fg(theme::dim()),
        ))
        .alignment(Alignment::Center),
        body,
    );
}

fn draw_tree(frame: &mut Frame, app: &mut App, area: Rect) {
    let text: Vec<Line> = app
        .snapshot
        .tree
        .lines()
        .map(|l| Line::styled(l.to_string(), Style::default().fg(theme::text())))
        .collect();
    frame.render_widget(
        Paragraph::new(Text::from(text)).block(panel("tree".to_string())),
        area,
    );
    if app.snapshot.tree.trim().is_empty() {
        draw_empty_hint(frame, area, "no tree — check haw.toml");
    }
}

/// Style a single line of the plain-text git report, coloring section headers.
fn detail_line(raw: &str) -> Line<'static> {
    let style = if raw.starts_with("== ") {
        Style::default()
            .fg(theme::accent())
            .add_modifier(Modifier::BOLD)
    } else if raw.starts_with("-- ") {
        Style::default()
            .fg(theme::mauve())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::text())
    };
    Line::styled(raw.to_string(), style)
}

fn draw_files(frame: &mut Frame, app: &mut App, area: Rect) {
    let fetched = app.files_entries.is_some();
    let rows = app.file_rows();
    let items: Vec<ListItem> = rows
        .iter()
        .map(|entry| {
            let (glyph, color) = if entry.is_dir {
                ("▸ ", theme::accent())
            } else {
                ("  ", theme::text())
            };
            let name = if entry.is_dir {
                format!("{}/", entry.name)
            } else {
                entry.name.clone()
            };
            ListItem::new(Line::from(vec![
                Span::styled(glyph, Style::default().fg(theme::accent())),
                Span::styled(name, Style::default().fg(color)),
            ]))
        })
        .collect();
    let count = items.len();
    let repo = app.files_repo.as_deref().unwrap_or("—");
    let scope = if app.files_remote { "forge" } else { "local" };
    let crumb = if app.files_subpath.is_empty() {
        format!("{repo}:/")
    } else {
        format!("{repo}:/{}/", app.files_subpath)
    };
    let list = List::new(items)
        .block(panel(format!(
            "files {crumb} ({scope}){}",
            row_indicator(app, count)
        )))
        .highlight_style(cursor_style())
        .highlight_symbol("▍");
    let mut state = ListState::default();
    state.select(app.cursor.selected());
    frame.render_stateful_widget(list, area, &mut state);

    if count == 0 {
        draw_empty_hint(
            frame,
            area,
            if fetched {
                "empty directory"
            } else if app.busy.is_some() {
                "loading files…"
            } else {
                "no files"
            },
        );
    }
}

fn draw_pr_files(frame: &mut Frame, app: &mut App, area: Rect) {
    let fetched = app.pr_file_entries.is_some();
    let rows = app.pr_file_rows();
    let items: Vec<ListItem> = rows
        .iter()
        .map(|entry| {
            // A one-letter status glyph, GitHub-style: A/M/D/R.
            let (glyph, color) = match entry.status.as_str() {
                "added" => ("A", theme::green()),
                "removed" => ("D", theme::red()),
                "renamed" => ("R", theme::accent()),
                _ => ("M", theme::yellow()),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{glyph} "), Style::default().fg(color)),
                Span::styled(entry.path.clone(), Style::default().fg(theme::text())),
            ]))
        })
        .collect();
    let count = items.len();
    let crumb = match &app.pr_files_pr {
        Some((repo, number, _)) => format!("{repo}#{number}"),
        None => "—".to_string(),
    };
    let list = List::new(items)
        .block(panel(format!(
            "PR files {crumb}{}",
            row_indicator(app, count)
        )))
        .highlight_style(cursor_style())
        .highlight_symbol("▍");
    let mut state = ListState::default();
    state.select(app.cursor.selected());
    frame.render_stateful_widget(list, area, &mut state);

    if count == 0 {
        draw_empty_hint(
            frame,
            area,
            if fetched {
                "no changed files"
            } else if app.busy.is_some() {
                "loading changed files…"
            } else {
                "no files"
            },
        );
    }
}

fn draw_grep(frame: &mut Frame, app: &mut App, area: Rect) {
    let fetched = app.grep_hits.is_some();
    let rows: Vec<Row> = app
        .grep_rows()
        .iter()
        .map(|hit| {
            Row::new(vec![
                Cell::from(Span::styled(
                    hit.repo.clone(),
                    Style::default()
                        .fg(theme::text())
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    format!("{}:{}", hit.path, hit.line),
                    Style::default().fg(theme::teal()),
                )),
                Cell::from(Span::styled(
                    hit.text.trim_end().to_string(),
                    Style::default().fg(theme::text()),
                )),
            ])
        })
        .collect();

    let count = rows.len();
    let table = Table::new(
        rows,
        [
            Constraint::Min(10),
            Constraint::Min(20),
            Constraint::Min(20),
        ],
    )
    .header(header_row(&["REPO", "PATH:LINE", "TEXT"]))
    .block(panel(format!(
        "grep `{}`({count}){}",
        app.grep_pattern,
        row_indicator(app, count)
    )))
    .row_highlight_style(cursor_style())
    .highlight_symbol(Span::styled("▍", Style::default().fg(theme::accent())));

    let mut state = TableState::default();
    state.select(app.cursor.selected());
    frame.render_stateful_widget(table, area, &mut state);

    if count == 0 {
        draw_empty_hint(
            frame,
            area,
            if app.busy.is_some() {
                "grepping…"
            } else if fetched {
                "no matches across the fleet"
            } else {
                "run :grep <pattern>"
            },
        );
    }
}

fn draw_plugins(frame: &mut Frame, app: &mut App, area: Rect) {
    let fetched = app.panels.is_some();
    let rows: Vec<Row> = app
        .panel_rows()
        .iter()
        .map(|panel| {
            let phases = if panel.phases.is_empty() {
                "—".to_string()
            } else {
                panel.phases.join(", ")
            };
            Row::new(vec![
                Cell::from(Span::styled(
                    panel.name.clone(),
                    Style::default()
                        .fg(theme::text())
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(phases, Style::default().fg(theme::teal()))),
            ])
        })
        .collect();

    let count = rows.len();
    let table = Table::new(rows, [Constraint::Min(16), Constraint::Min(20)])
        .header(header_row(&["PLUGIN", "PHASES"]))
        .block(panel(format!(
            "plugins({count}){}",
            row_indicator(app, count)
        )))
        .row_highlight_style(cursor_style())
        .highlight_symbol(Span::styled("▍", Style::default().fg(theme::accent())));

    let mut state = TableState::default();
    state.select(app.cursor.selected());
    frame.render_stateful_widget(table, area, &mut state);

    if count == 0 {
        draw_empty_hint(
            frame,
            area,
            if app.busy.is_some() {
                "discovering plugins…"
            } else if fetched {
                "no plugins — register [plugins] in haw.toml or add haw-* to PATH"
            } else {
                "press P to discover plugin panels"
            },
        );
    }
}

fn draw_errors(frame: &mut Frame, app: &mut App, area: Rect) {
    let rows: Vec<Row> = app
        .error_rows()
        .iter()
        .map(|entry| {
            Row::new(vec![
                Cell::from(Span::styled(
                    format!("#{}", entry.when_seq),
                    Style::default().fg(theme::dim()),
                )),
                Cell::from(Span::styled(
                    entry.context.clone(),
                    Style::default()
                        .fg(theme::yellow())
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    entry.message.clone(),
                    Style::default().fg(theme::red()),
                )),
            ])
        })
        .collect();

    let count = rows.len();
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Min(14),
            Constraint::Min(20),
        ],
    )
    .header(header_row(&["#", "CONTEXT", "MESSAGE"]))
    .block(panel(format!(
        "errors({count}){}",
        row_indicator(app, count)
    )))
    .row_highlight_style(cursor_style())
    .highlight_symbol(Span::styled("▍", Style::default().fg(theme::accent())));

    let mut state = TableState::default();
    state.select(app.cursor.selected());
    frame.render_stateful_widget(table, area, &mut state);

    if count == 0 {
        draw_empty_hint(frame, area, "no errors this session");
    }
}

fn draw_detail(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = app.detail_title.clone();
    match &app.detail_text {
        Some(report) => {
            let text: Vec<Line> = report.lines().map(detail_line).collect();
            frame.render_widget(
                Paragraph::new(Text::from(text))
                    .scroll((app.detail_scroll, 0))
                    .block(panel(title)),
                area,
            );
        }
        None => {
            frame.render_widget(panel(title), area);
            draw_empty_hint(frame, area, "loading…");
        }
    }
}

/// Alternates every 4 ticks (~500ms at the 120ms poll cadence) for an input caret blink.
fn cursor_glyph(app: &App) -> &'static str {
    if app.tick % 8 < 4 { "▏" } else { " " }
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let caret = cursor_glyph(app);
    let line = match (&app.input, app.busy) {
        (InputMode::Filter(buffer), _) => Line::from(vec![
            Span::styled(
                " /",
                Style::default()
                    .fg(theme::mauve())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(buffer.clone(), Style::default().fg(theme::text())),
            Span::styled(caret, Style::default().fg(theme::text())),
            Span::styled(
                "   (live filter by name or group)",
                Style::default().fg(theme::dim()),
            ),
        ]),
        (InputMode::Command(buffer), _) => Line::from(vec![
            Span::styled(
                " ❯ ",
                Style::default()
                    .fg(theme::accent())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(buffer.clone(), Style::default().fg(theme::text())),
            Span::styled(caret, Style::default().fg(theme::text())),
            Span::styled(
                "   (mirrors the CLI — try: sync · switch <stack> · run <cmd> · tree · theme <name>)",
                Style::default().fg(theme::dim()),
            ),
        ]),
        (InputMode::NewChangeset(buffer), _) => Line::from(vec![
            Span::styled(" new changeset: ", Style::default().fg(theme::mauve())),
            Span::styled(buffer.clone(), Style::default().fg(theme::text())),
            Span::styled(caret, Style::default().fg(theme::text())),
        ]),
        (InputMode::Exec(repo, buffer), _) => Line::from(vec![
            Span::styled(
                " ! ",
                Style::default()
                    .fg(theme::peach())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(buffer.clone(), Style::default().fg(theme::text())),
            Span::styled(caret, Style::default().fg(theme::text())),
            Span::styled(
                format!("   (shell command in {repo})"),
                Style::default().fg(theme::dim()),
            ),
        ]),
        (InputMode::None, Some(label)) => Line::from(vec![
            Span::styled(
                format!(" {} ", SPINNER[app.spinner]),
                Style::default().fg(theme::accent()),
            ),
            Span::styled(format!("{label}…"), Style::default().fg(theme::text())),
        ]),
        (InputMode::None, None) => {
            let msg = &app.message;
            let color =
                if msg.contains("failed") || msg.contains("error") || msg.contains("unknown") {
                    theme::red()
                } else if msg.starts_with('→') {
                    theme::teal()
                } else {
                    theme::dim()
                };
            Line::styled(format!(" {msg}"), Style::default().fg(color))
        }
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_crumbs(frame: &mut Frame, app: &App, area: Rect) {
    // The version tag gets its own right-aligned cell so the breadcrumb trail can
    // never overwrite it; the crumbs render into the remaining left cell.
    let version = format!("⚓ haw v{} ", env!("CARGO_PKG_VERSION"));
    let version_w = u16::try_from(version.chars().count()).unwrap_or(u16::MAX);
    let cells = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(version_w)])
        .split(area);

    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for view in &app.back {
        spans.push(Span::styled(
            format!(" {} ", view_name(app, *view)),
            Style::default().fg(theme::dim()).bg(theme::surface0()),
        ));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        format!(" {} ", view_name(app, app.view)),
        Style::default()
            .fg(theme::crust())
            .bg(theme::accent())
            .add_modifier(Modifier::BOLD),
    ));
    if app.input == InputMode::None && !app.filter.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!(" filter: {} — esc clears ", app.filter),
            Style::default()
                .fg(theme::crust())
                .bg(theme::yellow())
                .add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), cells[0]);
    frame.render_widget(
        Paragraph::new(Line::styled(version, Style::default().fg(theme::dim())))
            .alignment(Alignment::Right),
        cells[1],
    );
}

fn help_entry(key: &'static str, desc: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {key:<10}"),
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(desc, Style::default().fg(theme::text())),
    ])
}

fn help_section(title: &'static str) -> Line<'static> {
    Line::styled(
        format!(" {title}"),
        Style::default()
            .fg(theme::mauve())
            .add_modifier(Modifier::BOLD),
    )
}

/// The dimensions of the `?` help popup for a given terminal area: a wider,
/// taller box so more keys fit before scrolling. One source of truth so the
/// scroll clamp and the renderer agree.
fn help_popup_rect(area: Rect) -> Rect {
    let width = area.width.min(78);
    let height = area.height.saturating_sub(2).max(6);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

/// Max help scroll for a terminal of `term_height`: total help lines minus the
/// popup's visible inner height (its height less the two border rows).
fn help_max_scroll(term_height: u16) -> u16 {
    let area = Rect {
        x: 0,
        y: 0,
        width: 1,
        height: term_height,
    };
    let visible = usize::from(help_popup_rect(area).height).saturating_sub(2);
    let total = help_lines().len();
    u16::try_from(total.saturating_sub(visible)).unwrap_or(u16::MAX)
}

/// The current terminal height, for clamping the help scroll while handling keys.
fn frame_height(terminal: &ratatui::DefaultTerminal) -> u16 {
    terminal.size().map(|s| s.height).unwrap_or(24)
}

/// All help lines, in order — every key and every `:` command. Shared by the
/// renderer and the scroll-clamp so they never disagree.
fn help_lines() -> Vec<Line<'static>> {
    vec![
        help_section("navigation"),
        help_entry("j / k", "move · enter drill in · esc/b back"),
        help_entry("q", "quit · ctrl-c force quit"),
        help_entry("F5", "refresh now · ctrl-r also works"),
        Line::raw(""),
        help_section("fleet"),
        help_entry(
            "enter",
            "drill into the repo's live git detail (scrollable)",
        ),
        help_entry("s", "sync marked repos, else cursor repo (or stack)"),
        help_entry("F", "git fetch the cursor repo (distinct from s sync)"),
        help_entry("space", "mark/unmark repo · s / r act on the marked set"),
        help_entry("p", "problems-only filter (⚠ dirty/drift/behind/missing)"),
        help_entry("!", "run a shell command in the cursor repo (detail view)"),
        help_entry("S", "switch stack · l lock"),
        help_entry("t", "tree · c changesets · r run · g goto"),
        help_entry("x", "drop into a shell in the repo (exits the cockpit)"),
        help_entry("f", "browse the repo's files (local disk or forge)"),
        help_entry("< >", "move sort column · . toggles asc/desc"),
        help_entry(
            "/",
            "fuzzy filter by name or group — reopens with your text",
        ),
        Line::raw(""),
        help_section("fleet-wide (network)"),
        help_entry("m", "open PR/MRs across every repo"),
        help_entry("i", "recent CI runs across every repo"),
        help_entry("v", "governance — plugins, artifacts, findings"),
        help_entry("enter", "drill into a PR/MR or CI run (scrollable)"),
        help_entry("d", "read the PR/MR's diff (scrollable)"),
        help_entry("l", "read the CI run/pipeline's logs (scrollable)"),
        help_entry("o", "open the row's PR / run / artifact"),
        help_entry("< > .", "sort PR/CI columns (. toggles asc/desc)"),
        help_entry("PgUp/PgDn", "page through long lists · ctrl-d/u also"),
        help_entry("M / A", "merge / approve the PR/MR (asks y/n)"),
        help_entry("C", "check out the PR/MR branch locally (asks y/n)"),
        Line::raw(""),
        help_section("changeset"),
        help_entry("n", "new · space select repos"),
        help_entry("space", "toggle a repo · R with no selection = all repos"),
        help_entry("R", "request PR/MRs (cross-linked, asks y/n)"),
        help_entry("L", "land in dependency order (asks y/n)"),
        help_entry("g", "goto the repo under the cursor"),
        Line::raw(""),
        help_section("files"),
        help_entry("enter", "open a dir or view a file (scrollable)"),
        help_entry("R", "toggle local disk / forge view"),
        help_entry("b", "up a directory, then back to the fleet"),
        Line::raw(""),
        help_section("collaborative merge (MERGE column)"),
        help_entry("MERGE", "resolved/total slices of an in-progress merge"),
        help_entry(":merge", "list merges in progress"),
        help_entry(":merge cleanup <repo>", "seal a resolved merge (asks y/n)"),
        help_entry(":merge abort <repo>", "abort a planned merge"),
        Line::raw(""),
        help_section("command bar"),
        help_entry(":sync", "· :switch NAME · :run CMD · :tree"),
        help_entry(":prs", "· :ci · :governance · :plugins · :help"),
        help_entry(":pin", "· :lock — pin HEADs / commit the lock"),
        help_entry(":change", "[ID | start ID | land ID | request ID]"),
        help_entry(":grep <pat>", "· :sh CMD · :problems · :fetch"),
        help_entry(":<repo>", "jump the fleet cursor to a matching repo"),
        help_entry(":theme <name>", "switch skin (catppuccin/dracula/nord/…)"),
        Line::raw(""),
        help_section("help overlay"),
        help_entry("j / k", "scroll · PgUp/PgDn page · any other key closes"),
    ]
}

fn draw_help(frame: &mut Frame, scroll: u16) {
    let popup = help_popup_rect(frame.area());
    let lines = help_lines();
    let visible = usize::from(popup.height).saturating_sub(2);
    let max = u16::try_from(lines.len().saturating_sub(visible)).unwrap_or(u16::MAX);
    let scroll = scroll.min(max);
    let more = if scroll < max { " ▾ more " } else { " " };
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .scroll((scroll, 0))
            .wrap(ratatui::widgets::Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme::accent()))
                    .title(Span::styled(
                        " help — j/k scroll ",
                        Style::default()
                            .fg(theme::mauve())
                            .add_modifier(Modifier::BOLD),
                    ))
                    .title_bottom(Span::styled(more, Style::default().fg(theme::dim()))),
            ),
        popup,
    );
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo(name: &str, groups: &[&str]) -> RepoStatus {
        RepoStatus {
            name: name.to_string(),
            path: PathBuf::from(name),
            missing: false,
            branch: Some("main".to_string()),
            head: Some("a".repeat(40)),
            dirty: false,
            locked_rev: Some("a".repeat(40)),
            drift: false,
            ahead_behind: Some((0, 0)),
            groups: groups.iter().map(|g| g.to_string()).collect(),
        }
    }

    fn app_with(snapshot: Snapshot) -> App {
        let mut cursor = ListState::default();
        cursor.select(Some(0));
        App {
            view: View::Fleet,
            back: Vec::new(),
            snapshot,
            stack: Some("gw".to_string()),
            changeset: None,
            selected_repos: Vec::new(),
            sort: None,
            cursor,
            input: InputMode::None,
            filter: String::new(),
            message: String::new(),
            busy: None,
            spinner: 0,
            tick: 0,
            help: false,
            help_scroll: 0,
            exit: None,
            files_repo: None,
            files_subpath: String::new(),
            files_remote: false,
            files_entries: None,
            pending_confirm: None,
            output: None,
            prs: None,
            ci: None,
            gov: None,
            detail_text: None,
            detail_title: "detail".to_string(),
            detail_scroll: 0,
            pending_scroll: None,
            files_return: None,
            detail_pr: None,
            detail_ci: None,
            page_size: 10,
            problems_only: false,
            grep_hits: None,
            grep_pattern: String::new(),
            panels: None,
            pr_file_entries: None,
            pr_files_pr: None,
            pr_files_return: None,
            errors: Vec::new(),
            error_seq: 0,
        }
    }

    fn fleet_app() -> App {
        let snap = Snapshot {
            stacks: vec!["gw".to_string()],
            fleet: vec![(
                "gw".to_string(),
                vec![
                    repo("kernel", &["firmware", "ci"]),
                    repo("hal", &["firmware"]),
                    repo("app-mqtt", &[]),
                ],
            )],
            paths: vec![("kernel".to_string(), PathBuf::from("/w/kernel"))],
            merges: vec![(
                "kernel".to_string(),
                MergeBadge {
                    source: "origin/feature".to_string(),
                    resolved: 1,
                    total: 2,
                },
            )],
            ..Default::default()
        };
        app_with(snap)
    }

    // ---- pure helpers -----------------------------------------------------

    #[test]
    fn repo_matches_name_and_groups() {
        let groups = vec!["firmware".to_string(), "ci".to_string()];
        assert!(repo_matches("kernel", &groups, ""));
        assert!(repo_matches("kernel", &groups, "kern"));
        assert!(repo_matches("kernel", &groups, "firm"));
        assert!(repo_matches("kernel", &groups, "ci"));
        assert!(!repo_matches("kernel", &groups, "zzz"));
    }

    #[test]
    fn short_is_multibyte_safe() {
        assert_eq!(short(&"a".repeat(40)), "aaaaaaaa");
        assert_eq!(short("abc"), "abc");
        assert_eq!(short(""), "");
        assert_eq!(short("é"), "é");
    }

    #[test]
    fn ahead_behind_spans_colors() {
        let text = |ab| {
            ahead_behind_spans(ab)
                .iter()
                .map(|s| s.content.to_string())
                .collect::<String>()
        };
        assert_eq!(text(None), "—");
        assert_eq!(text(Some((0, 0))), "up to date");
        assert_eq!(text(Some((2, 0))), "↑2 ");
        assert_eq!(text(Some((0, 3))), "↓3");
        assert_eq!(text(Some((2, 3))), "↑2 ↓3");
    }

    #[test]
    fn ahead_behind_cell_dot_when_even() {
        let line = ahead_behind_cell(Some((0, 0)));
        assert_eq!(line.spans[0].content, "·");
    }

    #[test]
    fn groups_label_empty_vs_set() {
        assert_eq!(groups_label(&[]).0, "—");
        assert_eq!(groups_label(&["a".to_string(), "b".to_string()]).0, "a,b");
    }

    #[test]
    fn cursor_glyph_blinks() {
        let mut app = fleet_app();
        app.tick = 0;
        assert_eq!(cursor_glyph(&app), "▏");
        app.tick = 4;
        assert_eq!(cursor_glyph(&app), " ");
        app.tick = 8;
        assert_eq!(cursor_glyph(&app), "▏");
    }

    #[test]
    fn view_name_and_key_hints_cover_all_views() {
        let app = fleet_app();
        for v in ALL_VIEWS {
            assert!(!view_name(&app, v).is_empty());
            assert!(!key_hints(v).is_empty());
        }
    }

    const ALL_VIEWS: [View; 16] = [
        View::Stacks,
        View::Fleet,
        View::Changesets,
        View::Changeset,
        View::Tree,
        View::Prs,
        View::Ci,
        View::Governance,
        View::Plugins,
        View::Errors,
        View::RepoDetail,
        View::PrDetail,
        View::CiDetail,
        View::Files,
        View::PrFiles,
        View::Grep,
    ];

    /// Every key advertised in `key_hints` must be one the event loop actually
    /// handles for that view — the hint bar must never lie.
    #[test]
    fn every_hinted_key_is_handled() {
        for view in ALL_VIEWS {
            for (key, label) in key_hints(view) {
                // A hint label may pack several keys (e.g. Files' compound
                // "enter open · R remote · b up"); every advertised token must
                // resolve to a real handler, not just the first.
                for token in hint_key_tokens(key) {
                    assert!(
                        key_is_handled(view, token),
                        "view {view:?} advertises <{token}> (in `{key}` — {label}) but never handles it"
                    );
                }
            }
        }
    }

    /// Whether a hinted key resolves to a real handler for the given view.
    fn key_is_handled(view: View, key: &str) -> bool {
        match key {
            "enter" | "space" | "?" | "/" | ":" | "b" | "q" | "j" | "k" => true,
            "j/k" => matches!(view, View::RepoDetail | View::PrDetail | View::CiDetail),
            "t" | "c" | "m" | "i" | "v" | "r" | "g" | "P" | "E" => true,
            "s" => matches!(view, View::Fleet | View::Stacks),
            "S" => true,
            "p" => matches!(view, View::Fleet | View::Stacks),
            "!" | "F" => matches!(view, View::Fleet | View::RepoDetail),
            "n" => matches!(view, View::Changesets | View::Changeset),
            "L" => view == View::Changeset,
            "R" => matches!(view, View::Changeset | View::Files),
            "f" => matches!(
                view,
                View::Fleet | View::Changeset | View::Prs | View::PrDetail | View::Ci | View::Grep
            ),
            "x" => matches!(
                view,
                View::Fleet
                    | View::RepoDetail
                    | View::Files
                    | View::Changeset
                    | View::Prs
                    | View::Ci
                    | View::Grep
            ),
            "M" | "A" | "C" => matches!(view, View::Prs | View::PrDetail),
            "d" => matches!(view, View::Prs | View::PrDetail),
            "l" => matches!(view, View::Fleet | View::Stacks | View::Ci | View::CiDetail),
            "o" => matches!(view, View::Prs | View::Ci | View::Governance),
            "<" | ">" | "<>" | "." => matches!(view, View::Fleet | View::Prs | View::Ci),
            "PgUp/PgDn" => {
                matches!(
                    view,
                    View::Fleet | View::Changeset | View::Prs | View::Ci | View::Grep
                )
            }
            _ => false,
        }
    }

    // ---- App methods ------------------------------------------------------

    #[test]
    fn fleet_rows_filter_by_name_and_group() {
        let mut app = fleet_app();
        assert_eq!(app.fleet_rows().len(), 3);
        app.filter = "hal".to_string();
        assert_eq!(app.fleet_rows().len(), 1);
        app.filter = "firmware".to_string();
        assert_eq!(app.fleet_rows().len(), 2);
        app.filter = "ci".to_string();
        assert_eq!(app.fleet_rows().len(), 1);
    }

    #[test]
    fn clamp_cursor_bounds_to_last_row() {
        let mut app = fleet_app();
        app.cursor.select(Some(99));
        app.clamp_cursor();
        assert_eq!(app.cursor.selected(), Some(2));
        app.filter = "hal".to_string();
        app.clamp_cursor();
        assert_eq!(app.cursor.selected(), Some(0));
    }

    #[test]
    fn merge_badge_and_repo_path_lookup() {
        let app = fleet_app();
        assert_eq!(app.merge_badge("kernel").map(|b| b.total), Some(2));
        assert!(app.merge_badge("hal").is_none());
        assert_eq!(app.repo_path("kernel"), Some(PathBuf::from("/w/kernel")));
        assert!(app.repo_path("ghost").is_none());
    }

    #[test]
    fn goto_view_and_back_restore_previous() {
        let mut app = fleet_app();
        app.filter = "hal".to_string();
        app.goto_view(View::Tree);
        assert_eq!(app.view, View::Tree);
        assert!(app.filter.is_empty());
        app.go_back();
        assert_eq!(app.view, View::Fleet);
    }

    // ---- command bar (the :change ambiguity fix) --------------------------

    fn drain(rx: &Receiver<Job>) -> Vec<&'static str> {
        let mut labels = Vec::new();
        while let Ok(Job::Action(label, _)) = rx.try_recv() {
            labels.push(label);
        }
        labels
    }

    #[test]
    fn change_status_does_not_start_a_changeset() {
        let mut app = fleet_app();
        let (tx, rx) = channel();
        run_command_bar(&mut app, &tx, "change status");
        // navigates to the changeset view, never dispatches change start
        assert_eq!(app.view, View::Changeset);
        assert_eq!(app.changeset.as_deref(), Some("status"));
        assert!(!drain(&rx).contains(&"change start"));
    }

    #[test]
    fn change_start_with_id_dispatches() {
        let mut app = fleet_app();
        let (tx, rx) = channel();
        run_command_bar(&mut app, &tx, "change start FEAT-9");
        assert_eq!(drain(&rx), vec!["change start"]);
    }

    #[test]
    fn change_land_and_request_ask_confirmation() {
        let mut app = fleet_app();
        let (tx, _rx) = channel();
        run_command_bar(&mut app, &tx, "change land FEAT-9");
        assert!(matches!(app.pending_confirm, Some(Confirm::Land(_))));
        app.pending_confirm = None;
        run_command_bar(&mut app, &tx, "change request FEAT-9");
        assert!(matches!(app.pending_confirm, Some(Confirm::Request(_, _))));
    }

    // ---- plugin panels + error log ----------------------------------------

    fn panel(name: &str, phases: &[&str]) -> PluginPanel {
        PluginPanel {
            name: name.to_string(),
            phases: phases.iter().map(|p| p.to_string()).collect(),
        }
    }

    #[test]
    fn selecting_a_panel_requests_a_render_job() {
        let mut app = fleet_app();
        app.panels = Some(vec![panel("compliance", &["post-build"])]);
        let (tx, rx) = channel();
        open_plugin_render(&mut app, &tx, "compliance");
        // The detail view opens and a PluginRender job is enqueued.
        assert_eq!(app.view, View::RepoDetail);
        assert_eq!(app.detail_title, "plugin: compliance");
        let job = rx.try_recv();
        assert!(
            matches!(job, Ok(Job::PluginRender(ref n)) if n == "compliance"),
            "expected a PluginRender job",
        );
    }

    #[test]
    fn panel_rows_filter_by_name_and_phase() {
        let mut app = fleet_app();
        app.panels = Some(vec![
            panel("compliance", &["post-build"]),
            panel("artifact", &["post-land"]),
        ]);
        assert_eq!(app.panel_rows().len(), 2);
        app.filter = "comp".to_string();
        assert_eq!(app.panel_rows().len(), 1);
        app.filter = "post-land".to_string();
        assert_eq!(app.panel_rows().len(), 1);
        app.filter = "zzz".to_string();
        assert_eq!(app.panel_rows().len(), 0);
    }

    #[test]
    fn push_error_increments_count_and_stamps_seq() {
        let mut app = fleet_app();
        assert!(app.errors.is_empty());
        app.push_error("sync", "boom");
        app.push_error("PR/MR fetch", "network down");
        assert_eq!(app.errors.len(), 2);
        assert_eq!(app.errors[0].when_seq, 1);
        assert_eq!(app.errors[1].when_seq, 2);
        assert_eq!(app.error_seq, 2);
    }

    #[test]
    fn error_rows_are_newest_first() {
        let mut app = fleet_app();
        app.push_error("sync", "first");
        app.push_error("fetch", "second");
        let rows = app.error_rows();
        assert_eq!(rows[0].message, "second");
        assert_eq!(rows[0].when_seq, 2);
        assert_eq!(rows[1].message, "first");
    }

    #[test]
    fn error_log_is_capped() {
        let mut app = fleet_app();
        for i in 0..(ERROR_LOG_CAP + 25) {
            app.push_error("op", format!("err {i}"));
        }
        assert_eq!(app.errors.len(), ERROR_LOG_CAP);
        // The oldest entries are dropped; the newest is kept.
        assert_eq!(app.error_seq, (ERROR_LOG_CAP + 25) as u64);
        assert_eq!(
            app.errors.last().map(|e| e.when_seq),
            Some((ERROR_LOG_CAP + 25) as u64)
        );
    }

    #[test]
    fn detail_error_outcome_pushes_an_error_entry() {
        // Simulate the outcome-loop error branch: a failed Detail both sets the
        // transient message and appends to the rolling error log.
        let mut app = fleet_app();
        app.detail_title = "plugin: compliance".to_string();
        let err = io::Error::other("render blew up");
        app.detail_text = Some(format!("failed to load detail: {err}"));
        app.message = format!("detail failed: {err}");
        app.push_error(&app.detail_title.clone(), err.to_string());
        assert_eq!(app.errors.len(), 1);
        assert_eq!(app.errors[0].context, "plugin: compliance");
        assert_eq!(app.errors[0].message, "render blew up");
        assert!(app.message.contains("detail failed"));
    }

    #[test]
    fn merge_cleanup_requires_a_planned_merge() {
        let mut app = fleet_app();
        let (tx, _rx) = channel();
        run_command_bar(&mut app, &tx, "merge cleanup kernel");
        assert!(matches!(
            app.pending_confirm,
            Some(Confirm::MergeCleanup(_))
        ));
        app.pending_confirm = None;
        run_command_bar(&mut app, &tx, "merge cleanup hal");
        assert!(app.pending_confirm.is_none());
        assert!(app.message.contains("no merge planned"));
    }

    fn pr(repo: &str, title: &str) -> FleetPr {
        FleetPr {
            repo: repo.to_string(),
            forge: "github".to_string(),
            number: 1,
            title: title.to_string(),
            url: format!("https://github.com/acme/{repo}/pull/1"),
            state: "open".to_string(),
            approved: false,
            ci: None,
        }
    }

    fn ci_run(repo: &str, name: &str, branch: &str) -> FleetCiRun {
        FleetCiRun {
            repo: repo.to_string(),
            id: 1,
            name: name.to_string(),
            branch: branch.to_string(),
            event: "push".to_string(),
            status: "passed".to_string(),
            url: format!("https://github.com/acme/{repo}/actions/runs/1"),
        }
    }

    #[test]
    fn pr_rows_filter_by_repo_and_title() {
        let mut app = fleet_app();
        app.prs = Some(vec![pr("kernel", "fix boot"), pr("hal", "add driver")]);
        assert_eq!(app.pr_rows().len(), 2);
        app.filter = "kern".to_string();
        assert_eq!(app.pr_rows().len(), 1);
        app.filter = "driver".to_string();
        assert_eq!(app.pr_rows().len(), 1);
        app.filter = "zzz".to_string();
        assert!(app.pr_rows().is_empty());
    }

    #[test]
    fn ci_rows_filter_by_repo_branch_and_name() {
        let mut app = fleet_app();
        app.ci = Some(vec![
            ci_run("kernel", "build", "main"),
            ci_run("hal", "test", "feature/x"),
        ]);
        assert_eq!(app.ci_rows().len(), 2);
        app.filter = "hal".to_string();
        assert_eq!(app.ci_rows().len(), 1);
        app.filter = "feature".to_string();
        assert_eq!(app.ci_rows().len(), 1);
        app.filter = "build".to_string();
        assert_eq!(app.ci_rows().len(), 1);
    }

    #[test]
    fn current_pr_follows_cursor_and_drill_in() {
        let mut app = fleet_app();
        app.prs = Some(vec![pr("kernel", "fix boot"), pr("hal", "add driver")]);
        // Fleet list: the cursor row.
        assert_eq!(app.current_pr(), None);
        app.view = View::Prs;
        app.cursor.select(Some(1));
        assert_eq!(
            app.current_pr(),
            Some(("hal".to_string(), 1, "add driver".to_string()))
        );
        // Drill-in: the stored PR, regardless of cursor.
        app.view = View::PrDetail;
        assert_eq!(app.current_pr(), None);
        app.detail_pr = Some(("kernel".to_string(), 42, "fix boot".to_string()));
        assert_eq!(
            app.current_pr(),
            Some(("kernel".to_string(), 42, "fix boot".to_string()))
        );
    }

    #[test]
    fn merge_and_approve_pr_dispatch_the_right_actions() {
        let mut app = fleet_app();
        let (tx, rx) = channel();
        dispatch(
            &mut app,
            &tx,
            "merge PR",
            ActionKind::MergePr("kernel".to_string(), 7),
        );
        app.busy = None;
        dispatch(
            &mut app,
            &tx,
            "approve PR",
            ActionKind::ApprovePr("kernel".to_string(), 7),
        );
        assert_eq!(drain(&rx), vec!["merge PR", "approve PR"]);
    }

    #[test]
    fn checkout_pr_confirm_then_dispatch() {
        // Selecting a PR and asking to check it out arms the confirm gate...
        let mut app = fleet_app();
        app.prs = Some(vec![pr("kernel", "fix boot")]);
        app.view = View::Prs;
        app.cursor.select(Some(0));
        let (repo, number, title) = app.current_pr().expect("a PR under the cursor");
        app.pending_confirm = Some(Confirm::CheckoutPr {
            repo: repo.clone(),
            number,
            title,
        });
        assert!(matches!(
            app.pending_confirm,
            Some(Confirm::CheckoutPr { .. })
        ));
        // ...and confirming dispatches the "checkout PR" action to the worker.
        let (tx, rx) = channel();
        dispatch(
            &mut app,
            &tx,
            "checkout PR",
            ActionKind::CheckoutPr(repo, number),
        );
        assert_eq!(drain(&rx), vec!["checkout PR"]);
    }

    #[test]
    fn open_pr_detail_stores_the_current_pr() {
        let mut app = fleet_app();
        let (tx, _rx) = channel();
        open_pr_detail(&mut app, &tx, "kernel", 7, "fix boot");
        assert_eq!(app.view, View::PrDetail);
        assert_eq!(
            app.detail_pr,
            Some(("kernel".to_string(), 7, "fix boot".to_string()))
        );
    }

    #[test]
    fn cursor_url_follows_the_active_view() {
        let mut app = fleet_app();
        app.prs = Some(vec![pr("kernel", "fix boot")]);
        app.ci = Some(vec![ci_run("hal", "build", "main")]);
        assert_eq!(app.cursor_url(), None);
        app.view = View::Prs;
        assert_eq!(
            app.cursor_url().as_deref(),
            Some("https://github.com/acme/kernel/pull/1")
        );
        app.view = View::Ci;
        assert_eq!(
            app.cursor_url().as_deref(),
            Some("https://github.com/acme/hal/actions/runs/1")
        );
        app.cursor.select(Some(5));
        assert_eq!(app.cursor_url(), None);
    }

    #[test]
    fn prs_and_ci_commands_open_the_views() {
        let mut app = fleet_app();
        let (tx, rx) = channel();
        run_command_bar(&mut app, &tx, "prs");
        assert_eq!(app.view, View::Prs);
        // Entering the view from elsewhere reuses the cache (force = false).
        assert!(matches!(rx.try_recv(), Ok(Job::FleetPrs { force: false })));
        app.busy = None;
        run_command_bar(&mut app, &tx, "ci");
        assert_eq!(app.view, View::Ci);
        assert!(matches!(rx.try_recv(), Ok(Job::FleetCi { force: false })));
    }

    #[test]
    fn refetch_key_on_the_view_forces_a_cache_bypass() {
        let mut app = fleet_app();
        let (tx, rx) = channel();
        // Enter the PR view first (cached fetch).
        open_fleet_view(&mut app, &tx, View::Prs);
        assert!(matches!(rx.try_recv(), Ok(Job::FleetPrs { force: false })));
        app.busy = None;
        // Pressing `m` again while already on the view is a manual refetch.
        open_fleet_view(&mut app, &tx, View::Prs);
        assert!(matches!(rx.try_recv(), Ok(Job::FleetPrs { force: true })));
    }

    #[test]
    fn open_fleet_view_enqueues_fetch_even_while_busy() {
        let mut app = fleet_app();
        app.busy = Some("sync");
        let (tx, rx) = channel();
        open_fleet_view(&mut app, &tx, View::Prs);
        assert_eq!(app.view, View::Prs);
        // The read-only fetch still enqueues — the serial worker runs it after
        // the in-flight job, so navigation is never refused.
        assert!(matches!(rx.try_recv(), Ok(Job::FleetPrs { force: false })));
    }

    #[test]
    fn unknown_command_reports() {
        let mut app = fleet_app();
        let (tx, _rx) = channel();
        run_command_bar(&mut app, &tx, "frobnicate");
        assert!(app.message.contains("unknown command"));
    }

    #[test]
    fn theme_by_name_known_and_unknown() {
        assert!(Theme::by_name("dracula").is_some());
        assert!(Theme::by_name("CATPPUCCIN").is_some());
        assert!(Theme::by_name("monochrome").is_some());
        assert!(Theme::by_name("nope").is_none());
    }

    #[test]
    fn theme_list_covers_all_builtins() {
        assert_eq!(THEMES.len(), 6);
        for name in THEMES {
            assert!(Theme::by_name(name).is_some(), "missing theme {name}");
        }
    }

    #[test]
    fn theme_command_switches() {
        let mut app = fleet_app();
        let (tx, _rx) = channel();
        run_command_bar(&mut app, &tx, "theme dracula");
        assert!(app.message.contains("dracula"));
        assert!(!app.message.contains("unknown"));
    }

    #[test]
    fn theme_command_unknown_reports() {
        let mut app = fleet_app();
        let (tx, _rx) = channel();
        run_command_bar(&mut app, &tx, "theme bogus");
        assert!(app.message.contains("unknown theme"));
    }

    #[test]
    fn theme_command_bare_lists() {
        let mut app = fleet_app();
        let (tx, _rx) = channel();
        run_command_bar(&mut app, &tx, "theme");
        assert!(app.message.contains("catppuccin"));
    }

    fn gov() -> Governance {
        Governance {
            plugins: vec![
                GovPlugin {
                    name: "haw-compliance".to_string(),
                    phases: vec!["post-build".to_string()],
                },
                GovPlugin {
                    name: "haw-git-gate".to_string(),
                    phases: vec!["pre-request".to_string()],
                },
            ],
            artifacts: vec![GovArtifact {
                plugin: "haw-compliance".to_string(),
                kind: "sbom".to_string(),
                path: ".haw/sbom/app.cdx.json".to_string(),
                exists: true,
            }],
            findings: vec![GovFinding {
                plugin: "haw-git-gate".to_string(),
                level: "warn".to_string(),
                message: "no signer on PATH".to_string(),
            }],
        }
    }

    #[test]
    fn gov_rows_len_and_filter() {
        let mut app = fleet_app();
        app.view = View::Governance;
        assert_eq!(app.rows_len(), 0);
        app.gov = Some(gov());
        assert_eq!(app.rows_len(), 2);
        app.filter = "compliance".to_string();
        assert_eq!(app.rows_len(), 1);
        app.filter = "pre-request".to_string();
        assert_eq!(app.rows_len(), 1);
        app.filter = "zzz".to_string();
        assert_eq!(app.rows_len(), 0);
    }

    #[test]
    fn gov_cursor_path_finds_existing_artifact() {
        let mut app = fleet_app();
        app.view = View::Governance;
        app.gov = Some(gov());
        app.cursor.select(Some(0));
        assert_eq!(app.cursor_path().as_deref(), Some(".haw/sbom/app.cdx.json"));
        app.cursor.select(Some(1));
        assert_eq!(app.cursor_path(), None);
    }

    #[test]
    fn gov_status_reflects_worst_finding() {
        let g = gov();
        assert_eq!(
            gov_status_span(&g, "haw-git-gate").content.to_string(),
            "⏳ warn"
        );
        assert_eq!(
            gov_status_span(&g, "haw-compliance").content.to_string(),
            "✓ ok"
        );
    }

    #[test]
    fn governance_command_opens_the_view() {
        let mut app = fleet_app();
        let (tx, rx) = channel();
        run_command_bar(&mut app, &tx, "governance");
        assert_eq!(app.view, View::Governance);
        assert!(matches!(rx.try_recv(), Ok(Job::Governance)));
        app.busy = None;
        app.go_back();
        // `:plugins` now opens the first-class plugin-panels view (distinct
        // from `:governance`), enqueuing a panels discovery job.
        run_command_bar(&mut app, &tx, "plugins");
        assert_eq!(app.view, View::Plugins);
        assert!(matches!(rx.try_recv(), Ok(Job::PluginPanels)));
    }

    #[test]
    fn errors_command_opens_the_errors_view() {
        let mut app = fleet_app();
        let (tx, _rx) = channel();
        run_command_bar(&mut app, &tx, "errors");
        assert_eq!(app.view, View::Errors);
    }

    #[test]
    fn open_fleet_view_enqueues_governance_fetch_even_while_busy() {
        let mut app = fleet_app();
        app.busy = Some("sync");
        let (tx, rx) = channel();
        open_fleet_view(&mut app, &tx, View::Governance);
        assert_eq!(app.view, View::Governance);
        assert!(matches!(rx.try_recv(), Ok(Job::Governance)));
    }

    #[test]
    fn repo_detail_view_has_no_rows() {
        let mut app = fleet_app();
        app.view = View::RepoDetail;
        app.detail_title = "repo kernel".to_string();
        assert_eq!(app.rows_len(), 0);
        assert_eq!(view_name(&app, View::RepoDetail), "repo kernel");
    }

    #[test]
    fn hit_is_fuzzy_and_empty_matches_all() {
        assert!(hit("kernel", ""));
        assert!(hit("kernel", "kern"));
        assert!(hit("kernel", "krnl")); // fuzzy, non-contiguous
        assert!(hit("KERNEL", "kern")); // case-insensitive
        assert!(!hit("kernel", "zzz"));
    }

    #[test]
    fn fleet_rows_still_filter_after_fuzzy() {
        let mut app = fleet_app();
        assert_eq!(app.fleet_rows().len(), 3);
        app.filter = "hal".to_string();
        assert_eq!(app.fleet_rows().len(), 1);
        app.filter = "firmware".to_string();
        assert_eq!(app.fleet_rows().len(), 2);
        app.filter = "zzz".to_string();
        assert!(app.fleet_rows().is_empty());
    }

    #[test]
    fn sort_reorders_fleet_rows_by_name() {
        let mut app = fleet_app();
        // unsorted: manifest order kernel, hal, app-mqtt
        let names: Vec<_> = app.fleet_rows().iter().map(|r| r.name.clone()).collect();
        assert_eq!(names, vec!["kernel", "hal", "app-mqtt"]);
        // `>` from unset starts on the first sortable column (name, ascending).
        app.cycle_sort(true);
        assert_eq!(app.sort, Some((0, false)));
        let names: Vec<_> = app.fleet_rows().iter().map(|r| r.name.clone()).collect();
        assert_eq!(names, vec!["app-mqtt", "hal", "kernel"]);
        // `.` toggles to descending.
        app.toggle_sort_dir();
        assert_eq!(app.sort, Some((0, true)));
        let names: Vec<_> = app.fleet_rows().iter().map(|r| r.name.clone()).collect();
        assert_eq!(names, vec!["kernel", "hal", "app-mqtt"]);
    }

    #[test]
    fn cycle_sort_wraps_and_is_noop_off_sortable_views() {
        let mut app = fleet_app();
        // From unset, either direction starts on the first sortable column.
        app.cycle_sort(false);
        assert_eq!(app.sort, Some((0, false)));
        // Fleet has 6 sortable columns; going backward from 0 wraps to 5.
        app.cycle_sort(false);
        assert_eq!(app.sort, Some((5, false)));
        app.cycle_sort(true);
        assert_eq!(app.sort, Some((0, false)));
        // Non-sortable view: no-op.
        app.view = View::Governance;
        app.sort = None;
        app.cycle_sort(true);
        assert_eq!(app.sort, None);
    }

    #[test]
    fn goto_view_and_back_reset_sort_and_marks() {
        let mut app = fleet_app();
        app.sort = Some((1, true));
        app.selected_repos = vec!["kernel".to_string()];
        app.goto_view(View::Tree);
        assert_eq!(app.sort, None);
        assert!(app.selected_repos.is_empty());
    }

    #[test]
    fn space_in_fleet_toggles_marks() {
        // Mirror the event-loop's space handler for the Fleet view.
        let mut app = fleet_app();
        app.view = View::Fleet;
        app.cursor.select(Some(0));
        let repo = app.cursor_repo().unwrap();
        assert_eq!(repo, "kernel");
        // toggle on
        app.selected_repos.push(repo.clone());
        assert!(app.selected_repos.contains(&repo));
        // Marked rows sync as a set.
        let (tx, rx) = channel();
        let repos = app.selected_repos.clone();
        dispatch(&mut app, &tx, "sync", ActionKind::SyncRepos(repos));
        assert_eq!(drain(&rx), vec!["sync"]);
    }

    #[test]
    fn bulk_run_with_marks_dispatches_run_repos() {
        let mut app = fleet_app();
        app.view = View::Fleet;
        app.selected_repos = vec!["kernel".to_string(), "hal".to_string()];
        let (tx, rx) = channel();
        run_command_bar(&mut app, &tx, "run echo hi");
        // Still labelled "run" so the output overlay path is unchanged.
        assert_eq!(drain(&rx), vec!["run"]);
    }

    #[test]
    fn help_command_opens_the_overlay() {
        let mut app = fleet_app();
        let (tx, _rx) = channel();
        run_command_bar(&mut app, &tx, "help");
        assert!(app.help);
    }

    // ---- diff / logs / pagination -----------------------------------------

    #[test]
    fn open_pr_diff_opens_the_shared_detail_with_a_diff_title() {
        let mut app = fleet_app();
        let (tx, rx) = channel();
        open_pr_diff(&mut app, &tx, "kernel", 7);
        assert_eq!(app.view, View::RepoDetail);
        assert_eq!(app.detail_title, "diff kernel#7");
        assert!(matches!(rx.try_recv(), Ok(Job::PrDiff(repo, 7)) if repo == "kernel"));
    }

    #[test]
    fn open_ci_logs_opens_the_shared_detail_with_a_logs_title() {
        let mut app = fleet_app();
        let (tx, rx) = channel();
        open_ci_logs(&mut app, &tx, "hal", 42, "firmware-ci");
        assert_eq!(app.view, View::RepoDetail);
        assert_eq!(app.detail_title, "logs hal firmware-ci");
        assert!(matches!(rx.try_recv(), Ok(Job::CiLogs(repo, 42)) if repo == "hal"));
    }

    #[test]
    fn open_ci_detail_records_the_run_for_logs() {
        let mut app = fleet_app();
        let (tx, _rx) = channel();
        open_ci_detail(&mut app, &tx, "hal", 42, "firmware-ci");
        assert_eq!(
            app.detail_ci,
            Some(("hal".to_string(), 42, "firmware-ci".to_string()))
        );
    }

    // ---- PR files browser -------------------------------------------------

    #[test]
    fn open_pr_files_opens_the_list_and_enqueues_the_fetch() {
        let mut app = fleet_app();
        let (tx, rx) = channel();
        open_pr_files(&mut app, &tx, "kernel", 7, "add dma");
        assert_eq!(app.view, View::PrFiles);
        assert_eq!(
            app.pr_files_pr,
            Some(("kernel".to_string(), 7, "add dma".to_string()))
        );
        assert!(matches!(rx.try_recv(), Ok(Job::PrFiles(repo, 7)) if repo == "kernel"));
    }

    #[test]
    fn open_pr_file_content_opens_the_detail_titled_pr_and_path() {
        let mut app = fleet_app();
        let (tx, rx) = channel();
        open_pr_files(&mut app, &tx, "kernel", 7, "add dma");
        let _ = rx.try_recv(); // drain the PrFiles fetch
        app.pr_file_entries = Some(vec![PrFileEntry {
            path: "src/lib.rs".to_string(),
            status: "modified".to_string(),
        }]);
        open_pr_file_content(&mut app, &tx, "src/lib.rs");
        assert_eq!(app.view, View::PrDetail);
        assert_eq!(app.detail_title, "PR #7 src/lib.rs");
        assert!(app.pr_files_return.is_some());
        assert!(matches!(
            rx.try_recv(),
            Ok(Job::PrFileContent(repo, 7, path, _)) if repo == "kernel" && path == "src/lib.rs"
        ));
    }

    #[test]
    fn back_from_a_pr_file_restores_the_files_browser() {
        let mut app = fleet_app();
        let (tx, rx) = channel();
        app.back = vec![View::Prs];
        open_pr_files(&mut app, &tx, "kernel", 7, "add dma");
        let _ = rx.try_recv();
        app.pr_file_entries = Some(vec![PrFileEntry {
            path: "src/lib.rs".to_string(),
            status: "modified".to_string(),
        }]);
        open_pr_file_content(&mut app, &tx, "src/lib.rs");
        assert_eq!(app.view, View::PrDetail);
        app.go_back();
        assert_eq!(app.view, View::PrFiles);
        // The PR context and the cached listing are both restored.
        assert_eq!(
            app.pr_files_pr,
            Some(("kernel".to_string(), 7, "add dma".to_string()))
        );
        assert_eq!(app.pr_file_rows().len(), 1);
    }

    #[test]
    fn pr_file_rows_filter_by_path_and_status() {
        let mut app = fleet_app();
        app.pr_file_entries = Some(vec![
            PrFileEntry {
                path: "src/lib.rs".to_string(),
                status: "modified".to_string(),
            },
            PrFileEntry {
                path: "README.md".to_string(),
                status: "added".to_string(),
            },
        ]);
        assert_eq!(app.pr_file_rows().len(), 2);
        app.filter = "lib".to_string();
        assert_eq!(app.pr_file_rows().len(), 1);
        app.filter = "added".to_string();
        assert_eq!(app.pr_file_rows().len(), 1);
    }

    #[test]
    fn move_page_steps_by_a_page_and_clamps() {
        let mut app = fleet_app();
        app.prs = Some(vec![
            pr("a", "t"),
            pr("b", "t"),
            pr("c", "t"),
            pr("d", "t"),
            pr("e", "t"),
        ]);
        app.view = View::Prs;
        app.page_size = 3;
        app.cursor.select(Some(0));
        // one page down moves by page_size...
        app.move_page(true);
        assert_eq!(app.cursor.selected(), Some(3));
        // ...and clamps to the last row (5 rows, index 4).
        app.move_page(true);
        assert_eq!(app.cursor.selected(), Some(4));
        // page up steps back and floors at 0.
        app.move_page(false);
        assert_eq!(app.cursor.selected(), Some(1));
        app.move_page(false);
        assert_eq!(app.cursor.selected(), Some(0));
    }

    #[test]
    fn move_page_is_safe_on_empty_lists() {
        let mut app = fleet_app();
        app.view = View::Prs;
        app.prs = Some(Vec::new());
        app.page_size = 5;
        app.cursor.select(Some(0));
        app.move_page(true);
        assert_eq!(app.cursor.selected(), Some(0));
    }

    #[test]
    fn hint_grid_uses_fewer_columns_on_narrow_widths() {
        let hints: &[(&str, &str)] = &[("a", "one"), ("b", "two"), ("c", "three"), ("d", "four")];
        // Very narrow: a single column, one line per hint.
        assert_eq!(hint_grid(hints, 6).len(), 4);
        // Wide enough for at least two columns: fewer lines than hints.
        let wide = hint_grid(hints, 120);
        assert!(wide.len() < 4);
    }

    // ---- file browser -----------------------------------------------------

    fn fe(name: &str, is_dir: bool) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            is_dir,
        }
    }

    #[test]
    fn file_rows_sort_dirs_first_then_lexicographic() {
        let mut app = fleet_app();
        app.view = View::Files;
        app.files_entries = Some(vec![
            fe("zeta.rs", false),
            fe("alpha", true),
            fe("beta.rs", false),
            fe("gamma", true),
        ]);
        let names: Vec<_> = app.file_rows().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["alpha", "gamma", "beta.rs", "zeta.rs"]);
    }

    #[test]
    fn open_files_enters_the_view_and_requests_a_tree() {
        let mut app = fleet_app();
        let (tx, rx) = channel();
        open_files(&mut app, &tx, "kernel", false);
        assert_eq!(app.view, View::Files);
        assert_eq!(app.files_repo.as_deref(), Some("kernel"));
        assert!(app.files_subpath.is_empty());
        assert!(!app.files_remote);
        assert!(matches!(
            rx.try_recv(),
            Ok(Job::RepoTree(repo, sub, false)) if repo == "kernel" && sub.is_empty()
        ));
    }

    #[test]
    fn enter_dir_pushes_subpath_and_reloads() {
        let mut app = fleet_app();
        app.view = View::Files;
        app.files_repo = Some("kernel".to_string());
        app.files_entries = Some(vec![fe("drivers", true), fe("README.md", false)]);
        app.cursor.select(Some(0)); // "drivers" sorts first
        let (tx, rx) = channel();
        let selected = app.cursor.selected().unwrap();
        let entry = app.file_rows().get(selected).map(|e| (*e).clone()).unwrap();
        assert!(entry.is_dir);
        app.files_subpath = entry.name.clone();
        reload_files(&mut app, &tx);
        assert_eq!(app.files_subpath, "drivers");
        assert!(matches!(
            rx.try_recv(),
            Ok(Job::RepoTree(_, sub, _)) if sub == "drivers"
        ));
    }

    #[test]
    fn backspace_pops_one_subpath_segment() {
        let mut app = fleet_app();
        app.view = View::Files;
        app.files_repo = Some("kernel".to_string());
        app.files_subpath = "drivers/i2c".to_string();
        if let Some((parent, _)) = app.files_subpath.clone().rsplit_once('/') {
            app.files_subpath = parent.to_string();
        }
        assert_eq!(app.files_subpath, "drivers");
        assert!(app.files_subpath.rsplit_once('/').is_none());
        app.files_subpath.clear();
        assert!(app.files_subpath.is_empty());
    }

    #[test]
    fn open_file_content_opens_the_shared_detail_titled_with_the_path() {
        let mut app = fleet_app();
        app.view = View::Files;
        app.files_repo = Some("kernel".to_string());
        app.files_subpath = "drivers/i2c".to_string();
        let (tx, rx) = channel();
        open_file_content(&mut app, &tx, "dma.c");
        assert_eq!(app.view, View::RepoDetail);
        assert_eq!(app.detail_title, "kernel:/drivers/i2c/dma.c");
        assert!(matches!(
            rx.try_recv(),
            Ok(Job::FileContent(repo, path, false, _))
                if repo == "kernel" && path == "drivers/i2c/dma.c"
        ));
    }

    #[test]
    fn x_in_fleet_sets_shell_exit_when_cloned() {
        let mut app = fleet_app();
        app.view = View::Fleet;
        app.cursor.select(Some(0)); // kernel
        let repo = app.cursor_repo().unwrap();
        assert_eq!(repo, "kernel");
        let path = app.repo_path(&repo).unwrap();
        app.exit = Some(Exit::Shell(path));
        assert_eq!(app.exit, Some(Exit::Shell(PathBuf::from("/w/kernel"))));
    }

    #[test]
    fn files_view_name_and_hints() {
        let mut app = fleet_app();
        app.view = View::Files;
        app.files_repo = Some("kernel".to_string());
        app.files_subpath = "src".to_string();
        assert!(view_name(&app, View::Files).contains("kernel"));
        assert!(view_name(&app, View::Files).contains("src"));
        assert!(!key_hints(View::Files).is_empty());
    }

    // ---- Feature A/B: problems highlight + filter -------------------------

    fn dirty_repo(name: &str) -> RepoStatus {
        let mut r = repo(name, &[]);
        r.dirty = true;
        r
    }

    #[test]
    fn has_problem_truth_table() {
        // clean repo: no problem.
        assert!(!has_problem(&repo("clean", &[])));
        // dirty worktree.
        let mut r = repo("dirty", &[]);
        r.dirty = true;
        assert!(has_problem(&r));
        // drift (head != lock).
        let mut r = repo("drift", &[]);
        r.drift = true;
        assert!(has_problem(&r));
        // missing / not cloned.
        let mut r = repo("gone", &[]);
        r.missing = true;
        assert!(has_problem(&r));
        // behind upstream.
        let mut r = repo("behind", &[]);
        r.ahead_behind = Some((0, 2));
        assert!(has_problem(&r));
        // ahead-only is fine.
        let mut r = repo("ahead", &[]);
        r.ahead_behind = Some((3, 0));
        assert!(!has_problem(&r));
    }

    #[test]
    fn problems_only_filter_reduces_rows() {
        let snap = Snapshot {
            stacks: vec!["gw".to_string()],
            fleet: vec![(
                "gw".to_string(),
                vec![
                    repo("clean", &[]),
                    dirty_repo("dirty"),
                    repo("also-clean", &[]),
                ],
            )],
            ..Default::default()
        };
        let mut app = app_with(snap);
        assert_eq!(app.fleet_rows().len(), 3);
        app.problems_only = true;
        let rows = app.fleet_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "dirty");
        assert_eq!(app.fleet_total(), 3);
    }

    #[test]
    fn problems_command_toggles_the_filter() {
        let mut app = fleet_app();
        let (tx, _rx) = channel();
        assert!(!app.problems_only);
        run_command_bar(&mut app, &tx, "problems");
        assert!(app.problems_only);
        run_command_bar(&mut app, &tx, "problems");
        assert!(!app.problems_only);
    }

    // ---- Feature C: cross-repo grep ---------------------------------------

    #[test]
    fn parse_grep_line_splits_path_line_text() {
        let hit = parse_grep_line("kernel", "src/main.rs:12:let x = 1;").unwrap();
        assert_eq!(hit.repo, "kernel");
        assert_eq!(hit.path, "src/main.rs");
        assert_eq!(hit.line, 12);
        assert_eq!(hit.text, "let x = 1;");
        // Text may itself contain colons.
        let hit = parse_grep_line("hal", "a.rs:3:url = http://x").unwrap();
        assert_eq!(hit.line, 3);
        assert_eq!(hit.text, "url = http://x");
        // Non-matching lines yield None.
        assert!(parse_grep_line("hal", "garbage").is_none());
        assert!(parse_grep_line("hal", "a.rs:notaline:x").is_none());
    }

    #[test]
    fn grep_command_sets_pending_grep_and_opens_view() {
        let mut app = fleet_app();
        let (tx, rx) = channel();
        run_command_bar(&mut app, &tx, "grep foo");
        assert_eq!(app.view, View::Grep);
        assert_eq!(app.grep_pattern, "foo");
        assert!(matches!(
            rx.try_recv(),
            Ok(Job::Grep(pat, Some(stack))) if pat == "foo" && stack == "gw"
        ));
    }

    #[test]
    fn grep_rows_filter_by_repo_path_and_text() {
        let mut app = fleet_app();
        app.view = View::Grep;
        app.grep_hits = Some(vec![
            GrepHit {
                repo: "kernel".to_string(),
                path: "src/boot.rs".to_string(),
                line: 4,
                text: "fn boot()".to_string(),
            },
            GrepHit {
                repo: "hal".to_string(),
                path: "src/i2c.rs".to_string(),
                line: 9,
                text: "fn xfer()".to_string(),
            },
        ]);
        assert_eq!(app.grep_rows().len(), 2);
        app.filter = "kernel".to_string();
        assert_eq!(app.grep_rows().len(), 1);
        app.filter = "i2c".to_string();
        assert_eq!(app.grep_rows().len(), 1);
        app.filter = "boot".to_string();
        assert_eq!(app.grep_rows().len(), 1);
    }

    #[test]
    fn grep_view_name_and_hints_cover() {
        let mut app = fleet_app();
        app.view = View::Grep;
        app.grep_pattern = "todo".to_string();
        assert!(view_name(&app, View::Grep).contains("todo"));
        assert!(!key_hints(View::Grep).is_empty());
    }

    #[test]
    fn open_grep_hit_opens_detail_scrolled_to_line() {
        let mut app = fleet_app();
        let (tx, rx) = channel();
        let hit = GrepHit {
            repo: "kernel".to_string(),
            path: "src/boot.rs".to_string(),
            line: 12,
            text: "x".to_string(),
        };
        open_grep_hit(&mut app, &tx, &hit);
        assert_eq!(app.view, View::RepoDetail);
        assert_eq!(app.detail_title, "kernel:/src/boot.rs");
        // The 1-based line 12 → 0-based offset 11, carried on pending_scroll for
        // the Outcome::Detail arm to apply (clamped) once the text loads.
        assert_eq!(app.detail_scroll, 0);
        assert_eq!(app.pending_scroll, Some(11));
        assert!(matches!(
            rx.try_recv(),
            Ok(Job::FileContent(repo, path, false, _)) if repo == "kernel" && path == "src/boot.rs"
        ));
    }

    // ---- Feature D: single-repo exec + fetch ------------------------------

    #[test]
    fn exec_command_opens_detail_and_dispatches() {
        let mut app = fleet_app();
        app.view = View::Fleet;
        app.cursor.select(Some(0)); // kernel
        let (tx, rx) = channel();
        run_command_bar(&mut app, &tx, "sh echo hi");
        assert_eq!(app.view, View::RepoDetail);
        assert_eq!(app.detail_title, "$ echo hi @ kernel");
        assert_eq!(drain(&rx), vec!["exec"]);
    }

    #[test]
    fn fetch_command_dispatches_fetch() {
        let mut app = fleet_app();
        app.view = View::Fleet;
        app.cursor.select(Some(0));
        let (tx, rx) = channel();
        run_command_bar(&mut app, &tx, "fetch");
        assert_eq!(drain(&rx), vec!["fetch"]);
    }

    #[test]
    fn colon_repo_substring_jumps_the_cursor() {
        let mut app = fleet_app();
        app.view = View::Fleet;
        app.cursor.select(Some(0));
        let (tx, _rx) = channel();
        // `:hal` (a bare token) jumps to the `hal` row.
        run_command_bar(&mut app, &tx, "hal");
        let idx = app.cursor.selected().unwrap();
        assert_eq!(app.fleet_rows()[idx].name, "hal");
        assert!(app.message.contains("hal"));
    }

    #[test]
    fn row_indicator_shows_position_and_is_empty_when_no_rows() {
        let mut app = fleet_app();
        app.cursor.select(Some(1));
        assert_eq!(row_indicator(&app, 3), " · row 2/3");
        assert_eq!(row_indicator(&app, 0), "");
    }

    // ---- audit batch: fixes #1..#10 --------------------------------------

    /// #1 File viewer round-trip: drilling from Files into a file, then `b`,
    /// restores the same populated listing instead of wiping it.
    #[test]
    fn file_viewer_back_restores_the_browser() {
        let mut app = fleet_app();
        let (tx, _rx) = channel();
        open_files(&mut app, &tx, "kernel", false);
        app.files_subpath = "src".to_string();
        app.files_entries = Some(vec![fe("main.rs", false), fe("lib.rs", false)]);
        app.back = vec![View::Fleet];
        // Drill into a file — Files state is snapshotted, then cleared by goto_view.
        open_file_content(&mut app, &tx, "main.rs");
        assert_eq!(app.view, View::RepoDetail);
        assert!(app.files_return.is_some());
        // Back into Files: the listing is restored, not empty.
        app.go_back();
        assert_eq!(app.view, View::Files);
        assert_eq!(app.files_repo.as_deref(), Some("kernel"));
        assert_eq!(app.files_subpath, "src");
        assert_eq!(app.files_entries.as_ref().map(Vec::len), Some(2));
    }

    /// #2 pending_scroll is applied (clamped) when the detail text loads,
    /// simulating the `Outcome::Detail` arm.
    #[test]
    fn pending_scroll_applied_on_detail_load() {
        let mut app = fleet_app();
        app.pending_scroll = Some(5);
        // Simulate the Outcome::Detail Ok arm.
        app.detail_scroll = 0;
        app.detail_text = Some((0..20).map(|i| format!("line {i}\n")).collect());
        if let Some(target) = app.pending_scroll.take() {
            let max = usize::from(detail_max_scroll(&app));
            app.detail_scroll = u16::try_from(target.min(max)).unwrap_or(u16::MAX);
        }
        assert_eq!(app.detail_scroll, 5);
        assert!(app.pending_scroll.is_none());
    }

    #[test]
    fn pending_scroll_clamps_to_content_height() {
        let mut app = fleet_app();
        app.pending_scroll = Some(999);
        app.detail_text = Some("a\nb\nc\n".to_string());
        if let Some(target) = app.pending_scroll.take() {
            let max = usize::from(detail_max_scroll(&app));
            app.detail_scroll = u16::try_from(target.min(max)).unwrap_or(u16::MAX);
        }
        // 3 lines → max scroll 2.
        assert_eq!(app.detail_scroll, 2);
    }

    #[test]
    fn open_grep_hit_stashes_pending_scroll_not_detail_scroll() {
        let mut app = fleet_app();
        let (tx, _rx) = channel();
        let hit = GrepHit {
            repo: "kernel".to_string(),
            path: "LICENSE".to_string(),
            line: 6,
            text: "x".to_string(),
        };
        open_grep_hit(&mut app, &tx, &hit);
        // detail_scroll stays 0 (the Outcome arm would zero it anyway); the line
        // is carried on pending_scroll for the arm to apply post-load.
        assert_eq!(app.detail_scroll, 0);
        assert_eq!(app.pending_scroll, Some(5));
    }

    /// #3 Peer view switches reset the back-stack to root rather than growing it.
    #[test]
    fn peer_switches_do_not_grow_the_back_stack() {
        let mut app = fleet_app();
        assert!(app.back.is_empty());
        // A run of lateral letter switches, k9s-style.
        app.goto_view(View::Prs);
        app.goto_view(View::Ci);
        app.goto_view(View::Governance);
        app.goto_view(View::Plugins);
        app.goto_view(View::Errors);
        app.goto_view(View::Tree);
        // Back-stack holds only the root, never a chain of peers.
        assert_eq!(app.back, vec![View::Fleet]);
        // `b` returns straight to the fleet.
        app.go_back();
        assert_eq!(app.view, View::Fleet);
        assert!(app.back.is_empty());
    }

    #[test]
    fn drill_ins_still_push_the_back_stack() {
        let mut app = fleet_app();
        app.goto_view(View::Prs); // peer → back = [Fleet]
        let (tx, _rx) = channel();
        open_files(&mut app, &tx, "kernel", false); // drill-in → pushes Prs
        assert_eq!(app.back, vec![View::Fleet, View::Prs]);
        app.go_back();
        assert_eq!(app.view, View::Prs);
    }

    /// #4 The sort caret sits on exactly the header cell the comparator sorts by,
    /// for every sortable column of every sortable view.
    #[test]
    fn sort_caret_matches_comparator_columns() {
        // Fleet: comparator cols name/branch/head/dirty/drift/ahead_behind map to
        // header cells REPO/BRANCH/HEAD/DIRTY/DRIFT/↑↓.
        assert_eq!(sort_header_map(View::Fleet), &[1, 3, 4, 5, 6, 7]);
        assert_eq!(sort_header_map(View::Prs), &[0, 2, 3, 4]);
        assert_eq!(sort_header_map(View::Ci), &[0, 1, 2, 4]);
        // The caret index is exactly the mapped header cell.
        for (view, cols) in [(View::Fleet, 6u16), (View::Prs, 4), (View::Ci, 4)] {
            for col in 0..cols {
                let (idx, desc) = sort_caret(view, Some((col, true))).unwrap();
                assert_eq!(idx, sort_header_map(view)[col as usize]);
                assert!(desc);
            }
            // Out of range / unset → no caret.
            assert_eq!(sort_caret(view, None), None);
        }
    }

    #[test]
    fn sortable_cols_matches_the_header_map_len() {
        for view in ALL_VIEWS {
            let mut app = fleet_app();
            app.view = view;
            assert_eq!(
                app.sortable_cols() as usize,
                sort_header_map(view).len(),
                "sortable_cols disagrees with sort_header_map for {view:?}"
            );
        }
    }

    #[test]
    fn cycle_sort_caret_walks_all_sortable_columns() {
        let mut app = fleet_app();
        app.view = View::Fleet;
        let mut seen = Vec::new();
        for _ in 0..6 {
            app.cycle_sort(true);
            let (idx, _) = sort_caret(View::Fleet, app.sort).unwrap();
            seen.push(idx);
        }
        assert_eq!(seen, vec![1, 3, 4, 5, 6, 7]);
    }

    /// #5 cursor_repo resolves the repo of a Prs/Ci/Grep row.
    #[test]
    fn cursor_repo_resolves_prs_ci_and_grep_rows() {
        let mut app = fleet_app();
        app.prs = Some(vec![pr("kernel", "fix"), pr("hal", "add")]);
        app.ci = Some(vec![ci_run("app-mqtt", "build", "main")]);
        app.grep_hits = Some(vec![GrepHit {
            repo: "hal".to_string(),
            path: "a.rs".to_string(),
            line: 1,
            text: "x".to_string(),
        }]);
        app.view = View::Prs;
        app.cursor.select(Some(1));
        assert_eq!(app.cursor_repo().as_deref(), Some("hal"));
        app.view = View::Ci;
        app.cursor.select(Some(0));
        assert_eq!(app.cursor_repo().as_deref(), Some("app-mqtt"));
        app.view = View::Grep;
        app.cursor.select(Some(0));
        assert_eq!(app.cursor_repo().as_deref(), Some("hal"));
    }

    /// #6 Write actions are gated by PR state.
    #[test]
    fn pr_state_gates_write_actions() {
        let mut app = fleet_app();
        app.view = View::Prs;
        let mut open = pr("kernel", "fix");
        open.state = "open".to_string();
        let mut merged = pr("hal", "done");
        merged.number = 2;
        merged.state = "merged".to_string();
        app.prs = Some(vec![open, merged]);
        app.cursor.select(Some(0));
        assert!(app.current_pr_writable());
        assert_eq!(app.current_pr_state().as_deref(), Some("open"));
        app.cursor.select(Some(1));
        assert!(!app.current_pr_writable());
        assert_eq!(app.current_pr_state().as_deref(), Some("merged"));
    }

    #[test]
    fn draft_pr_is_writable() {
        let mut app = fleet_app();
        app.view = View::Prs;
        let mut draft = pr("kernel", "wip");
        draft.state = "draft".to_string();
        app.prs = Some(vec![draft]);
        app.cursor.select(Some(0));
        assert!(app.current_pr_writable());
    }

    /// #10 The hint parser covers compound key labels.
    #[test]
    fn hint_key_tokens_splits_compound_labels() {
        assert_eq!(hint_key_tokens("j/k"), vec!["j", "k"]);
        assert_eq!(hint_key_tokens("<>"), vec!["<", ">"]);
        assert_eq!(hint_key_tokens("PgUp/PgDn"), vec!["PgUp/PgDn"]);
        assert_eq!(hint_key_tokens("enter"), vec!["enter"]);
    }

    #[test]
    fn key_hinted_elsewhere_flags_inapplicable_but_not_global_or_unbound() {
        // `M` is hinted in Prs but not in Fleet → inapplicable feedback.
        assert!(key_hinted_elsewhere(View::Fleet, 'M'));
        // `q` is global → never flagged.
        assert!(!key_hinted_elsewhere(View::Fleet, 'q'));
        // `z` is not hinted anywhere → stays quiet (truly unbound).
        assert!(!key_hinted_elsewhere(View::Fleet, 'z'));
        // `f` is now hinted in Prs → not flagged there.
        assert!(!key_hinted_elsewhere(View::Prs, 'f'));
    }
}
