# CLI design — lexicon & options

Goal: a lexicon a new user understands without a glossary, and options that fix what
`repo`/`west` users always missed.

## Lexicon (canonical since v0.1)

| Term | Meaning | Replaces / rejected |
|------|---------|---------------------|
| **repo** | one Git repository in the workspace (`[repo.NAME]`) | ~~brick~~ (accepted alias), `project` (repo-tool jargon) |
| **stack** | a named composition of repos (`[stack.NAME]`, `repos = [...]`) | ~~product~~ (accepted alias) |
| **overlay** | named per-repo overrides applied at lock time | `profile`, `variant` |
| **changeset** | one feature across N repos (branch + PR/MRs) | `topic`, `issue` |
| **group** | free-form label on a repo, used to filter commands | kept from `repo` tool, now actually wired |
| **rev** | what you ask for: branch, tag, or SHA — kind auto-detected | `revision`, `refspec` |
| **lock / pin** | resolved SHA in `haw.lock` | `freeze` (planned rename: `haw pin` / `haw unpin`) |
| **drift** | HEAD differs from the locked SHA | — |

Old spellings (`brick`, `product`, `bricks`, `--product`, `--bricks`) parse forever as
aliases; serialization and docs use the new words only.

## Verbs (commands)

Rule: one guessable verb per action, git-adjacent, no jargon. Old names kept as hidden
aliases so nothing breaks.

| Verb | Does | Alias (accepted) |
|------|------|------------------|
| `haw init <path>` | bootstrap a workspace from a manifest | — |
| `haw sync` | materialize the tree to `haw.lock` (writes lock if absent) | — |
| `haw tree` | print the stack → repo tree | `graph` |
| `haw status` | fleet status: branch, head, dirty, drift per repo | `st` |
| `haw run '<cmd>'` | run a command in every repo, in parallel (positional) | `forall` (with `-c`) |
| `haw lock` | resolve every repo's rev → SHA into `haw.lock` | — |
| `haw pin` | pin `haw.lock` to current checkouts (no network) | `freeze` |
| `haw unpin` | restore `haw.lock` to manifest revs | `unfreeze` |
| `haw switch <stack>` | record a stack as current and sync it | — |
| `haw repo add\|remove\|list` | edit the repos of the manifest | `brick` |
| `haw stack add\|remove\|list` | edit the stacks of the manifest | `product` |
| `haw change start\|status\|list` | cross-repo feature (changeset) workflow | — |
| `haw grep <pat>` | fan-out grep across every repo | — |
| `haw verify` | drift gate — exit 3 if the tree diverges from `haw.lock` | — |
| `haw build` | run each repo's manifest `build` command across the fleet | — |
| `haw test` | run each repo's manifest `test` command across the fleet | — |
| `haw hooks install` | install lifecycle hooks from `.haw/hooks/` | — |
| `haw evidence` | bundle SBOM / provenance / signatures into `haw-evidence.tar.gz` | — |
| `haw publish <files> --to <registry>` | upload artifacts to a private registry (see [DISTRIBUTION.md](DISTRIBUTION.md)) | — |
| `haw import --from <west.yml\|default.xml>` | convert a `west` / Google-`repo` manifest to `haw.toml` | — |
| `haw merge plan\|resolve\|status\|cleanup\|abort` | parallel collaborative merge (per-slice) | — |
| `haw completions <shell>` | print a shell completion script to stdout | — |
| `haw plugins new\|list\|install` | scaffold, discover, and install `haw-<name>` plugins | — |
| `haw` (no args) / `haw dash` | open the TUI cockpit | `tui` |

`haw run` takes the command positionally (`haw run 'git fetch'`); `-c/--command` still works
via the `forall` alias. Running `haw` with no subcommand opens the dashboard (like `htop`,
`k9s`).

## Rev handling (user-friendly by default)

- One field: `rev = "main" | "v6.1.2" | "<40-hex sha>"`. No `type =` key; the kind is
  detected (`refs/heads` > peeled tag > tag > full SHA).
- Display: SHAs are shown 8 chars everywhere; `haw.lock` stores the full 40.
- Never detached: branch revs check out on a same-name branch, tags/SHAs on `haw/<rev>`.

## Groups (implemented)

- `groups = ["firmware", "ci"]` on a repo.
- `haw sync --group firmware`, `haw status --group ci`, `haw forall --group firmware -c ...`
  (repeatable; empty filter = everything; a filter excludes ungrouped repos).
- Groups are recorded in `haw.lock` so filtering works offline.

## Options grid

| Option | Commands | Note |
|--------|----------|------|
| `--stack <S>` | sync, tree | alias `--product`; default: last `switch`, else the only stack |
| `--overlay <O>` | lock, sync*, tree | repeatable, later wins; *sync only when generating the lock |
| `--group <G>` | sync, status, run | repeatable |
| `--repos a,b` | change start | alias `--bricks` |
| `--slug <S>` | repo add | repo path under `--remote` (alias `--repo`); with `--remote`, not `--url` |
| `-j, --jobs <N>` | sync, switch, run | default min(cores, 8) |
| `--recurse-submodules` | sync | clone/update each repo's git submodules, pinned to the superproject |
| `--skip-branch` | change start | adopt current branches (RepoFleet) |
| `--branch <B>` | change start | default `change/<id>` |

## TUI keymap

k9s/lazygit-style, keyboard-first. Three mechanisms carry everything:

- **digits `1`–`7` switch views** (from any top-level list view),
- **`a` opens the current view's context actions** (a lazygit-style menu),
- **`:` is the command bar** for the rest — its verbs mirror the CLI (learn one, know both).

Data loads on a background worker — the UI never blocks. The fleet grid auto-refreshes
every ~5s while idle (never while you're typing, in an overlay, a confirm, or a job is in
flight); `F5` / `ctrl-r` refresh on demand.

**Global (frozen — these keys mean the same thing in every view)**

| Key | Action |
|-----|--------|
| `↑`/`↓` or `k`/`j` | move cursor (in a drill-in: scroll one line) |
| `enter` | drill in (stack → fleet → repo/PR/CI detail) · confirm a `y/n` prompt |
| `esc` / `b` / `⌫` | clear an active filter, else go back one level |
| `q` | quit · `ctrl-c` force-quit |
| `/` | fuzzy filter the grid (live, case-insensitive: `/knl` → `kernel`) |
| `:` | command bar (mirrors the CLI verbs, see below) |
| `?` | help overlay |
| `F5` / `ctrl-r` | refresh now |
| `ctrl-d` / `ctrl-u` | half-page down / up · `PageUp` / `PageDown` full page |
| `g` | goto — quit and print the cursor repo's path (`cd "$(haw dash)"`) |
| `w` | toggle watch — auto-refresh the fleet & the open PR/CI view |
| `space` | mark / unmark the cursor repo (Fleet & Changeset only; shown as `◉`) |

**View jumps (`1`–`7`) — from any list view**

| Key | View | `:` alias |
|-----|------|-----------|
| `1` | fleet | `:fleet` |
| `2` | changesets | `:changesets` |
| `3` | PR/MRs | `:prs` |
| `4` | CI runs | `:ci` |
| `5` | tree | `:tree` |
| `6` | governance | `:governance` |
| `7` | plugins | `:plugins` |

Digits are inert in the scroll/detail views (repo/PR/CI detail, files, grep) — jump from a
list. Sorting (`<`/`>`/`.`) applies to the Fleet, PR/MR, and CI tables.

**Fleet view**

| Key | Action |
|-----|--------|
| `s` | sync — the marked repos if any, else the cursor repo, else the stack |
| `space` | mark / unmark the cursor repo (shown as `◉`) |
| `r` | run a command — across the marked repos if any, else the whole fleet |
| `p` | problems-only filter (⚠ dirty / drift / behind / missing) |
| `x` | drop into a shell in the cursor repo (exits the cockpit) |
| `f` | browse the cursor repo's files (local disk or forge) |
| `!` | run one shell command in the cursor repo (in its detail view) |
| `enter` | drill into the cursor repo's git detail (branch, SHA, status, log, diffstat, remotes) |

Switch-stack, lock, and git-fetch moved to the command bar: `:stack` (picker) / `:stack NAME`,
`:lock`, `:fetch`. Pinning the lock is `p` in the **Stacks** view (or `:pin`).

Marks persist across the Fleet and Changeset views; with marks set, both `s` (sync)
and `r`/`:run` act on just the marked set.

**Fleet PR/MR view (`3`) and CI view (`4`)**

| Key | Action |
|-----|--------|
| `enter` | drill in — PR/MR: reviewers, checks, body, url · CI run: jobs, steps, conclusion |
| `a` | actions menu — PR/MR: `m` merge · `a` approve · `c` checkout (each asks `y/n`) |
| `d` | read the PR/MR's diff (scrollable) |
| `l` | read the CI run/pipeline's logs (scrollable) |
| `f` | browse the PR/MR's changed files (PR-files view) |
| `o` | open the cursor row in your browser |
| `<` `>` `.` | sort the table |
| `b` / `esc` | back |

`a` (actions) and `d` are also available from within a PR/MR drill-in. Refetch is now just
`F5` / `ctrl-r`.

**Files view (`f` from a repo)**

A read-only browser: view or pick a file at any ref, on local disk or the forge (GitHub /
GitLab / Bitbucket). It never stages/commits — it only *reads*. Two modes share the same
repo / ref / scope context and toggle with `T`: a flat one-directory list (default) and a
navigable expandable **tree**.

| Key | Action |
|-----|--------|
| `enter` | open a directory, or view a file's content (scrollable) |
| `T` | toggle to the tree view (and back) |
| `r` | ref picker — read files AS OF a chosen branch / tag / SHA |
| `e` | edit the file under the cursor in `$EDITOR` (local files only) |
| `R` | toggle between the local-disk tree and the forge view |
| `b` / `esc` | up a directory, then back to the fleet |
| `x` | drop into a shell in the repo |

**Tree view (`T` from Files)**

| Key | Action |
|-----|--------|
| `enter` / `→` | expand the directory (or open the file) under the cursor |
| `←` | collapse the directory (or jump to and collapse its parent) |
| `r` | ref picker (same as Files) |
| `T` | back to the flat list |
| `R` | toggle local ⇄ forge |
| `b` / `esc` | back to the fleet |

The tree fetches every file path of the repo at the active ref once, then expands/collapses
client-side. Collapsed dirs show `▸`, expanded `▾`; files are indented under their parents.

**Ref picker (`r` in either mode)**

`r` opens a popup listing the repo's branches then tags (`j`/`k` + `enter` to pick), plus an
input row to type an arbitrary ref or commit SHA. Selecting a ref reloads the current view AS
OF that ref (the flat list re-roots at the repo root; the tree re-fetches its paths). The panel
title shows the active ref honestly: `@ main`, `@ v1.0.0`, `@ a1b2c3d`, or `@ HEAD` (local) /
`@ default` (remote) when none is pinned. Local refs come from `git for-each-ref` / `git
ls-tree` / `git show <ref>:<path>`; forge refs and trees come from each forge's REST API.

`e` suspends the cockpit, hands the current TTY to `$VISUAL`/`$EDITOR` (falling back to
`nvim`/`vim`/`vi`) on the file's absolute path, then resumes and reloads the listing. It is
declined on the forge view (`R`) and on directories; if the repo isn't on disk it prompts to
sync.

**Errors view, Plugins view, Governance view**

Reach them from a list view (Errors via `:errors`/`:err`, Plugins via `7`/`:plugins`,
Governance via `6`/`:governance`). In Governance, `o` opens the cursor plugin's artifact
(SBOM / provenance / …). Refetch is `F5` / `ctrl-r`; `b` / `esc` go back.

**Changeset view**

| Key | Action |
|-----|--------|
| `n` | new changeset |
| `space` | select / deselect a repo |
| `a` | actions menu — `r` request cross-linked PR/MRs (selected, or all if none) · `l` land in dependency order (each asks `y/n`) |
| `g` | goto the cursor repo |

**Actions menu (`a`)**

`a` opens a bordered ` actions ` popup listing the current view's context actions, each
with its sub-key. Pressing a listed sub-key fires that action — write actions keep their
`y/n` confirm gate. `esc` (or any unlisted key) cancels. Views with no actions report so.

**Command bar (`:`)**

Verbs mirror the CLI, and the status line echoes the exact command each one runs, so
the TUI doubles as a way to discover the CLI.

| Command | Action |
|---------|--------|
| `:stack` | open the switch-stack picker (alias `:stacks`) |
| `:stack NAME` / `:switch NAME` | switch to a stack |
| `:lock` | commit the lock (resolve revs → SHA) |
| `:fetch` | git fetch the cursor repo |
| `:errors` / `:err` | errors view — failures collected across the fleet |
| `:fleet` / `:changesets` / `:tree` | view jumps (same as `1` / `2` / `5`) |
| `:prs` / `:ci` | fleet-wide PR/MR / CI views (same as `3` / `4`) |
| `:governance` / `:plugins` | governance / plugins view (same as `6` / `7`) |
| `:sync` | sync the current stack |
| `:run CMD` | run a command (across marked repos in the Fleet, else the fleet) |
| `:build` / `:test` / `:verify` | fleet build / test / drift-verify |
| `:pin` / `:lock` | pin HEADs / commit the lock |
| `:change [ID \| start ID \| land ID \| request ID]` | changeset workflow |
| `:merge [cleanup <repo> \| abort <repo>]` | list / seal / abort in-progress merges |
| `:grep <pat>` | fan-out grep across every repo |
| `:sh CMD` | run a shell command in the cursor repo |
| `:problems` | toggle the problems-only filter (⚠ dirty/drift/behind/missing) |
| `:watch` | toggle watch auto-refresh (same as `w`) |
| `:<repo>` | jump the fleet cursor to a repo whose name matches |
| `:theme [NAME]` | switch skin live (no arg lists the built-ins) |
| `:help` | help overlay |

**Themes / skins**

Six built-in skins: `catppuccin` (default), `dracula`, `nord`, `gruvbox`,
`solarized`, `monochrome`. `NO_COLOR` forces `monochrome`; `HAW_THEME=<name>` selects
one at startup; `:theme <name>` switches live.

## Shipped since this design was written

- `haw pin` / `haw unpin` (aliases `freeze`/`unfreeze`).
- `--label <L>` on `change start`, forwarded to PR/MRs at `change request`.
- `forge = "github" | "gitlab"` key on `[remote.X]` for hosts the URL heuristic misses.
- `deps = [...]` on a repo — `change land` merges in stable topological order.
- `haw verify`, `haw sync --locked`, `--format json` on status/tree, exit 3 on drift.
- `haw build` / `haw test` (per-repo commands in the manifest), lifecycle hooks in
  `.haw/hooks/`, `haw hooks install`, `haw evidence`, `haw-<name>` plugins.
- Lexicon nuance: `--slug` on `repo add` accepts `--repo` as alias; `haw run` takes the
  command positionally (`forall -c` still works).
- TUI `g` (goto) quits and prints the repo path — `cd "$(haw dash)"` — instead of
  spawning a nested shell.
- TUI: live idle auto-refresh (~5s), fuzzy `/` filter (nucleo), column sorting
  (`<`/`>`/`.`), marks + bulk `s`/`r`, drill-ins for repos/PRs/CI runs, the `a`
  actions menu (merge / approve / checkout in PR/MR; request-PR / land in Changeset),
  fleet-wide governance (`6`) view, the file browser (`f`) with a navigable tree (`T`),
  ref picker (`r`), and local edit (`e`), and six themes (`HAW_THEME`, `NO_COLOR`,
  live `:theme`).

## Planned (not yet implemented)

- Tag conveniences: `haw lock --as-of <tag>`; `haw status` marking `rev` kind (branch/tag/sha).
- `haw auth login` — OAuth device flow + OS keychain (see ARCHITECTURE DR-14).
- TUI: mouse support.
