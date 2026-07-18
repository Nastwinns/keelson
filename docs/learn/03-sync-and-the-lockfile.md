# 3. Sync and the lockfile

This is where it clicks. You have a `haw.toml` from the last chapter. Now you'll run `haw
sync` to clone the fleet for real, read the lockfile that freezes it to exact commits,
prove it's reproducible, and — deliberately — break something to watch `haw` catch drift.

Everything here clones over HTTPS with no authentication, so you can actually run every
command as you read. Keep the `my-first-stack` workspace from Chapter 2 open.

<img class="chapter-illus" src="../assets/img/version-control.svg" alt="Syncing and pinning a fleet to a lockfile">

*Sync clones the fleet; the lockfile freezes it to exact commits — the whole reproducibility trick.*

<div class="objectives">
<strong>🎯 In this chapter, you'll learn to…</strong>
<ul>
<li>Run <code>haw sync</code> to clone the fleet and generate <code>haw.lock</code>.</li>
<li>Read the fleet with <code>haw tree</code> and <code>haw status</code>, and understand every column.</li>
<li>Read the lockfile's real fields — <code>rev</code> (the resolved SHA), <code>source-rev</code>, and <code>branch</code>.</li>
<li>Re-sync and confirm it's idempotent — the lock, not the branch, is now the truth.</li>
<li>Cause <strong>drift</strong> on purpose and catch it with <code>haw verify</code> (exit 3), then restore.</li>
</ul>
</div>

![Composing a stack: sync, tree, status, and the lockfile](../assets/cli-compose.gif)

*The full compose loop you're about to run: `sync` clones the fleet, then `tree` / `status` read it and the lock pins it.*

## 🔄 1. Sync — clone everything and write the lock

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

## 🔍 2. Explore the fleet

Now the read commands. First, the shape of things:

```bash
haw tree
```

```console
haw.toml
└─ site
   ├─ hello-world  master  (https://github.com/octocat/Hello-World.git)
   └─ spoon-knife  main  (https://github.com/octocat/Spoon-Knife.git)
```

That's your stack → repo tree: the stack `site`, the two repos under it, each with its
declared rev and origin.

Now the health check:

```bash
haw status
```

```console
REPO         BRANCH                   HEAD       DIRTY  DRIFT
hello-world  master                    7fd1a60b   -      -
spoon-knife  main                      d0dd1f61   -      -
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

## 🔒 3. Read the lockfile

Open `haw.lock` in your editor. Each repo is pinned to a **full 40-character SHA** in its
`rev` field — the exact commit, not the branch name — while `source-rev` records what you
*asked* for and `branch` records the branch that SHA came from:

```toml
# excerpt — your SHAs will differ
[[repo]]
name = "hello-world"
url = "https://github.com/octocat/Hello-World.git"
path = "hello-world"
rev = "7fd1a60b01f91b314f59955a4e4d4e80d8edf11d"   # the resolved commit — this is the pin
source-rev = "master"                               # what you declared in haw.toml
branch = "master"                                   # the branch that SHA was resolved from
groups = ["core"]
```

This is the whole reproducibility trick. Your manifest said "master" (a moving target),
but the lock froze the *exact* commit master pointed at when you synced — that's the SHA
now in `rev`. `source-rev` remembers your intent (`master`) and `branch` remembers where it
came from, so `haw` can later re-resolve if you ask. Commit `haw.lock` alongside
`haw.toml`, and anyone who clones and runs `haw sync` gets **precisely these commits** —
not whatever `master` happens to be today.

<div class="callout warning">

**Don't hand-edit it.** `haw.lock` is generated. You commit it, but you change intent in
`haw.toml` and let `haw lock` / `haw sync` regenerate the lock.

</div>

## ♻️ 4. Prove it's reproducible — re-run sync

```bash
haw sync
```

Because the lock exists, `haw` syncs *to the pinned SHAs*, not to wherever the branches
have moved. Run `haw status` again and you'll see the same clean fleet. That
idempotence is the point: the lock, not the branch, is the source of truth now.

## 🧭 5. See drift with your own eyes

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
REPO         BRANCH                   HEAD       DIRTY  DRIFT
hello-world  (detached)                553c2077   -      YES
spoon-knife  main                      d0dd1f61   -      -
```

There it is — **DRIFT: YES** on `hello-world`. Checking out a specific commit also
detaches HEAD (hence `(detached)` in the BRANCH column), and its HEAD no longer matches
the locked SHA.
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
  ✗ hello-world  drift (head != lock)
verify failed: 1 repo(s) diverge from haw.lock
exit code: 3
```

That exit `3` is what a CI pipeline keys on: "the tree drifted from the lock — stop the
build." (When everything matches, `verify` prints `verified: tree matches haw.lock (2 repos)` and
exits `0`.)

<img class="meme" src="https://media.giphy.com/media/IPjIcwdxtrNBIpL8f3/giphy.gif" alt="Wide-eyed, shocked reaction">

*`haw verify` finding drift in CI right before release.*

Now put it back. `haw sync` restores every repo to its locked SHA:

```bash
haw sync
haw verify; echo "exit code: $?"
```

```console
verified: tree matches haw.lock (2 repos)
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
<p>Prove the lockfile really is the source of truth:</p>
<ul>
<li>Open <code>haw.lock</code> and find each repo's pinned SHA in its <code>rev</code> field. Confirm it's a 40-char commit, not a branch name, and that <code>source-rev</code> still shows what you declared.</li>
<li>Drift one repo on purpose (<code>cd hello-world &amp;&amp; git checkout HEAD~1 &amp;&amp; cd ..</code>), run <code>haw verify; echo $?</code>, and confirm you get exit code <code>3</code>.</li>
<li>Run <code>haw sync</code> to restore, then <code>haw verify; echo $?</code> again — back to exit <code>0</code>. That round trip is the reproducibility guarantee.</li>
</ul>
</div>

## ✅ Recap

- `haw sync` clones every repo and writes `haw.lock` — real Git clones, no
  submodules/symlinks. It's idempotent.
- `haw tree` shows the stack→repo shape; `haw status` shows branch/HEAD/**dirty**/**drift**
  per repo.
- `haw.lock` pins each repo to an exact 40-char SHA in `rev`, with `source-rev` (your
  declared intent) and `branch` alongside — commit it for byte-identical rebuilds.
- **Drift** = HEAD differs from the lock. `haw status` flags it; `haw verify` **exits 3**
  on drift (your CI gate). `haw sync` restores the baseline.

## 👉 Next

You can compose and pin a fleet — now let's *live* in it. Meet the cockpit that drives the
whole thing from the keyboard → [4. The TUI cockpit](04-the-tui-cockpit.md).
