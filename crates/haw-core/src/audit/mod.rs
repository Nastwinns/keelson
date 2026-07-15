//! Structured audit log (COMPLIANCE §8): every mutating operation appends one
//! machine-readable JSON line to `.haw/audit.jsonl` — actor, operation,
//! affected repo, before/after object id, timestamp.

use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::workspace::Workspace;

/// Errors appending to the audit log.
#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("failed to append to the audit log at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not serialize the audit entry")]
    Serialize(#[from] serde_json::Error),
}

/// One audit log line (schema `haw.audit/1`).
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub schema: &'static str,
    /// Seconds since the Unix epoch (wall clock is allowed here: the audit
    /// trail is evidence, not part of deterministic resolution).
    pub ts: u64,
    pub actor: String,
    pub op: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

fn actor() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Append one entry for a mutating operation.
pub fn record(
    ws: &Workspace,
    op: &str,
    repo: Option<&str>,
    before: Option<&str>,
    after: Option<&str>,
) -> Result<(), AuditError> {
    let entry = Entry {
        schema: "haw.audit/1",
        ts: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        actor: actor(),
        op: op.to_string(),
        repo: repo.map(str::to_string),
        before: before.map(str::to_string),
        after: after.map(str::to_string),
    };
    let dir = ws.state_dir();
    std::fs::create_dir_all(&dir).map_err(|source| AuditError::Io {
        path: dir.clone(),
        source,
    })?;
    let path = dir.join("audit.jsonl");
    let mut line = serde_json::to_string(&entry)?;
    line.push('\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(line.as_bytes()))
        .map_err(|source| AuditError::Io { path, source })
}
