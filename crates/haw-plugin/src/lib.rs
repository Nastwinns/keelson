//! SDK for authoring `haw-<name>` plugins.
//!
//! A plugin is a standalone executable haw dispatches out-of-process (see
//! `docs/PLUGINS.md`). This crate parses the `haw.plugin/1` context haw passes on
//! the `HAW_JSON` env var / stdin, and helps emit a `haw.plugin.report/1` report.
//!
//! This is a skeleton — the SDK surface is filled in by the plugin-foundation work.

/// The wire-contract version this SDK targets.
pub const CONTRACT: &str = "haw.plugin/1";

/// The report-schema version plugins emit on stdout.
pub const REPORT_SCHEMA: &str = "haw.plugin.report/1";
