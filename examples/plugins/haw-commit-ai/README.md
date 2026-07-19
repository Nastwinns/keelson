# haw-commit-ai

A haw plugin that drafts **commit messages** and **PR text** from your workspace
— and doubles as an **MCP server** so Claude (Claude Code / Claude Desktop) reads
your diffs and writes the real text using haw's context.

Two faces, one script (`haw-commit-ai`):

1. **As a haw plugin** — `haw commit-ai`
   Reads the `haw.plugin/1` context, runs `git diff` per touched repo, and prints
   a conventional-commit skeleton + a PR body skeleton.
   - `haw commit-ai --format json` → a `haw.plugin.report/1` document.
   - In the cockpit's **Plugins** view (`7`), it renders a `haw.plugin.view/1`
     panel listing each repo + its proposed commit subject.
   - **Zero dependencies** — stdlib only.

2. **As an MCP server for Claude** — `haw-commit-ai --mcp` (stdio)
   Exposes small, safe tools so Claude does the writing.

   **Niveau 1 — mono-repo** (learn the protocol):

   | Tool | Does |
   | --- | --- |
   | `haw_context()` | workspace root, current stack, repos |
   | `repo_diff(repo)` | staged+unstaged `git diff` for a repo |
   | `changeset_repos_tool()` | repos touched by the current changeset (else dirty repos) |
   | `write_commit(repo, message)` | `git commit -m` — **path-guarded** to the workspace root |
   | `draft_pr(repo, title, body)` | returns PR text; **dry by default** (no push) |

   **Niveau 2 — cross-repo** (the real power of haw):

   | Tool | Does |
   | --- | --- |
   | `changeset_diff()` | the **combined** diff across **all** changeset repos, with `=== <repo> ===` headers |
   | `draft_changeset_pr(title)` | **one** coherent cross-repo PR skeleton narrating every repo together |

## Install

```console
$ chmod +x haw-commit-ai
$ export PATH="$PWD:$PATH"       # or copy haw-commit-ai into ~/.local/bin
$ haw commit-ai --help
```

MCP mode needs the official SDK (the plugin face does not):

```console
$ pip install mcp
```

## Wire it into Claude Code

```console
$ claude mcp add haw-commit-ai -- python3 /abs/path/to/haw-commit-ai --mcp
```

or add it to a project `.mcp.json`:

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

Then ask Claude, inside your haw workspace: *"read the diffs and write a
conventional-commit message for each touched repo, then a cross-repo PR body."*
Claude calls `repo_diff` → `write_commit` / `draft_pr`.

## Safety

- **Path-guarded writes** — `write_commit` / `draft_pr` refuse any path outside
  the workspace `root`.
- **Dry PR drafting** — `draft_pr` returns text only; it never pushes or force-pushes.
- **No secrets in the plugin** — forge auth comes from your env (haw's normal
  token resolution).

See the full tutorial: [docs/learn/08-build-a-plugin-mcp.md](../../../docs/learn/08-build-a-plugin-mcp.md).
