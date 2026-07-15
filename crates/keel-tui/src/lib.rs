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

use keel_core::workspace::RepoStatus;
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
    help: bool,
    goto: Option<PathBuf>,
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
                    .filter(|r| self.filter.is_empty() || r.name.contains(&self.filter))
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

fn run_command_bar(app: &mut App, jobs: &Sender<Job>, line: &str) {
    let (verb, rest) = line
        .trim()
        .split_once(' ')
        .map_or((line.trim(), ""), |(v, r)| (v, r.trim()));
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
        ("change", id) if !id.is_empty() => {
            app.message = format!("→ haw change start {id}");
            dispatch(
                app,
                jobs,
                "change start",
                ActionKind::ChangeStart(id.to_string()),
            );
        }
        ("pin", "") => {
            app.message = "→ haw pin".to_string();
            dispatch(app, jobs, "pin", ActionKind::Pin);
        }
        ("lock", "") => {
            app.message = "→ haw lock".to_string();
            dispatch(app, jobs, "lock", ActionKind::Lock);
        }
        ("tree", "") => app.goto_view(View::Tree),
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
        help: false,
        goto: None,
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
                        Ok(message) => app.message = message,
                        Err(err) => app.message = format!("{label} failed: {err}"),
                    }
                    request_refresh(&mut app, jobs);
                }
            }
        }

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

        if app.help {
            app.help = false;
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
            KeyCode::Char('/') => app.input = InputMode::Filter(String::new()),
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
                        app.changeset = Some(id.clone());
                        app.selected_repos.clear();
                        app.goto_view(View::Changeset);
                        if app.busy.is_none() {
                            app.busy = Some("PR status");
                            let _ = jobs.send(Job::ChangesetPrs(id));
                        }
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
                    app.message = format!("→ haw change request {id}");
                    dispatch(
                        &mut app,
                        jobs,
                        "change request",
                        ActionKind::ChangeRequest(id, only),
                    );
                }
            }
            KeyCode::Char('L') if app.view == View::Changeset => {
                if let Some(id) = app.changeset.clone() {
                    app.message = format!("→ haw change land {id}");
                    dispatch(&mut app, jobs, "change land", ActionKind::ChangeLand(id));
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

    if app.help {
        draw_help(frame);
    }
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

fn draw_fleet(frame: &mut Frame, app: &mut App, area: Rect) {
    let zones = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(area);

    let rows: Vec<Row> = app
        .fleet_rows()
        .iter()
        .map(|repo| {
            if repo.missing {
                return Row::new(vec![
                    Cell::from(state_dot(repo)),
                    Cell::from(Span::styled(
                        repo.name.clone(),
                        Style::default().fg(theme::RED),
                    )),
                    Cell::from(Span::styled(
                        "not cloned — press s",
                        Style::default().fg(theme::DIM),
                    )),
                ]);
            }
            let ahead_behind = repo
                .ahead_behind
                .map_or("—".to_string(), |(a, b)| format!("↑{a} ↓{b}"));
            Row::new(vec![
                Cell::from(state_dot(repo)),
                Cell::from(Span::styled(
                    repo.name.clone(),
                    Style::default()
                        .fg(theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                )),
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
                Cell::from(Span::styled(ahead_behind, Style::default().fg(theme::TEAL))),
            ])
        })
        .collect();

    let count = rows.len();
    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Min(10),
            Constraint::Min(14),
            Constraint::Length(9),
            Constraint::Length(5),
            Constraint::Length(6),
            Constraint::Length(9),
        ],
    )
    .header(header_row(&[
        "",
        "REPO",
        "BRANCH",
        "HEAD",
        "DIRTY",
        "DRIFT",
        "↑ / ↓",
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
    let line = match detail {
        Some(repo) => Line::from(vec![
            Span::styled(
                format!(" {} ", repo.name),
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("path ", Style::default().fg(theme::DIM)),
            Span::styled(
                format!("{} ", repo.path.display()),
                Style::default().fg(theme::TEXT),
            ),
            Span::styled("· locked ", Style::default().fg(theme::DIM)),
            Span::styled(
                repo.locked_rev.as_deref().map_or("—", short).to_string(),
                Style::default().fg(theme::TEXT),
            ),
            Span::styled(" · ", Style::default().fg(theme::DIM)),
            if repo.missing {
                Span::styled("NOT CLONED", Style::default().fg(theme::RED))
            } else if repo.drift {
                Span::styled("DRIFT (head ≠ lock)", Style::default().fg(theme::RED))
            } else if repo.dirty {
                Span::styled("dirty worktree", Style::default().fg(theme::YELLOW))
            } else {
                Span::styled("in sync ✓", Style::default().fg(theme::GREEN))
            },
        ]),
        None => Line::styled(
            " no repos — check keel.toml",
            Style::default().fg(theme::DIM),
        ),
    };
    frame.render_widget(
        Paragraph::new(line).block(panel("detail".to_string())),
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
            Constraint::Min(12),
            Constraint::Min(10),
        ],
    )
    .header(header_row(&[
        "", "REPO", "BRANCH", "ON IT", "DIRTY", "HEAD", "PR / MR", "CI",
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

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let line = match (&app.input, app.busy) {
        (InputMode::Filter(buffer), _) => Line::from(vec![
            Span::styled(
                " /",
                Style::default()
                    .fg(theme::MAUVE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(buffer.clone(), Style::default().fg(theme::TEXT)),
            Span::styled("▏", Style::default().fg(theme::DIM)),
        ]),
        (InputMode::Command(buffer), _) => Line::from(vec![
            Span::styled(
                " ❯ ",
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(buffer.clone(), Style::default().fg(theme::TEXT)),
            Span::styled("▏", Style::default().fg(theme::DIM)),
        ]),
        (InputMode::NewChangeset(buffer), _) => Line::from(vec![
            Span::styled(" new changeset: ", Style::default().fg(theme::MAUVE)),
            Span::styled(buffer.clone(), Style::default().fg(theme::TEXT)),
            Span::styled("▏", Style::default().fg(theme::DIM)),
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
        Line::raw(""),
        help_section("fleet"),
        help_entry("s", "sync repo under cursor (or stack)"),
        help_entry("S", "stacks view · p pin · l lock"),
        help_entry("t", "tree · c changesets · r run · g goto"),
        Line::raw(""),
        help_section("changeset"),
        help_entry("n", "new · space select repos"),
        help_entry("R", "request PR/MRs (cross-linked)"),
        help_entry("L", "land in dependency order"),
        Line::raw(""),
        help_section("command bar"),
        help_entry(":sync", "· :stack NAME · :run CMD"),
        help_entry(":change", "ID · :pin · :lock · :tree"),
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
