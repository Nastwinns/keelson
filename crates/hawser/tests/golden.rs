//! Golden CLI-output tests + lockfile determinism (README §Testing).
//!
//! Each test builds a real multi-repo workspace, drives the actual `haw`
//! binary, normalizes volatile parts (temp paths, SHAs), and compares
//! against a golden string. Runs on the CI matrix, so passing here means
//! the output and the lockfile bytes are identical on Linux/macOS/Windows.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn haw(ws: &Path, args: &[&str]) -> (String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_haw"))
        .args(args)
        .current_dir(ws)
        .env("NO_COLOR", "1")
        .env_remove("CLICOLOR_FORCE")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("haw runs");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// Two upstream repos + a workspace whose manifest composes them into
/// one stack. Returns (lab root, workspace dir).
fn workspace(root: &Path) -> PathBuf {
    for name in ["kernel", "hal"] {
        let repo = root.join(name);
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "test@keelson.dev"]);
        git(&repo, &["config", "user.name", "Keelson Test"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("README.md"), format!("{name}\n")).unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "init"]);
    }
    let ws = root.join("gateway");
    std::fs::create_dir_all(&ws).unwrap();
    let manifest = format!(
        "[repo.kernel]\nurl = \"{root}/kernel\"\nrev = \"main\"\ngroups = [\"firmware\"]\n\n\
         [repo.hal]\nurl = \"{root}/hal\"\nrev = \"main\"\n\n\
         [stack.gateway]\nrepos = [\"kernel\", \"hal\"]\n",
        root = root.display().to_string().replace('\\', "/"),
    );
    std::fs::write(ws.join("haw.toml"), manifest).unwrap();
    ws
}

/// Normalize volatile output parts: the lab path and hex SHAs.
fn normalize(text: &str, root: &Path) -> String {
    let root_fwd = root.display().to_string().replace('\\', "/");
    let mut out = text.replace(&root_fwd, "<LAB>");
    out = out.replace(&root.display().to_string(), "<LAB>");
    let mut normalized = String::with_capacity(out.len());
    let mut run = String::new();
    for c in out.chars() {
        if c.is_ascii_hexdigit() {
            run.push(c);
            continue;
        }
        flush_sha(&mut normalized, &mut run);
        normalized.push(c);
    }
    flush_sha(&mut normalized, &mut run);
    normalized
}

fn flush_sha(dest: &mut String, run: &mut String) {
    if run.len() >= 8 && run.chars().any(|c| c.is_ascii_digit()) {
        dest.push_str("<SHA>");
    } else {
        dest.push_str(run);
    }
    run.clear();
}

#[test]
fn golden_tree_output() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = workspace(tmp.path());
    let (out, code) = haw(&ws, &["tree"]);
    assert_eq!(code, 0);
    assert_eq!(
        normalize(&out, tmp.path()),
        "haw.toml\n\
         └─ gateway\n\
         \x20  ├─ kernel  main  (<LAB>/kernel)\n\
         \x20  └─ hal     main  (<LAB>/hal)\n"
    );
}

#[test]
fn golden_status_output_and_verify_gate() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = workspace(tmp.path());
    let (_, code) = haw(&ws, &["sync"]);
    assert_eq!(code, 0);

    let (out, code) = haw(&ws, &["status"]);
    assert_eq!(code, 0);
    assert_eq!(
        normalize(&out, tmp.path()),
        "REPO    BRANCH                   HEAD       DIRTY  DRIFT\n\
         kernel  main                      <SHA>   -      -\n\
         hal     main                      <SHA>   -      -\n"
    );

    let (_, code) = haw(&ws, &["status", "--verify"]);
    assert_eq!(code, 0, "clean tree passes the gate");

    std::fs::write(ws.join("hal").join("scratch.txt"), "wip\n").unwrap();
    let (_, code) = haw(&ws, &["status", "--verify"]);
    assert_eq!(code, 3, "dirty repo must exit 3 (CI contract)");
}

#[test]
fn golden_sync_output() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = workspace(tmp.path());
    let (out, code) = haw(&ws, &["sync"]);
    assert_eq!(code, 0);
    assert_eq!(
        normalize(&out, tmp.path()),
        "wrote haw.lock (2 repos pinned)\n\
         \x20 ✓ kernel  cloned\n\
         \x20 ✓ hal     cloned\n\
         synced stack `gateway` (2/2 repos)\n"
    );
}

#[test]
fn status_json_schema_is_stable() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = workspace(tmp.path());
    haw(&ws, &["sync"]);
    let (out, code) = haw(&ws, &["status", "--format", "json"]);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(parsed["schema"], "haw.status/1");
    let repos = parsed["repos"].as_array().expect("repos array");
    assert_eq!(repos.len(), 2);
    for repo in repos {
        for key in [
            "name",
            "path",
            "missing",
            "branch",
            "head",
            "dirty",
            "locked_rev",
            "drift",
        ] {
            assert!(repo.get(key).is_some(), "missing key `{key}`");
        }
    }
}

#[test]
fn lockfile_is_deterministic_and_lf_only() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = workspace(tmp.path());

    let (_, code) = haw(&ws, &["lock"]);
    assert_eq!(code, 0);
    let first = std::fs::read(ws.join("haw.lock")).unwrap();

    let (_, code) = haw(&ws, &["lock"]);
    assert_eq!(code, 0);
    let second = std::fs::read(ws.join("haw.lock")).unwrap();

    assert_eq!(first, second, "same inputs must produce identical bytes");
    let text = String::from_utf8(first).expect("lockfile is UTF-8");
    assert!(
        !text.contains('\r'),
        "lockfile must be LF-only on every OS (COMPLIANCE §8)"
    );
    assert!(text.ends_with('\n'));
    assert!(text.contains("version = 1"));
}
