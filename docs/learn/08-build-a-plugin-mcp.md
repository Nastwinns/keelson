# 8. Build a plugin — and let Claude write your commits

You've composed a fleet, pinned it, lived in the cockpit, shipped changesets, and gated it
in CI. Now the fun part: **extend haw yourself**. haw follows the git / cargo / kubectl
pattern — any subcommand it doesn't recognize is dispatched to an executable named
`haw-<name>` on your `PATH`. Ship `haw-jira`, `haw-sbom`, `haw-whatever` without touching
core.

We're going to build a real one: **`haw-commit-ai`**. It has two faces from a single
script. As a plain plugin (`haw commit-ai`) it drafts commit messages and PR text from your
diffs. And — the headline — it doubles as an **MCP server** so **Claude** (Claude Code /
Claude Desktop) can read your workspace and diffs and write the real commit + PR text
*itself*, safely.

We'll build it in **two levels**, and the second one is the whole point:

- **Niveau 1 — le socle (mono-repo).** Learn the plugin + MCP mechanics on a single repo:
  scaffold, read the context, expose a few small tools, wire it into Claude. Simple and
  honest — *at this stage Claude Code already sees a single repo's diff natively.*
- **Niveau 2 — la vision cross-repo (changeset-wide).** The differentiator. Claude on its
  own **cannot** see a *fleet-wide changeset* spanning N repos. haw can. We add two tools
  that hand Claude the combined cross-repo diff and let it write **one** coherent PR that
  narrates every repo together. **This is the thing neither Claude nor lazygit can do alone.**

<div class="objectives">
<strong>🎯 In this chapter, you'll learn to…</strong>
<ul>
<li>Build a plugin <em>and</em> give Claude fleet-wide vision across your whole changeset.</li>
<li>Scaffold a plugin with <code>haw plugins new</code> and understand the <code>haw.plugin/1</code> contract.</li>
<li>Read the workspace context haw hands every plugin (via <code>$HAW_JSON</code> / stdin).</li>
<li>Emit the two machine shapes: a <code>haw.plugin.report/1</code> for <code>--format json</code> and a <code>haw.plugin.view/1</code> panel for the cockpit's Plugins view (<code>7</code>).</li>
<li>Turn the same script into an <strong>MCP server</strong> with the official <code>mcp</code> SDK (FastMCP) so Claude can call small, safe tools.</li>
<li><strong>Niveau 2:</strong> hand Claude the <em>combined</em> cross-repo diff (<code>changeset_diff()</code>) and let it draft a single fleet-wide PR (<code>draft_changeset_pr()</code>) — the cross-repo story no single-repo tool can tell.</li>
</ul>
</div>

# 🪜 Niveau 1 : le socle (mono-repo)

This is the foundation: one repo at a time, and everything you need to understand the
plugin protocol and the MCP handshake. It's deliberately simple — get comfortable here,
then Niveau 2 unlocks the cross-repo superpower.

## 🧩 1. The plugin contract, in one breath

When you run `haw <name> <args…>` and `<name>` isn't built in, haw:

1. spawns `haw-<name>` from your `PATH` (a **separate process** — a broken plugin can't
   crash haw),
2. forwards your `<args…>` verbatim,
3. hands over the workspace context as a **`haw.plugin/1`** JSON document — **both** in the
   `HAW_JSON` environment variable **and** on the plugin's stdin (identical content, read
   whichever you like),
4. propagates the plugin's exit code as its own.

The context looks like this inside a workspace:

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

Run **outside** a workspace it degrades to just `{"schema": "haw.plugin/1"}` — a
well-behaved plugin checks for `root`/`repos` and does something sensible when they're
absent.

There are two more shapes a plugin can *print*:

- **`haw.plugin.report/1`** — a machine report (`{schema, plugin, ok, summary, findings}`)
  for `--format json`.
- **`haw.plugin.view/1`** — a panel (`{schema, title, lines[]}`) for the cockpit's Plugins
  view. haw sets `HAW_RENDER=1` and puts `"intent": "render"` in the context to ask for it.

The three JSON Schemas live in [`schemas/`](https://github.com/Nastwinns/hawser/tree/main/schemas)
— they're the source of truth for every field.

## 🏗️ 2. Scaffold it

haw writes you a runnable skeleton that already implements the contract:

```bash
haw plugins new commit-ai --lang python
```

```console
created ./haw-commit-ai/haw-commit-ai   (executable, python3)
created ./haw-commit-ai/README.md
next:
  chmod is already set — drop it on PATH:
    PATH="$PWD/haw-commit-ai:$PATH" haw commit-ai
```

That skeleton reads `$HAW_JSON`, handles `--help` and `--format json`, and emits a
`haw.plugin.report/1`. It's a correct plugin already — we're going to *replace* its body
with the MCP-capable version below.

<div class="callout note">

**Why Python?** Because the MCP SDK we'll use for the Claude side is Python-first. The
plugin *face* stays zero-dependency stdlib; only the `--mcp` face needs `pip install mcp`.

</div>

## 📖 3. Read the context

The heart of any plugin is reading `haw.plugin/1`. Prefer the env var (it never blocks),
fall back to stdin, and always degrade gracefully:

```python
import json, os, sys

def read_context() -> dict:
    raw = os.environ.get("HAW_JSON", "")
    if not raw and not sys.stdin.isatty():
        raw = sys.stdin.read()
    if not raw:
        return {"schema": "haw.plugin/1"}
    try:
        ctx = json.loads(raw)
    except ValueError:
        return {"schema": "haw.plugin/1"}
    return ctx if isinstance(ctx, dict) else {"schema": "haw.plugin/1"}
```

From `ctx["repos"]` you get each repo's on-disk `path` — everything else (`git diff`,
`git commit`) is just shelling out into that path.

## 🤖 4. The two faces: plugin + MCP server

Here's the full final `haw-commit-ai`. Paste it over the scaffold's script (keep the file
name `haw-commit-ai` and the `#!/usr/bin/env python3` shebang; `chmod +x` it). A ready copy
lives at [`examples/plugins/haw-commit-ai/`](https://github.com/Nastwinns/hawser/tree/main/examples/plugins/haw-commit-ai).

```python
#!/usr/bin/env python3
"""haw-commit-ai — draft commits + PRs from a haw workspace, and serve them to
Claude via MCP. The plugin face is zero-dependency stdlib; --mcp needs `pip install mcp`."""
import json, os, subprocess, sys
from typing import Any, Optional

def read_context() -> dict:
    raw = os.environ.get("HAW_JSON", "")
    if not raw and not sys.stdin.isatty():
        raw = sys.stdin.read()
    try:
        ctx = json.loads(raw) if raw else {}
    except ValueError:
        ctx = {}
    return ctx if isinstance(ctx, dict) else {"schema": "haw.plugin/1"}

def context_repos(ctx): 
    r = ctx.get("repos")
    return [x for x in r if isinstance(x, dict)] if isinstance(r, list) else []

def _run(cmd, cwd=None):
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, check=False)
    return p.returncode, p.stdout, p.stderr

def repo_diff_text(path):                        # staged + unstaged vs HEAD
    rc, out, _ = _run(["git", "-C", path, "diff", "HEAD"], cwd=path)
    return out

def changeset_repos(ctx):                        # touched repos, else dirty repos
    root, repos = ctx.get("root"), context_repos(ctx)
    if root:
        rc, out, _ = _run(["haw", "change", "status", "--format", "json"], cwd=root)
        if rc == 0 and out.strip():
            try: data = json.loads(out)
            except ValueError: data = {}
            names = {r.get("name") for r in data.get("repos", []) if isinstance(r, dict)}
            touched = [r for r in repos if r.get("name") in names]
            if touched: return touched
    dirty = []
    for r in repos:
        rc, out, _ = _run(["git", "-C", r["path"], "status", "--porcelain"], cwd=r["path"])
        if out.strip(): dirty.append(r)
    return dirty
```

The plugin faces — human text, `--format json` → `haw.plugin.report/1`, and the render
intent → `haw.plugin.view/1`:

```python
def emit_report(ctx):
    repos = changeset_repos(ctx) or context_repos(ctx)
    findings = [{"level": "info", "message": f"{r['name']}: draft a commit"} for r in repos]
    print(json.dumps({"schema": "haw.plugin.report/1", "plugin": "commit-ai",
                       "ok": True, "summary": f"{len(repos)} repo(s)", "findings": findings}, indent=2))

def emit_view(ctx):
    repos = changeset_repos(ctx) or context_repos(ctx)
    lines = [f"{r['name']:<16} draft a commit" for r in repos] or ["nothing to commit"]
    print(json.dumps({"schema": "haw.plugin.view/1",
                      "title": "commit-ai — proposed commits", "lines": lines}))
```

And the MCP face — the tools Claude will call. We use the official SDK's **FastMCP**:

```python
def run_mcp():
    try:
        from mcp.server.fastmcp import FastMCP
    except ImportError:
        sys.stderr.write("haw-commit-ai --mcp needs the MCP SDK: pip install mcp\n")
        return 1
    mcp = FastMCP("haw-commit-ai")

    def _repo_path(ctx, repo):
        return next((r.get("path") for r in context_repos(ctx) if r.get("name") == repo), None)

    def _within_root(root, path):                 # path-guard: writes stay inside root
        if not root or not path: return False
        root_abs, path_abs = os.path.realpath(root), os.path.realpath(path)
        return path_abs == root_abs or path_abs.startswith(root_abs + os.sep)

    @mcp.tool()
    def haw_context() -> dict:
        """Workspace root, current stack, and repos (name, path, rev, groups)."""
        ctx = read_context()
        return {"root": ctx.get("root"), "stack": ctx.get("stack"), "repos": context_repos(ctx)}

    @mcp.tool()
    def repo_diff(repo: str) -> str:
        """The staged+unstaged git diff for a repo — see what changed."""
        path = _repo_path(read_context(), repo)
        return repo_diff_text(path) if path else f"no repo named {repo!r}"

    @mcp.tool()
    def changeset_repos_tool() -> list:
        """Repos touched by the current changeset, else the dirty repos."""
        return changeset_repos(read_context())

    @mcp.tool()
    def write_commit(repo: str, message: str) -> str:
        """git commit -m in a repo. Path-guarded to the workspace root."""
        ctx = read_context(); path = _repo_path(ctx, repo)
        if not _within_root(ctx.get("root"), path):
            return f"refused: {repo!r} is outside the workspace root."
        rc, out, err = _run(["git", "-C", path, "commit", "-m", message], cwd=path)
        return f"committed {repo}:\n{out}" if rc == 0 else f"commit failed:\n{err or out}"

    @mcp.tool()
    def draft_pr(repo: str, title: str, body: str, submit: bool = False) -> str:
        """Return PR text. Dry by default — never pushes unless submit=True."""
        text = f"# {title}\n\n{body}"
        if not submit:
            return text + "\n\n(dry run — pass submit=True to run `haw change request`)"
        ctx = read_context()
        rc, out, err = _run(["haw", "change", "request", "--title", title, "--body", body],
                            cwd=ctx.get("root"))
        return f"{text}\n\n[request: {'ok' if rc == 0 else 'failed'}]\n{out or err}"

    mcp.run()
    return 0

def main():
    args = sys.argv[1:]
    if "-h" in args or "--help" in args:
        print("haw-commit-ai — draft commits/PRs; --mcp to serve Claude"); return 0
    if "--mcp" in args: return run_mcp()
    ctx = read_context()
    if "--format" in args and "json" in args: emit_report(ctx); return 0
    if os.environ.get("HAW_RENDER") == "1" or ctx.get("intent") == "render":
        emit_view(ctx); return 0
    repos = changeset_repos(ctx) or context_repos(ctx)
    print(f"haw-commit-ai — {len(repos)} repo(s). Run with --mcp to let Claude write.")
    return 0

if __name__ == "__main__":
    sys.exit(main())
```

<div class="callout note">

The listing above is the **Niveau 1** MCP face. The shipped
[`examples/plugins/haw-commit-ai/haw-commit-ai`](https://github.com/Nastwinns/hawser/tree/main/examples/plugins/haw-commit-ai)
is the fully-commented version (with a real conventional-commit skeleton and a PR-body
template) and *also* carries the **Niveau 2** cross-repo tools we add below. Both pass
`python3 -m py_compile` and run with zero deps in plugin mode.

</div>

Try the plugin face right now — no MCP, no Claude:

```bash
chmod +x haw-commit-ai
PATH="$PWD:$PATH" haw commit-ai              # human draft
PATH="$PWD:$PATH" haw commit-ai --format json # a haw.plugin.report/1
```

## 🔌 5. Wire the MCP server into Claude Code

First, the SDK (only for the `--mcp` face):

```bash
pip install mcp
```

Register the server with Claude Code — one command:

```bash
claude mcp add haw-commit-ai -- python3 /abs/path/to/haw-commit-ai --mcp
```

or, per project, drop it in `.mcp.json`:

```json
{
  "mcpServers": {
    "haw-commit-ai": {
      "command": "python3",
      "args": ["/abs/path/to/haw-commit-ai", "--mcp"]
    }
  }
}
```

Verify Claude sees the tools:

```bash
claude mcp list            # haw-commit-ai should be listed
```

Inside a Claude session, `/mcp` shows the connected server and its tools. At Niveau 1 that's
`haw_context`, `repo_diff`, `changeset_repos_tool`, `write_commit`, `draft_pr` — and once
you add Niveau 2, `changeset_diff` and `draft_changeset_pr` join them.

## 🎬 6. Worked example — Claude writes your commit + PR text

Make a change across two repos in your workspace (say `kernel` and `hal`), stage them, then
ask Claude — inside the workspace directory:

> *"Read the diffs for the repos touched by my current changeset and write a
> conventional-commit message for each. Then draft a single cross-repo PR body. Commit each
> repo with its message; leave the PR as a dry draft."*

Claude will:

1. call **`changeset_repos_tool()`** → sees `kernel`, `hal`,
2. call **`repo_diff("kernel")`** and **`repo_diff("hal")`** → reads exactly what changed,
3. write conventional-commit messages (e.g. `fix(kernel): guard against null irq handler`),
4. call **`write_commit("kernel", …)`** and **`write_commit("hal", …)`** — each
   **path-guarded** to your workspace,
5. call **`draft_pr("kernel", "…", "…")`** → returns a PR body **dry** (nothing pushed).

You review the drafts, and when you're happy, run `haw change request` yourself (or let
Claude call `draft_pr(..., submit=True)`).

<div class="callout note">

**Honnêteté niveau 1.** À ce stade, Claude Code voit déjà le diff d'un seul repo
nativement — tu ne lui as pas donné de superpouvoir, tu lui as juste appris à parler le
protocole haw proprement. Le vrai pouvoir arrive au **niveau 2** : lui montrer un
changeset *entier*, réparti sur plusieurs repos, en une seule vue.

</div>

# 🚀 Niveau 2 : la vision cross-repo (changeset-wide)

Here's the pitch, sharp: **Claude alone can't see a fleet-wide changeset.** It can read one
repo's diff — but a haw changeset spans N repos at once (`kernel`, `hal`, `app`…), and
that combined story lives *between* the repos. lazygit can't show it either; it's a
single-repo tool. haw knows the whole changeset, so haw can hand Claude the whole picture.

We add **two tools to the same plugin** — no new script, no new server. They turn
`haw-commit-ai` from "a nice commit helper" into "the thing that gives an LLM fleet-wide
vision."

## 🧩 7. Two cross-repo tools

Drop these alongside the Niveau 1 tools (the shipped
[`examples/plugins/haw-commit-ai/haw-commit-ai`](https://github.com/Nastwinns/hawser/tree/main/examples/plugins/haw-commit-ai)
already has them):

```python
def changeset_diff_text(ctx):
    """The COMBINED git diff across every repo of the current changeset."""
    repos = changeset_repos(ctx) or context_repos(ctx)
    if not repos:
        return "no changeset and no dirty repos — nothing to diff."
    chunks = []
    for r in repos:                                  # clear per-repo headers
        diff = repo_diff_text(r["path"]) if r.get("path") else ""
        body = diff.rstrip() if diff.strip() else "(no changes)"
        chunks.append(f"=== {r.get('name','?')} ===\n{body}")
    return "\n\n".join(chunks)                        # the whole story, top to bottom
```

`changeset_diff()` is the **killer tool**: one call, and Claude sees `kernel`, `hal`, and
`app`'s diffs concatenated under `=== <repo> ===` headers — the fleet-wide changeset as a
single readable document.

```python
def draft_changeset_pr_body(ctx, title):
    """ONE coherent cross-repo PR skeleton narrating every repo together."""
    repos = changeset_repos(ctx) or context_repos(ctx)
    lines = [f"# {title}", "", "## Combined summary", "",
             "<!-- one narrative covering all repos and why they move together -->", "",
             "## Per-repo changes", ""]
    for r in repos:
        files, changed = diff_stat(r.get("path", "")) if r.get("path") else (0, 0)
        stat = f" ({files} file(s), {changed} line(s))" if files or changed else ""
        lines += [f"### {r.get('name','?')}{stat}", "", "<!-- what changed here and why -->", ""]
    lines += ["## Testing", "", "- [ ] `haw build`", "- [ ] `haw test`", ""]
    return "\n".join(lines)
```

`draft_changeset_pr(title)` returns **one** PR body — the plugin assembles the skeleton
(per-repo sections + a combined-summary slot), and **Claude fills the prose** from the
diffs. Dry by default; feed the result to `haw change request` to open the linked PRs
across the fleet.

Both are wrapped as MCP tools with the FastMCP decorator, exactly like the Niveau 1 ones:

```python
    @mcp.tool()
    def changeset_diff() -> str:
        """The COMBINED git diff across ALL changeset repos, with =-headers per repo.
        The fleet-wide view a single-repo tool can't give you."""
        return changeset_diff_text(read_context())

    @mcp.tool()
    def draft_changeset_pr(title: str) -> str:
        """ONE coherent cross-repo PR skeleton narrating every repo together.
        Dry — assembles the scaffold; you fill the prose, then `haw change request`."""
        return draft_changeset_pr_body(read_context(), title)
```

<div class="callout note">

**`write_commit` stays path-guarded.** There's no cross-repo "write everything" tool by
design — for a changeset you commit **per repo** (Claude calls `write_commit` for each,
each guarded to `root`), then run `haw change request` to open the linked PRs across the
fleet. Writes stay small, reviewable, and inside your workspace.

</div>

## 🎬 8. Worked example — one PR for a three-repo changeset

Touch two or three repos in a changeset — say `kernel`, `hal`, and `app` — stage them, then
ask Claude, inside the workspace:

> *"Call `changeset_diff()` to read my whole changeset, then `draft_changeset_pr()` and
> write one PR that tells the combined story — a section per repo plus a combined summary."*

Claude will:

1. call **`changeset_diff()`** → one document with `=== kernel ===`, `=== hal ===`,
   `=== app ===`, each repo's diff underneath,
2. call **`draft_changeset_pr("…")`** → gets the skeleton with a section per repo,
3. **fill the prose** into a single coherent narrative, e.g.:

```markdown
# feat: propagate the new irq-mask flag end to end

## Combined summary
A new `irq_mask` flag flows from the kernel driver up through the HAL and into
the app's config surface. The three repos move together so the feature lands atomically.

## Per-repo changes
### kernel
Add `irq_mask` to the driver's register write and guard the null-handler path.
### hal
Thread `irq_mask` through the HAL's `configure()` and expose it in the C header.
### app
Surface `--irq-mask` on the CLI and wire it to the HAL call.
```

**This is what neither Claude nor lazygit can do alone.** A single-repo tool sees three
disconnected diffs; haw + this plugin hand Claude the *changeset*, so it writes the one
story that spans them. When you're happy, commit each repo (`write_commit`, per repo) and
run `haw change request` to open the linked PRs across the fleet.

## 🔒 9. Safety notes — this is the important bit

Writing tools + an LLM means guardrails matter. This plugin bakes them in:

- **Path-guarded writes.** `write_commit` and `draft_pr(submit=True)` refuse any repo path
  that isn't inside the workspace `root` — Claude can't commit outside your fleet.
- **Dry by default.** `draft_pr` returns text only; it never pushes or force-pushes unless
  you explicitly pass `submit=True`.
- **No secrets in the plugin.** Forge auth comes from *your* environment — haw's normal
  token resolution (`GITHUB_TOKEN`, etc.). The plugin stores nothing.
- **Separate process, honest exit codes.** The plugin runs out-of-process; a bug can't
  crash haw, and a non-zero exit propagates so CI still gates.

<div class="your-turn">
<strong>🙌 À toi de jouer</strong>
<ul>
<li>Scaffold your own: <code>haw plugins new commit-ai --lang python</code>, then run the zero-dep face with <code>haw commit-ai --format json</code> and confirm you get a <code>haw.plugin.report/1</code> document.</li>
<li>Drop the plugin on <code>PATH</code>, open <code>haw dash</code>, press <code>7</code>, and select <code>commit-ai</code> — your <code>haw.plugin.view/1</code> panel renders right in the cockpit.</li>
<li><code>pip install mcp</code>, register it with <code>claude mcp add …</code>, and ask Claude to read one repo's diff and propose a commit — <em>without</em> committing. Then let it call <code>write_commit</code> and watch the path-guard in action by asking it to commit a path outside the workspace (it should refuse).</li>
<li><strong>Niveau 2 :</strong> touch <em>three</em> repos in a changeset, then ask Claude to call <code>changeset_diff()</code> and <code>draft_changeset_pr()</code> and write <strong>one</strong> PR narrative covering all three (kernel / hal / app + a combined summary). Compare it to what you'd get asking Claude repo-by-repo — the cross-repo story only appears when it sees the whole changeset at once.</li>
<li>Extend it: add a <code>repo_log(repo, n)</code> tool so Claude can see recent history for better messages. Keep it read-only.</li>
</ul>
</div>

## ✅ Recap

- A plugin is any executable named `haw-<name>` on `PATH`; haw hands it the
  **`haw.plugin/1`** context via `$HAW_JSON` / stdin and propagates its exit code.
- It can print a **`haw.plugin.report/1`** (`--format json`) and a **`haw.plugin.view/1`**
  panel (render intent, `HAW_RENDER=1`) for the cockpit's Plugins view (`7`).
- The same script can be an **MCP server** (`--mcp`) using FastMCP, exposing small tools so
  **Claude** reads your diffs and writes the commit + PR text.
- **Niveau 1 (mono-repo)** teaches the protocol: `haw_context`, `repo_diff`,
  `changeset_repos_tool`, `write_commit`, `draft_pr` — but Claude already sees one repo
  natively.
- **Niveau 2 (cross-repo)** is the real power: `changeset_diff` hands Claude the *combined*
  diff across the whole changeset, and `draft_changeset_pr` gets it to write **one**
  fleet-wide PR narrative — the cross-repo story neither Claude nor lazygit can tell alone.
- **Guardrails:** path-guard writes to inside `root` (commit per repo, then
  `haw change request`), keep PR drafting dry by default, and never store secrets — auth
  stays in your env.

## 👉 Where to next

You can now extend haw in any language and give an LLM safe, context-rich tools. From here:

- Browse the [Plugins reference](../PLUGINS.md) — lifecycle phases, the community index, and
  the language bindings.
- Study more real manifests in the [Examples index](../EXAMPLES.md).
- Keep the [CLI design & keymap](../CLI-DESIGN.md) handy.

That's the whole tool — now go build your own beam. Welcome aboard. ⚓
