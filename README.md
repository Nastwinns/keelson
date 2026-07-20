<!-- markdownlint-disable MD033 MD041 -->
<div align="center">

<img src="docs/assets/hawser-comic.jpeg" alt="hawser — the beam that binds the repos" width="720">

# hawser

**Your product lives in 10 repos and nobody knows which commits go together.
`haw` pins them all to one lockfile — so you, your CI, and your teammate check
out the _identical_ tree, every time. One binary. In Rust.**

```sh
cargo install hawser        # or: brew, scoop, prebuilt binaries — see Install
```

<img src="demo/hawser-persona.gif" alt="haw persona journey: declare, pin to real SHAs, build & test 5 real embedded upstreams, install plugins, and drive the cockpit" width="820">

<sub>Five real embedded upstreams — CoreMark · cJSON · Monocypher · libcanard · Mbed-TLS —
declared, pinned to real SHAs, then <b>real terminal captures</b> of the parallel build,
the test recipes, and the live cockpit; plus plugins.
<a href="https://nastwinns.github.io/hawser/">Full demo on the site →</a></sub>

[![website](https://img.shields.io/badge/website%20%26%20course-nastwinns.github.io%2Fhawser-8A2BE2?logo=readthedocs&logoColor=white)](https://nastwinns.github.io/hawser/)
[![crates.io](https://img.shields.io/crates/v/hawser)](https://crates.io/crates/hawser)
[![CI](https://github.com/Nastwinns/hawser/actions/workflows/ci.yml/badge.svg)](https://github.com/Nastwinns/hawser/actions/workflows/ci.yml)
[![rust](https://img.shields.io/badge/rust-1.90%2B-orange?logo=rust)](https://www.rust-lang.org)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![unsafe](https://img.shields.io/badge/unsafe-forbidden-success.svg)](Cargo.toml)

[**Website & course**](https://nastwinns.github.io/hawser/) ·
[Learn step by step](https://nastwinns.github.io/hawser/docs/learn/00-what-is-hawser.html) ·
[Install](#install) · [Quick start](#quick-start) ·
[Docs](https://nastwinns.github.io/hawser/docs/) ·
[Try the TUI in your browser](https://nastwinns.github.io/hawser/try/)

</div>

---

> **New here?** The [**illustrated course**](https://nastwinns.github.io/hawser/) takes you
> from zero to productive — and you can try the cockpit in your browser, no install needed.
> This README is the quick tour.

## The idea

`haw` composes many Git repos into one reproducible stack. A manifest (`haw.toml`)
declares the repos; a lockfile (`haw.lock`) pins each to an exact SHA. Anyone — a
teammate, a CI runner, an auditor — rebuilds the byte-identical tree.

```sh
haw init haw.toml   # declare the repos
haw sync            # clone every repo, write haw.lock (exact SHAs)
haw verify          # CI gate: exit 3 if the tree drifts from the lock
```

No submodules. No detached HEADs. No Python. One static binary.

## Install

Pick a package manager — all install the same `haw` binary:

```bash
cargo install hawser                                             # Rust / crates.io (canonical)
brew install nastwinns/tap/hawser                                # macOS + Linux
scoop bucket add nastwinns https://github.com/Nastwinns/scoop-bucket && scoop install hawser   # Windows
```

Prefer a signed, static binary? Every release ships one per platform (with `.sha256` +
keyless **cosign** signature) on the
[GitHub Releases](https://github.com/Nastwinns/hawser/releases/latest) page. Full channel
matrix — musl static build, `.deb`/`.rpm`, AUR, Nix, Docker, air-gap workflow, signature
verification — in **[docs/INSTALL.md](docs/INSTALL.md)**.

## Quick start

```bash
haw init examples/quickstart/haw.toml   # bootstrap from a ready-made example
haw sync                                # clone every repo, write haw.lock
haw                                     # open the cockpit
```

A typical session — compose the fleet, inspect it, branch a feature across repos:

```console
$ haw tree
haw.toml
├─ gateway
│  ├─ kernel    v6.1.2       (git@gitlab.company.com:firmware/kernel.git)
│  ├─ hal       main         (git@gitlab.company.com:firmware/hal.git)
│  └─ app-mqtt  release/2.x  (git@github.com:acme/app-mqtt.git)
└─ sensor-node
   ├─ kernel  v6.1.2         (git@gitlab.company.com:firmware/kernel.git)
   └─ hal     main           (git@gitlab.company.com:firmware/hal.git)

$ haw status
REPO      BRANCH   HEAD      DIRTY  DRIFT
kernel    v6.1.2   a1b2c3d4  -      -
hal       main     9f8e7d6c  yes    -
app-mqtt  release  4d5e6f7a  -      YES

$ haw change start FEAT-42 --repos kernel,app-mqtt
changeset `FEAT-42` started across 2 repo(s):
  kernel    -> change/FEAT-42
  app-mqtt  -> change/FEAT-42
```

New here? [`examples/`](examples/) has runnable, copy-pasteable manifests to learn from.
The step-by-step [course](https://nastwinns.github.io/hawser/docs/learn/00-what-is-hawser.html)
walks the whole flow.

## The cockpit

Run bare `haw` and the fleet opens as a live, keyboard-driven cockpit — in the spirit of
`k9s`. The loop is **read → drill → act**: see a repo's branch and status, open a PR's
reviewers and checks, watch a CI run — then merge or approve, without leaving the terminal.

![The hawser TUI cockpit — live fleet grid; drill into a repo's git detail, a PR's reviewers and checks, or a CI run's live progress, then merge or approve without leaving the terminal](demo/haw-tui.gif)

<sub>Real VHS capture of `haw dash --demo`. **[Try it live in your browser →](https://nastwinns.github.io/hawser/try/)**
Full keymap in [docs/CLI-DESIGN.md](docs/CLI-DESIGN.md#tui-keymap).</sub>

## What you get

One binary, five capabilities — each solving a slice of the multi-repo problem:

- **🧱 Compose.** A manifest declares repos and stacks; the committed lockfile pins each to
  an exact SHA. Shallow/partial clone and opt-in object sharing scale it to big fleets.
- **⚙️ Orchestrate.** Run `build`, `test`, or any command across every repo in parallel;
  `haw grep` fans `git grep` fleet-wide; `haw verify` is a ready CI drift gate.
- **🔀 Collaborate.** One feature, one branch across N repos, with cross-linked PR/MRs on
  GitHub, GitLab, **and** Bitbucket — reviewed together, landed in dependency order.
- **🚁 Operate.** The k9s-style cockpit above: filter, sort, marks, a problems-only view,
  drop-to-shell, a `:` command bar, six themes.
- **🛡️ Govern.** Extend the CLI with `haw-<name>` plugins; SBOM, SLSA provenance, and
  cosign/minisign signing ship built in.

Each links to depth in the docs:
[manifest](https://nastwinns.github.io/hawser/docs/learn/02-the-manifest.html) ·
[build & test](https://nastwinns.github.io/hawser/docs/learn/06-build-test-and-verify.html) ·
[changesets](https://nastwinns.github.io/hawser/docs/learn/05-changesets-across-repos.html) ·
[plugins](docs/PLUGINS.md) · [full CLI](docs/CLI-DESIGN.md).

> `haw` orchestrates Git and the forge APIs — it does not reimplement Git's merge engine,
> replace a forge, or replace your toolchain.

## Why hawser

Splitting a product across repos is routine and **domain-agnostic** — shared HAL/BSP repos
across ECUs, a fleet of microservices, an ML platform's model + pipeline + serving infra,
Terraform/Helm modules, an app and its SDK. The loop is identical everywhere; only the
repos and commands differ.

Every existing tool solves one slice and gives up another — Google `repo` and Zephyr
`west` check out from a manifest but skip the lockfile and lean on Python and symlinks;
RepoFleet drives PR flow but not reproducible composition. `haw` is the union nobody
ships: reproducible composition **and** fleet orchestration **and** cross-forge PR flow
**and** supply-chain governance, behind one binary — and it runs on Windows, no symlinks.

Full comparison matrix and per-domain mapping:
**[docs/DOMAINS.md](docs/DOMAINS.md)** · **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**.

## Documentation

Published at **[nastwinns.github.io/hawser/docs](https://nastwinns.github.io/hawser/docs/)**
(mdBook, rebuilt on every push).

| Doc | What |
|-----|------|
| [Learn (course)](https://nastwinns.github.io/hawser/docs/learn/00-what-is-hawser.html) | Zero-to-productive, chapter by chapter |
| [docs/INSTALL.md](docs/INSTALL.md) | Full install matrix + signature verification |
| [docs/CLI-DESIGN.md](docs/CLI-DESIGN.md) | Full CLI lexicon + TUI keymap |
| [docs/DOMAINS.md](docs/DOMAINS.md) | How the loop maps onto each domain |
| [docs/INTEGRATION.md](docs/INTEGRATION.md) | Copy-paste `build`/`test` recipes (Docker, QEMU, tresos, Vector, IAR…) |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crate layout, concurrency, forge abstraction |
| [docs/PLUGINS.md](docs/PLUGINS.md) · [EXTENDING.md](docs/EXTENDING.md) | Write plugins + hooks; the JSON contract |
| [docs/COMPLIANCE.md](docs/COMPLIANCE.md) | Tool qualification, SBOM/CRA, signing, GDPR |
| [docs/SECURITY.md](docs/SECURITY.md) | Trust model — what haw executes, plugin trust, tokens |

## Security

Read the full **[trust model](docs/SECURITY.md)**. The essentials:

- **The manifest is trusted code.** `build`/`test`/`run` commands run through your shell —
  treat `haw.toml` like a `Makefile`. Only run it on manifests you trust.
- **Plugins are trusted binaries.** `haw <name>` runs `haw-<name>` from your `PATH` with
  your full environment. Install only plugins you trust.
- **Tokens** are read from env vars only, never stored or logged; git transport auth stays
  with your SSH keys / credential helper.
- **Hardened supply chain.** `#![forbid(unsafe_code)]`, rustls (no OpenSSL), every Action
  pinned to a SHA, releases cosign-signed, `cargo audit` + `cargo deny` on every push.

Report a vulnerability privately: see [SECURITY.md](SECURITY.md).

## Contributing & development

```bash
cargo test --workspace                                # unit + integration
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

See [CONTRIBUTING.md](CONTRIBUTING.md). The crate layout and test coverage are described in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
