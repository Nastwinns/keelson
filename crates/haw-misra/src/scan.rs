//! The MISRA pass this plugin runs against each repository.
//!
//! Coverage comes from `cppcheck --addon=misra`, the common open-source MISRA C
//! checker. The plugin is deliberately honest and fail-open: if `cppcheck` is
//! not on PATH, or a repo has no C/C++ sources, it reports the gate as *skipped*
//! (a `warn`) rather than manufacturing either a pass it cannot vouch for or a
//! block on a missing tool.

use std::path::Path;
use std::process::Command;

use crate::report::Finding;

/// The file extensions treated as C/C++ translation units or headers.
const C_EXTENSIONS: &[&str] = &["c", "h", "cpp", "cc", "cxx", "hpp", "hh", "hxx"];

/// Whether `cppcheck` is available on `PATH`.
pub fn cppcheck_available() -> bool {
    tool_on_path("cppcheck")
}

/// Whether `git` is available on `PATH`.
fn git_available() -> bool {
    tool_on_path("git")
}

/// Probe `PATH` for `tool` by running `tool --version`.
fn tool_on_path(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The outcome of scanning one repository.
pub struct RepoScan {
    /// Number of tracked C/C++ files considered.
    pub files_scanned: usize,
    /// The findings produced (one `error` per MISRA violation, or a `warn`
    /// when the gate was skipped for this repo).
    pub findings: Vec<Finding>,
}

/// True when `name` has a recognised C/C++ extension (case-insensitive).
fn is_c_file(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((_, ext)) => {
            let ext = ext.to_ascii_lowercase();
            C_EXTENSIONS.contains(&ext.as_str())
        }
        None => false,
    }
}

/// Turn a file path into a cppcheck *operand* that can never be parsed as an
/// option, even without a preceding `--`.
///
/// Only a path that could be parsed as a flag (leading `-`) is neutralized by a
/// leading `./` (so `--exitcode=0` becomes `./--exitcode=0`); absolute and
/// ordinary relative paths pass through unchanged. Platform-agnostic — it does
/// NOT rely on `is_absolute()` (a Unix-style `/abs` is not absolute on Windows)
/// — and belt-and-suspenders behind the `--` options terminator.
fn as_operand(path: &str) -> String {
    if path.starts_with('-') {
        format!("./{path}")
    } else {
        path.to_string()
    }
}

/// List tracked C/C++ files in `repo`, preferring `git ls-files` and falling
/// back to a shallow filesystem walk when git is unavailable.
pub fn collect_c_files(repo: &Path) -> Vec<String> {
    if git_available()
        && let Some(files) = collect_via_git(repo)
    {
        return files;
    }
    collect_via_walk(repo)
}

/// List tracked and staged C/C++ files via `git ls-files`.
fn collect_via_git(repo: &Path) -> Option<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("ls-files")
        .arg("-z")
        .arg("--cached")
        .arg("--others")
        .arg("--exclude-standard")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut files = Vec::new();
    for rel_bytes in output.stdout.split(|b| *b == 0) {
        if rel_bytes.is_empty() {
            continue;
        }
        let rel = String::from_utf8_lossy(rel_bytes).into_owned();
        if is_c_file(&rel) {
            files.push(rel);
        }
    }
    Some(files)
}

/// Shallow recursive walk collecting C/C++ files, skipping the `.git` dir.
fn collect_via_walk(repo: &Path) -> Vec<String> {
    let mut files = Vec::new();
    walk(repo, repo, &mut files);
    files
}

/// Recursive helper for [`collect_via_walk`].
fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => walk(root, &path, out),
            Ok(ft) if ft.is_file() => {
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();
                if is_c_file(&rel) {
                    out.push(rel);
                }
            }
            _ => {}
        }
    }
}

/// Run the MISRA pass over `repo`.
///
/// Assumes the caller has already verified `cppcheck` is on PATH. Returns a
/// `warn`-level skip when the repo has no C/C++ files; otherwise one `error`
/// per reported MISRA violation (and an `info` when clean).
pub fn run_misra(name: &str, repo: &Path) -> RepoScan {
    let files = collect_c_files(repo);
    if files.is_empty() {
        return RepoScan {
            files_scanned: 0,
            findings: vec![Finding::warn(format!(
                "no C/C++ files in repo '{name}'; MISRA gate skipped"
            ))],
        };
    }

    let files_scanned = files.len();
    // SECURITY: file names come from `git ls-files` over an untrusted, cloned
    // repo. A file literally named `--exitcode=0` or `--output-file=...` would
    // otherwise be parsed by cppcheck as an OPTION, bypassing the gate or
    // writing arbitrary files. Terminate options with `--` and force every path
    // into a leading-`./` operand form so nothing can be read as a flag.
    let operands: Vec<String> = files.iter().map(|f| as_operand(f)).collect();
    let output = Command::new("cppcheck")
        .arg("--addon=misra")
        .arg("--enable=style")
        .arg("--quiet")
        // Emit one machine-readable line per violation on stderr.
        .arg("--template={file}:{line}: {id}: {message}")
        .arg("--")
        .args(&operands)
        .current_dir(repo)
        .output();

    match output {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let violations = parse_violations(&stderr);
            if violations.is_empty() {
                RepoScan {
                    files_scanned,
                    findings: vec![Finding::info(format!(
                        "cppcheck/misra: no violations in repo '{name}' ({files_scanned} file(s))"
                    ))],
                }
            } else {
                let findings = violations
                    .into_iter()
                    .map(|v| Finding::error(format!("MISRA violation in '{name}': {v}")))
                    .collect();
                RepoScan {
                    files_scanned,
                    findings,
                }
            }
        }
        Err(e) => RepoScan {
            files_scanned,
            findings: vec![Finding::warn(format!(
                "cppcheck failed to run on repo '{name}': {e}; MISRA gate skipped"
            ))],
        },
    }
}

/// Parse cppcheck's stderr into one violation string per reported line.
///
/// cppcheck (with `--template`) prints one diagnostic per line to stderr;
/// blank lines and progress noise are ignored.
pub fn parse_violations(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| line.contains(':'))
        .map(|line| line.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crafted_flag_filename_becomes_operand() {
        // A file named like a cppcheck option must be neutralised.
        assert_eq!(as_operand("--exitcode=0"), "./--exitcode=0");
        assert_eq!(
            as_operand("--output-file=/etc/passwd"),
            "./--output-file=/etc/passwd"
        );
        assert_eq!(as_operand("-DFOO"), "./-DFOO");
    }

    #[test]
    fn operand_is_never_flag_like() {
        for p in [
            "--exitcode=0",
            "-x",
            "src/main.c",
            "./already/rel.c",
            "weird--name.c",
        ] {
            let op = as_operand(p);
            assert!(
                !op.starts_with('-'),
                "operand {op:?} for {p:?} must not start with '-'"
            );
        }
    }

    #[test]
    fn ordinary_paths_pass_through_unchanged() {
        // Only leading-dash names need neutralizing; normal paths are operands
        // already (and sit behind the `--` terminator). Unchanged on all OSes.
        assert_eq!(as_operand("src/a.c"), "src/a.c");
        assert_eq!(as_operand("./src/a.c"), "./src/a.c");
        assert_eq!(as_operand("/abs/path/a.c"), "/abs/path/a.c");
    }

    #[test]
    fn dash_leading_names_are_neutralized() {
        assert_eq!(as_operand("-x.c"), "./-x.c");
        assert_eq!(as_operand("--exitcode=0"), "./--exitcode=0");
    }
}
