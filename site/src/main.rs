//! haw.dev — the fleet cockpit, rendered in the browser with Ratzilla.
//!
//! A standalone showcase: real ratatui widgets, real Ratzilla DOM backend, a
//! scripted fleet of repos cycling through sync/dirty/drift states so the
//! page demonstrates the TUI's look without needing a real git backend
//! (which can't run inside a wasm sandbox). Colors mirror `haw-tui`'s theme.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Modifier, Style};
use ratzilla::ratatui::text::{Line, Span, Text};
use ratzilla::ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table};
use ratzilla::ratatui::Terminal;
use ratzilla::{DomBackend, WebRenderer};

mod theme {
    use ratzilla::ratatui::style::Color;

    pub const ACCENT: Color = Color::Rgb(137, 180, 250);
    pub const MAUVE: Color = Color::Rgb(203, 166, 247);
    pub const GREEN: Color = Color::Rgb(166, 227, 161);
    pub const YELLOW: Color = Color::Rgb(249, 226, 175);
    pub const RED: Color = Color::Rgb(243, 139, 168);
    pub const TEAL: Color = Color::Rgb(148, 226, 213);
    pub const TEXT: Color = Color::Rgb(205, 214, 244);
    pub const DIM: Color = Color::Rgb(127, 132, 156);
    pub const SURFACE: Color = Color::Rgb(69, 71, 90);
    pub const SURFACE0: Color = Color::Rgb(49, 50, 68);
}

#[derive(Clone, Copy)]
enum RepoState {
    Clean,
    Dirty,
    Drift,
    Missing,
}

struct Repo {
    name: &'static str,
    branch: &'static str,
    head: &'static str,
    state: RepoState,
}

const SCRIPT: &[(&str, &str, &str, u32)] = &[
    ("kernel", "v6.1.2", "a1b2c3d4", 0),
    ("hal", "main", "9f8e7d6c", 40),
    ("app-mqtt", "release/2.x", "4d5e6f7a", 90),
    ("sensor-fw", "main", "eeff0011", 140),
];

fn repos_at(tick: u32) -> Vec<Repo> {
    SCRIPT
        .iter()
        .map(|(name, branch, head, offset)| {
            let phase = (tick + offset) % 200;
            let state = match phase {
                0..=119 => RepoState::Clean,
                120..=149 => RepoState::Dirty,
                150..=169 => RepoState::Drift,
                _ => RepoState::Missing,
            };
            Repo {
                name,
                branch,
                head,
                state,
            }
        })
        .collect()
}

fn state_dot(state: RepoState) -> Span<'static> {
    match state {
        RepoState::Clean => Span::styled("●", Style::default().fg(theme::GREEN)),
        RepoState::Dirty => Span::styled("●", Style::default().fg(theme::YELLOW)),
        RepoState::Drift => Span::styled("●", Style::default().fg(theme::RED)),
        RepoState::Missing => Span::styled("○", Style::default().fg(theme::DIM)),
    }
}

fn state_cells(state: RepoState) -> (Span<'static>, Span<'static>) {
    let (dirty, drift) = match state {
        RepoState::Clean => ("·", "·"),
        RepoState::Dirty => ("yes", "·"),
        RepoState::Drift => ("·", "DRIFT"),
        RepoState::Missing => ("—", "—"),
    };
    let dirty_color = if dirty == "yes" {
        theme::YELLOW
    } else {
        theme::DIM
    };
    let drift_color = if drift == "DRIFT" {
        theme::RED
    } else {
        theme::DIM
    };
    (
        Span::styled(dirty, Style::default().fg(dirty_color)),
        Span::styled(drift, Style::default().fg(drift_color)),
    )
}

fn panel(title: &str) -> Block<'static> {
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

fn draw(frame: &mut ratzilla::ratatui::Frame, tick: u32) {
    let zones = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(2),
        ])
        .split(frame.area());

    draw_header(frame, zones[0], tick);
    draw_fleet(frame, zones[1], tick);
    draw_status(frame, zones[2], tick);
    draw_footer(frame, zones[3]);
}

fn draw_header(frame: &mut ratzilla::ratatui::Frame, area: Rect, tick: u32) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(20)])
        .split(area);

    let repos = repos_at(tick);
    let clean = repos
        .iter()
        .filter(|r| matches!(r.state, RepoState::Clean))
        .count();
    let info = vec![Line::from(vec![
        Span::styled(" context: ", Style::default().fg(theme::DIM)),
        Span::styled(
            "~/work/gateway",
            Style::default()
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   stack: ", Style::default().fg(theme::DIM)),
        Span::styled(
            "gateway",
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   lock: ", Style::default().fg(theme::DIM)),
        Span::styled("✓ committed", Style::default().fg(theme::GREEN)),
        Span::styled("   in sync: ", Style::default().fg(theme::DIM)),
        Span::styled(
            format!("{clean}/{}", repos.len()),
            Style::default().fg(theme::TEXT),
        ),
    ])];
    frame.render_widget(
        Paragraph::new(Text::from(info)).block(panel("haw ▸ fleet cockpit")),
        columns[0],
    );

    let logo = vec![Line::styled(
        "HAW ⚓",
        Style::default()
            .fg(theme::MAUVE)
            .add_modifier(Modifier::BOLD),
    )];
    frame.render_widget(
        Paragraph::new(Text::from(logo))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme::SURFACE)),
            ),
        columns[1],
    );
}

fn draw_fleet(frame: &mut ratzilla::ratatui::Frame, area: Rect, tick: u32) {
    let repos = repos_at(tick);
    let cursor = (tick / 25) as usize % repos.len();

    let header = Row::new(vec![
        Cell::from(""),
        Cell::from(Span::styled(
            "REPO",
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            "BRANCH",
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            "HEAD",
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            "DIRTY",
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            "DRIFT",
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
    ]);

    let rows: Vec<Row> = repos
        .iter()
        .enumerate()
        .map(|(i, repo)| {
            let (dirty, drift) = state_cells(repo.state);
            let name_style = if i == cursor {
                Style::default()
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD)
                    .bg(theme::SURFACE0)
            } else {
                Style::default().fg(theme::TEXT)
            };
            Row::new(vec![
                Cell::from(state_dot(repo.state)),
                Cell::from(Span::styled(repo.name, name_style)),
                Cell::from(Span::styled(
                    repo.branch,
                    Style::default().fg(theme::YELLOW),
                )),
                Cell::from(Span::styled(repo.head, Style::default().fg(theme::DIM))),
                Cell::from(dirty),
                Cell::from(drift),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Min(10),
            Constraint::Min(14),
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Length(6),
        ],
    )
    .header(header)
    .block(panel(&format!("fleet({})", repos.len())));

    frame.render_widget(table, area);
}

fn draw_status(frame: &mut ratzilla::ratatui::Frame, area: Rect, tick: u32) {
    let messages = [
        "→ haw sync --stack gateway",
        "wrote haw.lock (4 repos pinned)",
        "→ haw change start FEAT-42 --repos kernel,hal",
        "changeset `FEAT-42` started across 2 repo(s)",
        "ready — press ? for help",
    ];
    let message = messages[(tick / 40) as usize % messages.len()];
    frame.render_widget(
        Paragraph::new(Line::styled(
            format!(" {message}"),
            Style::default().fg(theme::TEAL),
        )),
        area,
    );
}

fn draw_footer(frame: &mut ratzilla::ratatui::Frame, area: Rect) {
    let lines = vec![
        Line::styled(
            "<s> sync  <S> stacks  <p> pin  <l> lock  <c> changesets  <r> run  <g> goto",
            Style::default().fg(theme::DIM),
        ),
        Line::styled(
            "scripted demo — the real cockpit: cargo install hawser",
            Style::default().fg(theme::SURFACE),
        ),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

// wasm32-unknown-unknown runs single-threaded on the browser's main thread —
// there's no real preemption, so a critical section is a no-op. ratatui-core
// needs an impl registered; the browser event loop provides no other one.
struct NoPreemption;
critical_section::set_impl!(NoPreemption);
unsafe impl critical_section::Impl for NoPreemption {
    unsafe fn acquire() -> critical_section::RawRestoreState {}
    unsafe fn release(_token: critical_section::RawRestoreState) {}
}

fn main() {
    let tick = Rc::new(RefCell::new(0u32));
    let backend = DomBackend::new().expect("DOM backend");
    let terminal = Terminal::new(backend).expect("terminal");

    terminal.draw_web(move |frame| {
        let mut t = tick.borrow_mut();
        *t = t.wrapping_add(1);
        draw(frame, *t);
    });
}
