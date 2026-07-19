# 7. Going to production

You can compose a fleet, work across it, ship changesets, build and test it. This final
chapter is about doing all of that *for real* — safely, in CI, with an audit trail — and
about the one extensibility escape hatch that ties the governance features together:
plugins. It's the difference between a neat local tool and something you trust to gate
releases.

<img class="chapter-illus" src="../assets/img/launching.svg" alt="Taking haw to production">

*From a neat local tool to something you trust to gate releases — let's launch.*

<div class="objectives">
<strong>🎯 In this chapter, you'll learn to…</strong>
<ul>
<li>Internalize the <strong>trust model</strong> — the manifest is trusted code, tokens live only in the environment.</li>
<li>Wire the four-move CI pipeline: <strong><code>sync → verify → build → test</code></strong>.</li>
<li>Enforce reproducibility with <code>haw sync --locked</code> and <code>haw verify</code>.</li>
<li>Extend <code>haw</code> with <strong>plugins</strong> — any unknown <code>haw &lt;name&gt;</code> runs <code>haw-&lt;name&gt;</code>, no fork required.</li>
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

## 🧩 2. Extend it with plugins — no fork required

Before we wire the pipeline, meet the escape hatch that powers the governance features
below. `haw` follows the same pattern as `git`, `cargo`, and `kubectl`: **any subcommand
`haw` doesn't recognize is dispatched to a `haw-<name>` executable on your `PATH`.**

<img class="side-illus" src="../assets/img/design-tools.svg" alt="Extending haw with plugins">

*A plugin is just a program that reads some JSON and prints some JSON — write one in any language.*

```bash
haw jira sync      # not built in → runs `haw-jira sync`
```

The plugin runs as a **separate process** (a broken plugin can never crash `haw`), `haw`
hands it the current fleet as JSON via `HAW_JSON` + stdin (`haw.plugin/1`), and the
plugin's exit code becomes `haw`'s — so a plugin is a first-class CI gate. Discover, install,
and scaffold them:

```bash
haw plugins list                     # first-party + installed plugins
haw plugins list --remote            # the community index
haw plugins install aspice           # shells out to cargo install
haw plugins new mycheck --lang python   # runnable skeleton (rust|python|go|shell)
```

The scaffold is a complete, runnable plugin: it reads the context, handles `--help` and
`--format json`, emits a `haw.plugin.report/1` document, and fails open outside a
workspace. Put it on `PATH` and it's instantly a `haw` subcommand — no rebuild of `haw`.

<div class="callout tip">

**Tip:** `haw`'s own governance features — SBOM, signing, secret-gate — ship *as plugins*
on exactly this model, so nothing here is second-class. The full contract, JSON Schemas,
and language bindings are in [Plugins](../PLUGINS.md).

</div>

The real power is **lifecycle hooks**: subscribe a plugin to a phase in `[plugins]` and it
fires automatically around fleet operations — which is exactly how the supply-chain
features below are wired.

## 🔧 3. The CI pipeline — always the same four moves

Here's the payoff of everything you learned in Chapters 3 and 6. A `haw` pipeline is the
same shape everywhere:

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

## ♻️ 4. Reproducibility, enforced

The whole pipeline rests on `haw.lock`. Two flags make it airtight in CI:

```bash
haw sync --locked        # fail unless haw.lock exists (no rev resolution)
haw verify               # assert tree == lock, exit 3 on drift
```

`--locked` refuses to invent a lock on the fly — CI must build from the *committed*
baseline, never resolve fresh. `verify` then proves the checkout matches it. Together they
guarantee the tree in CI is byte-for-byte the tree you committed.

## 📦 5. Distribution — publish what the fleet produced

Once the fleet builds, `haw publish` uploads its artifacts to a generic/raw artifact
registry — **Nexus, Artifactory, GitLab, or Bitbucket**:

```bash
haw publish dist/*.tar.gz --to nexus
haw publish dist/*.tar.gz --to nexus --dry-run   # print the plan, no creds, no network
```

`--dry-run` prints exactly what *would* upload (target, name, version, each file) without
touching the network or needing credentials — perfect for wiring the step up safely first.
Credentials come from the target's env vars (e.g. `NEXUS_URL`, `NEXUS_USER`, `NEXUS_PASS`).

## 🔏 6. Signing, SBOM, and provenance — the supply chain

This is where `haw` earns its keep for serious releases. Every signed release ships with a
`.sha256` checksum and a **keyless cosign signature** (`.sig`/`.pem`) you can verify
offline — even on an air-gapped host (see [Installing hawser](../INSTALL.md)).

For *your* fleet, the supply-chain features ship as the governance plugins you met at the
top of this chapter, subscribed to lifecycle phases:

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
haw evidence --out haw-evidence.tar.gz
```

```console
wrote evidence bundle haw-evidence.tar.gz
```

(Run bare, `haw evidence` writes `./haw-evidence.tar.gz` by default; `--out` just picks the path.)

## 🏛️ 7. The compliance / automotive angle

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

## 🪝 8. Integrity hooks — catch drift before it's committed

One last guardrail. `haw hooks install` writes a pre-commit hook in every repo that runs
`haw verify` — so a commit that would drift the tree from the lock is caught *locally*,
before it ever reaches CI:

```bash
haw hooks install
```

```console
  ✓ hello-world  pre-commit -> haw verify
  ✓ spoon-knife  pre-commit -> haw verify
installed the integrity pre-commit in 2 repo(s)
```

`haw hooks list` shows the *lifecycle* hooks the workspace defines (executables under
`.haw/hooks`) — separate from the per-repo integrity pre-commit above. On a fresh
workspace with none defined yet, it tells you where to add them:

```bash
haw hooks list
```

```console
no lifecycle hooks — add executables under /path/to/my-first-stack/.haw/hooks
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
- Any unknown `haw <name>` runs a `haw-<name>` **plugin** from `PATH` — no fork; governance
  plugins add **SBOM, provenance, and signing** on lifecycle phases; `haw evidence` bundles
  the audit trail; releases are cosign-signed.
- The same primitives map straight onto ASPICE / ISO 26262 / DO-178C / CRA compliance.

## 🎉 You did it

<img class="chapter-illus" src="../assets/img/completed-tasks.svg" alt="Course complete — you've learned hawser">

*The whole loop, checked off — compose, orchestrate, ship, extend, and govern.*

You've gone from "what even is this?" to composing a fleet, pinning it to a lockfile,
living in the cockpit, shipping cross-repo changesets, building and testing the whole
thing, extending it with a plugin, and running it all in production with an audit trail.
That's the whole tool.

Where to next:
- Ready to build your own? [8. Build a plugin — and let Claude write your commits](08-build-a-plugin-mcp.md)
  turns everything you learned into a real plugin that doubles as an MCP server for Claude.
- Keep the [CLI design & keymap](../CLI-DESIGN.md) handy as a reference.
- Deepen the domain fit in [Domains](../DOMAINS.md).
- Extend further with [Plugins](../PLUGINS.md) and [Extending](../EXTENDING.md).

Now go compose something. Welcome aboard.
