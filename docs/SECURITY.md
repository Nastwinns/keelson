# Security & trust model

This page describes what `haw` trusts and executes, so you can reason about the
blast radius of running it against a given repository. For **reporting a
vulnerability** and **supported versions**, see the repository-root
[`SECURITY.md`](https://github.com/Nastwinns/hawser/blob/main/SECURITY.md).

The one-line summary: **`haw` runs code that is written in the manifest and code
that lives in your `PATH`.** Treat both as trusted inputs.

## The manifest is trusted code

A workspace is defined by a `haw.toml` manifest. That manifest can declare
`build`, `test`, `run`, and `exec` commands, plus lifecycle hooks. When you run
a fleet operation, `haw` executes those commands **through your shell**, with
your environment and your working tree:

- `haw build` / `haw test` run the manifest's `build` / `test` commands.
- `haw run <cmd>` / `haw exec <cmd>` run arbitrary commands across the fleet.
- `haw sync` can trigger hooks and post-checkout steps declared in the manifest.

Consequently, **running `haw build`, `haw run`, `haw exec`, or `haw sync` on an
untrusted checkout is equivalent to running that repository's `Makefile`.** A
malicious `haw.toml` can do anything your shell can do.

Rule of thumb: **treat `haw.toml` exactly like a `Makefile` or the `scripts`
block of a `package.json`.** Only run lifecycle commands on manifests you have
reviewed or otherwise trust. Read-only inspection commands (`haw status`,
`haw tree`, `haw verify`) do not execute manifest commands and are safe to run
on an untrusted manifest.

## Plugins are trusted binaries

`haw` follows the `git` / `cargo` / `kubectl` extension pattern: any subcommand
`haw` does not recognize is dispatched to a `haw-<name>` executable found on your
`PATH`. For example, `haw jira sync` runs `haw-jira`.

Two properties matter for security:

1. **Plugins are ordinary executables resolved from `PATH`.** Whatever
   `haw-<name>` your `PATH` resolves to is what runs. A crafted `PATH` (or a
   `haw-*` binary dropped into a directory on it) can hijack a subcommand.
2. **Plugins inherit your full environment**, including any forge tokens
   (`GITHUB_TOKEN`, `GITLAB_TOKEN`, `HAW_*`, …) that are exported in the shell
   that launched `haw`.

Therefore: **install only plugins you trust, and keep your `PATH` clean** —
prefer absolute, well-known install locations and avoid putting untrusted or
world-writable directories ahead of system paths.

Plugins do run as **separate processes**, so a broken or hanging plugin cannot
crash `haw` — but process isolation is not a security boundary here: a plugin
runs with your privileges and your secrets.

## Tokens and credentials

- Forge tokens are **read from environment variables only** and used solely for
  API requests (opening PRs, reading CI status). They are **never written to
  disk and never logged** by `haw`.
- Git transport authentication is **not handled by `haw`** — it stays with your
  existing SSH keys or your git credential helper.
- Read-only composition (`haw sync`, `status`, `tree`, `verify`) needs no token
  at all; only forge features do.

See the [Secrets & tokens](https://github.com/Nastwinns/hawser#secrets--tokens)
section of the README for the exact precedence order per forge.

## Supply-chain hardening (this repository)

The `haw` project itself applies standard supply-chain controls:

- **GitHub Actions are pinned to full commit SHAs** (with the human-readable tag
  in a trailing comment) in every workflow, so a compromised or re-pointed
  action tag cannot alter release artifacts before they are signed.
- **Release artifacts are signed** with cosign (keyless / OIDC).
- **`cargo audit` and `cargo deny`** run on every push/PR and on a weekly
  schedule (see `.github/workflows/audit.yml` and `deny.toml`) to gate known
  advisories, disallowed licenses, and unexpected dependency sources.

## Reporting

To report a vulnerability, follow the process in the repository-root
[`SECURITY.md`](https://github.com/Nastwinns/hawser/blob/main/SECURITY.md):
use GitHub private vulnerability reporting or email the maintainer. Do **not**
open a public issue for security problems.
