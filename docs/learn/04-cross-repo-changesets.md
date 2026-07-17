# 4. Cross-repo changesets

This is hawser's signature feature — the one that pays for the whole tool. In this chapter
you'll take *one* feature that touches *several* repos and drive it as a single unit: one
branch across all of them, cross-linked pull requests, and a merge that lands in the right
order.

<img class="chapter-illus" src="../assets/img/code-review.svg" alt="Reviewing cross-repo pull requests as one changeset">

*One feature, one review flow — even when the code is spread across repos and forges.*

<div class="objectives">
<strong>🎯 In this chapter, you'll learn to…</strong>
<ul>
<li>Understand what a <strong>changeset</strong> is: one feature = one branch across N repos.</li>
<li>Start a changeset with <code>haw change start</code> — the same branch in every repo, in one move.</li>
<li>Track the whole feature on one screen with <code>haw change status</code>.</li>
<li>Open cross-linked PR/MRs with <code>haw change request</code> — across GitHub, GitLab, and Bitbucket.</li>
<li>Merge in dependency order with <code>haw change land</code>, so <code>main</code> never breaks.</li>
</ul>
</div>

![Driving a cross-repo changeset from the CLI](../assets/cli-changeset.gif)

*One feature, many repos: `change start` branches them all, `status` aggregates, `request` opens linked PRs.*

## 😤 1. The pain this removes

Think back to the last time a feature spanned repos. You did this dance:

1. `git checkout -b feature` in repo A. And repo B. And repo C.
2. Push each. Open a PR in A. Then B. Then C.
3. Chase reviews across three separate PR pages.
4. Merge them — and remember that the shared library **must** merge *before* the services
   that depend on it, or `main` breaks.

Nothing tied those three PRs together. There was no single artifact that said "these are
one feature." A **changeset** is exactly that artifact: one feature = one branch across N
repos, with linked PR/MRs and an ordered land.

<div class="callout note">

Changesets need a real forge (GitHub, GitLab, or Bitbucket) and a token to open PRs.
The `start` and `status` steps are local and safe to try anywhere; `request` and `land`
talk to the forge. We'll flag which is which.

</div>

## 🌱 2. Start the changeset

You name the feature and (optionally) the repos it touches:

```bash
haw change start FEAT-42 --repos api,billing,proto
```

```console
changeset `FEAT-42` started across 3 repo(s):
  proto    -> change/FEAT-42
  billing  -> change/FEAT-42
  api      -> change/FEAT-42
```

In one move, `haw` created the **same branch** — `change/FEAT-42` by default — in each of
the three repos. Now you make your changes and commit in each repo as normal Git. The
branch name ties them together.

A few useful options at start time:

- `--repos a,b,c` — limit to the repos the feature touches (default: all repos).
- `--branch <name>` — use a custom branch name instead of `change/<id>`.
- `--skip-branch` — adopt whatever branch each repo is already on, instead of creating one.
- `--label <l>` — attach a label (repeatable) that gets forwarded to the PR/MRs later.

## 📊 3. Watch it come together — `change status`

At any point, get the whole feature on one screen:

```bash
haw change status FEAT-42
```

This aggregates, per repo: the branch, whether it's dirty, its HEAD — and once PRs exist,
the **review state and CI status** of each PR/MR. Instead of three browser tabs, one
dashboard. It's the changeset equivalent of `haw status`.

<div class="callout tip">

**Tip:** Add `--format json` to `change status` (it emits a stable `haw.change-status/1`
document) when you want to pipe the state into another tool or a script.

</div>

## 🔀 4. Open the pull requests — `change request`

When your branches are pushed and ready, one command opens **cross-linked PR/MRs** — one
per repo — on whichever forge each repo lives on:

```bash
haw change request FEAT-42
```

Here's the quietly powerful part: your repos don't all have to be on the same forge.
`haw` speaks **GitHub, GitLab, *and* Bitbucket**, so a feature spanning a GitHub service
and a GitLab library gets a PR on GitHub *and* an MR on GitLab, cross-linked, from this
one command. Any labels you passed at `start` are forwarded here. Target a specific base
branch with `--base <branch>`.

<div class="callout warning">

This step needs a forge token in your environment — e.g. `export GITHUB_TOKEN=$(gh auth
token)`. `haw` reads tokens from env vars only, never stores them. Read-only steps
(`start`, `status`) need no token.

</div>

## 🛬 5. Land in dependency order — `change land`

Reviews are in, checks are green. Now merge — but in the *right order*. Remember `proto`
is a shared library that `api` and `billing` depend on. Merging a service before its
library lands is how you break `main`.

`haw` knows the order because your manifest declares it. Recall the `deps` key from the
microservices example:

```toml
[repo.gateway]
deps = ["proto"]     # proto must land before gateway
```

So `land` merges the PR/MRs in **stable topological order** — dependencies first — and
stops at the first failure rather than leaving a half-merged mess:

```bash
haw change land FEAT-42
```

```console
landing changeset `FEAT-42` in dependency order:
  proto    ✓ merged
  billing  ✓ merged
  api      ✓ merged
changeset `FEAT-42` landed.
```

`proto` merged first because everything depends on it; the services followed. One command,
correct order, no broken `main`.

![Merging a changeset in dependency order](../assets/cli-merge.gif)

*`change land` merges the linked PR/MRs in topological order — dependencies first — and stops at the first failure.*

## 🖼️ 6. The value, in one picture

```text
        one feature (FEAT-42)
                │
   ┌────────────┼────────────┐
 proto        billing        api          ← change start:  one branch across N repos
   │            │             │
 MR/PR        PR             PR            ← change request: cross-linked, any forge
   │            │             │
   └──── land in deps order ──┘            ← change land:  proto → billing → api
```

You never lost track of which PRs were "the feature," you never merged in the wrong order,
and you drove all of it from four commands. That's the whole point of a changeset:
**a multi-repo feature that behaves like a single, coherent change.**

<div class="callout tip">

**Tip:** Working across repos and want to hop into one? `haw change goto FEAT-42 <repo>`
prints its path so you can `cd "$(haw change goto FEAT-42 api)"`. And `haw change
snapshot save <name>` records every repo's branch + HEAD so you can restore the exact
multi-repo state later.

</div>

<div class="your-turn">
<strong>🙌 Your turn</strong>
<p>The local half of the flow needs no forge token, so try it in a workspace right now:</p>
<ul>
<li>Run <code>haw change start DEMO-1 --repos hello-world,spoon-knife</code> and confirm the same <code>change/DEMO-1</code> branch appears in both repos (peek with <code>haw run 'git branch --show-current'</code>).</li>
<li>Run <code>haw change status DEMO-1</code> and read the aggregated branch/dirty/HEAD — one dashboard instead of two tabs.</li>
<li>Now sketch it on paper: for a real feature spanning a shared lib and two services, in what order must they <code>land</code>? (Lib first — that's the <code>deps</code> key doing its job.)</li>
</ul>
</div>

## ✅ Recap

- A **changeset** is one feature across N repos: a shared branch, linked PR/MRs, an
  ordered merge.
- `haw change start <id> --repos a,b,c` creates the same branch in each repo.
- `haw change status <id>` aggregates branch + review + CI across the whole feature.
- `haw change request <id>` opens cross-linked PR/MRs on **GitHub, GitLab, and Bitbucket**.
- `haw change land <id>` merges in **dependency order** (from each repo's `deps`), stopping
  on the first failure.
- `start`/`status` are local; `request`/`land` need a forge token in the environment.

## 👉 Next

You've driven the changeset from the CLI. Now meet the cockpit that does all of this —
and more — from the keyboard → [5. The TUI cockpit](05-the-tui-cockpit.md).
