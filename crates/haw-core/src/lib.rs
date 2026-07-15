//! Keelson domain logic. No I/O opinions leaked: front-ends (CLI, TUI, GUI)
//! call this API and render the results. Git side effects go through the
//! [`git::GitBackend`] trait so the core stays testable and backend-agnostic.

pub mod audit;
pub mod change;
pub mod git;
pub mod hooks;
pub mod lock;
pub mod manifest;
pub mod resolver;
pub mod snapshot;
pub mod workspace;
