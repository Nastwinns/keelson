# Plugins

haw follows the git / cargo / kubectl pattern: any subcommand haw doesn't recognize
is dispatched to an executable on your `PATH`. Ship `haw-jira`, `haw-bazel`,
`haw-sbom-scan` without touching core.

A plugin is:

- an executable named `haw-<name>` somewhere on `PATH`,
- run as a **separate process** — a broken or hanging plugin can't crash haw,
- handed the workspace context as JSON so it can act on the current fleet.

## Dispatch contract

When you run `haw <name> <args...>` and `<name>` is not a built-in command, haw:

1. Resolves the binary name `haw-<name>` and spawns it via `PATH` lookup.
2. Forwards `<args...>` verbatim as the plugin's argv (haw does **not** parse them).
3. Passes the workspace context as a `haw.plugin/1` JSON document **two ways**:
   - in the `HAW_JSON` environment variable, and
   - written to the plugin's **stdin**.
   (Both carry the identical document — read whichever is convenient.)
4. Leaves the plugin's **stdout** and **stderr** connected to the terminal (or the
   pipe haw was invoked with) — the plugin prints directly.
5. Waits for the plugin and **propagates its exit code** as haw's own exit code
   (clamped to 0–255; a killed-by-signal plugin surfaces as `1`).

If no built-in matches **and** no `haw-<name>` is found on `PATH`, haw fails with:

```
error: no built-in `<name>` and no `haw-<name>` on PATH: No such file or directory (os error 2)
```

and exits non-zero. Dispatch fails open: an unknown word is an error, never a crash.

### The `haw.plugin/1` context

The JSON document haw provides (via `HAW_JSON` and stdin). Inside a workspace it is
fully populated:

```json
{
  "schema": "haw.plugin/1",
  "root": "/path/to/workspace",
  "stack": "gateway",
  "repos": [
    { "name": "kernel", "path": "/path/to/workspace/kernel", "rev": "v6.1.2", "groups": ["firmware"] },
    { "name": "hal",    "path": "/path/to/workspace/hal",    "rev": "main",   "groups": ["firmware"] }
  ]
}
```

| Field         | Meaning                                                            |
|---------------|-------------------------------------------------------------------|
| `schema`      | Always `"haw.plugin/1"`. Check this before trusting the rest.      |
| `root`        | Absolute workspace root (the directory tree containing `haw.toml`).|
| `stack`       | Current stack name, or `null` if none is selected.                |
| `repos[]`     | Resolved repos: `name`, absolute `path`, `rev`, and `groups`.     |

Run **outside** a workspace, haw still dispatches the plugin but the context degrades
to the schema marker only:

```json
{ "schema": "haw.plugin/1" }
```

A well-behaved plugin checks for `root`/`repos` and does something sensible when they
are absent (print help, operate on cwd, or exit cleanly).

### TUI panels — rendering your own cockpit surface

The cockpit (`haw dash`) has a first-class **Plugins** view (press `7`, or `:plugins`)
that lists every available plugin — the manifest `[plugins]` keys unioned with the
`haw-*` executables discovered on `PATH`. Selecting one with `Enter` runs the plugin in
a **render intent** and shows its output in the scrollable detail panel titled
`plugin: <name>`.

The render contract adds two signals on top of the normal `haw.plugin/1` context so a
plugin can tell it is being asked for a human-readable panel (rather than being fired
for a lifecycle phase):

- the environment variable **`HAW_RENDER=1`** is set, and
- the context JSON (on `HAW_JSON` and stdin) carries **`"intent": "render"`**.

```json
{
  "schema": "haw.plugin/1",
  "intent": "render",
  "root": "/path/to/workspace",
  "stack": "gateway",
  "repos": [ /* ... as above ... */ ]
}
```

When it sees these, the plugin should print a panel to **stdout** and exit. Two output
shapes are accepted:

1. **Structured** — a `haw.plugin.view/1` document. haw renders its `title` followed by
   each string in `lines`:

   ```json
   {
     "schema": "haw.plugin.view/1",
     "title": "SBOM status",
     "lines": [
       "kernel   ✓ SBOM emitted",
       "hal      ✓ SBOM emitted",
       "app-mqtt ⚠ stale"
     ]
   }
   ```

2. **Raw text** — anything that is not a `haw.plugin.view/1` document is shown verbatim
   as the panel body. This lets a plugin `printf` a plain report with no JSON at all.

Output is line-capped to keep the panel bounded. A plugin that produces no output shows
a short placeholder. A plugin that is not on `PATH` reports a clear error in the cockpit
rather than crashing it.

## Managing plugins

`haw plugins` (plural) is the management surface — discover what exists, see what's
installed, and install first-party plugins. It is a defined subcommand, so it never
collides with `haw <name>` dispatch: `haw plugins list` always runs the built-in, even
if a `haw-plugins` binary sits on `PATH`.

### `haw plugins list`

A table merging three sources, deduped by name:

- **Official catalog** — the first-party plugins shipped in this repo.
- **Installed** — every `haw-<name>` executable found on `PATH`.
- **Subscribed** — the workspace manifest `[plugins]` entries and their phases (when
  run inside a workspace; it degrades gracefully when there is none).

```
$ haw plugins list
NAME        STATUS     SUBSCRIBED    DESCRIPTION
artifact    available  -             SLSA/in-toto provenance + cosign/minisign signing
aspice      installed  pre-request   ASPICE/qualification traceability from the pinned fleet
compliance  available  post-build    SBOM (CycloneDX + SPDX) generation
...
```

`STATUS` is `installed` when the `haw-<name>` binary is on `PATH`, else `available`.
`SUBSCRIBED` lists the phases from the manifest, or `-`. A plugin discovered on `PATH`
that is not in the catalog still appears (with source `path`).

`--format json` emits a `haw.plugins/1` document for tooling:

```sh
haw plugins list --format json | jq '.plugins[] | select(.installed | not) | .name'
```

```json
{
  "schema": "haw.plugins/1",
  "plugins": [
    {
      "name": "aspice",
      "crate": "haw-aspice",
      "installed": true,
      "subscribed_phases": ["pre-request"],
      "description": "ASPICE/qualification traceability from the pinned fleet",
      "source": "catalog"
    }
  ]
}
```

### `haw plugins install <name>`

Install a plugin binary via `cargo install`. A catalog name (`aspice`) resolves to its
crate (`haw-aspice`); any other value is used verbatim, so a full crate name works too.
The first-party plugins are workspace members (not yet on crates.io), so the default
source is `--git https://github.com/Nastwinns/hawser`:

```sh
haw plugins install aspice                 # cargo install --git <repo> haw-aspice
haw plugins install aspice --dry-run        # print the command, run nothing
haw plugins install haw-foo --git https://example.com/me/plugins   # custom source
haw plugins install haw-foo --git https://example.com/me/plugins --tag v1.2.0   # pin to a tag
haw plugins install haw-foo --git https://example.com/me/plugins --rev 9f3c1a2   # pin to a commit
haw plugins install some-crate --locked     # honor the crate's Cargo.lock
```

Pin a custom `--git` source with `--tag <TAG>` or `--rev <SHA>` (mutually exclusive).
They only apply to a custom `--git` source — the default first-party source is already
pinned to this hawser version automatically. Installs are always `--locked` for
reproducibility (`--locked` is now a no-op, kept for compatibility).

haw prints exactly what it will run (`$ cargo install …`) before running it, streams
cargo's output, and propagates cargo's exit code. `--dry-run` prints the command and
exits without touching cargo. If `cargo` is not on `PATH`, haw fails with an actionable
error pointing at <https://rustup.rs>.

### `haw plugins path`

Print the directories haw scans for `haw-*` plugins (the `PATH` entries) — drop a
`haw-<name>` executable into any of them to make it discoverable:

```sh
haw plugins path
```

## Scaffold a plugin

`haw plugins new <name> --lang <rust|python|go|shell> [--dir <path>]` writes a
**runnable** plugin skeleton that already implements the contract: it reads the
`haw.plugin/1` context from `HAW_JSON` (falling back to stdin), handles `--help`
and `--format json`, emits a `haw.plugin.report/1` document, and degrades
gracefully when run outside a workspace. The target defaults to `./haw-<name>`;
`--dir` overrides it. haw refuses to overwrite a non-empty directory.

```sh
haw plugins new sbom --lang shell     # ./haw-sbom/haw-sbom (POSIX sh) + README.md
haw plugins new sbom --lang python    # ./haw-sbom/haw-sbom (python3) + README.md
haw plugins new sbom --lang go        # ./haw-sbom/{main.go, go.mod, README.md}
haw plugins new sbom --lang rust      # cargo crate: Cargo.toml + src/main.rs + README.md
haw plugins new sbom --lang shell --dir /tmp/sbom   # choose the target dir
```

Per language, the entry point and build step differ:

| `--lang` | Entry file(s)                          | Make it runnable                          |
|----------|----------------------------------------|-------------------------------------------|
| `shell`  | `haw-<name>` (executable POSIX sh)      | already executable — drop on `PATH`       |
| `python` | `haw-<name>` (executable, `python3`)    | already executable — drop on `PATH`       |
| `go`     | `main.go` + `go.mod` (module `haw-<name>`) | `go build -o haw-<name>`               |
| `rust`   | `Cargo.toml` (`[[bin]] haw-<name>`) + `src/main.rs` | `cargo build --release`      |

Each skeleton ships a `README.md` with the "drop on `PATH` → `haw <name>`" recipe
and a `[plugins]` subscribe snippet. The rust and go skeletons are standalone —
the rust one depends only on `serde`/`serde_json`, not on any haw crate. After
building, put the binary on `PATH` and run it:

```sh
haw plugins new demo --lang shell --dir /tmp/haw-demo
PATH="/tmp/haw-demo:$PATH" haw demo
HAW_JSON='{"schema":"haw.plugin/1"}' /tmp/haw-demo/haw-demo --format json
```

The `haw plugins new` output lists every file it created and prints the exact
next steps (build + `PATH=…` invocation) for the chosen language.

## Discover community plugins

`haw plugins list --remote` merges a community index into the local table. Each
merged-in plugin shows STATUS `available` and source `remote` with its
description; anything already installed, in the catalog, or subscribed keeps its
own status (dedup is by name).

```sh
haw plugins list --remote
haw plugins list --remote --index https://example.com/plugins-index.json
haw plugins list --remote --format json   # remote entries carry "source":"remote"
```

The default index URL is
`https://raw.githubusercontent.com/Nastwinns/hawser/main/plugins-index.json`;
pass `--index <url>` to point at your own. A network or parse failure is **not**
fatal — haw prints a warning and falls back to the local-only list.

### The `haw.plugins.index/1` format

The index is a single JSON document:

```json
{
  "schema": "haw.plugins.index/1",
  "plugins": [
    {
      "name": "sbom",
      "crate": "haw-sbom",
      "git": "https://github.com/you/haw-sbom",
      "description": "CycloneDX SBOM generation for the pinned fleet"
    }
  ]
}
```

| Field         | Meaning                                                     |
|---------------|-------------------------------------------------------------|
| `schema`      | Always `"haw.plugins.index/1"`.                             |
| `plugins[]`   | One entry per plugin.                                       |
| `name`        | The verb users type (`haw <name>`).                        |
| `crate`       | Crate name for `cargo install` (optional).                 |
| `git`         | Source repository URL (optional).                          |
| `description` | One-sentence summary shown in `haw plugins list`.          |

### Add your plugin to the community index

Open a PR against the repo-root [`plugins-index.json`](https://github.com/Nastwinns/hawser/blob/main/plugins-index.json)
that adds one entry — `name`, `crate`, `git`, and a one-sentence `description`.
Once merged it appears for everyone running `haw plugins list --remote`.

## Machine interface — consuming haw's own output

Plugins rarely need to re-derive fleet state: haw's read commands already speak JSON.
Every read command (`status`, `tree`, `change status`, `verify`, `evidence`) offers
`--format json` with a stable, versioned schema and stable exit codes. Shell out to
haw and parse it:

```sh
haw status --format json | jq '.repos[] | select(.dirty)'
```

The `haw.plugin/1` context tells you *where* the workspace is; `--format json` tells
you *what state it's in*. See [EXTENDING.md §1.5](EXTENDING.md) for the machine
interface contract.

## Hello, plugin — in two languages

Both versions below implement the same command: `haw hello` prints a greeting,
`--help` describes itself, and `--format json` emits `haw.plugin/1` JSON.

### POSIX shell

A full working version lives in [`examples/haw-hello`](https://github.com/Nastwinns/hawser/tree/main/examples/haw-hello). The core:

```sh
#!/usr/bin/env sh
set -eu

case "${1:-}" in
-h | --help)
	echo "haw-hello — say hello. Options: --help, --format json"
	exit 0
	;;
esac

# haw hands us the workspace context in $HAW_JSON (and on stdin).
root=$(printf '%s' "${HAW_JSON:-}" | sed -n 's/.*"root":"\([^"]*\)".*/\1/p')

if [ "${1:-}" = "--format" ] && [ "${2:-}" = "json" ]; then
	printf '{"schema":"haw.plugin/1","plugin":"hello","root":"%s"}\n' "$root"
	exit 0
fi

if [ -n "$root" ]; then
	printf 'hello from haw-hello — workspace at %s\n' "$root"
else
	printf 'hello from haw-hello (no workspace here)\n'
fi
```

Make it executable and drop it on `PATH`:

```sh
chmod +x haw-hello
PATH="$PWD:$PATH" haw hello
```

### Rust

A standalone binary — no dependency on any haw crate.

```sh
cargo new --bin haw-hello
cd haw-hello
```

`src/main.rs`:

```rust
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("haw-hello — say hello. Options: --help, --format json");
        return ExitCode::SUCCESS;
    }

    // haw passes the haw.plugin/1 context in HAW_JSON (also on stdin).
    let ctx = env::var("HAW_JSON").unwrap_or_default();
    let root = ctx
        .split("\"root\":\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap_or("");

    if args == ["--format", "json"] {
        println!(r#"{{"schema":"haw.plugin/1","plugin":"hello","root":"{root}"}}"#);
        return ExitCode::SUCCESS;
    }

    if root.is_empty() {
        println!("hello from haw-hello (no workspace here)");
    } else {
        println!("hello from haw-hello — workspace at {root}");
    }
    ExitCode::SUCCESS
}
```

Build and run it as a plugin:

```sh
cargo build --release
PATH="$PWD/target/release:$PATH" haw hello
```

(For real plugins, parse `HAW_JSON` with `serde_json` instead of string slicing.)

## Write in any language

The plugin contract is **language-agnostic** — it is just JSON on `HAW_JSON` /
stdin (`haw.plugin/1`) and JSON on stdout (`haw.plugin.report/1` for lifecycle
phases, `haw.plugin.view/1` for TUI render intent). Any language that can read an
env var and print JSON can be a haw plugin.

The [`schemas/`](https://github.com/Nastwinns/hawser/tree/main/schemas) directory holds the **official JSON Schemas**
(draft 2020-12) — the source of truth for every field name and shape. Validate
your plugin's I/O against them.

Thin reference bindings mirror those schemas so you don't hand-roll the JSON:

- **Python** — [`bindings/python`](https://github.com/Nastwinns/hawser/tree/main/bindings/python) (`haw_plugin`):
  `Context.from_env()`, `Report.emit()`, `view(title, lines)`. No deps beyond stdlib.
- **Go** — [`bindings/go`](https://github.com/Nastwinns/hawser/tree/main/bindings/go) (`hawplugin`): `ReadContext()`,
  `Report.Emit()`, `View(title, lines)`. Stdlib only.
- **POSIX shell** and **Rust** — the [`examples/haw-hello`](https://github.com/Nastwinns/hawser/tree/main/examples/haw-hello)
  and the "Hello, plugin" section above show zero-dependency implementations.

For a curated list of existing plugins to install or learn from, see
[AWESOME-HAW-PLUGINS.md](https://github.com/Nastwinns/hawser/blob/main/AWESOME-HAW-PLUGINS.md).

## Conventions

- **Name it `haw-<verb>`.** The verb is what users type: `haw-jira` → `haw jira`.
  Keep it short and unclaimed by built-ins (`haw --help` lists those).
- **Self-describing `--help`.** Users discover your plugin's flags through it; haw
  does not document plugins for you.
- **Human on stdout, JSON on `--format json`.** Print a readable line by default;
  emit a `haw.plugin/1` document (or your own versioned schema) under `--format json`
  so other tools can pipe you.
- **Exit codes carry meaning.** `0` = success. Non-zero = failure, and haw
  propagates it — CI gates and `&&` chains rely on it. Don't exit `0` on error.
- **Fail open.** Handle the workspace-less context (schema-only JSON) gracefully.
  Don't assume `root`/`repos` exist. Never hang: your process blocks haw until it exits.
- **Stay a separate process.** You get isolation for free — don't try to reach into
  haw's internals; consume `--format json` and the `haw.plugin/1` context instead.

## Distributing your plugin

Any executable named `haw-<name>` on `PATH` works. Two common paths:

- **Publish a crate.** Name the binary `haw-<name>`; users get it with
  `cargo install haw-<name>`, which drops it into `~/.cargo/bin` (usually on `PATH`).
- **Ship a binary or script.** Drop `haw-<name>` into any `PATH` directory
  (`/usr/local/bin`, `~/.local/bin`, `~/bin`). Shell scripts count — mark them
  executable.

Verify with:

```sh
which haw-<name>   # haw finds exactly what your shell finds
haw <name> --help
```

## Submitting your plugin

Built something useful? Share it. See [CONTRIBUTING.md](https://github.com/Nastwinns/hawser/blob/main/CONTRIBUTING.md) for the
build/test checklist and PR etiquette, then open a PR that adds your plugin to the
community list — one line: name, one-sentence description, and a link. We keep core
small on purpose; the ecosystem lives in plugins.

## Lifecycle phases

Plugins can subscribe to lifecycle phases in the manifest's `[plugins]` table and
are invoked out-of-process with `--haw-phase <name>` (e.g. an SBOM plugin on
`post-build`). The optional `haw-plugin` SDK crate gives Rust authors the
`Context`/`Report` ergonomics while still compiling to a standalone `haw-<name>`
binary.
