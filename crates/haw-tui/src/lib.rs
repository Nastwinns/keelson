//! The haw cockpit: a k9s-style, keyboard-first ratatui dashboard.
//!
//! Views: stacks -> fleet grid -> repo detail, changesets -> changeset grid,
//! tree, help overlay. `/` filters the grid, `:` opens a command bar whose
//! verbs mirror the CLI. Actions run on a worker thread so the UI never
//! freezes; a spinner shows progress.
//!
//! All domain work goes through the [`Controller`] trait — this crate renders
//! and dispatches, nothing more.

use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use haw_core::workspace::RepoStatus;
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table,
    TableState,
};

/// The cockpit skin: Catppuccin-Mocha-leaning, chosen to read on dark terms.
mod theme {
    use ratatui::style::Color;

    pub const ACCENT: Color = Color::Rgb(137, 180, 250);
    pub const MAUVE: Color = Color::Rgb(203, 166, 247);
    pub const GREEN: Color = Color::Rgb(166, 227, 161);
    pub const YELLOW: Color = Color::Rgb(249, 226, 175);
    pub const RED: Color = Color::Rgb(243, 139, 168);
    pub const TEAL: Color = Color::Rgb(148, 226, 213);
    pub const PEACH: Color = Color::Rgb(250, 179, 135);
    pub const TEXT: Color = Color::Rgb(205, 214, 244);
    pub const DIM: Color = Color::Rgb(127, 132, 156);
    pub const SURFACE: Color = Color::Rgb(69, 71, 90);
    pub const SURFACE0: Color = Color::Rgb(49, 50, 68);
    pub const CRUST: Color = Color::Rgb(17, 17, 27);
}

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

/// Everything the cockpit can ask the application to do. Implementations run
/// on a worker thread, so they must be `Send`.
pub trait Controller: Send {
    fn snapshot(&mut self) -> io::Result<Snapshot>;
    /// PR/CI cells for one changeset (network; fetched on drill-in).
    fn changeset_prs(&mut self, id: &str) -> io::Result<ChangesetSummary>;
    fn sync_stack(&mut self, stack: &str) -> io::Result<String>;
    fn sync_repo(&mut self, repo: &str) -> io::Result<String>;
    fn switch(&mut self, stack: &str) -> io::Result<String>;
    fn pin(&mut self) -> io::Result<String>;
    fn lock(&mut self) -> io::Result<String>;
    fn run_cmd(&mut self, cmd: &str) -> io::Result<String>;
    fn change_start(&mut self, id: &str) -> io::Result<String>;
    fn change_request(&mut self, id: &str, only: Option<Vec<String>>) -> io::Result<String>;
    fn change_land(&mut self, id: &str) -> io::Result<String>;
    /// Seal a fully-resolved merge plan for `repo` (see `haw merge cleanup`).
    fn merge_cleanup(&mut self, repo: &str) -> io::Result<String>;
    /// Abort a planned merge for `repo` (see `haw merge abort`).
    fn merge_abort(&mut self, repo: &str) -> io::Result<String>;
}

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug, Clone, Copy, PartialEq)]
enum View {
    Stacks,
    Fleet,
    Changesets,
    Changeset,
    Tree,
}

#[derive(Debug, Clone, PartialEq)]
enum InputMode {
    None,
    Filter(String),
    Command(String),
    NewChangeset(String),
}

enum Job {
    Refresh,
    ChangesetPrs(String),
    Action(&'static str, ActionKind),
}

enum ActionKind {
    SyncStack(String),
    SyncRepo(String),
    Switch(String),
    Pin,
    Lock,
    Run(String),
    ChangeStart(String),
    ChangeRequest(String, Option<Vec<String>>),
    ChangeLand(String),
    MergeCleanup(String),
    MergeAbort(String),
}

enum Outcome {
    Snapshot(Box<io::Result<Snapshot>>),
    ChangesetPrs(Box<io::Result<ChangesetSummary>>),
    Action(&'static str, io::Result<String>),
}

struct App {
    view: View,
    back: Vec<View>,
    snapshot: Snapshot,
    stack: Option<String>,
    changeset: Option<String>,
    selected_repos: Vec<String>,
    cursor: ListState,
    input: InputMode,
    filter: String,
    message: String,
    busy: Option<&'static str>,
    spinner: usize,
    /// Free-running frame counter; paces the input cursor blink.
    tick: u64,
    help: bool,
    goto: Option<PathBuf>,
    /// Set when an action with real side effects (land, request) awaits y/n.
    pending_confirm: Option<Confirm>,
    /// Full multi-repo output from the last `r`/`:run`, shown as a dismissable overlay.
    output: Option<String>,
}

/// A repo matches a filter if its name or any of its groups contains it.
fn repo_matches(name: &str, groups: &[String], filter: &str) -> bool {
    filter.is_empty() || name.contains(filter) || groups.iter().any(|g| g.contains(filter))
}

impl App {
    fn fleet_rows(&self) -> Vec<&RepoStatus> {
        let stack = self.stack.as_deref().unwrap_or_default();
        self.snapshot
            .fleet
            .iter()
            .find(|(name, _)| name == stack)
            .map(|(_, repos)| {
                repos
                    .iter()
                    .filter(|r| repo_matches(&r.name, &r.groups, &self.filter))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn stack_rows(&self) -> Vec<&str> {
        self.snapshot
            .stacks
            .iter()
            .map(String::as_str)
            .filter(|s| self.filter.is_empty() || s.contains(&self.filter))
            .collect()
    }

    fn changeset_rows(&self) -> Vec<&ChangesetSummary> {
        self.snapshot
            .changesets
            .iter()
            .filter(|c| self.filter.is_empty() || c.id.contains(&self.filter))
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
                    .filter(|r| self.filter.is_empty() || r.name.contains(&self.filter))
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
        }
    }

    fn cursor_repo(&self) -> Option<String> {
        let index = self.cursor.selected()?;
        match self.view {
            View::Fleet => self.fleet_rows().get(index).map(|r| r.name.clone()),
            View::Changeset => self.change_repo_rows().get(index).map(|r| r.name.clone()),
            _ => None,
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

    fn goto_view(&mut self, view: View) {
        if self.view != view {
            self.back.push(self.view);
            self.view = view;
            self.cursor.select(Some(0));
            self.filter.clear();
        }
    }

    fn go_back(&mut self) {
        if let Some(previous) = self.back.pop() {
            self.view = previous;
            self.filter.clear();
            self.clamp_cursor();
        }
    }
}

/// Run the cockpit until quit. Returns a path when the user asked to `goto`
/// a repo, so the caller can print it (`cd "$(haw dash)"`).
pub fn run(controller: Box<dyn Controller>) -> io::Result<Option<PathBuf>> {
    let (job_tx, job_rx) = channel::<Job>();
    let (out_tx, out_rx) = channel::<Outcome>();
    spawn_worker(controller, job_rx, out_tx);

    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &job_tx, &out_rx);
    ratatui::restore();
    result
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
                Job::Action(label, kind) => {
                    let result = match kind {
                        ActionKind::SyncStack(stack) => controller.sync_stack(&stack),
                        ActionKind::SyncRepo(repo) => controller.sync_repo(&repo),
                        ActionKind::Switch(stack) => controller.switch(&stack),
                        ActionKind::Pin => controller.pin(),
                        ActionKind::Lock => controller.lock(),
                        ActionKind::Run(cmd) => controller.run_cmd(&cmd),
                        ActionKind::ChangeStart(id) => controller.change_start(&id),
                        ActionKind::ChangeRequest(id, only) => controller.change_request(&id, only),
                        ActionKind::ChangeLand(id) => controller.change_land(&id),
                        ActionKind::MergeCleanup(repo) => controller.merge_cleanup(&repo),
                        ActionKind::MergeAbort(repo) => controller.merge_abort(&repo),
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
    if app.busy.is_none() {
        app.busy = Some("PR status");
        let _ = jobs.send(Job::ChangesetPrs(id.to_string()));
    }
}

/// A confirmation gate for actions with real side effects (opens/merges PRs).
/// `Some` describes the pending action; `y`/`n` (or Enter/Esc) resolve it.
#[derive(Debug, Clone)]
enum Confirm {
    Land(String),
    Request(String, Option<Vec<String>>),
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
            app.message = format!("→ haw run '{cmd}'");
            dispatch(app, jobs, "run", ActionKind::Run(cmd.to_string()));
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
        _ => app.message = format!("unknown command `{line}`"),
    }
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    jobs: &Sender<Job>,
    outcomes: &Receiver<Outcome>,
) -> io::Result<Option<PathBuf>> {
    let mut app = App {
        view: View::Fleet,
        back: Vec::new(),
        snapshot: Snapshot::default(),
        stack: None,
        changeset: None,
        selected_repos: Vec::new(),
        cursor: ListState::default(),
        input: InputMode::None,
        filter: String::new(),
        message: "loading…".to_string(),
        busy: None,
        spinner: 0,
        tick: 0,
        help: false,
        goto: None,
        pending_confirm: None,
        output: None,
    };
    app.cursor.select(Some(0));
    request_refresh(&mut app, jobs);

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
                        Err(err) => app.message = format!("refresh failed: {err}"),
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
                        Err(err) => app.message = format!("PR status failed: {err}"),
                    }
                }
                Outcome::Action(label, result) => {
                    app.busy = None;
                    match result {
                        Ok(message) if label == "run" => {
                            app.message = "ran — press any key to dismiss the output".to_string();
                            app.output = Some(message);
                        }
                        Ok(message) => app.message = message,
                        Err(err) => app.message = format!("{label} failed: {err}"),
                    }
                    request_refresh(&mut app, jobs);
                }
            }
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
            return Ok(app.goto);
        }
        if key.code == KeyCode::F(5)
            || (key.code == KeyCode::Char('r') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            app.message = "refreshing…".to_string();
            request_refresh(&mut app, jobs);
            continue;
        }

        if app.help {
            app.help = false;
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
            | InputMode::NewChangeset(buffer) => {
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
            KeyCode::Char('q') => return Ok(app.goto),
            KeyCode::Char('?') => app.help = true,
            KeyCode::Char('/') => app.input = InputMode::Filter(app.filter.clone()),
            KeyCode::Char(':') => app.input = InputMode::Command(String::new()),
            KeyCode::Esc | KeyCode::Char('b') => {
                if !app.filter.is_empty() {
                    app.filter.clear();
                } else {
                    app.go_back();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.cursor
                    .select(Some((selected + 1).min(app.rows_len().saturating_sub(1))));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.cursor.select(Some(selected.saturating_sub(1)));
            }
            KeyCode::Char('t') => app.goto_view(View::Tree),
            KeyCode::Char('c') => app.goto_view(View::Changesets),
            KeyCode::Char('g') => {
                if let Some(repo) = app.cursor_repo()
                    && let Some(path) = app.repo_path(&repo)
                {
                    app.goto = Some(path);
                    return Ok(app.goto);
                }
                app.message = "goto: put the cursor on a repo row".to_string();
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
                _ => {}
            },
            KeyCode::Char('s') if app.view == View::Fleet => {
                if let Some(repo) = app.cursor_repo() {
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
            KeyCode::Char('p') if app.view == View::Fleet || app.view == View::Stacks => {
                app.message = "→ haw pin".to_string();
                dispatch(&mut app, jobs, "pin", ActionKind::Pin);
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
            KeyCode::Char(' ') if app.view == View::Changeset => {
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
            _ => {}
        }
    }
}

fn view_name(app: &App, view: View) -> String {
    match view {
        View::Stacks => "stacks".to_string(),
        View::Fleet => "fleet".to_string(),
        View::Changesets => "changesets".to_string(),
        View::Changeset => format!("change {}", app.changeset.as_deref().unwrap_or("—")),
        View::Tree => "tree".to_string(),
    }
}

fn key_hints(view: View) -> &'static [(&'static str, &'static str)] {
    match view {
        View::Stacks => &[
            ("enter", "open fleet"),
            ("s", "sync stack"),
            ("S", "switch"),
            ("p", "pin"),
            ("l", "lock"),
            ("c", "changesets"),
            ("t", "tree"),
            ("?", "help"),
        ],
        View::Fleet => &[
            ("s", "sync"),
            ("S", "stacks"),
            ("p", "pin"),
            ("l", "lock"),
            ("c", "changesets"),
            ("r", "run"),
            ("g", "goto"),
            ("t", "tree"),
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
            ("b", "back"),
        ],
        View::Tree => &[("b", "back"), ("q", "quit")],
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

    draw_header(frame, app, zones[0]);
    match app.view {
        View::Stacks => draw_stacks(frame, app, zones[1]),
        View::Fleet => draw_fleet(frame, app, zones[1]),
        View::Changesets => draw_changesets(frame, app, zones[1]),
        View::Changeset => draw_changeset(frame, app, zones[1]),
        View::Tree => draw_tree(frame, app, zones[1]),
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
        draw_help(frame);
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
        .map(|l| Line::styled((*l).to_string(), Style::default().fg(theme::TEXT)))
        .collect();
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(text)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::TEAL))
                .title(Span::styled(
                    format!(" output ({} lines) — any key closes ", all_lines.len()),
                    Style::default()
                        .fg(theme::MAUVE)
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
        Confirm::MergeCleanup(repo) => (
            format!("haw merge cleanup --repo {repo}"),
            "this commits and rewrites branches:",
            "seal the merge and fast-forward its target branch".to_string(),
        ),
    };
    let area = frame.area();
    let width = area.width.min(64);
    let height = 7;
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
                    .fg(theme::YELLOW)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" — {reach}"), Style::default().fg(theme::TEXT)),
        ]),
        Line::raw(""),
        Line::styled(format!(" {detail}"), Style::default().fg(theme::TEXT)),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "y",
                Style::default()
                    .fg(theme::GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("/enter confirm   ", Style::default().fg(theme::DIM)),
            Span::styled(
                "any other key",
                Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" cancels", Style::default().fg(theme::DIM)),
        ]),
    ];
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(text)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::YELLOW))
                .title(Span::styled(
                    " confirm ",
                    Style::default()
                        .fg(theme::YELLOW)
                        .add_modifier(Modifier::BOLD),
                )),
        ),
        popup,
    );
}

fn kv(key: &str, value: Span<'static>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {key:<12}"), Style::default().fg(theme::DIM)),
        value,
    ])
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
        Span::styled("✓ committed", Style::default().fg(theme::GREEN))
    } else {
        Span::styled("✗ absent — run haw lock", Style::default().fg(theme::RED))
    };
    let info = vec![
        kv(
            "context:",
            Span::styled(
                app.snapshot.root_label.clone(),
                Style::default()
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
        ),
        kv(
            "stack:",
            Span::styled(
                app.stack.clone().unwrap_or_else(|| "—".to_string()),
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
        ),
        kv("lock:", lock),
        kv(
            "repos:",
            Span::styled(
                format!("{}", app.fleet_rows().len()),
                Style::default().fg(theme::TEXT),
            ),
        ),
        kv(
            "changesets:",
            Span::styled(
                format!("{}", app.snapshot.changesets.len()),
                Style::default().fg(theme::TEXT),
            ),
        ),
    ];
    frame.render_widget(Paragraph::new(Text::from(info)), columns[0]);

    let hints = key_hints(app.view);
    let mut key_lines: Vec<Line> = Vec::new();
    for pair in hints.chunks(2) {
        let mut spans = Vec::new();
        for (key, label) in pair {
            spans.push(Span::styled(
                format!("<{key}>"),
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!(" {label:<12}"),
                Style::default().fg(theme::DIM),
            ));
        }
        key_lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(Text::from(key_lines)), columns[1]);

    let logo = vec![
        Line::styled("┬ ┬┌─┐┬ ┬", Style::default().fg(theme::MAUVE)),
        Line::styled("├─┤├─┤│││", Style::default().fg(theme::MAUVE)),
        Line::styled("┴ ┴┴ ┴└┴┘", Style::default().fg(theme::MAUVE)),
        Line::styled(" cockpit ⚓", Style::default().fg(theme::DIM)),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(logo)).alignment(Alignment::Right),
        columns[2],
    );
}

fn panel(title: String) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::SURFACE))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(theme::MAUVE)
                .add_modifier(Modifier::BOLD),
        ))
}

fn header_row(cells: &[&'static str]) -> Row<'static> {
    Row::new(
        cells
            .iter()
            .map(|c| {
                Cell::from(Span::styled(
                    *c,
                    Style::default()
                        .fg(theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ))
            })
            .collect::<Vec<_>>(),
    )
}

fn cursor_style() -> Style {
    Style::default()
        .bg(theme::SURFACE0)
        .add_modifier(Modifier::BOLD)
}

fn short(sha: &str) -> &str {
    sha.get(..8).unwrap_or(sha)
}

fn state_dot(repo: &RepoStatus) -> Span<'static> {
    let (dot, color) = if repo.missing {
        ("○", theme::DIM)
    } else if repo.drift {
        ("●", theme::RED)
    } else if repo.dirty {
        ("●", theme::YELLOW)
    } else {
        ("●", theme::GREEN)
    };
    Span::styled(dot, Style::default().fg(color))
}

/// Spans for `↑N ↓N`, green ahead / red behind; `—` without an upstream.
fn ahead_behind_spans(ahead_behind: Option<(u64, u64)>) -> Vec<Span<'static>> {
    match ahead_behind {
        None => vec![Span::styled("—", Style::default().fg(theme::DIM))],
        Some((0, 0)) => vec![Span::styled("up to date", Style::default().fg(theme::DIM))],
        Some((ahead, behind)) => {
            let mut spans = Vec::new();
            if ahead > 0 {
                spans.push(Span::styled(
                    format!("↑{ahead} "),
                    Style::default().fg(theme::GREEN),
                ));
            }
            if behind > 0 {
                spans.push(Span::styled(
                    format!("↓{behind}"),
                    Style::default().fg(theme::RED),
                ));
            }
            spans
        }
    }
}

fn ahead_behind_cell(ahead_behind: Option<(u64, u64)>) -> Line<'static> {
    match ahead_behind {
        Some((0, 0)) => Line::styled("·", Style::default().fg(theme::DIM)),
        other => Line::from(ahead_behind_spans(other)),
    }
}

fn groups_label(groups: &[String]) -> (String, ratatui::style::Color) {
    if groups.is_empty() {
        ("—".to_string(), theme::DIM)
    } else {
        (groups.join(","), theme::TEAL)
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
            let merge_cell = match app.merge_badge(&repo.name) {
                Some(badge) => Cell::from(Span::styled(
                    format!("{}/{}", badge.resolved, badge.total),
                    Style::default().fg(theme::YELLOW),
                )),
                None => Cell::from(Span::styled("—", Style::default().fg(theme::DIM))),
            };
            if repo.missing {
                return Row::new(vec![
                    Cell::from(state_dot(repo)),
                    Cell::from(Span::styled(
                        repo.name.clone(),
                        Style::default().fg(theme::RED),
                    )),
                    Cell::from(Span::styled(groups, Style::default().fg(groups_color))),
                    Cell::from(Span::styled(
                        "not cloned — press s",
                        Style::default().fg(theme::DIM),
                    )),
                ]);
            }
            Row::new(vec![
                Cell::from(state_dot(repo)),
                Cell::from(Span::styled(
                    repo.name.clone(),
                    Style::default()
                        .fg(theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(groups, Style::default().fg(groups_color))),
                Cell::from(Span::styled(
                    repo.branch.clone().unwrap_or_else(|| "(detached)".into()),
                    Style::default().fg(theme::YELLOW),
                )),
                Cell::from(Span::styled(
                    repo.head.as_deref().map_or("—", short).to_string(),
                    Style::default().fg(theme::DIM),
                )),
                Cell::from(if repo.dirty {
                    Span::styled("yes", Style::default().fg(theme::YELLOW))
                } else {
                    Span::styled("·", Style::default().fg(theme::DIM))
                }),
                Cell::from(if repo.drift {
                    Span::styled("DRIFT", Style::default().fg(theme::RED))
                } else {
                    Span::styled("·", Style::default().fg(theme::DIM))
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
            Constraint::Length(5),
            Constraint::Length(6),
            Constraint::Length(9),
            Constraint::Length(7),
        ],
    )
    .header(header_row(&[
        "",
        "REPO",
        "GROUPS",
        "BRANCH",
        "HEAD",
        "DIRTY",
        "DRIFT",
        "↑ / ↓",
        "MERGE",
    ]))
    .block(panel(format!("fleet({count})")))
    .row_highlight_style(cursor_style())
    .highlight_symbol(Span::styled("▍", Style::default().fg(theme::ACCENT)));

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
                            .fg(theme::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("groups ", Style::default().fg(theme::DIM)),
                    Span::styled(groups, Style::default().fg(groups_color)),
                    Span::styled("  · path ", Style::default().fg(theme::DIM)),
                    Span::styled(
                        repo.path.display().to_string(),
                        Style::default().fg(theme::TEXT),
                    ),
                ]),
                {
                    let mut spans = vec![
                        Span::styled(" locked ", Style::default().fg(theme::DIM)),
                        Span::styled(
                            repo.locked_rev.as_deref().map_or("—", short).to_string(),
                            Style::default().fg(theme::TEXT),
                        ),
                        Span::styled("  · remote ", Style::default().fg(theme::DIM)),
                    ];
                    spans.extend(ahead_behind_spans(repo.ahead_behind));
                    spans.push(Span::styled("  · ", Style::default().fg(theme::DIM)));
                    spans.push(if repo.missing {
                        Span::styled("NOT CLONED", Style::default().fg(theme::RED))
                    } else if repo.drift {
                        Span::styled("DRIFT (head ≠ lock)", Style::default().fg(theme::RED))
                    } else if repo.dirty {
                        Span::styled("dirty worktree", Style::default().fg(theme::YELLOW))
                    } else {
                        Span::styled("in sync ✓", Style::default().fg(theme::GREEN))
                    });
                    Line::from(spans)
                },
            ]
        }
        None => vec![Line::styled(
            " no repos — check haw.toml",
            Style::default().fg(theme::DIM),
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
            Span::styled(" merge ", Style::default().fg(theme::MAUVE)),
            Span::styled(badge.source.clone(), Style::default().fg(theme::YELLOW)),
            Span::styled(
                format!("  {}/{} slices resolved", badge.resolved, badge.total),
                Style::default().fg(theme::TEXT),
            ),
            Span::styled(
                "  · :merge cleanup / :merge abort",
                Style::default().fg(theme::DIM),
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
                        .fg(theme::ACCENT)
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
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT)
            };
            ListItem::new(Line::from(vec![
                marker,
                Span::styled((*name).to_string(), style),
                Span::styled(
                    format!("  · {count} repos"),
                    Style::default().fg(theme::DIM),
                ),
            ]))
        })
        .collect();
    let count = items.len();
    let list = List::new(items)
        .block(panel(format!("stacks({count})")))
        .highlight_style(cursor_style());
    frame.render_stateful_widget(list, area, &mut app.cursor);
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
                        .fg(theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  · {} repos", c.repos.len()),
                    Style::default().fg(theme::DIM),
                ),
            ]))
        })
        .collect();
    let count = items.len();
    let list = List::new(items)
        .block(panel(format!("changesets({count})")))
        .highlight_style(cursor_style());
    frame.render_stateful_widget(list, area, &mut app.cursor);
}

fn pr_span(text: &str) -> Span<'static> {
    let color = if text.contains("open") {
        theme::GREEN
    } else if text.contains("merged") {
        theme::MAUVE
    } else if text.contains("draft") {
        theme::PEACH
    } else if text.contains("closed") {
        theme::RED
    } else {
        theme::DIM
    };
    Span::styled(text.to_string(), Style::default().fg(color))
}

fn ci_span(text: &str) -> Span<'static> {
    let lower = text.to_lowercase();
    let color = if text.contains('✓') || lower.contains("pass") {
        theme::GREEN
    } else if lower.contains("fail") || text.contains('✗') {
        theme::RED
    } else if lower.contains("run") || lower.contains("pend") || text.contains('⏳') {
        theme::YELLOW
    } else {
        theme::DIM
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
                    Span::styled("◉", Style::default().fg(theme::TEAL))
                } else {
                    Span::styled("·", Style::default().fg(theme::DIM))
                }),
                Cell::from(Span::styled(
                    repo.name.clone(),
                    Style::default()
                        .fg(theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    repo.branch.clone(),
                    Style::default().fg(theme::YELLOW),
                )),
                Cell::from(if repo.on_branch {
                    Span::styled("yes", Style::default().fg(theme::GREEN))
                } else {
                    Span::styled("NO", Style::default().fg(theme::RED))
                }),
                Cell::from(if repo.dirty {
                    Span::styled("yes", Style::default().fg(theme::YELLOW))
                } else {
                    Span::styled("·", Style::default().fg(theme::DIM))
                }),
                Cell::from(Span::styled(
                    repo.head.as_deref().map_or("—", short).to_string(),
                    Style::default().fg(theme::DIM),
                )),
                Cell::from(Span::styled(
                    repo.forge.clone(),
                    Style::default().fg(if repo.forge == "github" {
                        theme::ACCENT
                    } else if repo.forge == "gitlab" {
                        theme::PEACH
                    } else {
                        theme::DIM
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
        "change {}",
        app.changeset.as_deref().unwrap_or_default()
    )))
    .row_highlight_style(cursor_style())
    .highlight_symbol(Span::styled("▍", Style::default().fg(theme::ACCENT)));

    let mut state = TableState::default();
    state.select(app.cursor.selected());
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_tree(frame: &mut Frame, app: &mut App, area: Rect) {
    let text: Vec<Line> = app
        .snapshot
        .tree
        .lines()
        .map(|l| Line::styled(l.to_string(), Style::default().fg(theme::TEXT)))
        .collect();
    frame.render_widget(
        Paragraph::new(Text::from(text)).block(panel("tree".to_string())),
        area,
    );
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
                    .fg(theme::MAUVE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(buffer.clone(), Style::default().fg(theme::TEXT)),
            Span::styled(caret, Style::default().fg(theme::TEXT)),
            Span::styled(
                "   (live filter by name or group)",
                Style::default().fg(theme::DIM),
            ),
        ]),
        (InputMode::Command(buffer), _) => Line::from(vec![
            Span::styled(
                " ❯ ",
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(buffer.clone(), Style::default().fg(theme::TEXT)),
            Span::styled(caret, Style::default().fg(theme::TEXT)),
            Span::styled(
                "   (mirrors the CLI — try: sync · switch <stack> · run <cmd> · tree)",
                Style::default().fg(theme::DIM),
            ),
        ]),
        (InputMode::NewChangeset(buffer), _) => Line::from(vec![
            Span::styled(" new changeset: ", Style::default().fg(theme::MAUVE)),
            Span::styled(buffer.clone(), Style::default().fg(theme::TEXT)),
            Span::styled(caret, Style::default().fg(theme::TEXT)),
        ]),
        (InputMode::None, Some(label)) => Line::from(vec![
            Span::styled(
                format!(" {} ", SPINNER[app.spinner]),
                Style::default().fg(theme::ACCENT),
            ),
            Span::styled(format!("{label}…"), Style::default().fg(theme::TEXT)),
        ]),
        (InputMode::None, None) => {
            let msg = &app.message;
            let color =
                if msg.contains("failed") || msg.contains("error") || msg.contains("unknown") {
                    theme::RED
                } else if msg.starts_with('→') {
                    theme::TEAL
                } else {
                    theme::DIM
                };
            Line::styled(format!(" {msg}"), Style::default().fg(color))
        }
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_crumbs(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for view in &app.back {
        spans.push(Span::styled(
            format!(" {} ", view_name(app, *view)),
            Style::default().fg(theme::DIM).bg(theme::SURFACE0),
        ));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        format!(" {} ", view_name(app, app.view)),
        Style::default()
            .fg(theme::CRUST)
            .bg(theme::ACCENT)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    frame.render_widget(
        Paragraph::new(Line::styled(
            "⚓ haw v0.1.0 ",
            Style::default().fg(theme::DIM),
        ))
        .alignment(Alignment::Right),
        area,
    );
}

fn help_entry(key: &'static str, desc: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {key:<10}"),
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(desc, Style::default().fg(theme::TEXT)),
    ])
}

fn help_section(title: &'static str) -> Line<'static> {
    Line::styled(
        format!(" {title}"),
        Style::default()
            .fg(theme::MAUVE)
            .add_modifier(Modifier::BOLD),
    )
}

fn draw_help(frame: &mut Frame) {
    let area = frame.area();
    let width = area.width.min(60);
    let height = area.height.min(24);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    let text = vec![
        help_section("navigation"),
        help_entry("j / k", "move · enter drill in · esc/b back"),
        help_entry("q", "quit · ctrl-c force quit"),
        help_entry("F5", "refresh now · ctrl-r also works"),
        Line::raw(""),
        help_section("fleet"),
        help_entry("s", "sync repo under cursor (or stack)"),
        help_entry("S", "stacks view · p pin · l lock"),
        help_entry("t", "tree · c changesets · r run · g goto"),
        help_entry("/", "filter by name or group — reopens with your text"),
        Line::raw(""),
        help_section("changeset"),
        help_entry("n", "new · space select repos"),
        help_entry("R", "request PR/MRs (cross-linked, asks y/n)"),
        help_entry("L", "land in dependency order (asks y/n)"),
        Line::raw(""),
        help_section("command bar"),
        help_entry(":sync", "· :stack NAME · :run CMD · :tree"),
        help_entry(":change", "[ID | start ID | land ID | request ID]"),
        Line::raw(""),
        Line::styled(" press any key to close", Style::default().fg(theme::DIM)),
    ];
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Text::from(text)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(theme::ACCENT))
                .title(Span::styled(
                    " help ",
                    Style::default()
                        .fg(theme::MAUVE)
                        .add_modifier(Modifier::BOLD),
                )),
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
            cursor,
            input: InputMode::None,
            filter: String::new(),
            message: String::new(),
            busy: None,
            spinner: 0,
            tick: 0,
            help: false,
            goto: None,
            pending_confirm: None,
            output: None,
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
        for v in [
            View::Stacks,
            View::Fleet,
            View::Changesets,
            View::Changeset,
            View::Tree,
        ] {
            assert!(!view_name(&app, v).is_empty());
            assert!(!key_hints(v).is_empty());
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

    #[test]
    fn unknown_command_reports() {
        let mut app = fleet_app();
        let (tx, _rx) = channel();
        run_command_bar(&mut app, &tx, "frobnicate");
        assert!(app.message.contains("unknown command"));
    }
}
