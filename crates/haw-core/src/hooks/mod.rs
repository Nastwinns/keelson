//! Lifecycle hooks: scripts in `.haw/hooks/` fired around haw operations.
//! Context arrives via env (`HAW_ROOT`, `HAW_HOOK`) and JSON on stdin.
//! A failing `pre-*` hook aborts the operation; `post-*` failures surface
//! as errors the caller may downgrade to warnings.

use std::io::Write;
use std::path::PathBuf;

use crate::workspace::Workspace;

/// The lifecycle points a workspace can hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hook {
    PreSync,
    PostSync,
    PreLock,
    PostLock,
    PostSwitch,
    PostChangeStart,
}

impl Hook {
    pub fn name(self) -> &'static str {
        match self {
            Hook::PreSync => "pre-sync",
            Hook::PostSync => "post-sync",
            Hook::PreLock => "pre-lock",
            Hook::PostLock => "post-lock",
            Hook::PostSwitch => "post-switch",
            Hook::PostChangeStart => "post-change-start",
        }
    }
}

/// Errors from a hook run.
#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("hook `{hook}` could not run")]
    Spawn {
        hook: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("hook `{hook}` failed with {status}")]
    Failed { hook: &'static str, status: String },
}

fn script_path(ws: &Workspace, hook: Hook) -> PathBuf {
    let dir = ws.state_dir().join("hooks");
    if cfg!(windows) {
        let bat = dir.join(format!("{}.bat", hook.name()));
        if bat.exists() {
            return bat;
        }
    }
    dir.join(hook.name())
}

/// Run the hook if a script exists; silently succeed otherwise.
pub fn fire(ws: &Workspace, hook: Hook, context: &serde_json::Value) -> Result<(), HookError> {
    let script = script_path(ws, hook);
    if !script.exists() {
        return Ok(());
    }
    let mut child = std::process::Command::new(&script)
        .current_dir(&ws.root)
        .env("HAW_ROOT", &ws.root)
        .env("HAW_HOOK", hook.name())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|source| HookError::Spawn {
            hook: hook.name(),
            source,
        })?;
    if let Some(stdin) = child.stdin.take() {
        let mut stdin = stdin;
        let _ = stdin.write_all(context.to_string().as_bytes());
    }
    let status = child.wait().map_err(|source| HookError::Spawn {
        hook: hook.name(),
        source,
    })?;
    if !status.success() {
        return Err(HookError::Failed {
            hook: hook.name(),
            status: status.to_string(),
        });
    }
    Ok(())
}
