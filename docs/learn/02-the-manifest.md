# 2. The manifest

Everything `haw` does starts from one file: `haw.toml`, the **manifest**. It's your
*intent* — which repos exist, where they live, which revision you want, and how they
compose into stacks. In this chapter you'll write a real one, top to bottom, and build the
mental model. We won't sync yet — that's the next chapter. Here we get the *declaration*
right first.

<img class="chapter-illus" src="../assets/img/building-blocks.svg" alt="Composing a stack from building blocks">

*A stack is just building blocks — repos snapped together under one name, declared in the manifest.*

<div class="objectives">
<strong>🎯 In this chapter, you'll learn to…</strong>
<ul>
<li>Declare a <strong>remote</strong> once and reuse it across repos.</li>
<li>Add <strong>repos</strong>, each with a <code>rev</code> (branch, tag, or SHA) and free-form <code>groups</code>.</li>
<li>Compose repos into named <strong>stacks</strong> — shared, never copied.</li>
<li>Reach for <strong>overlays</strong> when one repo needs per-variant overrides.</li>
<li>Read a whole <code>haw.toml</code> and know exactly what it will do.</li>
</ul>
</div>

## 🛠️ 1. Create a workspace

Make an empty directory and drop in a manifest. We'll use small, real public repos from
GitHub's `octocat` account — the same ones the shipped
[`examples/quickstart`](https://github.com/Nastwinns/hawser/tree/main/examples/quickstart)
uses — so that next chapter you can actually `haw sync` this over HTTPS with no
authentication.

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

Read that top to bottom, because it *is* the mental model. Let's take it one block at a
time.

## 🌐 2. The remote — name a base URL once

```toml
[remote.gh]
url = "https://github.com/octocat"
forge = "github"
```

A `[remote.NAME]` names a base URL once so you don't repeat it on every repo. `forge`
tells `haw` which API to speak later (for PRs, CI, and so on) — `github`, `gitlab`, or
`bitbucket`. You can declare several remotes; a fleet can span forges, and later a
changeset can open a PR on one and an MR on another from a single command.

## 📦 3. The repos — one declaration each

```toml
[repo.hello-world]
remote = "gh"
repo = "Hello-World.git"
rev = "master"
groups = ["core"]
```

Each `[repo.NAME]` declares one repository:

- `remote` + `repo` — which remote it lives on and its path there. Combined, they resolve
  to `https://github.com/octocat/Hello-World.git`.
- `rev` — **the revision you want**: a branch (`master`), a tag (`v6.1.2`), or an exact
  SHA. `haw` auto-detects which kind it is. This is your *intent* — a moving branch until
  the lockfile freezes it (next chapter).
- `groups` — free-form labels you'll filter commands by later (e.g. `haw status --group
  core`). A repo can be in several groups.

<div class="callout note">

**`rev` is intent, not a pin.** Writing `rev = "master"` means "track master." Nothing is
frozen until you sync and a **lockfile** records the exact commit — that's the whole
reproducibility story, and it's the entire point of Chapter 3.

</div>

## 🧱 4. Stacks — compose repos under a name

```toml
[stack.site]
repos = ["hello-world", "spoon-knife"]
description = "A shared core repo plus the fork-demo front end."
```

A **stack** is a named composition of repos. It doesn't copy anything — it just says
"these repos, together, under this name." One manifest can define several stacks that
*share* the same repos:

```toml
[stack.site]
repos = ["hello-world", "spoon-knife"]

[stack.core-only]
repos = ["hello-world"]
```

`hello-world` appears in both stacks, but there's only ever one clone of it on disk.
Stacks are how you carve a big fleet into the working sets you actually build and test
together — you'll `haw switch <stack>` between them, and scope commands to one with
`--stack`.

## 🧬 5. Overlays — per-variant overrides (when you need them)

Most repos need nothing more than the fields above. But sometimes *one* repo should look
slightly different in a particular composition — a different branch for a release variant,
say. That's an **overlay**: a named set of per-repo overrides applied at lock time.

```toml
[repo.app-mqtt]
remote = "gh"
repo = "app-mqtt.git"
rev = "main"

# Override app-mqtt's rev only when the `release` overlay is active.
[overlay.release.app-mqtt]
rev = "release/2.x"
```

With the `release` overlay active, `app-mqtt` locks to `release/2.x` instead of `main`;
everything else stays put. Reach for overlays only when a repo genuinely needs to vary by
variant — for the rest of this course we won't need one. See
[CLI design](../CLI-DESIGN.md) for the full overlay semantics.

<div class="callout tip">

**Tip:** You don't have to hand-write every repo. `haw repo add` and `haw stack add`
edit the manifest for you, and `haw import` converts an existing `west.yml` or
Google-`repo` `default.xml` straight into a `haw.toml`.

</div>

## 🗺️ 6. The whole picture

Here's the manifest as a shape — intent flowing down into stacks:

```text
haw.toml
├─ [remote.gh]        base URL + forge, named once
├─ [repo.hello-world] remote + repo + rev + groups   ┐
├─ [repo.spoon-knife] remote + repo + rev + groups   ├─ the repos (your intent)
│                                                    ┘
└─ [stack.site]       repos = [hello-world, spoon-knife]   ← a named composition
```

That's the complete mental model of the manifest: **remotes** name where, **repos**
declare what and which `rev`, **groups** slice them, **stacks** compose them, and
**overlays** tweak them per variant. Nothing here has touched the network or the disk yet —
it's pure declaration.

<div class="callout success">

**You just declared a fleet.** A remote, two repos, and a stack — that's a real,
syncable `haw.toml`. It scales the same way to a hundred repos and a dozen stacks.

</div>

<div class="your-turn">
<strong>🙌 Your turn</strong>
<p>Make the manifest your own — no sync required, this is all declaration:</p>
<ul>
<li>Add a third repo to <code>haw.toml</code> — try <code>octocat/git-consortium.git</code> on the <code>gh</code> remote — give it a <code>rev</code> and put it in a new group.</li>
<li>Add it to the <code>site</code> stack's <code>repos</code> list, then add a second stack, <code>core-only</code>, that lists just <code>hello-world</code>. Notice the repo is <em>shared</em>, not copied.</li>
<li>Sketch on paper what you expect on disk after a sync: how many clones? (Hint: one per <em>repo</em>, no matter how many stacks reference it.)</li>
</ul>
</div>

## ✅ Recap

- The **manifest** (`haw.toml`) is your *intent* — nothing is cloned or frozen until you
  sync.
- `[remote.NAME]` names a base URL + forge once; repos reference it.
- `[repo.NAME]` declares one repo with a `rev` (branch/tag/SHA, auto-detected) and
  free-form `groups`.
- `[stack.NAME]` composes repos under a name — **shared, never copied**; a repo can be in
  many stacks and groups.
- `[overlay.…]` applies per-variant overrides at lock time, for the rare repo that must
  vary.

## 👉 Next

You've declared the fleet. Now let's make it real — clone it, freeze it to a lockfile, and
watch `haw` catch drift → [3. Sync and the lockfile](03-sync-and-the-lockfile.md).
