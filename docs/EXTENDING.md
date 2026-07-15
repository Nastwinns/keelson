# Keelson — Extensions, Plugins, Auth & CI/CD

How Keelson stays open at the edges: it **orchestrates** git, forges, and build tools — it
never reimplements them. Everything task-specific (build systems, forges, custom steps)
plugs in from outside through stable, boring interfaces. Pairs with
[ARCHITECTURE.md](ARCHITECTURE.md) (internals) and [COMPLIANCE.md](COMPLIANCE.md) (evidence).

Design rules, in order:
1. **Git-native.** If it's already a text file git understands (lock, alternates, gitdir),
   use that. Never a hidden database.
2. **Orchestrate, don't reimplement.** haw decides *what* and *when*; the user's tools do
   *how*. haw knows nothing about CMake, Bazel, Yocto, Jenkins — it shells out.
3. **Unix pipes.** Human view in the TUI; machine view in `--format json` on stdout. Anything
   haw shows, a script can consume.
4. **Fail open.** An unknown forge, a missing token, an absent plugin degrades one feature —
   it never blocks `sync`/`tree`/`status`.

---

## 1. Extension mechanisms

Four layers, cheapest first. Reach for the lowest one that solves the problem.

### 1.1 `run` — run any command across repos
The universal escape hatch. Parallel command execution across every repo (group-filterable).

```bash
haw run 'git fetch --tags'
haw run --group firmware 'cmake --build build'
```
No plugin needed for the 80% case: "do X in every repo".

### 1.2 Hooks — run scripts at lifecycle points
Git-style hooks fired around haw operations. Scripts live in `.haw/hooks/` (or are declared
in the manifest) and receive context via env + stdin JSON.

| Hook | Fires |
|------|-------|
| `pre-sync` / `post-sync` | before/after a `haw sync` |
| `pre-lock` / `post-lock` | around lockfile (re)generation |
| `post-switch` | after `haw switch <stack>` |
| `post-change-start` | after a changeset branch is created |

Example — install a git hook that rejects a commit when `haw.lock` is stale (the "git-way"
integrity guarantee):
```bash
haw hooks install    # writes a pre-commit that runs `haw verify --lock`
```

### 1.3 Per-repo commands in the manifest
Declare how a repo is built/tested so haw can drive it without hard-coding any build tool.

```toml
[repo.app-mqtt]
remote = "github"
repo   = "app-mqtt.git"
rev    = "release/2.x"
build  = "cmake --build build --preset release"   # haw just shells out
test   = "ctest --test-dir build"
```
haw stays build-system-agnostic: CMake, Bazel, Meson, Cargo, Make — all just strings.

### 1.4 Subcommand plugins — `haw-<name>` on PATH
The git / cargo / kubectl pattern. `haw foo …` that isn't a built-in execs `haw-foo` from
PATH, forwarding args and the workspace context (via env + `HAW_JSON` on stdin). The
community ships `haw-jira`, `haw-bazel`, `haw-sbom-scan` without touching core.

- Discovery: any executable named `haw-*` on `PATH`.
- Contract: haw passes workspace root, current stack, and resolved repos as JSON on stdin;
  the plugin prints results as JSON on stdout; haw renders or pipes them.
- Isolation: plugins are separate processes — a broken plugin can't crash haw.

> Core stays small. haw maintains the composition + orchestration engine; integrations
> (every forge quirk, every build tool, every tracker) live in plugins and hooks.

### 1.5 Machine interface — `--format json`
Every read command (`status`, `tree`, `change status`, `verify`, `evidence`) offers
`--format json` with a stable, versioned schema, plus stable exit codes. This is what CI,
dashboards, and plugins consume. The TUI is for humans; JSON is for machines.

---

## 2. Authentication (works on any repo)

The adoption unlock: **transport auth is free; forge auth is opt-in.** haw never invents its
own credential store.

### 2.1 Transport (clone / fetch / push) — zero config
haw shells out to the user's `git`, so it inherits existing git auth automatically:
- **SSH** keys via `ssh-agent` (the enterprise/embedded default).
- **HTTPS** via `git credential` helpers (Git Credential Manager, `osxkeychain`, `cache`).

Works with **any** host — GitHub, GitLab, Gitea, Bitbucket, self-hosted, plain SSH — with no
integration. If `git clone` works, `haw sync` works. This is exactly how `repo` and `west`
authenticate: they don't.

### 2.2 Forge API (open/read PR-MR, CI status) — token, only when used
Only the PR/MR features need API credentials, resolved in this order:
1. Env: `GITHUB_TOKEN` / `GH_TOKEN`, `GITLAB_TOKEN`, `HAW_FORGE_TOKEN`.
2. Reuse an existing CLI's stored token: `gh auth token`, `glab auth`.
3. `git credential` for HTTPS tokens.
4. **OAuth device flow** (`haw auth login`) — prints a code to enter in a browser, no
   localhost redirect, works headless/over SSH; token stored in the **OS keychain**. This is
   the `gh` / `docker login` / `aws sso` model.

- **Self-hosted:** configurable API base URL per forge (GitHub Enterprise, GitLab
  self-managed). Mandatory for the target market.
- **Air-gapped:** token via env/file only; no browser flow, no egress (see COMPLIANCE §6).
- **Never** persist a token in `haw.toml`, `haw.lock`, logs, or workspace state
  (COMPLIANCE §5.6). Redact credential-shaped strings.

### 2.3 Forge detection
haw maps each repo's remote URL → forge (GitHub/GitLab/…) via the `Forge` trait. Unknown
host → transport still works; only PR/MR features disable for that repo. Fail open.

---

## 3. CI/CD integration

haw is designed to be driven by pipelines, not just humans.

### 3.1 Reproducible checkout
```bash
haw sync --locked      # materialize the exact haw.lock tree; fail if lock is missing/stale
haw verify             # assert on-disk tree == lock (drift gate); non-zero on drift
```
`--locked` is the CI contract: no rev resolution, no network nondeterminism — the committed
lock is law. Deterministic on Linux/macOS/Windows (COMPLIANCE §8).

### 3.2 Gates via JSON + exit codes
```bash
haw status --format json | jq -e '.repos[] | select(.dirty or .drift)' && exit 1 || true
```
Stable exit codes: `0` ok, distinct non-zero for drift / verify failure / signature failure.

### 3.3 Tokens in CI
Inject forge tokens as CI secrets (`GITHUB_TOKEN` in Actions, masked vars in GitLab). No
interactive login in pipelines. Transport uses the runner's SSH key or a deploy token.

### 3.4 Object-sharing cache (fast CI)
```bash
haw sync --locked --shared   # git alternates against a warm mirror cache; text file, no symlinks
```
Cache the mirror between runs to avoid re-cloning large repo trees.

### 3.5 Evidence in the release pipeline
```bash
haw evidence --out haw-evidence.tar.zst   # baseline + SBOM + provenance + tool config record
```
Attach to the release for the certification data package (COMPLIANCE §3, §4).

### 3.6 GitHub Actions (sketch)
```yaml
- uses: actions/checkout@v4
- run: cargo install hawser
- run: haw sync --locked --shared
  env: { GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }} }
- run: haw verify
- run: haw run --group firmware 'cmake --build build --preset release'
```

### 3.7 GitLab CI (sketch)
```yaml
build:
  script:
    - haw sync --locked
    - haw verify
    - haw run 'make'
  variables: { GITLAB_TOKEN: $CI_JOB_TOKEN }
```

---

## 4. Where this lands in the plan

Additions to the phased plan in [ARCHITECTURE.md §6](ARCHITECTURE.md); each item is scoped
to keep core small and push integrations to the edges. Status as of 2026-07-15: everything
below is shipped except the OAuth device-flow login (deferred — ARCHITECTURE DR-14) and
the full `haw evidence` SBOM/provenance payload (today's bundle: manifest, lock, audit
log, status JSON, tool record).

| Capability                                   | Layer            | Phase |
|----------------------------------------------|------------------|-------|
| `run` parallel across repos (alias forall)               | core             | 3     |
| `--format json` + stable schemas/exit codes  | core             | 1→3   |
| Forge transport (git-native, zero config)    | haw-git         | 1     |
| Forge API tokens (env / gh-glab reuse)        | haw-forge       | 1 (GH), 3 (GL) |
| OAuth device-flow login + keychain           | haw-forge       | deferred (DR-14) |
| Self-hosted forge base URL                   | haw-forge       | 3     |
| Lifecycle hooks (`pre/post-sync`, …)         | core + `haw hooks` | 4  |
| `haw hooks install` (stale-lock pre-commit) | core             | 4     |
| Per-repo `build`/`test` commands in manifest | manifest model   | 4     |
| Subcommand plugins (`haw-<name>` on PATH)   | hawser dispatch| 5     |
| Plugin stdin/stdout JSON contract            | core             | 5     |
| `haw verify` drift gate (CI)                | core             | 1→2   |
| `haw evidence` bundle                       | core             | 3     |
| `--shared` object-sharing cache              | haw-git         | 2     |

Guiding constraint: **the core never grows a hard dependency on a specific build tool,
tracker, or CI system.** Those arrive as hooks, per-repo commands, or `haw-*` plugins.
