---
paths:
  - "crates/**"
  - "xtask/**"
  - "Cargo.toml"
---

# Rust standards (Keelson)

- Edition 2024. All external deps declared once in `[workspace.dependencies]`; crates use
  `dep.workspace = true`.
- Every crate sets `[lints] workspace = true`. `unsafe_code` is forbidden workspace-wide.
- Must pass before any commit: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Errors: `thiserror` typed enums in library crates; `anyhow` only in binaries
  (`hawser`, `xtask`).
- No `unwrap()`/`expect()` in library code paths; tests may (allowed per-file).
- All business logic in `haw-core`. `hawser` and `haw-tui` stay thin: parse args,
  call core, render. No domain decisions in front-ends.
- Formats and forges behind traits (`ManifestLoader`, `Forge`); new format/forge = new impl.
- Cross-platform: `PathBuf`/`Path` only, never hard-coded `/`. No symlinks in any
  workspace layout. Don't assume LF.
- Public API items get rustdoc (`///`). No inline `//` comments explaining code.
- Git mutations shell out to `git`; reads go through gitoxide (`gix`).
