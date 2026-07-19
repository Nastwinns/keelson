# 4. The TUI cockpit

Everything you've done at the command line has a home: a live, keyboard-driven cockpit —
your mission control for the whole fleet. It's in the spirit of `k9s` or `htop`: a
full-screen terminal UI where you *see* the fleet, *drill* into any repo or PR or CI run,
and *act* — merge, approve, checkout — without ever leaving the terminal.

You met it for a moment in Chapter 1. Now that you have a real fleet, let's take the full
guided tour. Open it and follow along.

<img class="chapter-illus" src="../assets/img/dashboard.svg" alt="The hawser TUI cockpit dashboard">

*Mission control for the whole fleet — read, drill, and act without leaving the terminal.*

<div class="objectives">
<strong>🎯 In this chapter, you'll learn to…</strong>
<ul>
<li>Open the cockpit with bare <code>haw</code> (or <code>haw dash --demo</code> to explore offline).</li>
<li>Read the live fleet grid — the same columns as <code>haw status</code>, refreshing themselves.</li>
<li>Work the core loop: <strong>read → drill (<code>Enter</code>) → act → back (<code>Esc</code>)</strong>.</li>
<li>Act from the home row: sync, mark, run, filter, browse files, and jump to the PR / CI / governance views with the digits <code>1</code>–<code>7</code>.</li>
<li>Open a view's context <strong>actions menu</strong> with <code>a</code> — merge, approve, checkout, request-PR, land — each confirm-gated.</li>
<li>Browse any repo's files at any branch, tag, or SHA — local <em>or</em> straight from the forge, no checkout.</li>
<li>Use the command bar (<code>:</code>) that mirrors the CLI you already learned.</li>
</ul>
</div>

![The hawser TUI cockpit — mission control for the fleet](../assets/haw-tui.gif)

*Your mission control: the fleet grid, drill-downs, PR/CI views, and keyboard actions — all in the terminal.*

## 🚁 1. Open it

Run `haw` with **no subcommand**:

```bash
haw
```

Just want to explore without a real workspace or network? Use the built-in demo
controller — it's populated with canned repos, PRs, and CI runs, so every view has
something to show:

```bash
haw dash --demo
```

Either way you land on the **fleet grid** — the cockpit's home screen.

## 📋 2. Read the fleet grid

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
 [s]ync [f]iles [x]shell [!]exec [/]filter [p]roblems [a]ctions [1-7]views [:]cmd [?]help
```

This is `haw status`, alive. Each row is a repo; the columns are the same ones you already
know — branch, HEAD, dirty, drift, ahead/behind. The `▸` is your cursor, `◉` marks a
selected repo, and `⚠` flags a problem (like the drift on `app-mqtt`). The grid
**auto-refreshes** about every 5 seconds while idle — never while you're typing — and
`F5` / `Ctrl-R` refresh on demand.

Move with `↑`/`↓` (or `k`/`j`), exactly like Vim.

## 🔬 3. Drill in — the core loop is read → drill → act

Put the cursor on a repo and press **`Enter`**. You drill into that repo's Git detail:
branch, SHA, working-tree status, recent log, diffstat, remotes. Press `Esc` (or `b`) to
come back up a level.

That's the rhythm of the whole cockpit: **read** the grid → **drill** into a thing → **act**
on it → back out. You're never more than a keystroke from detail or from action.

## ⌨️ 4. Act on the fleet from the home row

Single keys on the cursor row do things. The essentials:

| Key | Does |
|-----|------|
| `s` | **sync** — the marked repos if any, else the cursor repo, else the stack |
| `f` | browse the repo's **files** — local disk or straight from the forge |
| `x` | drop into a **shell** in that repo (exits the cockpit) |
| `!` | run one **command** (`exec`) in the repo, in its detail view |
| `/` | fuzzy **filter** the grid live — `/knl` narrows to `kernel` |
| `p` | **problems-only** view — just the repos that need attention |
| `Space` | **mark** / unmark the cursor repo (`◉`) |
| `r` | **run** a command — across the marked repos if any, else the whole fleet |
| `g` | **goto** — quit and print the cursor repo's path (`cd "$(haw dash)"`) |

Git-fetch, switch-stack, and lock moved to the command bar — `:fetch`, `:stack`
(picker) or `:stack NAME`, and `:lock`. One key you'll use everywhere is **`a`** — the
**actions menu**. It opens a little popup listing exactly the actions the current view
supports, each with its own sub-key; pick one and any *write* action (merge, land, …)
still asks you `y/n` first. Views with no actions just say so.

<div class="callout tip">

**Tip:** Marks are the cockpit's superpower. Press `Space` on a few repos, then `s`
(sync) or `r` (run) acts on *just that set*. It's how you do a surgical fleet operation
without touching a manifest.

</div>

## 🌐 5. The network views — PRs, CI, and acting on them

View-switching is one keystroke: the **digits `1`–`7`** jump straight to a view from any
top-level list. The network ones load on demand (nothing hits the network until you ask):

| Key | View | `:` alias |
|-----|------|-----------|
| `1` | fleet | `:fleet` |
| `2` | changesets | `:changesets` |
| `3` | **PR/MRs** — every open PR/MR across the fleet | `:prs` |
| `4` | **CI runs** — recent runs, live progress | `:ci` |
| `5` | tree | `:tree` |
| `6` | **governance** — plugins, SBOM, findings | `:governance` |
| `7` | **plugins** | `:plugins` |

Inside the PR/MR (`3`) or CI (`4`) views, `Enter` drills into detail — a PR's reviewers and
checks, or a CI run's jobs and live progress. Press `d` to read a PR's diff, `l` for a CI
run's logs, and `f` to browse a PR's **changed files**. (A CI log that the forge has aged
out — GitHub returns 410 — renders honestly as *"logs unavailable — expired or empty"*
rather than an error.)

To *act*, press **`a`** for the actions menu. In the PR/MR view it offers:

| Sub-key | Does |
|-----|------|
| `m` | **merge** the PR/MR on its forge |
| `a` | **approve** the PR/MR |
| `c` | **checkout** the PR branch locally |

Every write is **confirm-gated** with a `y/n` so you never merge by fat-finger. `o` opens
the cursor row in your browser. So the full cross-forge flow from Chapter 5 — see PRs,
approve, merge — is right here, keyboard-only, all under `a`.

## 📂 6. Browsing files & branches — any ref, local or forge

Press **`f`** on any repo to open its **file browser** — a *read-only* view (it never
stages or commits, only reads). Three keys turn it into a proper code explorer:

| Key | Does |
|-----|------|
| `T` | **toggle** the flat one-directory list ⇄ a navigable **tree** (`▸`/`▾` to expand/collapse, `→`/`Enter` expand, `←` collapse) |
| `r` | **ref picker** — pick a branch or tag from the list, or type any SHA; the view reloads *as of* that ref and the header shows `@ <ref>` |
| `e` | **edit** the file under the cursor in your `$EDITOR` (local files only) |
| `R` | **toggle** local disk ⇄ the forge |

The payoff: with `r` you can read a file **as it exists on any branch of the remote** —
across GitHub, GitLab, or Bitbucket — **without checking anything out**. Point at a
colleague's `feature/x` branch, read the file straight from the forge API, and never touch
your working tree. `e` is the one exception to read-only: it hands the file to your editor
for a quick local fix, then reloads the listing.

## 💬 7. The command bar — one language for CLI and TUI

Press **`:`** to open the command bar (the command palette). Its verbs *mirror the CLI you
already learned*, and the status line echoes the exact command each one runs — so the
cockpit doubles as a way to discover the CLI:

```text
:sync              sync the current stack
:grep TODO         fleet-wide grep
:switch platform   switch to another stack
:change land FEAT-42
:theme nord        change the skin live
```

Learn one, know both. `:name` also jumps the cursor to a repo by name.

## 🎨 8. Two more views, and themes

- **`7`** (`:plugins`) — the **Plugins** view: every available plugin (manifest `[plugins]`
  keys unioned with `haw-*` executables on your `PATH`); `Enter` runs one and shows its
  output in a panel. You'll build one that lands here in
  [Chapter 8](08-build-a-plugin-mcp.md).
- **`:errors`** (`:err`) — the **Errors** view: a rolling log of this session's failures, so
  a transient error never scrolls away before you can read it.

**Themes.** Six built-in skins — `catppuccin` (default), `dracula`, `nord`, `gruvbox`,
`solarized`, `monochrome`. Switch live with `:theme nord`, set one at startup with
`HAW_THEME=nord haw`, and `NO_COLOR` forces `monochrome`.

Press **`?`** any time for the help overlay, and `q` (or `Ctrl-C`) to quit.

<div class="callout tip">

**Tip:** Every heavy action runs on a background worker, so the UI never freezes while a
sync, a fetch, or a forge call is in flight. Keep navigating.

</div>

<div class="your-turn">
<strong>🙌 Your turn</strong>
<p>No workspace or network needed — the demo controller has everything to poke at:</p>
<ul>
<li>Launch <code>haw dash --demo</code>. Move with <code>↑</code>/<code>↓</code>, then press <code>Enter</code> to drill into a repo and <code>Esc</code> to come back — feel the read → drill → act → back rhythm.</li>
<li>Press <code>Space</code> to mark a couple of repos (watch the <code>◉</code>), then <code>/</code> to fuzzy-filter the grid. Press <code>3</code> for PRs and <code>4</code> for CI, then <code>a</code> to see the actions menu. Press <code>?</code> for the help overlay.</li>
<li>Press <code>f</code> on a repo to browse its files, then <code>T</code> for the tree and <code>r</code> to read a file as of another branch or tag. Press <code>b</code> to come back.</li>
<li>Press <code>:</code> and type <code>:theme nord</code>. The skin changes live — and the status line echoes the exact command, so the cockpit is teaching you the CLI as you go.</li>
</ul>
</div>

## ✅ Recap

- Bare `haw` (or `haw dash --demo`) opens the cockpit — a live `haw status` you can act on.
- The loop is **read → drill (`Enter`) → act → back (`Esc`)**.
- Fleet keys: `s` sync, `f` files, `x` shell, `!` exec, `/` filter, `p` problems, `Space`
  mark, `r` run, `g` goto, `a` actions.
- View jumps are the digits **`1`–`7`**: `1` fleet, `2` changesets, `3` PRs, `4` CI, `5`
  tree, `6` governance, `7` plugins. In PRs/CI, `a` (actions) does merge/approve/checkout
  (confirm-gated), `d` diff, `l` logs, `f` PR files.
- File browser (`f`): `T` toggles tree, `r` picks a branch/tag/SHA (read any ref, local or
  forge, no checkout), `e` edits locally, `R` toggles local ⇄ forge.
- `:` is a command bar (palette) mirroring the CLI; `:plugins` and `:errors` reach those
  views; six themes via `:theme` / `HAW_THEME`.

## 👉 Next

You've seen the whole cockpit read the fleet. Now the signature move it drives — shipping
*one feature across many repos* as a single coordinated changeset →
[5. Changesets across repos](05-changesets-across-repos.md).
