//! Release/packaging automation: `cargo xtask dist` builds a release binary,
//! archives it under `dist/`, and prints its SHA-256 for the Homebrew formula
//! and Scoop manifest in `packaging/`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

fn main() -> Result<()> {
    let task = std::env::args().nth(1).unwrap_or_default();
    match task.as_str() {
        "dist" => dist(),
        _ => {
            eprintln!("tasks:\n  dist  build a release archive under dist/");
            Ok(())
        }
    }
}

fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .context("xtask lives one level under the workspace root")
}

fn run(cmd: &mut Command, what: &str) -> Result<String> {
    let output = cmd.output().with_context(|| format!("running {what}"))?;
    if !output.status.success() {
        bail!(
            "{what} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn host_triple() -> Result<String> {
    let verbose = run(Command::new("rustc").arg("-vV"), "rustc -vV")?;
    verbose
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_string)
        .context("no host triple in rustc -vV")
}

fn sha256(path: &Path) -> Option<String> {
    let unix = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let out = unix.or_else(|| {
        Command::new("certutil")
            .arg("-hashfile")
            .arg(path)
            .arg("SHA256")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    })?;
    out.split_whitespace()
        .find(|token| token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_string)
}

fn dist() -> Result<()> {
    let root = workspace_root()?;
    let version = env!("CARGO_PKG_VERSION");
    let triple = host_triple()?;

    println!("building haw {version} ({triple})…");
    run(
        Command::new("cargo")
            .args(["build", "--release", "-p", "hawser"])
            .current_dir(&root),
        "cargo build --release",
    )?;

    let binary = if cfg!(windows) { "haw.exe" } else { "haw" };
    let built = root.join("target").join("release").join(binary);
    if !built.exists() {
        bail!("release binary missing at {}", built.display());
    }

    let dist = root.join("dist");
    std::fs::create_dir_all(&dist)?;
    let archive = dist.join(if cfg!(windows) {
        format!("haw-{version}-{triple}.zip")
    } else {
        format!("haw-{version}-{triple}.tar.gz")
    });
    let _ = std::fs::remove_file(&archive);

    let mut tar = Command::new("tar");
    if cfg!(windows) {
        tar.arg("-a").arg("-c").arg("-f");
    } else {
        tar.arg("-czf");
    }
    run(
        tar.arg(&archive)
            .arg(binary)
            .current_dir(root.join("target").join("release")),
        "tar",
    )?;

    println!("wrote {}", archive.display());
    match sha256(&archive) {
        Some(digest) => {
            println!("sha256  {digest}");
            println!("update packaging/homebrew/keelson.rb and packaging/scoop/keelson.json");
        }
        None => println!("(no shasum/certutil found — compute the sha256 yourself)"),
    }
    Ok(())
}
