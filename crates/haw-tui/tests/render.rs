//! Terminal-render (rasterized) snapshot tests for the fleet dashboard.
//!
//! The rest of the suite tests `App` state in isolation and never renders to a
//! terminal, so layout/wrapping/overflow regressions slip through. Here we
//! drive the *real* draw path via ratatui's [`TestBackend`] (no process spawn,
//! fully deterministic) through the `haw_tui::render_snapshot` test seam, and
//! assert on the resulting cell grid.
//!
//! Two sizes are checked:
//!   * normal (40x160): the fleet header row + every repo row is visible;
//!   * small  (10x40): the collapsible header shrinks to one line so data rows
//!     stay on screen (audit fix #3).
//!
//! The `⚓ haw v<version>` footer must appear in its own right-aligned cell.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use haw_core::workspace::RepoStatus;
use haw_tui::Snapshot;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

fn repo(name: &str, groups: &[&str]) -> RepoStatus {
    RepoStatus {
        name: name.to_string(),
        path: PathBuf::from(format!("/w/{name}")),
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

fn fleet_snapshot() -> Snapshot {
    Snapshot {
        root_label: "acme".to_string(),
        stacks: vec!["gw".to_string()],
        current_stack: Some("gw".to_string()),
        fleet: vec![(
            "gw".to_string(),
            vec![
                repo("kernel", &["firmware", "ci"]),
                repo("hal", &["firmware"]),
                repo("app-mqtt", &[]),
            ],
        )],
        ..Default::default()
    }
}

/// Flatten a rendered [`Buffer`] into one `String` per row (symbols joined,
/// styling dropped) so tests can assert on visible text.
fn rows_text(buf: &Buffer) -> Vec<String> {
    let area = buf.area;
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

fn render(width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    haw_tui::render_snapshot(&mut terminal, fleet_snapshot()).unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn fleet_grid_renders_header_and_all_repo_rows_at_normal_size() {
    let buf = render(160, 40);
    let rows = rows_text(&buf);
    let screen = rows.join("\n");

    // Header row: the fleet table column headers are all present.
    for col in [
        "REPO", "GROUPS", "BRANCH", "HEAD", "DIRTY", "DRIFT", "MERGE",
    ] {
        assert!(
            screen.contains(col),
            "fleet header column {col:?} missing from render:\n{screen}"
        );
    }

    // Every repo row is rendered.
    for name in ["kernel", "hal", "app-mqtt"] {
        assert!(
            screen.contains(name),
            "repo row {name:?} missing from render:\n{screen}"
        );
    }

    // Group labels show up too (data cells, not just headers).
    assert!(
        screen.contains("firmware"),
        "group label missing:\n{screen}"
    );
}

#[test]
fn footer_shows_anchored_version_cell() {
    let buf = render(160, 40);
    let rows = rows_text(&buf);
    // Footer is the last rendered row; the version tag is right-aligned into its
    // own cell so the breadcrumb trail can never overwrite it.
    let footer = rows.last().expect("at least one row");
    // The anchor glyph is double-width, so the terminal pads it; assert on the
    // stable text portion plus the anchor's presence.
    let expected = format!("haw v{}", env!("CARGO_PKG_VERSION"));
    assert!(
        footer.contains(&expected) && footer.contains('⚓'),
        "footer {footer:?} missing version tag {expected:?}"
    );
    // Right-aligned: the tag sits at the far right of the line.
    assert!(
        footer.trim_end().ends_with(env!("CARGO_PKG_VERSION")),
        "version tag is not right-aligned in footer {footer:?}"
    );
}

#[test]
fn fleet_header_shows_digit_view_jumps_and_no_stale_capital_hints() {
    let buf = render(160, 40);
    let rows = rows_text(&buf);
    let screen = rows.join("\n");

    // The k9s-style digit view-jump cell is present in the fleet hint grid.
    assert!(
        screen.contains("1-7"),
        "fleet header is missing the `1-7` digit view-jump hint:\n{screen}"
    );
    // A representative frozen global is still advertised.
    assert!(
        screen.contains("watch"),
        "fleet header is missing the `w watch` hint:\n{screen}"
    );

    // The retired `switch stack` label (now the `:stack` command) is gone.
    assert!(
        !screen.contains("switch stack"),
        "fleet header still advertises the retired `switch stack` (now :stack):\n{screen}"
    );

    // The retired capital-letter keycaps must be gone from the hint grid. Each
    // hint key renders wrapped in a `<key>` field, so match the exact keycaps.
    for stale in ["<S>", "<F>", "<M>"] {
        assert!(
            !screen.contains(stale),
            "fleet header still shows retired keycap {stale:?}:\n{screen}"
        );
    }
}

#[test]
fn small_terminal_collapses_header_and_keeps_data_rows_visible() {
    // 10 rows is below COMPACT_HEADER_HEIGHT (16): the ~6-row header must
    // collapse to a single compact line, leaving data rows on screen.
    let buf = render(40, 10);
    let rows = rows_text(&buf);
    let screen = rows.join("\n");

    // The compact header banner (⚓ + root label) is present...
    assert!(
        screen.contains("⚓") && screen.contains("acme"),
        "compact header banner missing on small terminal:\n{screen}"
    );

    // ...and — the whole point of the collapse — at least one repo data row is
    // still visible despite the cramped height.
    let visible_repos = ["kernel", "hal", "app-mqtt"]
        .iter()
        .filter(|name| screen.contains(**name))
        .count();
    assert!(
        visible_repos >= 1,
        "no repo data rows visible after header collapse:\n{screen}"
    );

    // The header did collapse: with only 10 rows, the full multi-row header
    // would leave no room for the fleet panel title. Assert the panel rendered.
    assert!(
        screen.contains("fleet"),
        "fleet panel title missing on small terminal:\n{screen}"
    );
}

/// Render the FileTree view and assert on the indented rows, expand markers,
/// and the honest `@ <ref>` header — the tree-mode counterpart of the fleet
/// grid checks above, driven through the `render_file_tree` test seam.
fn render_tree(width: u16, height: u16) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    let paths = vec![
        "README.md".to_string(),
        "src/lib.rs".to_string(),
        "src/bin/main.rs".to_string(),
    ];
    // `src` expanded, `src/bin` collapsed, pinned at tag v1.0.0.
    haw_tui::render_file_tree(&mut terminal, "kernel", paths, &["src"], Some("v1.0.0")).unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn file_tree_renders_indented_rows_expand_markers_and_ref_header() {
    let buf = render_tree(40, 160);
    let rows = rows_text(&buf);
    let screen = rows.join("\n");

    // The panel title carries the repo and the honest active-ref label.
    assert!(
        screen.contains("tree kernel") && screen.contains("@ v1.0.0"),
        "tree header/ref label missing:\n{screen}"
    );
    // The expanded dir shows the ▾ marker; a collapsed dir shows ▸.
    assert!(
        screen.contains('▾'),
        "expanded dir marker (▾) missing:\n{screen}"
    );
    assert!(
        screen.contains('▸'),
        "collapsed dir marker (▸) missing:\n{screen}"
    );
    // The child of the expanded `src/` is visible and indented (its leaf name
    // sits deeper than the top-level entries).
    let lib_row = rows
        .iter()
        .find(|r| r.contains("lib.rs"))
        .expect("lib.rs row present after expanding src");
    let bin_row = rows
        .iter()
        .find(|r| r.contains("bin/"))
        .expect("bin/ row present under expanded src");
    let readme_row = rows
        .iter()
        .find(|r| r.contains("README.md"))
        .expect("top-level README row present");
    let indent = |s: &str| s.len() - s.trim_start_matches([' ', '│', '▍', '·', '▸', '▾']).len();
    assert!(
        indent(lib_row) > indent(readme_row),
        "child row should be indented deeper than a top-level row:\nchild={lib_row:?}\ntop={readme_row:?}"
    );
    // The nested `bin` dir stays collapsed (its child main.rs is hidden).
    assert!(
        !screen.contains("main.rs"),
        "collapsed nested dir must hide its children:\n{screen}"
    );
    let _ = bin_row;
}
