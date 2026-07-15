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
| **lock / pin** | resolved SHA in `keel.lock` | `freeze` (planned rename: `keel pin` / `keel unpin`) |
| **drift** | HEAD differs from the locked SHA | — |

Old spellings (`brick`, `product`, `bricks`, `--product`, `--bricks`) parse forever as
aliases; serialization and docs use the new words only.

## Verbs (commands)

Rule: one guessable verb per action, git-adjacent, no jargon. Old names kept as hidden
aliases so nothing breaks.

| Verb | Does | Alias (accepted) |
|------|------|------------------|
| `keel init <path>` | bootstrap a workspace from a manifest | — |
| `keel sync` | materialize the tree to `keel.lock` (writes lock if absent) | — |
| `keel tree` | print the stack → repo tree | `graph` |
| `keel status` | fleet status: branch, head, dirty, drift per repo | `st` |
| `keel run '<cmd>'` | run a command in every repo, in parallel (positional) | `forall` (with `-c`) |
| `keel lock` | resolve every repo's rev → SHA into `keel.lock` | — |
| `keel pin` | pin `keel.lock` to current checkouts (no network) | `freeze` |
| `keel unpin` | restore `keel.lock` to manifest revs | `unfreeze` |
| `keel switch <stack>` | record a stack as current and sync it | — |
| `keel repo add\|remove\|list` | edit the repos of the manifest | `brick` |
| `keel stack add\|remove\|list` | edit the stacks of the manifest | `product` |
| `keel change start\|status\|list` | cross-repo feature (changeset) workflow | — |
| `keel` (no args) / `keel dash` | open the TUI cockpit | `tui` |

`keel run` takes the command positionally (`keel run 'git fetch'`); `-c/--command` still works
via the `forall` alias. Running `keel` with no subcommand opens the dashboard (like `htop`,
`k9s`).

## Rev handling (user-friendly by default)

- One field: `rev = "main" | "v6.1.2" | "<40-hex sha>"`. No `type =` key; the kind is
  detected (`refs/heads` > peeled tag > tag > full SHA).
- Display: SHAs are shown 8 chars everywhere; `keel.lock` stores the full 40.
- Never detached: branch revs check out on a same-name branch, tags/SHAs on `keel/<rev>`.

## Groups (implemented)

- `groups = ["firmware", "ci"]` on a repo.
- `keel sync --group firmware`, `keel status --group ci`, `keel forall --group firmware -c ...`
  (repeatable; empty filter = everything; a filter excludes ungrouped repos).
- Groups are recorded in `keel.lock` so filtering works offline.

## Options grid

| Option | Commands | Note |
|--------|----------|------|
| `--stack <S>` | sync, tree | alias `--product`; default: last `switch`, else the only stack |
| `--overlay <O>` | lock, sync*, tree | repeatable, later wins; *sync only when generating the lock |
| `--group <G>` | sync, status, run | repeatable |
| `--repos a,b` | change start | alias `--bricks` |
| `--slug <S>` | repo add | repo path under `--remote` (alias `--repo`); with `--remote`, not `--url` |
| `-j, --jobs <N>` | sync, switch, run | default min(cores, 8) |
| `--skip-branch` | change start | adopt current branches (RepoFleet) |
| `--branch <B>` | change start | default `change/<id>` |

## TUI keymap

k9s-style, keyboard-first. Every action is a single key on the cursor row; `:` opens a
command bar whose verbs mirror the CLI (learn one, know both).

**Global**

| Key | Action |
|-----|--------|
| `↑`/`↓` or `k`/`j` | move cursor |
| `enter` | drill in (stack → repos → repo detail) |
| `esc` / `b` | back / up one level |
| `/` | filter the grid (live) |
| `:` | command bar (`:sync`, `:stack sensor-node`, `:run git status`, `:change FEAT-9`) |
| `r` | run a command across repos (`:run …`) |
| `?` | help overlay |
| `q` / `ctrl-c` | quit |

**Fleet view**

| Key | Action |
|-----|--------|
| `s` | sync (cursor repo, or whole stack from the header) |
| `S` | switch stack |
| `p` | pin lock to current checkouts |
| `l` | lock (resolve revs → SHA) |
| `t` | tree view |
| `c` | change menu (start / open a changeset) |
| `g` | goto — drop to a shell in the cursor repo |

**Changeset view**

| Key | Action |
|-----|--------|
| `n` | new changeset |
| `space` | select / deselect a repo |
| `R` | request — open PR/MR for selected repos |
| `L` | land — merge PR/MRs in dependency order |
| `g` | goto the cursor repo |

Command bar mirrors the verbs table above, so nothing new to learn. The bar echoes the exact
CLI command it runs, so the TUI doubles as a way to discover the CLI.

## Shipped since this design was written

- `keel pin` / `keel unpin` (aliases `freeze`/`unfreeze`).
- `--label <L>` on `change start`, forwarded to PR/MRs at `change request`.
- `forge = "github" | "gitlab"` key on `[remote.X]` for hosts the URL heuristic misses.
- `deps = [...]` on a repo — `change land` merges in stable topological order.
- `keel verify`, `keel sync --locked`, `--format json` on status/tree, exit 3 on drift.
- `keel build` / `keel test` (per-repo commands in the manifest), lifecycle hooks in
  `.keel/hooks/`, `keel hooks install`, `keel evidence`, `keel-<name>` plugins.
- Lexicon nuance: `--slug` on `repo add` accepts `--repo` as alias; `keel run` takes the
  command positionally (`forall -c` still works).
- TUI `g` (goto) quits and prints the repo path — `cd "$(keel dash)"` — instead of
  spawning a nested shell.

## Planned (not yet implemented)

- Tag conveniences: `keel lock --as-of <tag>`; `keel status` marking `rev` kind (branch/tag/sha).
- `keel auth login` — OAuth device flow + OS keychain (see ARCHITECTURE DR-14).
- TUI: mouse support, themes beyond `NO_COLOR`, live ahead/behind refresh.
