# haw plugin contract — JSON Schemas

These JSON Schemas (draft 2020-12) are the **source of truth** for the haw
plugin wire contract. A plugin written in *any* language interoperates with haw
as long as it reads and emits documents that validate against these schemas.

The Rust types in `crates/haw-core/src/plugin/mod.rs` and the reference bindings
under `bindings/` all mirror these shapes.

## Schemas

| File | Schema string | Direction | Purpose |
|------|---------------|-----------|---------|
| [`haw.plugin.v1.json`](haw.plugin.v1.json) | `haw.plugin/1` | haw → plugin | Workspace context: `root`, `stack`, `repos[]`, optional `phase`/`intent`. Delivered on the `HAW_JSON` env var and stdin. |
| [`haw.plugin.report.v1.json`](haw.plugin.report.v1.json) | `haw.plugin.report/1` | plugin → haw | A lifecycle-phase report: `ok`, `summary`, `artifacts[]`, `findings[]`. |
| [`haw.plugin.view.v1.json`](haw.plugin.view.v1.json) | `haw.plugin.view/1` | plugin → haw | A structured TUI panel (`title`, `lines[]`) printed under render intent (`HAW_RENDER=1`, context `intent: "render"`). |
| [`haw.plugins.index.v1.json`](haw.plugins.index.v1.json) | `haw.plugins.index/1` | catalog | A curated list of known plugins (`name`, `crate`, `git`, `description`). |

## How they map to the Rust structs

- `haw.plugin/1` ⇄ the context built by `phase_context()` plus `RepoContext`.
- `haw.plugin.report/1` ⇄ `Report` / `Artifact` / `Finding` (`schema`, `plugin`,
  `phase`, `ok`, `summary`, `artifacts`, `findings`). `phase`, `summary`,
  `artifacts`, and `findings` all default when omitted — only `schema`,
  `plugin`, `ok` are required.
- `haw.plugin.view/1` ⇄ the render-panel document accepted by the cockpit.
- `haw.plugins.index/1` ⇄ the curated index shape (`plugins[]`).

## Versioning policy

**The `schema` string is the contract version.** A document declares its
contract in its `schema` field (`haw.plugin/1`, `haw.plugin.report/1`, …). haw
checks this marker before trusting a document (see `Report::parse`, which
rejects any report whose `schema` is not `haw.plugin.report/1`).

- **Additive changes** (new optional fields) do **not** bump the version — every
  schema sets `additionalProperties: true` so old consumers keep working.
- **Breaking changes** (removing/renaming a required field, changing a type) bump
  the trailing integer: `haw.plugin/1` → `haw.plugin/2`. haw and plugins can
  then negotiate on the marker.

Consumers should always check the `schema` marker before relying on any other
field, and ignore unknown fields.

## Validating

```sh
python3 -c "import json,glob; [json.load(open(f)) for f in glob.glob('schemas/*.json')]"
```

For full schema-aware validation of a document, use any draft 2020-12 validator
(e.g. Python `jsonschema`, Go `santhosh-tekuri/jsonschema`, `ajv` for JS).
