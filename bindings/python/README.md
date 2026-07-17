# haw-plugin (Python)

A thin, dependency-free Python binding for the [haw plugin contract](../../schemas/).
It parses the `haw.plugin/1` context haw hands you and helps you emit
`haw.plugin.report/1` and `haw.plugin.view/1` documents — nothing more.

The JSON Schemas in [`schemas/`](../../schemas/) are the source of truth; this
package just mirrors them.

## Install

```sh
pip install haw-plugin
# or, from this repo:
pip install ./bindings/python
```

## API

- `Context.from_env()` — read the context from `HAW_JSON` (falls back to stdin).
  Fail-open: malformed/missing input yields a schema-only context. Exposes
  `root`, `stack`, `repos`, `phase`, `intent`, `raw`, and `is_render()`
  (`HAW_RENDER=1` or `intent="render"`).
- `Report(plugin, ok, summary=..., phase=..., artifacts=[], findings=[])` with
  `.emit()` — prints a `haw.plugin.report/1` document.
- `Artifact(path, kind)` and `Finding(level, message)` dataclasses.
- `view(title, lines)` — prints a `haw.plugin.view/1` panel.

## A 15-line plugin

Save as `haw-greet` (executable) on your `PATH`:

```python
#!/usr/bin/env python3
import sys
from haw_plugin import Context, Report, Finding

ctx = Context.from_env()
if ctx.is_render():
    from haw_plugin import view
    view("greet", [r["name"] for r in ctx.repos] or ["(no repos)"])
    sys.exit(0)
rep = Report(plugin="greet", ok=True, summary="greetings")
for r in ctx.repos:
    rep.findings.append(Finding("info", f"repo {r['name']} @ {r['rev']}"))
rep.emit()
```

Then: `chmod +x haw-greet && PATH="$PWD:$PATH" haw greet --format json`

## Runnable example

[`example/haw-hello`](example/haw-hello) is a complete plugin using the binding:

```sh
HAW_JSON='{"schema":"haw.plugin/1","repos":[]}' \
  python3 bindings/python/example/haw-hello --format json
```
