# 7. Going to production

You can compose a fleet, work across it, ship changesets, and extend `haw` with plugins.
This final chapter is about doing all of that *for real* — safely, in CI, with an audit
trail. It's the difference between a neat local tool and something you trust to gate
releases.

<img class="chapter-illus" src="../assets/img/launching.svg" alt="Taking haw to production">

*From a neat local tool to something you trust to gate releases — let's launch.*

<div class="objectives">
<strong>🎯 In this chapter, you'll learn to…</strong>
<ul>
<li>Internalize the <strong>trust model</strong> — the manifest is trusted code, tokens live only in the environment.</li>
<li>Wire the four-move CI pipeline: <strong><code>sync → verify → build → test</code></strong>.</li>
<li>Enforce reproducibility with <code>haw sync --locked</code> and <code>haw verify</code>.</li>
<li>Distribute artifacts with <code>haw publish</code>, and produce SBOM, provenance, and signatures.</li>
<li>Bundle an audit trail with <code>haw evidence</code> and map it to compliance standards.</li>
</ul>
</div>

## 🛡️ 1. First, the trust model — because it matters

Before you run `haw` on anything you didn't write, internalize one rule:

<img class="side-illus" src="../assets/img/secure-server.svg" alt="The hawser trust model">

*Treat a `haw.toml` like a Makefile: powerful, and only run one you trust.*

<div class="callout warning">

**The manifest is trusted code.** A `haw.toml`'s `build`, `test`, `run`, and `exec`
commands are executed through *your* shell.

</div>

Running `haw build`, `haw run`, or `haw sync` on an **untrusted** checkout is equivalent to
running its `Makefile`. Treat `haw.toml` exactly like a `Makefile` or a `package.json`
`scripts` block: **only run it on manifests you trust.**

Two corollaries you already half-know:

- **Plugins are trusted binaries.** `haw <name>` runs `haw-<name>` from your `PATH` with
  your full environment (including any tokens). Install only plugins you trust; keep
  `PATH` clean.
- **Tokens live in the environment only.** `haw` reads forge tokens from env vars at call
  time, and **never stores or logs them**. Git transport auth stays with your existing SSH
  keys / credential helper — `haw` doesn't touch it.

Read the full [trust model](../SECURITY.md) before you wire `haw` into anything shared.

## 🔧 2. The CI pipeline — always the same four moves

Here's the payoff of everything you learned in Chapters 2–3. A `haw` pipeline is the same
shape everywhere:

```text
sync  →  verify  →  build  →  test
```

- **`sync`** the tree to the SHAs pinned in `haw.lock`,
- **`verify`** that the tree matches the lock — the drift gate, **exit 3** on drift,
- **`build`** and **`test`** the whole fleet (each fails the job non-zero).

A GitHub Actions job, straight from the README:

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

GitLab CI is the identical four moves with `variables: { GITLAB_TOKEN: $CI_JOB_TOKEN }`.

<div class="callout tip">

**Tip:** On a big fleet, pair `haw sync --filter=blob:none` (partial clone — all history,
lazy blobs) with a cache of the shared object store. Clones stay fast *without* breaking
the pinned SHAs, because every commit is still reachable.

</div>

## ♻️ 3. Reproducibility, enforced

The whole pipeline rests on `haw.lock`. Two flags make it airtight in CI:

```bash
haw sync --locked        # fail unless haw.lock exists (no rev resolution)
haw verify               # assert tree == lock, exit 3 on drift
```

`--locked` refuses to invent a lock on the fly — CI must build from the *committed*
baseline, never resolve fresh. `verify` then proves the checkout matches it. Together they
guarantee the tree in CI is byte-for-byte the tree you committed.

## 📦 4. Distribution — publish what the fleet produced

Once the fleet builds, `haw publish` uploads its artifacts to a generic/raw artifact
registry — **Nexus, Artifactory, GitLab, or Bitbucket**:

```bash
haw publish dist/*.tar.gz --to nexus
haw publish dist/*.tar.gz --to nexus --dry-run   # print the plan, no creds, no network
```

`--dry-run` prints exactly what *would* upload (target, name, version, each file) without
touching the network or needing credentials — perfect for wiring the step up safely first.
Credentials come from the target's env vars (e.g. `NEXUS_URL`, `NEXUS_USER`, `NEXUS_PASS`).

## 🔏 5. Signing, SBOM, and provenance — the supply chain

This is where `haw` earns its keep for serious releases. Every signed release ships with a
`.sha256` checksum and a **keyless cosign signature** (`.sig`/`.pem`) you can verify
offline — even on an air-gapped host (see [Installing hawser](../INSTALL.md)).

For *your* fleet, the supply-chain features ship as the governance plugins you met in
Chapter 6, subscribed to lifecycle phases:

```toml
[plugins]
compliance = ["post-build"]    # SBOM (CycloneDX + SPDX) after a build
artifact   = ["post-land"]     # SLSA/in-toto provenance + cosign/minisign signing
gate       = ["pre-request"]   # secret/hygiene gate blocks a bad PR before it opens
```

- **SBOM** — a bill of materials (CycloneDX + SPDX) of what went into the build.
- **Provenance** — SLSA/in-toto records of *how* it was built and by whom.
- **Signing** — cosign/minisign signatures so consumers can verify authenticity.

And when someone asks "what exactly shipped?", `haw evidence` bundles the manifest, the
lock, the audit log, and status into one archive:

```bash
haw evidence -o haw-evidence.tar.gz
```

## 🏛️ 6. The compliance / automotive angle

If you work under a standard — ISO 26262, DO-178C, Automotive SPICE, CRA — the pieces
above *are* your evidence, because they're grounded in the pinned lock:

| Standard / artifact | How `haw` covers it |
|---|---|
| **Automotive SPICE** | `haw-aspice` emits repo → pinned SHA → process-area traceability |
| **MISRA C** | `haw-misra` runs `cppcheck --addon=misra` fleet-wide as a `pre-request` gate |
| **ISO 26262 / DO-178C** | `haw evidence` bundle + SBOM + provenance from the governance plugins |
| **AUTOSAR ARXML** | config repos pinned to exact SHAs in `haw.lock`, versioned with the code |

The reproducible lock is the foundation: it makes "the baseline that was live in March"
an exact, re-buildable, auditable fact rather than a guess. See
[Domains](../DOMAINS.md) and [Compliance](../COMPLIANCE.md) for the full mapping.

## 🪝 7. Integrity hooks — catch drift before it's committed

One last guardrail. `haw hooks install` writes a pre-commit hook in every repo that runs
`haw verify` — so a commit that would drift the tree from the lock is caught *locally*,
before it ever reaches CI:

```bash
haw hooks install
haw hooks list      # see the lifecycle hooks the workspace defines
```

<div class="callout success">

**That's the whole production loop.** Pinned lock, drift gate, fleet build/test, signed
artifacts, and an evidence bundle — the same primitives serve a hobby project or an
ISO 26262 audit.

</div>

## ✅ Recap

- **The manifest is trusted code** — only run `haw` on `haw.toml` / plugins you trust;
  tokens stay in the environment, never stored.
- The CI pipeline is always **`sync → verify → build → test`**; `verify` exits 3 on drift.
- `--locked` + `verify` enforce that CI builds the *committed* baseline, reproducibly.
- `haw publish --to <nexus|artifactory|gitlab|bitbucket>` distributes artifacts (`--dry-run`
  to preview).
- Governance plugins add **SBOM, provenance, and signing** on lifecycle phases; `haw
  evidence` bundles the audit trail; releases are cosign-signed.
- The same primitives map straight onto ASPICE / ISO 26262 / DO-178C / CRA compliance.

## 🎉 You did it

<img class="chapter-illus" src="../assets/img/completed-tasks.svg" alt="Course complete — you've learned hawser">

*The whole loop, checked off — compose, orchestrate, ship, extend, and govern.*

You've gone from "what even is this?" to composing a fleet, working across it, shipping
cross-repo changesets, living in the cockpit, writing a plugin, and running it all in
production with an audit trail. That's the whole tool.

Where to next:
- Keep the [CLI design & keymap](../CLI-DESIGN.md) handy as a reference.
- Deepen the domain fit in [Domains](../DOMAINS.md).
- Extend further with [Plugins](../PLUGINS.md) and [Extending](../EXTENDING.md).

Now go compose something. Welcome aboard.
