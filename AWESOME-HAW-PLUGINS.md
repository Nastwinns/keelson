# Awesome haw plugins

A curated list of plugins for [haw](https://github.com/Nastwinns/hawser). A plugin
is any executable named `haw-<name>` on your `PATH` — see
[docs/PLUGINS.md](docs/PLUGINS.md) for the contract and the language-agnostic
[JSON Schemas](schemas/) and [reference bindings](bindings/) (Python, Go, plus
shell/Rust examples).

## First-party plugins

Shipped in this repo as workspace members. Install with `haw plugins install <name>`
(resolves to the crate via `cargo install --git https://github.com/Nastwinns/hawser`).

| Name | Crate | Description |
|------|-------|-------------|
| `aspice` | `haw-aspice` | ASPICE / qualification traceability from the pinned fleet. |
| `jira` | `haw-jira` | Sync fleet state and change sets to Jira issues. |
| `misra` | `haw-misra` | MISRA C/C++ compliance checks across the pinned repos. |
| `compliance` | `haw-compliance` | SBOM (CycloneDX + SPDX) generation for the fleet. |
| `artifact` | `haw-artifact` | SLSA / in-toto provenance and cosign / minisign signing. |
| `git-gate` | `haw-git-gate` | Policy gate on git state (dirty, unsigned, ahead/behind) at lifecycle phases. |

## Community plugins

_Your plugin here._ See "Submit yours" below.

## Submit yours

Built a `haw-<name>` plugin? Add it to the community list.

1. Make sure it follows the contract in [docs/PLUGINS.md](docs/PLUGINS.md): reads the
   `haw.plugin/1` context, self-describing `--help`, meaningful exit codes, fails open.
2. Open a PR that adds an entry to [`plugins-index.json`](plugins-index.json) — the
   machine-readable `haw.plugins.index/1` index (`name`, `crate`, `git`, `description`),
   validated against [`schemas/haw.plugins.index.v1.json`](schemas/haw.plugins.index.v1.json).
3. Add a one-line row to the **Community plugins** table above: name, crate/binary, and
   a one-sentence description.

We keep core small on purpose; the ecosystem lives in plugins.
