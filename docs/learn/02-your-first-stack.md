# 2. Your first stack

This is where it clicks. In this chapter you'll write a tiny `haw.toml`, sync it against
**real public GitHub repos**, explore the fleet, read the lockfile, and — deliberately —
break something to see how `haw` catches drift.

Everything here clones over HTTPS with no authentication, so you can actually run every
command as you read. Grab a terminal.

<img class="chapter-illus" src="../assets/img/building-blocks.svg" alt="Composing a stack from building blocks">

*A stack is just building blocks — repos snapped together under one name.*

<div class="objectives">
<strong>🎯 In this chapter, you'll learn to…</strong>
<ul>
<li>Write a real <code>haw.toml</code> with a remote, two repos, and a stack.</li>
<li>Run <code>haw sync</code> to clone the fleet and generate <code>haw.lock</code>.</li>
<li>Read the fleet with <code>haw tree</code> and <code>haw status</code>, and understand every column.</li>
<li>See how the lockfile pins exact SHAs — the whole reproducibility trick.</li>
<li>Cause <strong>drift</strong> on purpose and catch it with <code>haw verify</code>.</li>
</ul>
</div>

![Composing a stack: sync, tree, status, and the lockfile](../assets/cli-compose.gif)

*The full compose loop you're about to run: `sync` clones the fleet, then `tree` / `status` read it and the lock pins it.*

## 🛠️ 1. Create a workspace

Make an empty directory and drop in a manifest. We'll use three small, real public repos
from GitHub's `octocat` account — the same ones the shipped
[`examples/quickstart`](https://github.com/Nastwinns/hawser/tree/main/examples/quickstart)
uses.

```bash
mkdir my-first-stack && cd my-first-stack
```

Now create `haw.toml` with this content:

```toml
[remote.gh]
url = "https://github.com/octocat"
forge = "github"

[repo.hello-world]
remote = "gh"
repo = "Hello-World.git"
rev = "master"
groups = ["core"]

[repo.spoon-knife]
remote = "gh"
repo = "Spoon-Knife.git"
rev = "main"
groups = ["web"]

# `site` composes the two repos into one named stack.
[stack.site]
repos = ["hello-world", "spoon-knife"]
description = "A shared core repo plus the fork-demo front end."
```

Read that top to bottom, because it *is* the mental model:

- `[remote.gh]` names a base URL once, so you don't repeat it per repo.
- Each `[repo.NAME]` declares one repository and the `rev` you want — a branch here.
- `groups` are free-form labels you'll filter commands by later.
- `[stack.site]` composes those repos into a stack. A repo is *shared*, never copied.

## 🔄 2. Sync — clone everything and write the lock

One command materializes the tree:

```bash
haw sync
```

`haw` resolves each repo's `rev` to an exact commit, clones it, and — because there's no
lockfile yet — writes one. You'll see progress per repo and a summary. Afterwards, look
around:

```console
$ ls
haw.lock   haw.toml   hello-world/   spoon-knife/
```

There they are: two **real, complete Git clones**, plus a brand-new `haw.lock`. No
submodules, no symlinks — you could `cd hello-world && git log` and it's just Git.

<div class="callout tip">

**Tip:** `haw sync` is *idempotent*. Run it again and, since the lock already pins
exact SHAs, `haw` just makes sure your tree matches — no surprises, safe to repeat in
scripts and CI.

</div>

## 🔍 3. Explore the fleet

Now the read commands. First, the shape of things:

```bash
haw tree
```

```console
haw.toml
└─ site
   ├─ hello-world  master  (https://github.com/octocat/Hello-World.git)
   └─ spoon-knife  main    (https://github.com/octocat/Spoon-Knife.git)
```

That's your stack → repo tree: the stack `site`, the two repos under it, each with its
declared rev and origin.

Now the health check:

```bash
haw status
```

```console
REPO          BRANCH   HEAD       DIRTY  DRIFT
hello-world   master   7fd1a60b   -      -
spoon-knife   main     d0dd1f61   -      -
```

Read the columns left to right: the repo, the branch it's on, its short HEAD SHA, whether
the working tree has uncommitted changes (**DIRTY**), and whether HEAD differs from the
locked SHA (**DRIFT**). Right now everything is clean and in sync — the dashes mean "all
good."

<div class="callout tip">

**Tip:** On a terminal these are color-coded — cyan repo names, yellow revs, green for
clean, red for drift. Pipe the output anywhere and it falls back to plain text
automatically (it honors `NO_COLOR`), so it's script-friendly by default.

</div>

## 🔒 4. Read the lockfile

Open `haw.lock` in your editor. You'll see each repo pinned to a **full 40-character
SHA** — not the branch name, the exact commit:

```toml
# excerpt — your SHAs will differ
[[repo]]
name = "hello-world"
rev  = "master"
locked = "7fd1a60b01f91b314f59955a4e4d4e80d8edf11d"
```

This is the whole reproducibility trick. Your manifest said "master" (a moving target),
but the lock froze the *exact* commit master pointed at when you synced. Commit
`haw.lock` alongside `haw.toml`, and anyone who clones and runs `haw sync` gets
**precisely these commits** — not whatever `master` happens to be today.

<div class="callout warning">

**Don't hand-edit it.** `haw.lock` is generated. You commit it, but you change intent in
`haw.toml` and let `haw lock` / `haw sync` regenerate the lock.

</div>

## ♻️ 5. Prove it's reproducible — re-run sync

```bash
haw sync
```

Because the lock exists, `haw` syncs *to the pinned SHAs*, not to wherever the branches
have moved. Run `haw status` again and you'll see the same clean fleet. That
idempotence is the point: the lock, not the branch, is the source of truth now.

## 🧭 6. See drift with your own eyes

Reproducibility is only useful if you can *detect* when the tree wanders off the baseline.
Let's cause that on purpose. Move one repo to a different commit by hand:

```bash
cd hello-world
git checkout HEAD~1     # step one commit back — now HEAD ≠ the locked SHA
cd ..
```

Ask `haw` what it thinks:

```bash
haw status
```

```console
REPO          BRANCH   HEAD       DIRTY  DRIFT
hello-world   master   553c2077   -      YES
spoon-knife   main     d0dd1f61   -      -
```

There it is — **DRIFT: YES** on `hello-world`. Its HEAD no longer matches the locked SHA.
`haw status` flagged it, but for CI you want a command that *fails* on drift. That's
`verify`:

```bash
haw verify
```

`verify` asserts the on-disk tree matches `haw.lock` and **exits 3** when it doesn't —
a clean, scriptable drift gate. Check the exit code:

```bash
haw verify; echo "exit code: $?"
```

```console
exit code: 3
```

That exit `3` is what a CI pipeline keys on: "the tree drifted from the lock — stop the
build." (When everything matches, `verify` exits `0`.)

Now put it back. `haw sync` restores every repo to its locked SHA:

```bash
haw sync
haw verify; echo "exit code: $?"
```

```console
exit code: 0
```

Clean again. You just watched the full loop: **declare → sync → pin → detect drift →
restore.**

<div class="callout success">

**You just did the core loop.** Declare intent, pin it, detect drift, and restore the
baseline — the same four moves scale from two octocat repos to a hundred-repo fleet.

</div>

<div class="your-turn">
<strong>🙌 Your turn</strong>
<p>Make the stack your own and watch <code>haw</code> react:</p>
<ul>
<li>Add a third repo to <code>haw.toml</code> — try <code>octocat/git-consortium.git</code> — put it in a new group, and add it to the <code>site</code> stack. Run <code>haw sync</code> again and confirm it appears in <code>haw tree</code>.</li>
<li>Open <code>haw.lock</code> and find the new repo's pinned SHA. Notice it froze the exact commit, not the branch name.</li>
<li>Drift it on purpose (<code>git checkout HEAD~1</code> inside it), run <code>haw verify; echo $?</code>, and confirm you get exit code <code>3</code>. Then <code>haw sync</code> to restore.</li>
</ul>
</div>

## ✅ Recap

- A `haw.toml` declares **remotes**, **repos** (each with a `rev`), and **stacks**.
- `haw sync` clones every repo and writes `haw.lock` — real Git clones, no
  submodules/symlinks. It's idempotent.
- `haw tree` shows the stack→repo shape; `haw status` shows branch/HEAD/**dirty**/**drift**
  per repo.
- `haw.lock` pins each repo to an exact 40-char SHA — commit it for byte-identical
  rebuilds.
- **Drift** = HEAD differs from the lock. `haw status` flags it; `haw verify` **exits 3**
  on drift (your CI gate). `haw sync` restores the baseline.

## 👉 Next

You can compose and pin a fleet. Now let's *work* across it — run commands, builds, tests,
and searches everywhere at once → [3. The daily workflow](03-the-daily-workflow.md).
