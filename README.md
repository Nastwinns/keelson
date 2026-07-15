<!-- markdownlint-disable MD033 MD041 -->
<div align="center">

```
██╗  ██╗███████╗███████╗██╗     ███████╗ ██████╗ ███╗   ██╗
██║ ██╔╝██╔════╝██╔════╝██║     ██╔════╝██╔═══██╗████╗  ██║
█████╔╝ █████╗  █████╗  ██║     ███████╗██║   ██║██╔██╗ ██║
██╔═██╗ ██╔══╝  ██╔══╝  ██║     ╚════██║██║   ██║██║╚██╗██║
██║  ██╗███████╗███████╗███████╗███████║╚██████╔╝██║ ╚████║
╚═╝  ╚═╝╚══════╝╚══════╝╚══════╝╚══════╝ ╚═════╝ ╚═╝  ╚═══╝
      ⚓  the beam that binds the repos  ⚓
```

**Reproducible multi-repo stack composition + cross-repo MR orchestration. In Rust.**

[![build](https://img.shields.io/badge/CI-Linux%20%7C%20macOS%20%7C%20Windows-brightgreen?logo=github)](.github/workflows/ci.yml)
[![crates.io](https://img.shields.io/badge/crates.io-keel--cli-orange?logo=rust)](https://crates.io)
[![rust](https://img.shields.io/badge/rust-1.90%2B-orange?logo=rust)](https://www.rust-lang.org)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![unsafe](https://img.shields.io/badge/unsafe-forbidden-success.svg)](#)
[![platform](https://img.shields.io/badge/platform-linux%20%7C%20macos%20%7C%20windows-blueviolet)](#)

</div>

---

`keel` is a command-line tool (with a TUI) for assembling a software stack out of
many independent Git repositories — without submodules, without detached HEADs, and
without a Python runtime. A single declarative manifest describes your **stacks** and
the **repos** (repositories) they are composed of; a committed **lockfile** pins every
repo to an exact revision so any teammate or CI machine reproduces the exact same tree.

On top of composition, Keelson orchestrates the day-to-day multi-repo workflow: create a
feature branch across all affected repos at once, open the matching Pull/Merge Requests
on GitHub and GitLab, and track their review + CI state from one screen.

Keelson runs natively on **Linux, macOS and Windows**. It uses [`gitoxide`](https://github.com/GitoxideLabs/gitoxide)
for fast native introspection and shells out to `git` only for the heavy plumbing.

---

## Quick start

```bash
# install (any of)
cargo install keel-cli          # from crates.io
brew install keelson            # macOS / Linuxbrew
scoop install keelson           # Windows

# bootstrap a workspace from a manifest, then materialize a stack
keel init keel.toml
keel sync                       # clones every repo, writes keel.lock
```

A typical session — compose, inspect, branch across repos:

```console
$ keel tree
keel.toml
├─ gateway
│  ├─ kernel    v6.1.2       (git@gitlab.company.com:firmware/kernel.git)
│  ├─ hal       main         (git@gitlab.company.com:firmware/hal.git)
│  └─ app-mqtt  release/2.x  (git@github.com:acme/app-mqtt.git)
└─ sensor-node
   ├─ kernel  v6.1.2         (git@gitlab.company.com:firmware/kernel.git)
   └─ hal     main           (git@gitlab.company.com:firmware/hal.git)

$ keel status
REPO      BRANCH   HEAD      DIRTY  DRIFT
kernel    v6.1.2   a1b2c3d4  -      -
hal       main     9f8e7d6c  yes    -
app-mqtt  release  4d5e6f7a  -      YES

$ keel change start FEAT-42 --repos kernel,app-mqtt
changeset `FEAT-42` started across 2 repo(s):
  kernel    -> change/FEAT-42
  app-mqtt  -> change/FEAT-42
```

> Output is colorized on a terminal, plain when piped. Honors `NO_COLOR`.

## How it composes

One manifest declares **repos** (the Git repositories) and composes them into **stacks**
(named sets of repos). A repo is shared, never duplicated. A committed lockfile pins every
repo to an exact SHA.

```
              keel.toml  (intent)                 keel.lock  (pinned SHAs, committed)
                   │                                        │
      ┌────────────┼────────────┐                          ▼
      ▼            ▼            ▼                   reproducible on any machine / CI
 ┌─────────┐  ┌─────────┐  ┌──────────┐
 │ kernel  │  │  hal    │  │ app-mqtt │   ← repos (full autonomous git clones)
 └────┬────┘  └────┬────┘  └────┬─────┘
      │            │            │
      ├──────┬─────┤            │          stacks reuse the SAME repos,
      ▼      │     ▼            ▼          no submodules, no detached HEAD, no symlinks
 ┌──────────┴──┐  ┌────────────┴─────┐
 │  gateway    │  │   sensor-node    │   ← stacks (compositions)
 │ kernel+hal  │  │   kernel + hal   │
 │  +app-mqtt  │  │                  │
 └─────────────┘  └──────────────────┘
```

---

## Why Keelson exists

Splitting a stack into many repositories is common in embedded/automotive/avionics
(shared BSW, HAL, MCAL repos reused across ECUs) and in microservice backends. The
existing tooling each solves one slice of the problem:

- **Google `repo` / `west`** give you a manifest, but no lockfile, a Python runtime,
  detached HEADs, and (for `repo`) symlink-based layouts that fight Windows.
- **RepoFleet** (Go) nails the *issue → branches across repos → PR/MR* workflow, but has
  no notion of stack composition or a reproducible pinned manifest.
- **mergetopus** (Rust) brilliantly parallelizes one big risky merge, but is single-repo.

Keelson is the union that nobody ships: **reproducible stack composition** (the `repo`
job, done properly with a lockfile) **+ cross-repo MR orchestration** (the RepoFleet job,
in Rust) **+ optional parallel collaborative merge** (the mergetopus idea), behind one
binary and one TUI.

### What Keelson is not

Keelson orchestrates Git and the GitHub/GitLab APIs. It does **not** reimplement Git's
merge engine, replace a forge, or replace domain toolchains (AUTOSAR generators, DO-178C
traceability tools). It is the orchestration layer those toolchains sit on top of.

---

## Core concepts

**Repo** — one Git repository, cloned as a full autonomous repo (its own `.git`, its own
branches, no detached HEAD). A repo can be shared by several stacks.

**Stack** — a named composition: a set of repos at chosen revisions. Checking out a
stack materializes the union of its repos at the paths the manifest declares.

**Manifest** (`keel.toml`) — human-authored intent: remotes, repos, stacks, overlays.
TOML, for the same reasons Cargo uses it: no indentation traps, no YAML type coercion
("Norway problem"), stable serde ecosystem, clean diffs in review.

**Lockfile** (`keel.lock`) — machine-generated, committed: every repo resolved to an
exact SHA. This is the reproducibility + audit guarantee (a real argument in
automotive/avionics) that `repo` and `west` lack.

**Overlay** — a named set of per-repo overrides (rev, path) applied on top of the
manifest, so variants (dev, bleeding-edge, customer builds) never duplicate repo lists.

**Changeset** — a feature spanning several repos: one logical branch created across N
repos, with N linked PR/MRs and an aggregated status.

---

## Layout on disk (no symlinks, ever)

```
mystack/
├── keel.toml           # manifest (intent)
├── keel.lock           # lockfile (resolved SHAs, committed)
├── kernel/             # real, complete git repo
├── hal/                # real, complete git repo
└── app-mqtt/           # real, complete git repo
```

Repos are plain clones at their final path — exactly what west does, and the reason it
works on Windows where `repo` struggled. Object sharing across stacks on one machine is
available as an **opt-in optimization** via git's native `alternates`
(`git clone --reference`), which writes a text file, not a symlink. Keelson uses three
text-based indirections git already provides (`alternates`, the `.git: gitdir:` file, and
its own lockfile) to replace everything `repo` did with symlinks.

---

## Manifest example

```toml
[remote.internal]
url = "git@gitlab.company.com:firmware"

[remote.github]
url = "git@github.com:acme"

# --- repos ---------------------------------------------------------------

[repo.kernel]
remote = "internal"
repo   = "kernel.git"
rev    = "v6.1.2"        # tag or sha => pinned & reproducible
groups = ["firmware"]

[repo.hal]
remote = "internal"
repo   = "hal.git"
rev    = "main"          # branch => follows head, until locked
groups = ["firmware"]

[repo.app-mqtt]
remote = "github"
repo   = "app-mqtt.git"
rev    = "release/2.x"
path   = "apps/mqtt"     # optional; defaults to the repo name

# --- stacks -------------------------------------------------------------

[stack.gateway]
repos = ["kernel", "hal", "app-mqtt"]

[stack.sensor-node]
repos = ["kernel", "hal"]        # shares kernel + hal, no duplication

# --- overlays -------------------------------------------------------------

[overlay.dev.repo.kernel]
rev = "main"                      # `keel sync --overlay dev`: kernel follows main
```

---

## Command surface

```
keel                             Open the TUI cockpit (no subcommand)
├── init <manifest-url|path>     Bootstrap a workspace from a manifest
├── sync [--stack S]             Clone/pull repos to the state in keel.lock
│                                (resolves + writes lock if absent)  [--shared]
├── lock                         Resolve every repo's rev to a SHA -> keel.lock
├── pin / unpin                  Pin lock to current checkouts / restore to manifest revs
├── switch <stack>               Materialize a different stack in the workspace
├── status                       Aggregated fleet status (dirty/ahead/behind per repo)
├── run '<cmd>'                  Run a command across repos, in parallel
├── tree                         Print the stack -> repo tree
│
├── repo   add|remove|list       Edit repos in the manifest
├── stack  add|remove|list       Edit stacks in the manifest
│
├── verify                       Assert tree == keel.lock; exit 3 on drift (CI gate)
├── build / test                 Run each repo's declared build/test command, in parallel
├── hooks  install|list          Git integrity pre-commit + lifecycle hooks (.keel/hooks)
├── evidence                     Bundle manifest+lock+audit+status for audits
│
├── change                       Cross-repo feature ("changeset") workflow
│   ├── start <id> [--repos ..]  Create one branch across the affected repos
│   │                            [--skip-branch] adopt each repo's current branch instead
│   ├── status                   Per-repo branch + PR/MR review + CI dashboard
│   ├── request                  Open linked PR/MR on GitHub/GitLab for each repo
│   ├── goto                     Interactive picker; cd into a repo
│   ├── snapshot save|restore    Save/restore the multi-repo state of a changeset
│   └── land                     Merge PR/MRs in dependency order
│
├── merge                        (optional) parallel collaborative merge (mergetopus-style)
│   ├── plan <source>            Split a big merge into integration + slice branches
│   ├── resolve <slice>          Resolve one slice with your merge tool
│   └── cleanup                  Promote and remove temporary branches
│
├── import --from <west.yml|default.xml>   Convert a west/repo manifest to keel.toml
└── dash                         Open the fleet dashboard (same as bare `keel`)
```

Verbs are one guessable word each; old names (`graph`, `forall`, `freeze`, `tui`) stay as
hidden aliases. Full lexicon: [docs/CLI-DESIGN.md](docs/CLI-DESIGN.md).

Key differentiators vs the field: `lock`/`pin` (reproducibility), `switch <stack>`
(composition), parallel `run` and `sync`, `change request` on **both** GitHub and
GitLab from Rust, and a real fleet **TUI**.

---

## The TUI — a k9s-grade cockpit

The dashboard is a **first-class product, not an afterthought.** Target: the polish and flow
of [`k9s`](https://k9scli.io) — keyboard-first, fast, discoverable, beautiful in a terminal.
Multi-repo state is intrinsically 2-D (N repos × their state) and works over SSH, so a
`ratatui` cockpit is the right shape for embedded/CI users.

Design bar (non-negotiable):
- **Keyboard-first, modal, k9s-style.** `:` command bar, `/` filter, single-key actions,
  a live-updating grid. Mouse optional, never required.
- **Instant feedback.** Async refresh, spinners on long ops, no frozen frames.
- **Legible at a glance.** Color-coded status (clean / dirty / drift / missing), consistent
  glyphs, a help bar that always shows the next keystrokes.
- **Themeable + `NO_COLOR`-aware.** Sane in light and dark terminals.

Views:
- left: stack → repo tree; right: per-repo detail (branch, SHA, dirty, ahead/behind, drift).
- changeset view: the N branches of a feature, each with PR/MR review + CI status.
- actions: sync, switch, `pin`, start/land a changeset — all keyboard-driven.

### Cockpit layout — fleet view

```text
 keel ▸ ~/work/gateway ───────────────────────── stack: gateway   lock: ✓   repos: 3/3
────────────────────────────────────────────────────────────────────────────────────────
 REPO        BRANCH        HEAD       DIRTY   DRIFT   AHEAD/BEHIND
▸kernel      v6.1.2        a1b2c3d4     ·       ·        0 / 0
 hal         main          9f8e7d6c    yes      ·        2 / 0
 app-mqtt    release/2.x   4d5e6f7a     ·      DRIFT     0 / 5
────────────────────────────────────────────────────────────────────────────────────────
 hal  ›  path hal/   branch main (ahead 2)   dirty 3 files   locked 9f8e7d6c   grp firmware
────────────────────────────────────────────────────────────────────────────────────────
 [s]ync [S]witch [p]in [l]ock [t]ree [c]hange [r]un  [/]filter [:]cmd [?]help [q]uit    :█
```

Green = clean · yellow = dirty · red = drift · dim = not cloned. `▸` marks the cursor row;
the bottom strip details it live.

### Cockpit layout — changeset view

```text
 keel ▸ change FEAT-42 ───────────────────────────────── 2 repos   branch: change/FEAT-42
────────────────────────────────────────────────────────────────────────────────────────
 REPO        BRANCH          ON IT  DIRTY   HEAD       PR / MR        CI
▸kernel      change/FEAT-42   yes     ·     a1b2c3d4   #128 ● open    ✓ passed
 app-mqtt    change/FEAT-42   yes    yes    4d5e6f7a   !47  ◐ review   ⏳ running
────────────────────────────────────────────────────────────────────────────────────────
 [n]ew [␣]select [R]equest-PR [L]and [g]oto [b]ack  [/]filter [:]cmd [?]help [q]uit     :█
```

Keyboard-first, k9s-style: `:` opens a command bar mirroring the CLI verbs (`:sync`,
`:stack sensor-node`, `:run git status`), `/` filters the grid, single keys act on the cursor
row. Full keymap: [docs/CLI-DESIGN.md](docs/CLI-DESIGN.md#tui-keymap).

Open it with a bare `keel` (or `keel dash`). A richer GUI is possible later via **Tauri**,
reusing the exact same Rust core. The TUI ships first: one binary, low cost, on-target.

---

## Cookbook — commands & output

Illustrative output for the shipped commands (Phase 1). Colorized on a TTY, plain when piped.

```console
$ keel init keel.toml
initialized workspace from keel.toml
next: keel sync

$ keel sync
wrote keel.lock (3 repos pinned)
  ✓ kernel    cloned
  ✓ hal       cloned
  ✓ app-mqtt  cloned
synced stack `gateway` (3/3 repos)

$ keel tree
keel.toml
└─ gateway
   ├─ kernel    v6.1.2       (git@gitlab.company.com:firmware/kernel.git)
   ├─ hal       main         (git@gitlab.company.com:firmware/hal.git)
   └─ app-mqtt  release/2.x  (git@github.com:acme/app-mqtt.git)

$ keel status
REPO      BRANCH   HEAD      DIRTY  DRIFT
kernel    v6.1.2   a1b2c3d4  -      -
hal       main     9f8e7d6c  yes    -
app-mqtt  release  4d5e6f7a  -      YES

$ keel run 'git fetch --tags'
── kernel ──
── hal ──
── app-mqtt ──
ran in 3/3 repos

$ keel lock
wrote keel.lock (3 repos pinned)
  kernel    a1b2c3d4e5f6  <- v6.1.2
  hal       9f8e7d6c5b4a  <- main
  app-mqtt  4d5e6f7a8b9c  <- release/2.x

$ keel pin                       # snapshot current checkouts (no network)
pinned keel.lock to current HEADs (3 repos)

$ keel change start FEAT-42 --repos kernel,app-mqtt
changeset `FEAT-42` started across 2 repo(s):
  kernel    -> change/FEAT-42
  app-mqtt  -> change/FEAT-42

$ keel change status FEAT-42
changeset `FEAT-42`
REPO      BRANCH           ON IT  DIRTY  HEAD      PR
kernel    change/FEAT-42   yes    -      a1b2c3d4  —
app-mqtt  change/FEAT-42   yes    yes    4d5e6f7a  —
(no PR/MRs yet — open them with `keel change request FEAT-42`)
```

## Testing

```bash
cargo test --workspace        # unit + integration; 43 tests, all green
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Covered today: manifest parse + referential validation, TOML round-trip, resolver + overlay
precedence, lockfile read/write, changeset start/status. Planned (see roadmap):

- **Golden CLI-output tests** — snapshot `tree`/`status`/`lock` output so lexicon and format
  changes are caught in review.
- **Determinism tests** — assert `keel.lock` is byte-identical across Linux/macOS/Windows for
  the same inputs (a hard requirement for certification evidence, [COMPLIANCE §8](docs/COMPLIANCE.md)).

## Status

Phases 0-5 of [the plan](docs/ARCHITECTURE.md#6-implementation-plan-phased) are
implemented: composition (`sync`/`lock`/`pin`/`switch`, overlays, `--shared`), the full
changeset lifecycle (`start`/`request`/`land`/`goto`, snapshots) on GitHub **and** GitLab,
the interactive TUI cockpit, `import` from west/repo manifests, CI gates (`verify`,
`--locked`, `--format json`), lifecycle hooks, plugins, and packaging. Phase 6
(collaborative merge) stays demand-driven.

## Documentation

| Doc | What |
|-----|------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crate layout, data flows, phased implementation plan |
| [docs/EXTENDING.md](docs/EXTENDING.md) | Extensions, plugins, hooks, auth, CI/CD integration |
| [docs/COMPLIANCE.md](docs/COMPLIANCE.md) | Tool qualification, SBOM/CRA, crypto/signing, GDPR, secure SDLC |
| [docs/COMMERCIALIZATION.md](docs/COMMERCIALIZATION.md) | Editions, licensing, LTS, qualification kit, pricing, GTM |
| [docs/LAUNCH.md](docs/LAUNCH.md) | Reddit/HN launch playbook: timing gate, media assets, copy-ready post drafts |
| [AGENTS.md](AGENTS.md) | Token-saving output rules for AI coding agents in this repo |

## License

Dual-licensed under MIT or Apache-2.0, at your option.
