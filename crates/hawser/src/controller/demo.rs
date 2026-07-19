//! Demo controller: hardcoded fixtures powering `haw dash --demo`.

use std::path::PathBuf;

use haw_core::workspace::RepoStatus;

/// A cockpit controller backed entirely by canned, in-memory data. It reaches
/// no workspace, git, or network, so `haw dash --demo` renders every view —
/// fleet, PR/MRs, CI, changesets, merges — deterministically for recordings.
pub(crate) struct DemoController;

impl DemoController {
    #[allow(clippy::too_many_arguments)]
    fn repo(
        name: &str,
        groups: &[&str],
        branch: Option<&str>,
        head: Option<&str>,
        dirty: bool,
        drift: bool,
        locked_rev: Option<&str>,
        ahead_behind: Option<(u64, u64)>,
        missing: bool,
    ) -> RepoStatus {
        RepoStatus {
            name: name.to_string(),
            path: PathBuf::from("repos").join(name),
            missing,
            branch: branch.map(str::to_string),
            head: head.map(str::to_string),
            dirty,
            locked_rev: locked_rev.map(str::to_string),
            drift,
            ahead_behind,
            groups: groups.iter().map(|g| g.to_string()).collect(),
        }
    }

    fn gateway_fleet() -> Vec<RepoStatus> {
        vec![
            Self::repo(
                "kernel",
                &["firmware"],
                Some("release/6.1"),
                Some("a1c9f4e2b7d80516"),
                false,
                false,
                Some("a1c9f4e2b7d80516"),
                Some((0, 0)),
                false,
            ),
            Self::repo(
                "hal",
                &["firmware"],
                Some("feature/i2c-dma"),
                Some("7f3b21d0e5a4c9b6"),
                true,
                false,
                Some("7f3b21d0e5a4c9b6"),
                Some((3, 1)),
                false,
            ),
            Self::repo(
                "app-mqtt",
                &["ci", "apps"],
                Some("main"),
                Some("d4e88a1c60b3f279"),
                false,
                true,
                Some("22aa77bc11ee9930"),
                Some((0, 4)),
                false,
            ),
            Self::repo(
                "telemetry",
                &["apps"],
                None,
                None,
                false,
                false,
                Some("55cc33ee11aa8842"),
                None,
                true,
            ),
        ]
    }

    fn sensor_fleet() -> Vec<RepoStatus> {
        vec![
            Self::repo(
                "kernel",
                &["firmware"],
                Some("release/6.1"),
                Some("a1c9f4e2b7d80516"),
                false,
                false,
                Some("a1c9f4e2b7d80516"),
                Some((0, 0)),
                false,
            ),
            Self::repo(
                "sensor-drv",
                &["firmware", "ci"],
                Some("main"),
                Some("9b0a1c2d3e4f5061"),
                false,
                false,
                Some("9b0a1c2d3e4f5061"),
                Some((0, 0)),
                false,
            ),
        ]
    }
}

impl haw_tui::Controller for DemoController {
    fn snapshot(&mut self) -> std::io::Result<haw_tui::Snapshot> {
        let paths = [
            "kernel",
            "hal",
            "app-mqtt",
            "telemetry",
            "sensor-drv",
            "edge-daemon",
        ]
        .iter()
        .map(|name| {
            (
                name.to_string(),
                PathBuf::from("/home/you/work/gateway").join(name),
            )
        })
        .collect();

        let changesets = vec![
            haw_tui::ChangesetSummary {
                id: "FEAT-42".to_string(),
                repos: vec![
                    haw_tui::ChangeRepoRow {
                        name: "kernel".to_string(),
                        branch: "change/FEAT-42".to_string(),
                        on_branch: true,
                        dirty: false,
                        head: Some("a1c9f4e2b7d80516".to_string()),
                        forge: "github".to_string(),
                        pr: "#128 ● open".to_string(),
                        ci: "✓ passed".to_string(),
                    },
                    haw_tui::ChangeRepoRow {
                        name: "hal".to_string(),
                        branch: "change/FEAT-42".to_string(),
                        on_branch: true,
                        dirty: true,
                        head: Some("7f3b21d0e5a4c9b6".to_string()),
                        forge: "gitlab".to_string(),
                        pr: "!47 ◐ review".to_string(),
                        ci: "⏳ running".to_string(),
                    },
                    haw_tui::ChangeRepoRow {
                        name: "app-mqtt".to_string(),
                        branch: "change/FEAT-42".to_string(),
                        on_branch: false,
                        dirty: false,
                        head: Some("d4e88a1c60b3f279".to_string()),
                        forge: "github".to_string(),
                        pr: "—".to_string(),
                        ci: "—".to_string(),
                    },
                ],
            },
            haw_tui::ChangesetSummary {
                id: "BUG-1187".to_string(),
                repos: vec![
                    haw_tui::ChangeRepoRow {
                        name: "sensor-drv".to_string(),
                        branch: "change/BUG-1187".to_string(),
                        on_branch: true,
                        dirty: false,
                        head: Some("9b0a1c2d3e4f5061".to_string()),
                        forge: "github".to_string(),
                        pr: "#91 ● merged".to_string(),
                        ci: "✓ passed".to_string(),
                    },
                    haw_tui::ChangeRepoRow {
                        name: "telemetry".to_string(),
                        branch: "change/BUG-1187".to_string(),
                        on_branch: true,
                        dirty: false,
                        head: Some("c0ffee1234567890".to_string()),
                        forge: "gitlab".to_string(),
                        pr: "!12 ✗ closed".to_string(),
                        ci: "✗ failed".to_string(),
                    },
                ],
            },
        ];

        let tree = "\
└─ gateway
   ├─ kernel      release/6.1
   ├─ hal         feature/i2c-dma
   ├─ app-mqtt    main
   └─ telemetry   main
├─ sensor-node
   ├─ kernel      release/6.1
   └─ sensor-drv  main"
            .to_string();

        Ok(haw_tui::Snapshot {
            root_label: "~/work/gateway".to_string(),
            stacks: vec!["gateway".to_string(), "sensor-node".to_string()],
            current_stack: Some("gateway".to_string()),
            fleet: vec![
                ("gateway".to_string(), Self::gateway_fleet()),
                ("sensor-node".to_string(), Self::sensor_fleet()),
            ],
            changesets,
            lock_present: true,
            paths,
            tree,
            merges: vec![(
                "hal".to_string(),
                haw_tui::MergeBadge {
                    source: "origin/feature/i2c-dma".to_string(),
                    resolved: 2,
                    total: 3,
                },
            )],
        })
    }

    fn changeset_prs(&mut self, id: &str) -> std::io::Result<haw_tui::ChangesetSummary> {
        self.snapshot()?
            .changesets
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| std::io::Error::other(format!("no changeset `{id}`")))
    }

    fn sync_stack(&mut self, stack: &str) -> std::io::Result<String> {
        Ok(format!("synced stack `{stack}` (4 repos, 0 failed)"))
    }

    fn sync_repo(&mut self, repo: &str) -> std::io::Result<String> {
        Ok(format!("synced `{repo}` — up to date"))
    }

    fn sync_repos(&mut self, repos: &[String]) -> std::io::Result<String> {
        Ok(format!("synced {} repo(s) — up to date", repos.len()))
    }

    fn switch(&mut self, stack: &str) -> std::io::Result<String> {
        Ok(format!("switched to `{stack}` — synced (4 repos)"))
    }

    fn pin(&mut self) -> std::io::Result<String> {
        Ok("pinned haw.lock to current HEADs (6 repos)".to_string())
    }

    fn lock(&mut self) -> std::io::Result<String> {
        Ok("wrote haw.lock (6 repos pinned)".to_string())
    }

    fn run_cmd(&mut self, cmd: &str) -> std::io::Result<String> {
        Ok(format!(
            "$ {cmd}\n── kernel ──\nOK\n── hal ──\nOK\n── app-mqtt ──\nOK\nran in 3/3 repos"
        ))
    }

    fn run_cmd_in(&mut self, cmd: &str, repos: &[String]) -> std::io::Result<String> {
        let mut report = format!("$ {cmd}\n");
        for repo in repos {
            report.push_str(&format!("── {repo} ──\nOK\n"));
        }
        report.push_str(&format!("ran in {}/{} repos", repos.len(), repos.len()));
        Ok(report)
    }

    fn build(&mut self) -> std::io::Result<String> {
        Ok(
            "$ haw build\n── kernel ──\n(demo) OK\n── hal ──\n(demo) OK\nbuild ran in 2/2 repos"
                .to_string(),
        )
    }

    fn test(&mut self) -> std::io::Result<String> {
        Ok(
            "$ haw test\n── kernel ──\n(demo) OK\n── hal ──\n(demo) OK\ntest ran in 2/2 repos"
                .to_string(),
        )
    }

    fn verify(&mut self) -> std::io::Result<String> {
        Ok("$ haw verify\n✓ verified: tree matches haw.lock (4 repos)".to_string())
    }

    fn grep(
        &mut self,
        pattern: &str,
        _stack: Option<&str>,
    ) -> std::io::Result<Vec<haw_tui::GrepHit>> {
        let hit = |repo: &str, path: &str, line: u32, text: &str| haw_tui::GrepHit {
            repo: repo.to_string(),
            path: path.to_string(),
            line,
            text: text.to_string(),
        };
        Ok(vec![
            hit(
                "kernel",
                "drivers/i2c/dma.c",
                42,
                &format!("    /* {pattern}: DMA-backed transfer path */"),
            ),
            hit(
                "hal",
                "src/i2c.rs",
                17,
                &format!("fn {pattern}_xfer(bus: &mut Bus) {{"),
            ),
            hit(
                "app-mqtt",
                "src/main.rs",
                88,
                &format!("// TODO({pattern}): reconnect backoff"),
            ),
        ])
    }

    fn repo_fetch(&mut self, repo: &str) -> std::io::Result<String> {
        Ok(format!("fetched {repo} (demo)"))
    }

    fn exec_in(&mut self, repo: &str, cmd: &str) -> std::io::Result<String> {
        Ok(format!(
            "$ {cmd}\n@ /home/you/work/gateway/{repo}\n\n(demo) OK\n"
        ))
    }

    fn change_start(&mut self, id: &str) -> std::io::Result<String> {
        Ok(format!("changeset `{id}` started across 3 repos"))
    }

    fn change_request(&mut self, id: &str, only: Option<Vec<String>>) -> std::io::Result<String> {
        let count = only.map_or(3, |repos| repos.len());
        Ok(format!("requested `{id}` ({count} PR/MRs, cross-linked)"))
    }

    fn change_land(&mut self, id: &str) -> std::io::Result<String> {
        Ok(format!("landed `{id}` (3 repos)"))
    }

    fn pr_merge(&mut self, repo: &str, number: u64) -> std::io::Result<String> {
        Ok(format!("merged {repo}#{number}"))
    }

    fn pr_approve(&mut self, repo: &str, number: u64) -> std::io::Result<String> {
        Ok(format!("approved {repo}#{number}"))
    }

    fn pr_checkout(&mut self, repo: &str, number: u64) -> std::io::Result<String> {
        Ok(format!("checked out {repo} PR #{number} (demo)"))
    }

    fn merge_cleanup(&mut self, repo: &str) -> std::io::Result<String> {
        Ok(format!(
            "merged 3 slice(s) into `main` on `{repo}` (e91f0a4c); dropped haw/merge branch"
        ))
    }

    fn merge_abort(&mut self, repo: &str) -> std::io::Result<String> {
        Ok(format!("aborted merge of `{repo}`; back on `main`"))
    }

    fn fleet_prs(&mut self) -> std::io::Result<Vec<haw_tui::FleetPr>> {
        let pr = |repo: &str,
                  forge: &str,
                  number: u64,
                  title: &str,
                  state: &str,
                  approved: bool,
                  ci: Option<bool>| haw_tui::FleetPr {
            repo: repo.to_string(),
            forge: forge.to_string(),
            number,
            title: title.to_string(),
            url: format!("https://{forge}.com/acme/{repo}/pull/{number}"),
            state: state.to_string(),
            approved,
            ci,
        };
        Ok(vec![
            pr(
                "kernel",
                "github",
                128,
                "i2c: add DMA-backed transfers",
                "open",
                true,
                Some(true),
            ),
            pr(
                "hal",
                "gitlab",
                47,
                "hal: wire i2c DMA descriptors",
                "draft",
                false,
                Some(false),
            ),
            pr(
                "app-mqtt",
                "github",
                214,
                "mqtt: reconnect backoff + jitter",
                "open",
                false,
                None,
            ),
            pr(
                "sensor-drv",
                "github",
                91,
                "drv: calibrate on cold boot",
                "merged",
                true,
                Some(true),
            ),
            pr(
                "telemetry",
                "gitlab",
                12,
                "telemetry: batch OTLP exports",
                "open",
                true,
                Some(true),
            ),
            pr(
                "edge-daemon",
                "github",
                8,
                "edge: graceful shutdown on SIGTERM",
                "draft",
                false,
                None,
            ),
        ])
    }

    fn fleet_ci(&mut self) -> std::io::Result<Vec<haw_tui::FleetCiRun>> {
        let run = |repo: &str, id: u64, name: &str, branch: &str, event: &str, status: &str| {
            haw_tui::FleetCiRun {
                repo: repo.to_string(),
                id,
                name: name.to_string(),
                branch: branch.to_string(),
                event: event.to_string(),
                status: status.to_string(),
                url: format!("https://github.com/acme/{repo}/actions/runs/{id}"),
            }
        };
        Ok(vec![
            run(
                "kernel",
                9001,
                "build-and-test",
                "release/6.1",
                "push",
                "passed",
            ),
            run(
                "hal",
                9002,
                "firmware-ci",
                "feature/i2c-dma",
                "pull_request",
                "running",
            ),
            run("app-mqtt", 9003, "integration", "main", "push", "failed"),
            run("telemetry", 9004, "lint", "main", "pull_request", "queued"),
            run(
                "sensor-drv",
                9005,
                "nightly",
                "main",
                "schedule",
                "cancelled",
            ),
            run("edge-daemon", 9006, "build", "main", "push", "passed"),
        ])
    }

    fn governance(&mut self) -> std::io::Result<haw_tui::Governance> {
        let plugin = |name: &str, phases: &[&str]| haw_tui::GovPlugin {
            name: name.to_string(),
            phases: phases.iter().map(|p| p.to_string()).collect(),
        };
        let artifact = |plugin: &str, kind: &str, path: &str, exists: bool| haw_tui::GovArtifact {
            plugin: plugin.to_string(),
            kind: kind.to_string(),
            path: path.to_string(),
            exists,
        };
        let finding = |plugin: &str, level: &str, message: &str| haw_tui::GovFinding {
            plugin: plugin.to_string(),
            level: level.to_string(),
            message: message.to_string(),
        };
        Ok(haw_tui::Governance {
            plugins: vec![
                plugin("haw-compliance", &["post-build"]),
                plugin("haw-artifact", &["post-land"]),
                plugin("haw-git-gate", &["pre-request"]),
            ],
            artifacts: vec![
                artifact("haw-compliance", "sbom", ".haw/sbom/kernel.cdx.json", true),
                artifact("haw-compliance", "sbom", ".haw/sbom/kernel.spdx.json", true),
                artifact(
                    "haw-artifact",
                    "provenance",
                    ".haw/provenance/kernel.intoto.jsonl",
                    true,
                ),
                artifact(
                    "haw-artifact",
                    "signature",
                    ".haw/provenance/kernel.sig",
                    false,
                ),
            ],
            findings: vec![
                finding("haw-compliance", "info", "SBOM generated for 4 repos"),
                finding("haw-git-gate", "warn", "no signer on PATH"),
            ],
        })
    }

    fn plugin_panels(&mut self) -> std::io::Result<Vec<haw_tui::PluginPanel>> {
        let panel = |name: &str, phases: &[&str]| haw_tui::PluginPanel {
            name: name.to_string(),
            phases: phases.iter().map(|p| p.to_string()).collect(),
        };
        Ok(vec![
            panel("compliance", &["post-build"]),
            panel("artifact", &["post-land"]),
        ])
    }

    fn plugin_render(&mut self, name: &str) -> std::io::Result<String> {
        Ok(format!(
            "{name} panel\n\
\n\
status:  green\n\
repos:   4 scanned, 0 findings\n\
last run: post-build\n\
\n\
  ✓ kernel     SBOM emitted (.haw/sbom/kernel.cdx.json)\n\
  ✓ hal        SBOM emitted (.haw/sbom/hal.cdx.json)\n\
  ✓ app-mqtt   SBOM emitted (.haw/sbom/app-mqtt.cdx.json)\n\
  ✓ bootloader SBOM emitted (.haw/sbom/bootloader.cdx.json)\n"
        ))
    }

    fn repo_detail(&mut self, repo: &str) -> std::io::Result<String> {
        Ok(format!(
            "== {repo} ==\n\
branch release/6.1  @ a1c9f4e\n\
\n\
-- status --\n\
## release/6.1...origin/release/6.1\n\
\n\
-- recent commits --\n\
a1c9f4e (HEAD -> release/6.1, origin/release/6.1) i2c: add DMA-backed transfers\n\
7f3b21d hal: wire i2c DMA descriptors\n\
d4e88a1 mqtt: reconnect backoff + jitter\n\
9b0a1c2 drv: calibrate on cold boot\n\
c0ffee1 build: bump toolchain to 1.79\n\
\n\
-- last commit --\n\
a1c9f4e i2c: add DMA-backed transfers\n\
 drivers/i2c/dma.c | 142 ++++++++++++++++++++++++++++\n\
 drivers/i2c/i2c.h |  12 +++\n\
 2 files changed, 154 insertions(+)\n\
\n\
-- remotes --\n\
origin\tgit@github.com:acme/{repo}.git (fetch)\n\
origin\tgit@github.com:acme/{repo}.git (push)\n"
        ))
    }

    fn pr_detail(&mut self, repo: &str, number: u64) -> std::io::Result<String> {
        Ok(format!(
            "#{number} i2c: add DMA-backed transfers — open\n\
head feature/i2c-dma @ a1c9f4e  ->  base release/6.1\n\
mergeable: yes\n\
\n\
-- reviewers --\n\
  octavia: APPROVED\n\
  rui: CHANGES_REQUESTED\n\
\n\
-- checks --\n\
  build-and-test: completed/success\n\
  clippy: completed/success\n\
  integration: completed/failure\n\
\n\
-- body --\n\
Adds DMA-backed transfers to the {repo} i2c driver.\n\
\n\
- new descriptor ring in drivers/i2c/dma.c\n\
- falls back to PIO when no channel is free\n\
- part of changeset FEAT-42\n\
\n\
url: https://github.com/acme/{repo}/pull/{number}\n"
        ))
    }

    fn ci_detail(&mut self, repo: &str, run_id: u64) -> std::io::Result<String> {
        // A realistic in-flight pipeline: 6 of 9 jobs done (66%), still running,
        // with runner names on each job — mirrors the live forge report shape.
        let bar = haw_forge::progress_bar(6, 9);
        Ok(format!(
            "progress: {bar}  ·  🔄 running\n\
🔄 firmware-ci — in_progress/—\n\
🌿 branch feature/i2c-dma  event pull_request  @ 7fe1b02\n\
\n\
🧩 -- jobs --\n\
  ✅ build: completed/success  on ubuntu-22.04-16core\n\
    - checkout: success\n\
    - configure: success\n\
    - compile: success\n\
  ✅ unit-tests: completed/success  on ubuntu-22.04-16core\n\
    - checkout: success\n\
    - unit: success\n\
  ✅ clippy: completed/success  on ubuntu-22.04-4core\n\
    - clippy: success\n\
  ✅ fmt: completed/success  on ubuntu-22.04-4core\n\
    - fmt: success\n\
  ✅ docs: completed/success  on ubuntu-22.04-4core\n\
    - build-docs: success\n\
  ✅ package: completed/success  on ubuntu-22.04-4core\n\
    - bundle: success\n\
  🔄 integration: in_progress/—  on self-hosted-hw-rig-3\n\
    - checkout: success\n\
    - flash-board: in_progress\n\
  ⏳ hardware-smoke: queued/—  on self-hosted-hw-rig-3\n\
  ⏳ deploy: queued/—  on ubuntu-22.04-4core\n\
\n\
url: https://github.com/acme/{repo}/actions/runs/{run_id}\n"
        ))
    }

    fn pr_diff(&mut self, repo: &str, number: u64) -> std::io::Result<String> {
        Ok(format!(
            "diff --git a/drivers/i2c/dma.c b/drivers/i2c/dma.c\n\
new file mode 100644\n\
--- /dev/null\n\
+++ b/drivers/i2c/dma.c\n\
@@ -0,0 +1,8 @@\n\
+// DMA-backed transfers for the {repo} i2c driver (PR #{number})\n\
+#include \"i2c.h\"\n\
+\n\
+int i2c_dma_xfer(struct i2c_bus *bus, struct i2c_msg *msg) {{\n\
+    if (!bus->dma) return i2c_pio_xfer(bus, msg);\n\
+    return dma_submit(bus->dma, msg->buf, msg->len);\n\
+}}\n\
diff --git a/drivers/i2c/i2c.h b/drivers/i2c/i2c.h\n\
--- a/drivers/i2c/i2c.h\n\
+++ b/drivers/i2c/i2c.h\n\
@@ -12,6 +12,7 @@ struct i2c_bus {{\n\
     int speed_hz;\n\
+    struct dma_chan *dma;\n\
 }};\n\
+int i2c_dma_xfer(struct i2c_bus *bus, struct i2c_msg *msg);\n"
        ))
    }

    fn pr_files(
        &mut self,
        _repo: &str,
        _number: u64,
    ) -> std::io::Result<Vec<haw_tui::PrFileEntry>> {
        let file = |path: &str, status: &str| haw_tui::PrFileEntry {
            path: path.to_string(),
            status: status.to_string(),
        };
        Ok(vec![
            file("drivers/i2c/dma.c", "added"),
            file("drivers/i2c/i2c.h", "modified"),
            file("drivers/i2c/legacy_pio.c", "removed"),
        ])
    }

    fn pr_file_content(&mut self, repo: &str, number: u64, path: &str) -> std::io::Result<String> {
        if path.ends_with("legacy_pio.c") {
            return Ok("(file not present at this ref)\n".to_string());
        }
        Ok(format!(
            "// {repo}:/{path} — at PR #{number} head\n\
// canned demo content, read at the PR's version\n\
\n\
#include \"i2c.h\"\n\
\n\
int i2c_dma_xfer(struct i2c_bus *bus, struct i2c_msg *msg) {{\n\
    if (!bus->dma) return i2c_pio_xfer(bus, msg);\n\
    return dma_submit(bus->dma, msg->buf, msg->len);\n\
}}\n"
        ))
    }

    fn repo_tree(
        &mut self,
        _repo: &str,
        subpath: &str,
        _remote: bool,
        _git_ref: Option<&str>,
    ) -> std::io::Result<Vec<haw_tui::FileEntry>> {
        let dir = |name: &str| haw_tui::FileEntry {
            name: name.to_string(),
            is_dir: true,
        };
        let file = |name: &str| haw_tui::FileEntry {
            name: name.to_string(),
            is_dir: false,
        };
        Ok(match subpath {
            "" => vec![
                dir("drivers"),
                dir("include"),
                file("Cargo.toml"),
                file("README.md"),
            ],
            "drivers" => vec![dir("i2c"), file("Kconfig")],
            "drivers/i2c" => vec![file("dma.c"), file("i2c.h")],
            "include" => vec![file("kernel.h")],
            _ => Vec::new(),
        })
    }

    fn file_content(
        &mut self,
        repo: &str,
        path: &str,
        _remote: bool,
        git_ref: Option<&str>,
    ) -> std::io::Result<String> {
        let at = git_ref.unwrap_or("HEAD");
        Ok(format!(
            "// {repo}:/{path} @ {at}\n\
// canned demo content\n\
\n\
#include \"i2c.h\"\n\
\n\
int i2c_dma_xfer(struct i2c_bus *bus, struct i2c_msg *msg) {{\n\
    if (!bus->dma) return i2c_pio_xfer(bus, msg);\n\
    return dma_submit(bus->dma, msg->buf, msg->len);\n\
}}\n"
        ))
    }

    fn repo_file_paths(
        &mut self,
        _repo: &str,
        _remote: bool,
        _git_ref: Option<&str>,
    ) -> std::io::Result<Vec<String>> {
        Ok(vec![
            "Cargo.toml".to_string(),
            "README.md".to_string(),
            "drivers/Kconfig".to_string(),
            "drivers/i2c/dma.c".to_string(),
            "drivers/i2c/i2c.h".to_string(),
            "include/kernel.h".to_string(),
        ])
    }

    fn list_refs(&mut self, _repo: &str, _remote: bool) -> std::io::Result<Vec<haw_tui::RefEntry>> {
        Ok(vec![
            haw_tui::RefEntry {
                name: "main".to_string(),
                kind: haw_tui::RefKind::Head,
            },
            haw_tui::RefEntry {
                name: "dev".to_string(),
                kind: haw_tui::RefKind::Branch,
            },
            haw_tui::RefEntry {
                name: "v1.0.0".to_string(),
                kind: haw_tui::RefKind::Tag,
            },
        ])
    }

    fn ci_logs(&mut self, repo: &str, run_id: u64) -> std::io::Result<String> {
        Ok(format!(
            "== build (success) ==\n\
[00:00:01] Checking out {repo}@a1c9f4e\n\
[00:00:04] cargo build --release\n\
[00:01:12]    Compiling {repo} v0.1.0\n\
[00:02:03]     Finished release [optimized] target(s) in 1m 51s\n\
\n\
== test (success) ==\n\
[00:00:02] cargo test --workspace\n\
[00:00:48] test result: ok. 72 passed; 0 failed\n\
\n\
== integration (failure) ==\n\
[00:00:03] running integration suite\n\
[00:00:19] FAILED: i2c_dma_roundtrip — expected 8 bytes, got 0\n\
[00:00:19] error: 1 test failed (run #{run_id})\n"
        ))
    }
}
