<!-- markdownlint-disable MD033 MD041 -->
<div align="center">

<img src="docs/assets/hawser-comic.jpeg" alt="hawser — the beam that binds the repos" width="720">

# hawser

**Your product lives in 10 repos and nobody knows which commits go together.
`haw` pins them all to one lockfile — so you, your CI, and your teammate check
out the _identical_ tree, every time. One binary. In Rust.**

```sh
cargo install hawser        # or: brew, scoop, prebuilt binaries — see Install
```

<img src="demo/hawser-persona.gif" alt="haw persona journey: declare, pin to real SHAs, build & test 5 real embedded upstreams, install plugins, and drive the cockpit" width="820">

<sub>Five real embedded upstreams — CoreMark · cJSON · Monocypher · libcanard · Mbed-TLS —
declared, pinned to real SHAs, then <b>real terminal captures</b> of the parallel build,
the test recipes, and the live cockpit; plus plugins.
<a href="https://nastwinns.github.io/hawser/">Full demo on the site →</a></sub>

### 🌐 [**hawser.dev — website & interactive course →**](https://nastwinns.github.io/hawser/)

**New here? Start there.** A friendly, illustrated, step-by-step course takes you from zero
to productive, and you can even try the cockpit in your browser — no install required.

[![website](https://img.shields.io/badge/website%20%26%20course-nastwinns.github.io%2Fhawser-8A2BE2?logo=readthedocs&logoColor=white)](https://nastwinns.github.io/hawser/)
[![crates.io](https://img.shields.io/crates/v/hawser)](https://crates.io/crates/hawser)
[![CI](https://github.com/Nastwinns/hawser/actions/workflows/ci.yml/badge.svg)](https://github.com/Nastwinns/hawser/actions/workflows/ci.yml)
[![rust](https://img.shields.io/badge/rust-1.90%2B-orange?logo=rust)](https://www.rust-lang.org)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![unsafe](https://img.shields.io/badge/unsafe-forbidden-success.svg)](Cargo.toml)

[**Website & course**](https://nastwinns.github.io/hawser/) ·
[Learn (step by step)](https://nastwinns.github.io/hawser/docs/learn/00-what-is-hawser.html) ·
[Install](#install) · [Quick start](#quick-start) · [The cockpit](#the-tui-cockpit) ·
[Docs](https://nastwinns.github.io/hawser/docs/) ·
[Try the TUI in your browser](https://nastwinns.github.io/hawser/try/)

</div>

---

**`haw` composes many Git repos into one reproducible stack.** A manifest
(`haw.toml`) declares the repos; a lockfile (`haw.lock`) pins each to an exact SHA.
Anyone — a teammate, a CI runner, an auditor — rebuilds the byte-identical tree.

```sh
haw init haw.toml   # declare the repos
haw sync            # clone every repo, write haw.lock (exact SHAs)
haw verify          # CI gate: exit 3 if the tree drifts from the lock
```

No submodules. No detached HEADs. No Python. One static binary.

<sub>`haw` does more — fleet-wide `run`/`test`, cross-forge changesets, a k9s-style
cockpit, and supply-chain evidence. **[See all capabilities →](#what-haw-does)**</sub>

![haw TUI cockpit](demo/haw-tui.gif)

## What haw does

Reproducible **compose** is the core (below). Built on it, four more capabilities —
one binary, each solving a slice of the multi-repo problem:

### 🧱 Compose — a reproducible stack from many repos

- **Manifest + lockfile.** `haw.toml` declares repos; `haw.lock` pins each to an exact
  SHA — so a teammate, a CI runner, or an auditor rebuilds the *identical* tree.
- **Stacks & overlays.** Compose repos into named stacks; overlays override revisions
  per variant without duplicating a repo list.
- **Built to scale.** Shallow (`--depth`) and partial (`--filter=blob:none`) clone, and
  a shared object store via git `alternates` — no symlinks, so it works on Windows.

### ⚙️ Orchestrate — run work across the whole fleet

- **Parallel everything.** `run`, `build`, `test` any command across every repo at once
  (`-j N`), reading through [gitoxide](https://github.com/GitoxideLabs/gitoxide).
- **Fleet-wide search.** `haw grep` fans `git grep` across all repos in one shot.
- **A ready CI gate.** `haw verify` asserts the tree matches the lock and exits 3 on
  drift — drop it straight into a pipeline.

### 🔀 Collaborate — cross-repo, cross-forge change flow

- **Changesets.** One feature = one branch across N repos, with cross-linked PR/MRs on
  GitHub, GitLab, **and** Bitbucket, an aggregated review + CI status, and `land` to merge them in
  dependency order.
- **Collaborative merge.** Slice a big merge per-directory, resolve each unit in
  parallel, then seal — or abort cleanly.

### 🚁 Operate — the k9s-style TUI cockpit (bare `haw`)

- **Read → drill → act.** A live fleet grid; drill into a repo's diff, a PR's checks, or
  a CI run's live progress bar + runner — then merge, approve, or checkout from the
  keyboard.
- **A real daily driver.** Fuzzy filter, sort, marks + bulk actions, a **problems-only**
  view, a **drift ⚠** highlight, cross-repo `grep`, a file browser (local *and* remote),
  drop-to-shell, a `:` command bar, and six themes.

### 🛡️ Govern — supply-chain & audit, built in

- **Plugins.** `haw <name>` runs `haw-<name>` from PATH — extend the CLI without forking.
- **Provenance & signing.** SBOM (CycloneDX + SPDX), SLSA/in-toto provenance,
  cosign/minisign signatures on every release.
- **Integrity.** Pre-commit and lifecycle **hooks**, a secret/hygiene **gate**, and
  `evidence` bundles (manifest + lock + audit + status) for qualification.

> No submodules. No detached HEADs. No symlinks. No Python runtime. `haw` orchestrates
> Git and the forge APIs — it does not reimplement Git's merge engine, replace a forge,
> or replace your toolchain.

## Why it exists

Splitting a product across repositories is routine — and it is **domain-agnostic**. It
happens with shared HAL/BSP repos reused across ECUs in automotive and avionics, with a
fleet of backend microservices, with an ML platform whose models, pipelines, and serving
infra must move together, and with Terraform/Helm module repos or an app-plus-SDK pair.
`haw` is built for all of them: embedded/automotive is one proof point, not its identity.

### Who it's for

| Domain | The shape of the fleet |
|--------|------------------------|
| **Embedded & automotive** | Shared HAL/BSP/MCAL reused across many ECUs; AUTOSAR/ARXML config repos pinned beside the code |
| **Backend microservices** | One feature spanning N services plus a shared proto/lib — branched, PR'd, and landed together |
| **ML / data platforms** | Model repo + data-pipeline repo + serving infra, pinned as one reproducible baseline |
| **Platform / infra** | Terraform, Helm, and reusable module repos composed and versioned as a unit |
| **Mobile** | An app repo and its SDK repo(s), changed in lockstep across a single changeset |

The manifest, lockfile, changeset flow, fleet build/test, and governance hooks are the
same in every one — only the repos and the declared `build`/`test` commands differ. See
**[docs/DOMAINS.md](docs/DOMAINS.md)** for how each maps onto `haw`.

Every existing tool solves one slice of the multi-repo problem and gives up another:

| Tool | Gives you | Gives up |
|------|-----------|----------|
| Google `repo` | manifest-driven checkout | lockfile; **symlinks git internals** (breaks on Windows); detached HEADs; needs Python |
| Zephyr `west` | manifest + per-project update | reproducible pinning; detached HEADs; needs Python |
| RepoFleet (Go) | issue → branches → PR/MR flow | stack composition; reproducible pinning; build/CI orchestration |
| mergetopus (Rust) | parallel single-repo merges | anything multi-repo |

`haw` is the union nobody ships — reproducible composition **and** fleet orchestration
**and** cross-forge PR flow **and** supply-chain governance, behind one binary.

Feature by feature (✅ built-in · ~ partial/manual · ✗ not offered):

| Capability | `haw` | `repo` | `west` | `gita` | `meta` | RepoFleet |
|------------|:-----:|:------:|:------:|:------:|:------:|:---------:|
| Committed lockfile (pinned SHAs) | ✅ | ~ | ✗ | ✗ | ✗ | ✗ |
| Single static binary, no runtime | ✅ | ✗ | ✗ | ✗ | ✗ | ✅ |
| Runs on Windows, no symlinks | ✅ | ✗ | ✅ | ✅ | ✅ | ✅ |
| Stack composition + overlays | ✅ | ~ | ~ | ~ | ✗ | ✗ |
| Parallel build / test / run | ✅ | ✅ | ~ | ✅ | ✅ | ✗ |
| Cross-repo grep | ✅ | ~ | ✗ | ✗ | ✗ | ✗ |
| Shallow / partial clone | ✅ | ✅ | ✅ | ✗ | ✗ | ✗ |
| Cross-forge PR/MR (GitHub + GitLab + Bitbucket) | ✅ | ✗ | ✗ | ✗ | ✗ | ~ |
| Land PRs in dependency order | ✅ | ✗ | ✗ | ✗ | ✗ | ✗ |
| k9s-style TUI cockpit | ✅ | ✗ | ✗ | ✗ | ✗ | ✗ |
| SBOM + provenance + signing | ✅ | ✗ | ✗ | ✗ | ✗ | ✗ |

Best-effort as of each tool's current release; corrections welcome via an issue.

> **Why no symlinks, when `repo` needs them?** `repo` shares one object store across
> hundreds of Android repos and wires each checkout to it with symlinks — a hard
> requirement in 2008, before `git worktree` and partial clone existed. `haw` clones each
> repo as a plain, autonomous git repo (disk is cheap now) and gets reproducibility from
> `haw.lock`, not a shared store. Object sharing is *opt-in* via git's native
> `alternates` (`--shared`) — a text file, never a symlink. So `haw` runs on Windows.

## Install

Pick a package manager — all install the same `haw` binary:

```bash
cargo install hawser                                             # Rust / crates.io (canonical)
brew install nastwinns/tap/hawser                                # macOS + Linux (Homebrew)
scoop bucket add nastwinns https://github.com/Nastwinns/scoop-bucket && scoop install hawser   # Windows
```

**Static Linux binary (recommended for servers, containers, air-gap).** The musl build
is fully static — no glibc, no runtime — so one file runs on any Linux host:

```bash
curl -sSL https://github.com/Nastwinns/hawser/releases/download/v0.1.16/haw-0.1.16-x86_64-unknown-linux-musl.tar.gz \
  | tar xz && sudo install haw /usr/local/bin/
```

*Air-gapped host?* Download the archive plus its `.sha256`, `.sig`, and `.pem` on a
connected machine, verify them (below), copy all four files across, and install. The
static binary has no runtime dependencies, so nothing else needs to cross the gap.

**Signed releases.** Every platform — x86_64/aarch64 Linux (glibc), x86_64 musl
(static), x86_64/aarch64 macOS, x86_64 Windows — ships on the
[GitHub Release](https://github.com/Nastwinns/hawser/releases/latest) with a `.sha256`
checksum and a keyless **cosign** signature (`.sig`/`.pem`) you can verify offline.

Other channels — `.deb`/`.rpm`, AUR (`hawser-bin`), Nix flake, Docker, from source:

```bash
cargo install --git https://github.com/Nastwinns/hawser hawser   # latest main
nix run github:Nastwinns/hawser                                  # run once, no install
docker build -t haw . && docker run --rm haw --version           # container
```

Full channel matrix, signature verification, and the air-gap workflow:
**[docs/INSTALL.md](docs/INSTALL.md)**.

## Quick start

```bash
haw init examples/quickstart/haw.toml   # bootstrap from a ready-made example
haw sync                                # clone every repo, write haw.lock
haw                                     # open the cockpit
```

New here? [`examples/`](examples/) has runnable, copy-pasteable manifests to learn from.

A typical session — compose, inspect, branch across repos:

```console
$ haw tree
haw.toml
├─ gateway
│  ├─ kernel    v6.1.2       (git@gitlab.company.com:firmware/kernel.git)
│  ├─ hal       main         (git@gitlab.company.com:firmware/hal.git)
│  └─ app-mqtt  release/2.x  (git@github.com:acme/app-mqtt.git)
└─ sensor-node
   ├─ kernel  v6.1.2         (git@gitlab.company.com:firmware/kernel.git)
   └─ hal     main           (git@gitlab.company.com:firmware/hal.git)

$ haw status
REPO      BRANCH   HEAD      DIRTY  DRIFT
kernel    v6.1.2   a1b2c3d4  -      -
hal       main     9f8e7d6c  yes    -
app-mqtt  release  4d5e6f7a  -      YES

$ haw change start FEAT-42 --repos kernel,app-mqtt
changeset `FEAT-42` started across 2 repo(s):
  kernel    -> change/FEAT-42
  app-mqtt  -> change/FEAT-42
```

One color scheme runs across the CLI and the TUI — colored on a TTY, plain when piped
(`NO_COLOR` / `CLICOLOR_FORCE` honored, exactly like `bat`, `eza`, and `ripgrep`):

| Color | Means |
|-------|-------|
| 🟦 **cyan** | repo & stack names |
| 🟨 **yellow** | revisions, branches, and a dirty working tree |
| ⬛ dim | short SHAs |
| 🟩 **green** | clean / in sync |
| 🟥 **red** ⚠ | drift, conflict, or a repo that isn't cloned |

## The manifest

One file declares your **repos** and composes them into **stacks**. A repo is shared,
never copied. The committed lockfile pins each one to an exact SHA.

```toml
[remote.internal]
url = "git@gitlab.company.com:firmware"

[repo.kernel]
remote = "internal"
repo   = "kernel.git"
rev    = "v6.1.2"        # tag or SHA => pinned and reproducible
groups = ["firmware"]

[repo.hal]
remote = "internal"
repo   = "hal.git"
rev    = "main"          # branch => follows HEAD, until you lock it

[repo.app-mqtt]
url    = "git@github.com:acme/app-mqtt.git"
rev    = "release/2.x"
path   = "apps/mqtt"     # optional; defaults to the repo name

[stack.gateway]
repos = ["kernel", "hal", "app-mqtt"]

[stack.sensor-node]
repos = ["kernel", "hal"]         # shares kernel + hal, no duplication

[overlay.dev.repo.kernel]
rev = "main"                      # `haw sync --overlay dev` follows main for kernel
```

On disk, stacks reuse the same clones — and there is never a symlink:

```
mystack/
├── haw.toml            # manifest (intent)
├── haw.lock            # lockfile (resolved SHAs, committed)
├── kernel/             # real, complete git repo
├── hal/                # real, complete git repo
└── app-mqtt/           # real, complete git repo
```

Sharing objects across stacks on one machine is opt-in via git's native `alternates`
(`git clone --reference`, enabled by `haw sync --shared`) — a text file, not a symlink.

## Build & test the fleet

`haw` stays build-system-agnostic: each repo declares the shell command to build or test
it, and `haw` runs them across the fleet in parallel. Declare them in the manifest:

```toml
[repo.kernel]
remote = "internal"
repo   = "kernel.git"
rev    = "v6.1.2"
build  = "make -j$(nproc)"        # `haw build` runs this in kernel/
test   = "ctest --output-on-failure"

[repo.app-mqtt]
url    = "git@github.com:acme/app-mqtt.git"
rev    = "release/2.x"
build  = "cargo build --release"  # any toolchain — haw only shells out
test   = "cargo test"
```

**Locally** — run every declared command in parallel, filter by group, cap concurrency:

```bash
haw build                     # every repo's `build =`, in parallel
haw test                      # every repo's `test =`
haw build --group firmware    # only firmware-grouped repos
haw test  -j 4                # at most 4 at a time
```

Repos without the command (or not cloned) are skipped. Pre/post hooks
(`pre-build`/`post-build`, `pre-test`/`post-test`) fire around the run. `haw build` exits
**non-zero if any repo fails** — so it doubles as a CI step.

**In CI/CD** — the pipeline is always the same three moves: `sync` to the locked SHAs,
`verify` the tree matches the lock (a drift gate — exit 3), then `build`/`test`.

<details>
<summary><b>GitHub Actions</b></summary>

```yaml
jobs:
  fleet:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4          # the manifest repo (haw.toml + haw.lock)
      - run: cargo install hawser          # or download the signed musl binary
      - run: haw sync --filter=blob:none   # partial clone → fast on large fleets
      - run: haw verify                    # exit 3 if the tree drifts from haw.lock
      - run: haw build
      - run: haw test
    env:
      GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}   # only if a step hits the forge API
```

</details>

<details>
<summary><b>GitLab CI</b></summary>

```yaml
fleet:
  image: rust:latest
  script:
    - cargo install hawser
    - haw sync --filter=blob:none
    - haw verify            # drift gate
    - haw build
    - haw test
  variables:
    GITLAB_TOKEN: $CI_JOB_TOKEN
```

</details>

On a big fleet, pair `haw sync --filter=blob:none` (partial clone — all history, lazy
blobs) with a cache of the shared object store to keep CI clones fast without breaking
the pinned SHAs.

**Any toolchain — embedded, AUTOSAR, emulated.** `build =`/`test =` are just shell
commands, so `haw` drives *any* toolchain: a Docker cross-compiler, an emulator, or a
licensed suite. Validated recipes (with real captured output) — littlefs cross-compiled to
**Cortex-M4** and **FreeRTOS booted under QEMU**, both orchestrated by `haw` — plus wiring
patterns for **EB tresos, Vector MICROSAR, Green Hills, IAR, Tasking, Zephyr, Renode**:
**[docs/INTEGRATION.md](docs/INTEGRATION.md)**.

Want a validated, zero-toolchain-hunting starting point? [`examples/rpi-pico`](examples/rpi-pico)
cross-compiles **two real Raspberry Pi Pico (RP2040) firmwares** (embassy blinky + rp-hal)
to Cortex-M0+ using Rust's built-in `thumbv6m-none-eabi` — **no external ARM GCC** — plus
cJSON (ctest 19/19). `haw sync && haw build -j3 && haw test`, all three with active GitHub
Actions CI. And [`examples/embedded-real`](examples/embedded-real) builds five real
embedded upstreams (CoreMark · cJSON · Monocypher · libcanard · Mbed-TLS) with one
`haw build -j4`. See the full [examples index](docs/EXAMPLES.md).

## Secrets & tokens

`haw` never stores a credential. It reads forge tokens from the environment at call
time and uses them only for API requests (opening PRs, reading CI). Git transport auth
stays with your existing SSH keys or git credential helper — `haw` doesn't touch it.

| Forge | Read in order (first set wins) |
|-------|--------------------------------|
| GitHub | `HAW_GITHUB_TOKEN` → `GITHUB_TOKEN` → `GH_TOKEN` → `HAW_FORGE_TOKEN` |
| GitLab | `HAW_GITLAB_TOKEN` → `GITLAB_TOKEN` → `HAW_FORGE_TOKEN` |
| Bitbucket | `HAW_BITBUCKET_TOKEN` → `BITBUCKET_TOKEN` → `HAW_FORGE_TOKEN` (+ `BITBUCKET_USER` for Basic auth) |

```bash
export GITHUB_TOKEN=$(gh auth token)     # or any PAT, in your shell / CI secret store
```

Read-only composition (`sync`, `status`, `tree`, `verify`) needs no token at all — only
the forge features do.

## Extend haw with plugins

`haw` is designed to become the **central hub** of your multi-repo workflow — you grow
it without forking. It follows the git / cargo / kubectl pattern: any subcommand `haw`
doesn't recognize is dispatched to a `haw-<name>` executable on your `PATH`.

```bash
haw jira sync          # runs haw-jira (any language), fed the fleet context
haw bazel-graph        # runs haw-bazel-graph
```

Each plugin runs as a **separate process** — a broken or hanging plugin can never crash
`haw`. It receives the current fleet as a `haw.plugin/1` JSON document (in `HAW_JSON`
and on stdin), and its exit code becomes `haw`'s. A 20-line script is a valid plugin.

Two ways to extend:

- **New subcommands** — drop a `haw-<name>` binary on `PATH`; it's instantly a `haw`
  command with full workspace context.
- **Lifecycle hooks** — subscribe a plugin to workflow phases in the manifest, so it
  fires automatically around fleet operations:

  ```toml
  [plugins]
  sbom      = ["post-build", "pre-request"]   # generate an SBOM after builds
  gate      = ["pre-request"]                 # block a PR that fails policy
  provenance = ["post-land"]                  # sign + record what shipped
  ```

The governance features ship this way — `haw`'s own SBOM, signing/provenance, and
secret-gate are plugins (`haw-compliance`, `haw-artifact`, `haw-git-gate`), so the
extension model is the same one the core is built on.

**Reference plugins in this repo** — real, tested, ready to copy:

| Plugin | What it does |
|--------|--------------|
| [`haw-aspice`](crates/haw-aspice) | Generates Automotive-SPICE **traceability** (repo → pinned SHA → process area) from the fleet — audit evidence, as a `post-land` hook or `haw aspice` |
| [`haw-jira`](crates/haw-jira) | Links a changeset to a **Jira** issue and transitions it as the change lands (`pre-request` → *In Review*, `post-land` → *Done*); fail-open dry-run without creds |
| [`haw-misra`](crates/haw-misra) | Runs a **MISRA C** static-analysis pass across the fleet's C/C++ sources via `cppcheck --addon=misra`; blocks a PR on violations as a `pre-request` hook or reports via `haw misra`; **fail-open** when cppcheck is absent |

**Reach into the TUI, too.** A plugin's `Report` — its findings and artifacts — surfaces
in the cockpit's **governance view** (`v`), whether the hook fired from the CLI or the
TUI. (Plugins can feed that view today; rendering their *own* TUI panels is on the
roadmap.)

**Any language.** The contract is a subprocess + JSON (`haw.plugin/1` in, `haw.plugin.report/1`
out) — write plugins in Rust, Python, Go, or shell. Published [JSON Schemas](schemas/) and
thin [bindings](bindings/) (Python, Go) make it trivial; scaffold one in seconds:

```bash
haw plugins new my-check --lang python   # runnable skeleton implementing the contract
haw plugins list --remote                # discover community plugins from the index
haw plugins install aspice               # install a first-party or community plugin
```

Write your own: **[docs/PLUGINS.md](docs/PLUGINS.md)** (the dispatch contract + the JSON
schemas), the [reference bindings](bindings/), and the curated
**[AWESOME-HAW-PLUGINS](AWESOME-HAW-PLUGINS.md)** list — add yours via a PR to
[`plugins-index.json`](plugins-index.json).

## Command surface

```
haw                              Open the TUI cockpit (no subcommand)
├── init <manifest-url|path>     Bootstrap a workspace from a manifest
├── sync [--shared] [--depth N]  Clone/pull repos to the state in haw.lock
│        [--filter blob:none]      shallow/partial clone on large fleets
│        [--recurse-submodules]    init/update each repo's git submodules
├── lock / pin / unpin           Resolve revs -> haw.lock / pin to checkouts / restore
├── switch <stack>               Materialize a different stack in the workspace
├── status                       Aggregated fleet status (dirty/ahead/behind per repo)
├── grep <pattern> [--stack S]   git-grep across every cloned repo at once
├── run '<cmd>'                  Run a command across repos, in parallel
├── tree                         Print the stack -> repo tree
│
├── repo   add|remove|list       Edit repos in the manifest
├── stack  add|remove|list       Edit stacks in the manifest
│
├── verify                       Assert tree == haw.lock; exit 3 on drift (CI gate)
├── build / test                 Run each repo's declared build/test command, in parallel
├── hooks  install|list          Git integrity pre-commit + lifecycle hooks (.haw/hooks)
├── evidence                     Bundle manifest+lock+audit+status for audits
│
├── change                       Cross-repo feature ("changeset") workflow
│   ├── start <id> [--repos ..]  Create one branch across the affected repos
│   ├── status                   Per-repo branch + PR/MR review + CI dashboard
│   ├── request                  Open linked PR/MRs on GitHub/GitLab/Bitbucket per repo
│   ├── goto                     Interactive picker; cd into a repo
│   ├── snapshot save|restore    Save/restore the multi-repo state of a changeset
│   └── land                     Merge PR/MRs in dependency order
│
├── merge                        Parallel collaborative merge
│   ├── plan <source>            Slice a big merge into per-directory conflict units
│   ├── resolve <slice>          Resolve one slice (--take ours|theirs, or by hand)
│   └── status / cleanup / abort Track, seal, or undo the planned merge
│
├── plugins list|install|new    Discover/install plugins (`list --remote` for the community index)
├── publish <files> --to <t>    Upload fleet artifacts to Nexus/Artifactory/GitLab/Bitbucket
├── completions <shell>         Emit shell completions (bash/zsh/fish/…)
├── import --from <west.yml|default.xml>   Convert a west/repo manifest to haw.toml
└── dash                         Open the fleet dashboard (same as bare `haw`)
```

Read commands (`status`, `lock`, `tree`, `grep`, …) accept `--json` for scripting,
and `completions <shell>` wires up tab-completion for bash, zsh, fish, PowerShell,
and elvish.

Each verb is one guessable word; old names (`graph`, `forall`, `freeze`, `tui`) stay as
hidden aliases. Full lexicon: [docs/CLI-DESIGN.md](docs/CLI-DESIGN.md).

## The TUI cockpit

Keyboard-first and modal, in the spirit of k9s. The loop is **read → drill in → act**:
see a repo's branch, SHA, and status; open a PR's reviewers and checks; watch a CI run's
progress — then merge or approve it, without leaving the terminal. Everything heavy runs
on a background worker, so the UI never freezes.

![The hawser TUI cockpit — live fleet grid; drill into a repo's git detail, a PR's reviewers and checks, or a CI run's live progress, then merge or approve without leaving the terminal](demo/haw-tui.gif)

<sub>Real VHS capture of `haw dash --demo` (rendered from [`demo/tui.tape`](demo/tui.tape); CI re-renders on every TUI change). Annotated static view of the same grid:</sub>

```text
 haw ▸ ~/work/gateway ───────────────────────── stack: gateway   lock: ✓   repos: 3/3
──────────────────────────────────────────────────────────────────────────────────────
   REPO        BRANCH ▲      HEAD       DIRTY   DRIFT   ↑ / ↓    MERGE
   kernel      v6.1.2        a1b2c3d4     ·       ·      0 / 0     —
 ◉ hal         main          9f8e7d6c    yes      ·      2 / 0     —
▸⚠ app-mqtt    release/2.x   4d5e6f7a     ·      DRIFT   0 / 5     —
──────────────────────────────────────────────────────────────────────────────────────
 hal  ›  path hal/   branch main (ahead 2)   dirty   locked 9f8e7d6c   grp firmware
──────────────────────────────────────────────────────────────────────────────────────
 [s]ync [f]iles [x]shell [!]exec [/]filter [p]roblems [:]cmd [Enter]drill [?]help
```

The grid auto-refreshes (~5s idle, or `F5`/`ctrl-r`) without disturbing your input;
network views (PR/MR, CI, governance) stay on-demand. The essentials:

| Key | Does |
|-----|------|
| `Enter` | **Drill in** — repo git detail · PR reviewers + checks · CI live progress + runner + logs |
| `p` · `/` · `>` `<` `.` | Problems-only · fuzzy filter (`/knl`→`kernel`) · move/toggle sort column |
| `f` · `x` · `!` | Browse **files** (local, or `R` for the forge API) · **shell** in the repo · run one **command** |
| `M` · `A` · `C` · `F` | **Merge** · **approve** · **checkout** a PR locally · **fetch** one repo *(confirm-gated)* |
| `m` · `i` · `v` | Fleet-wide open **PR/MRs** · recent **CI** runs · **governance** (plugins, SBOM, findings) |
| `space` · `s` `r` | **Mark** repos (`◉`); then `s`/`r` act on the marked set · `o` opens the row in a browser |
| `:` | **Command bar** — mirrors the CLI: `:sync`, `:grep TODO`, `:switch NAME`, `:theme nord`, or `:name` to jump |

**Themes** — `catppuccin` (default), `dracula`, `nord`, `gruvbox`, `solarized`,
`monochrome`. `NO_COLOR` forces `monochrome`; `HAW_THEME` sets one at startup;
`:theme <name>` switches live. Full keymap: [docs/CLI-DESIGN.md](docs/CLI-DESIGN.md#tui-keymap).

## Demos

Rendered with [VHS](https://github.com/charmbracelet/vhs) from the tapes in
[`demo/`](demo/); CI re-renders them on every CLI/TUI change, so they never drift.

**[Try the cockpit in your browser →](https://nastwinns.github.io/hawser/)** — real
ratatui widgets over [Ratzilla](https://github.com/ratatui/ratzilla), Rust compiled to
WASM, no server. Source: [`site/`](site/).

The TUI demo above runs against a built-in controller (`haw dash --demo`) — no
workspace, git, or network needed — so its PR/MR and CI views are always populated.
Feature-by-feature CLI walkthroughs, paced to read along:

| Tape | Teaches |
|------|---------|
| [`cli-compose`](demo/cli-compose.gif) | `tree` → `sync` → `status` → `lock` → `pin` → `switch` |
| [`cli-changeset`](demo/cli-changeset.gif) | `change start` / `status`; where `request` / `land` open PR/MRs |
| [`cli-run-verify`](demo/cli-run-verify.gif) | parallel `run`, and `verify` as a CI drift gate (exit 3) |
| [`cli-merge`](demo/cli-merge.gif) | the collaborative merge: `plan` → `resolve` → `cleanup` |

## Architecture

`haw-tui` depends only on `ratatui` — it renders and dispatches, and knows nothing about
git or the network. Every side effect crosses a `Controller` trait, which lets the whole
cockpit run headless in tests against a fake. Heavy work runs off the UI thread over
`Job`/`Outcome` channels. The forge layer hides GitHub (octocrab) and GitLab (reqwest)
behind one `Forge` trait.

The full write-up — crate graph, the concurrency model, the forge abstraction, the
reproducibility contract — is in **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**.

| Crate | Role |
|-------|------|
| [`haw-core`](crates/haw-core) | Manifest, lockfile, resolver, workspace, changesets — the domain logic |
| [`haw-git`](crates/haw-git) | Git backend: gitoxide reads, `git` shell-outs for plumbing |
| [`haw-forge`](crates/haw-forge) | GitHub/GitLab behind one `Forge` trait; changeset + fleet orchestration |
| [`haw-merge`](crates/haw-merge) | Collaborative merge: plan/resolve/cleanup/abort |
| [`haw-tui`](crates/haw-tui) | The ratatui cockpit — renders and dispatches, nothing more |
| [`hawser`](crates/hawser) | The `haw` binary: clap CLI, thin glue |

## Development

```bash
cargo test --workspace                                # unit + integration
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Covered: manifest parse + validation, TOML round-trip, resolver + overlay precedence,
lockfile determinism (byte-identical, LF-only, cross-OS in CI), changeset lifecycle, the
full collaborative merge against real git repos, golden CLI snapshots
(`crates/hawser/tests/golden.rs`), forge orchestration against a fake forge, and the
cockpit logic (filter, sort, marks, command bar, drill-ins, grep, themes).

## Documentation

Published at **[nastwinns.github.io/hawser/docs](https://nastwinns.github.io/hawser/docs/)**
(mdBook, rebuilt on every push). Sources:

| Doc | What |
|-----|------|
| [docs/DOMAINS.md](docs/DOMAINS.md) | How the manifest/lock/changeset/build/govern loop maps onto each domain — embedded/automotive, microservices, ML/data, infra, mobile |
| [docs/INTEGRATION.md](docs/INTEGRATION.md) | Copy-paste `build`/`test` recipes: Docker cross-compile, QEMU/Renode emulation, EB tresos, Vector, GHS/IAR/Tasking, Zephyr (real captured output for the open ones) |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crate layout, concurrency model, forge abstraction, data flows |
| [docs/CLI-DESIGN.md](docs/CLI-DESIGN.md) | Full CLI lexicon + TUI keymap |
| [docs/EXTENDING.md](docs/EXTENDING.md) | Extensions, plugins, hooks, auth, CI/CD integration |
| [docs/PLUGINS.md](docs/PLUGINS.md) | Writing subcommand plugins — `haw <name>` runs `haw-<name>` from PATH |
| [docs/COMPLIANCE.md](docs/COMPLIANCE.md) | Tool qualification, SBOM/CRA, crypto/signing, GDPR |
| [docs/INSTALL.md](docs/INSTALL.md) | Full install matrix + signature verification |
| [docs/SECURITY.md](docs/SECURITY.md) | Trust model — what haw executes, plugin trust, tokens |

## Security

Read the full **[trust model](docs/SECURITY.md)**. The essentials:

- **The manifest is trusted code.** A `haw.toml`'s `build`/`test`/`run`/`exec`
  commands are executed through your shell. Running `haw build`, `haw run`, or
  `haw sync` on an **untrusted** checkout is equivalent to running its
  `Makefile` — only do it on manifests you trust. Treat `haw.toml` like a
  `Makefile` or a `package.json` `scripts` block.
- **Plugins are trusted binaries.** `haw <name>` runs `haw-<name>` from your
  `PATH`, and plugins inherit your full environment (including any tokens).
  Install only plugins you trust and keep your `PATH` clean.
- **Tokens** are read from environment variables only, never stored or logged;
  git transport auth stays with your existing SSH keys / credential helper.
- **Supply-chain hardening.** The crate is `#![forbid(unsafe_code)]`, HTTPS uses
  **rustls** (no OpenSSL), every GitHub Action is **pinned to a full commit SHA**,
  release artifacts are **cosign**-signed (keyless/OIDC), and `cargo audit` +
  `cargo deny` gate advisories, licenses, and sources on every push.
- **Reporting a vulnerability:** see [SECURITY.md](SECURITY.md) for how to
  report privately and which versions are supported.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
