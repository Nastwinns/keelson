# Keelson — Architecture & Implementation Plan

## 1. Crate architecture (Cargo workspace)

A workspace of small crates so the core stays reusable by the CLI, the TUI, and later a
Tauri GUI. The golden rule: **all business logic lives in `keel-core`; the CLI and TUI are
thin front-ends.** Formats and forges sit behind traits so a format or API change is an
impl swap, not a rewrite.

```
keelson/
├── Cargo.toml                     # [workspace]
├── crates/
│   ├── keel-core/                 # domain logic, no I/O opinions leaked
│   │   ├── manifest/              # serde structs + `ManifestLoader` trait
│   │   │   ├── model.rs           #   Manifest, Repo, Stack, Remote, Overlay
│   │   │   ├── toml_loader.rs     #   default loader
│   │   │   └── import/            #   west.yml + repo default.xml -> model
│   │   ├── lock/                  # keel.lock read/write, resolve, drift detection
│   │   ├── workspace/             # on-disk layout, stack materialization
│   │   ├── resolver/              # manifest + overlays -> concrete repo set
│   │   └── change/                # changeset model (feature across repos)
│   │
│   ├── keel-git/                  # git operations abstraction
│   │   ├── introspect.rs          #   gitoxide: status, ahead/behind, current SHA
│   │   ├── ops.rs                 #   shell-out: clone, fetch, checkout, --reference
│   │   └── parallel.rs            #   tokio-driven fan-out across repos
│   │
│   ├── keel-forge/                # PR/MR orchestration
│   │   ├── mod.rs                 #   `Forge` trait: open_pr, pr_status, merge_pr
│   │   ├── github.rs              #   octocrab
│   │   ├── gitlab.rs              #   gitlab crate / REST
│   │   └── detect.rs              #   remote URL -> which forge
│   │
│   ├── keel-merge/                # optional: mergetopus-style slicing (later phase)
│   │
│   ├── keel-cli/                  # clap-based binary `keel` (thin over core)
│   └── keel-tui/                  # ratatui dashboard (thin over core)
└── xtask/                         # release/packaging automation
```

### Why these boundaries

- **`keel-core` knows nothing about clap, ratatui, or a terminal.** It exposes an API
  (`Workspace::sync`, `Changeset::start`, …). This is what lets the TUI and a future Tauri
  GUI reuse everything.
- **`ManifestLoader` trait** — TOML is the reference format, but `import` produces the same
  in-memory model from west/repo files. Supporting another format later = new impl.
- **`Forge` trait** — GitHub and GitLab differ enough (PR vs MR, review APIs) that a common
  trait with two impls keeps `change` logic forge-agnostic.
- **`keel-git` splits introspect (gitoxide) from ops (shell-out).** gitoxide is fast and
  native for reads; heavy/rare mutating operations shell out to the user's `git` for
  correctness and to avoid gitoxide's still-maturing high-level clone/push APIs.

## 2. Key dependencies

| Concern            | Crate                    | Note                                            |
|--------------------|--------------------------|--------------------------------------------------|
| Git introspection  | `gix` (gitoxide)         | status, refs, ahead/behind, SHA — fast, native  |
| Git heavy ops      | shell-out to `git`       | clone `--reference`, fetch, checkout, merge     |
| Async fan-out      | `tokio`                  | parallel sync/forall across repos              |
| Manifest/lock      | `serde` + `toml`         | typed structs; lock is generated TOML           |
| CLI                | `clap` (derive)          | subcommand tree                                 |
| TUI                | `ratatui` + `crossterm`  | fleet dashboard, cross-platform                 |
| GitHub API         | `octocrab`               | PRs, reviews, checks                            |
| GitLab API         | `gitlab` or `reqwest`    | MRs, approvals, pipelines                       |
| Errors             | `thiserror` + `anyhow`   | typed in core, contextual at edges              |
| Config/paths       | `directories`            | cross-platform cache dir for `--reference`      |

## 3. Cross-platform discipline (Linux + Windows + macOS)

- `PathBuf` everywhere, never a hard-coded `/`.
- **No symlinks in the workspace layout.** Object sharing uses git `alternates` via
  `--reference` (a text file). This is the single most important design choice for Windows.
- Handle `core.autocrlf`; do not assume LF.
- Assume `git` is on PATH (reasonable on Windows); gitoxide covers the read side natively.
- CI matrix builds all three OSes from day one (like mergetopus does).

## 4. Data flow: `keel sync`

```
read keel.toml ──▶ resolver (apply overlays, pick stack)
                        │
                        ▼
              does keel.lock exist?
                 │              │
                yes             no
                 │              │
    for each repo:      resolve each rev ──▶ SHA
    target = lock SHA          │
                 │             ▼
                 │        write keel.lock
                 └──────┬───────┘
                        ▼
        tokio fan-out over repos (parallel):
          not cloned?  -> git clone [--reference cache] to declared path
          cloned?      -> git fetch + checkout target (NOT detached: real branch)
                        ▼
              gitoxide: verify each repo SHA == target
              report drift (local SHA != lock)
```

## 5. Data flow: `keel change` (the RepoFleet-beating part)

```
change start FEAT-123 --repos kernel,app-mqtt
        │
        ▼  keel-core/change: create branch feat/123 in each listed repo (real branch)
        ▼  record changeset (which repos, which branch) in workspace state

change request
        │
        ▼  keel-forge/detect: per repo, GitHub or GitLab?
        ▼  Forge::open_pr on each -> collect PR/MR URLs, cross-link them in descriptions

change status
        │
        ▼  Forge::pr_status per repo (review state + CI/pipeline) -> aggregated table/TUI

change land
        │
        ▼  topological order from stack->repo graph
        ▼  Forge::merge_pr in order; stop on failure
```

## 6. Implementation plan (phased)

Two design decisions shape this plan:

1. **The v0.1 MVP spans both value layers** — reproducible composition *and* cross-repo
   MR orchestration — rather than shipping composition alone and bolting MR on later. The
   union is the differentiator; a composition-only v0.1 would just be "repo with a lock",
   and a MR-only v0.1 would just be "RepoFleet in Rust". Shipping both, even minimally, is
   what makes the first release defensible.
2. **The TUI ships in v0.1**, not as a late phase. The fleet dashboard is the most visible
   differentiator and the cheapest way to make the double-layer value legible. It is a thin
   front-end over `keel-core`, so building it early also validates that the core API is
   genuinely UI-agnostic.

Each phase still ends with a usable binary. Ship early, narrow, correct.

> **Status (2026-07-15): Phases 0–5 are implemented** (Phase 6 stays demand-driven).
> Per-phase deltas vs the original plan are recorded in §9's decision records.

### Phase 0 — Skeleton (week 1) — ✅ shipped
- Cargo workspace, the crate boundaries above, CI matrix (Linux/macOS/Windows) from day one.
- `keel-core::manifest` serde model + TOML loader + round-trip tests.
- `keel --version`, `keel graph` (parse manifest, print stack→repo tree).
- **Deliverable:** parses a manifest, prints the composition. Nothing clones yet.

### Phase 1 — Double-layer MVP (weeks 2–6) — *the whole point, minimally* — ✅ shipped

The MVP deliberately cuts a thin vertical slice through **both** layers plus the TUI,
rather than completing one layer fully. Scope each item to the minimum that proves value.

*Composition (minimal):*
- `keel-git`: clone (shell-out), fetch, checkout as a real branch; gitoxide introspection.
- `keel init`, `keel sync`, `keel lock`, `keel status`.
- `keel.lock` generation + drift detection. Parallel sync via tokio.
- Stacks modeled and parsed; `keel switch <stack>` for the single-stack common case.
  (Overlays and `--shared` object sharing deferred to Phase 2 — not needed to prove value.)

*MR orchestration (minimal):*
- `keel-forge`: `Forge` trait + URL→forge detection + **GitHub (octocrab) first**, GitLab
  stubbed behind the same trait.
- `keel change start` (branch across repos) and `keel change status` (aggregated view).
  `request` and `land` land in Phase 3; `start`+`status` alone already beat manual `cd`-ing.

*TUI (minimal):*
- `keel tui`: read-only ratatui fleet dashboard — stack→repo tree, per-repo state
  (branch, SHA, dirty, ahead/behind, drift-vs-lock), and the changeset view. Actions
  (sync/switch/start) added in Phase 4; a read-only cockpit is already the visual hook.

- **Deliverable:** from a manifest, reproducibly clone a stack with a committed lockfile,
  start a feature branch across its repos, and see the whole fleet + changeset in a TUI.
  No competitor ships this combination.

### Phase 2 — Composition depth (weeks 7–8) — ✅ shipped (freeze/unfreeze shipped as `pin`/`unpin` + aliases)
- Overlays / profile inheritance in the resolver (grit-style, kills manifest duplication).
- `--shared` object sharing via `git clone --reference` (text file, **no symlinks**).
- `keel freeze` / `unfreeze`; `stack`/`repo` add/remove editing the manifest.
- **Deliverable:** the stacks×repos model at full power, incl. shared repos and DRY
  manifests for large (50+ repo) trees.

### Phase 3 — MR orchestration depth (weeks 9–11) — ✅ shipped (see DR-11/DR-13)
- GitLab impl fully behind the `Forge` trait (MRs, approvals, pipelines).
- `keel change request` (open cross-linked PR/MRs on both forges) and `keel change land`
  (merge in topological order from the stack→repo graph).
- `keel change goto` (interactive picker + cd, RepoFleet-style shell integration).
- `keel forall -c` parallel.
- Changeset **snapshots** (save/restore multi-repo feature state), a RepoFleet idea worth
  matching.
- **Deliverable:** full cross-repo feature lifecycle on GitHub *and* GitLab, with
  composition underneath — strictly a superset of RepoFleet.

### Phase 4 — TUI actions & polish (week 12) — ✅ shipped
- Promote the read-only TUI to interactive: keyboard-driven sync, switch, `pin`, change
  start/request/land, goto.
- **Design bar (non-negotiable): match [`k9s`](https://k9scli.io).** Keyboard-first + modal:
  `:` command bar, `/` filter, single-key actions, live-updating grid, always-visible help
  bar. Async refresh (no frozen frames), color-coded status, themeable + `NO_COLOR`-aware.
  Mouse optional, never required. Open with a bare `keel` or `keel dash`.
- **Deliverable:** the fleet cockpit becomes a polished control surface, not just a viewer.

### Phase 5 — Migration & distribution (week 13) — ✅ shipped (`keel import`, packaging/, examples/, `cargo xtask dist`)
- `keel import --from west.yml | default.xml` (convert existing manifests).
- Homebrew tap + Scoop bucket + `cargo install` (match RepoFleet's distribution channels).
- Docs, an embedded/BSP example, an automotive-style pinned-manifest example.
- **Deliverable:** low-friction adoption path for `repo`/`west` users.

### Phase 6 (optional, later) — Collaborative merge
- `keel merge plan | resolve | cleanup` (mergetopus-style slicing), reusing its proven
  workflow. Only if user demand appears; it is orthogonal to the core value.

### Extensibility, auth & CI/CD (cross-phase)
Keelson stays open at the edges: it orchestrates git, forges, and build tools without
reimplementing them. The extension surface — `forall`, lifecycle hooks, per-repo build
commands, `keel-<name>` subcommand plugins, the `--format json` machine interface — plus the
auth model (git-native transport + opt-in forge tokens / OAuth device flow) and the CI/CD
integration (`sync --locked`, `verify`, `evidence`, object-sharing cache) are specified in
[EXTENDING.md](EXTENDING.md), with each item mapped to the phase that ships it. Guiding
constraint: **the core never grows a hard dependency on a specific build tool, tracker, or
CI system** — those arrive as hooks, per-repo commands, or plugins.

### Lexicon & testing (cross-phase)
- **Lexicon**: verbs are one guessable word each — `tree` (was `graph`), `run` (was
  `forall -c`), `pin`/`unpin` (was `freeze`/`unfreeze`), bare `keel`/`dash` (was `tui`);
  flag `--slug` (was `--repo`) on `repo add`. Old names stay as hidden aliases. Canonical
  spec: [CLI-DESIGN.md](CLI-DESIGN.md). Landed incrementally across Phases 1–2, cosmetic.
- **Golden CLI-output tests** (Phase 2): snapshot `tree`/`status`/`lock` output so lexicon or
  format changes surface in review.
- **Determinism tests** (Phase 1→2): assert `keel.lock` is byte-identical across
  Linux/macOS/Windows for identical inputs — a hard certification requirement
  ([COMPLIANCE.md §8](COMPLIANCE.md)).

## 7. Sequencing rationale

The MVP (Phase 1) is intentionally a thin slice of the *full* vision rather than a complete
slice of *one* layer, because the value proposition is the union. After that, Phases 2 and 3
deepen the two layers independently, so each can be released, tested, and reprioritized on
its own — you deepen composition or MR orchestration based on which users actually pull on,
without having bet the first release on either one alone.

## 8. What we borrow from the field (and how we differ)

| Source        | What we take                                              | Where we go further                                  |
|---------------|-----------------------------------------------------------|------------------------------------------------------|
| Google `repo` | manifest-driven multi-repo checkout, groups               | + lockfile, no Python, no detached HEAD, no symlinks |
| `west`        | plain-clone layout (Windows-safe), `manifest --freeze`    | + stacks×repos composition, MR orchestration      |
| `grit`        | overlays / profile inheritance to keep manifests DRY      | integrated with lock + forge layers                  |
| RepoFleet     | workspace + issue-centered branches, status dashboard, `goto`, snapshots, `--skip-branch` to adopt existing branches | + reproducible composition, both forges, TUI, Rust |
| mergetopus    | parallel collaborative merge slicing (Phase 6)            | wired into a multi-repo changeset, not single-repo    |

RepoFleet is the closest prior art on the MR side, so its concrete choices are worth
matching deliberately: a **workspace** grouping repos, an **issue/changeset** as the unit of
cross-repo work, a **status dashboard** as the primary view, `goto` shell integration,
**snapshots** of multi-repo state, and `--skip-branch` to adopt already-checked-out branches
instead of forcing new ones. Keelson's edge is everything underneath and around that:
a committed lockfile, stack composition, GitHub *and* GitLab, a real TUI, and a
gitoxide-native Rust core.

## 9. Decision records (as implemented)

Decisions the plan left open, fixed during Phase 1. Each one is reversible behind a
trait or a file format version.

- **DR-1 — Lock covers the whole manifest, not one stack.** `keel.lock` pins every
  repo; `sync --stack` consumes a subset. Switching stacks never rewrites the lock.
- **DR-2 — Overlays only apply at lock time.** `keel lock --overlay dev` re-resolves;
  `keel sync` with an existing lock ignores overlays (and says so). Lock stays the single
  source of truth for reproducibility.
- **DR-3 — Branch policy, never detached.** A branch rev checks out on a local branch of
  the same name; tags and SHAs check out on `keel/<rev>`. The lock records the branch
  (`branch` field) so re-syncs need no network. `checkout -B` is guarded: local commits
  not contained in the target abort the sync (`GitError::LocalCommits`).
- **DR-4 — Rev resolution without cloning** uses `git ls-remote --heads --tags` (peeled
  `^{}` entries win for annotated tags). Full 40-hex revs pass through unresolved.
- **DR-5 — Reads shell out too, for now.** `gix` is deferred: the `GitBackend` trait in
  `keel-core::git` is the seam, `keel-git::ShellGit` the only impl. Swapping reads to
  gitoxide later touches one crate, zero callers. Keeps Phase 1's dep tree small.
- **DR-6 — Threads, not tokio, for fan-out.** Git work is process-spawning; a bounded
  `std::thread::scope` pool (`keel-git::parallel::fan_out`) suffices and stays sync.
  tokio arrives with the async forge APIs (octocrab) in Phase 3.
- **DR-7 — Workspace state lives in `.keel/`** (uncommitted): `stack` records the
  current stack; `changesets/<id>.toml` records changeset membership + branches.
- **DR-8 — keel-core depends on nothing that does I/O by policy**, but performs manifest,
  lock, and state file I/O itself (it owns those formats). Network/git I/O stays behind
  `GitBackend`.
- **DR-9 — Lockfile is versioned** (`version = 1`); unknown versions are a hard error,
  schema evolution goes through explicit migration.
- **DR-10 — Forge detection is hostname-substring** (`github`/`gitlab`), which covers
  self-hosted GitLab; an explicit `forge = "github" | "gitlab"` key on `[remote.X]`
  overrides the heuristic for hosts it misses (shipped in Phase 3).
- **DR-11 — Forge clients: octocrab's generic verbs + reqwest, sync trait.** GitHub goes
  through `octocrab` driven by a private current-thread tokio runtime; GitLab uses
  `reqwest` (blocking, REST v4). The `Forge` trait stays synchronous, so `keel-core` and
  the CLI never see an async runtime. Generic JSON verbs (get/post/patch/put) are used
  instead of octocrab's typed builders to stay stable across its releases.
- **DR-12 — Snapshots capture the whole workspace.** `keel change snapshot save`
  records every repo's branch + HEAD (not just the changeset's members): a feature's
  state includes where the rest of the fleet stood. Restore refuses on dirty repos and
  never touches the network.
- **DR-13 — `change land` order is manifest `deps`, then changeset order.** A repo's
  optional `deps = [...]` gives the product→repo graph real edges; land performs a
  stable topological sort over the changeset members and stops at the first failure.
- **DR-14 — OAuth device flow is deferred.** Token resolution today: env
  (`KEEL_GITHUB_TOKEN`/`GITHUB_TOKEN`/`GH_TOKEN`, `KEEL_GITLAB_TOKEN`/`GITLAB_TOKEN`,
  `KEEL_FORGE_TOKEN`) then a logged-in `gh` CLI. `keel auth login` + OS keychain
  (EXTENDING §2.2) needs a keyring dependency and interactive UX — postponed until the
  CLI's audience demands it; air-gapped and CI environments are already covered.
