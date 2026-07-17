# 3. The daily workflow

You can compose a fleet and pin it. Now let's make it *earn its keep* day to day. The
promise of a multi-repo tool is simple: do something across every repo at once, in
parallel, without a hand-rolled `for` loop.

In this chapter you'll run arbitrary commands across the fleet, wire up `build` and
`test`, search every repo in one shot, and control parallelism and scope. Keep the
`my-first-stack` workspace from the last chapter open.

<img class="chapter-illus" src="../assets/img/data-processing.svg" alt="Running commands across the whole fleet in parallel">

*One command in, the whole fleet processed in parallel — no hand-rolled `for` loop.*

<div class="objectives">
<strong>🎯 In this chapter, you'll learn to…</strong>
<ul>
<li>Fan any command across every repo with <code>haw run</code>.</li>
<li>Declare <code>build =</code> / <code>test =</code> per repo and drive the whole fleet with <code>haw build</code> / <code>haw test</code>.</li>
<li>Search every repo at once with <code>haw grep</code>.</li>
<li>Control the blast radius with <code>-j</code> (parallelism) and <code>--group</code> (scope).</li>
<li>Pick the right verb for each job without thinking.</li>
</ul>
</div>

![Running commands and verifying across the fleet](../assets/cli-run-verify.gif)

*`haw run` fans a command across every repo in parallel; `verify` gates the fleet against the lock.*

## ⚙️ 1. Run any command across every repo — `haw run`

The workhorse. `haw run` takes a command *positionally* and runs it in every repo, in
parallel:

```bash
haw run 'git log -1 --oneline'
```

```console
hello-world   7fd1a60 Merge pull request #6 from Spoon-Knife/patch-1
spoon-knife   d0dd1f6 Pointing to the guide for forking
```

`haw` groups the output per repo so you always know which repo said what. Anything you'd
type in one repo works: `git fetch`, `git status -s`, a linter, a shell one-liner.

<div class="callout tip">

**Tip:** Quote the command. `haw run 'git status -s'` passes the whole string as one
command; without quotes your shell would try to interpret the flags itself.

</div>

## 🔨 2. Declare build and test — then run them fleet-wide

`haw` stays build-system-agnostic: each repo names the shell command that builds or tests
*it*, and `haw` fans those out. You declare them in the manifest with `build =` and
`test =`.

Our octocat repos have nothing to compile, but the mechanism is what matters. Here's the
shape, from the shipped [`microservices`](https://github.com/Nastwinns/hawser/tree/main/examples/microservices)
example — note how each repo brings its *own* toolchain:

```toml
[repo.proto]
build = "buf generate"
test  = "buf lint"

[repo.gateway]
build = "go build ./..."
test  = "go test ./..."

[repo.billing]
build = "cargo build --release"
test  = "cargo test"

[repo.accounts]
build = "npm ci && npm run build"
test  = "npm test"
```

With those declared, two commands drive the whole fleet:

```bash
haw build     # runs every repo's `build =`, in parallel
haw test      # runs every repo's `test =`, in parallel
```

Repos that don't declare the command (or aren't cloned) are simply skipped. And here's
the detail that makes this a CI building block:

<div class="callout note">

**CI building block:** `haw build` and `haw test` **exit non-zero if any repo fails**.
That's why the same command you run locally drops straight into a pipeline — the pipeline
stops when any repo's build breaks.

</div>

## 🔎 3. Search the whole fleet — `haw grep`

Need to find every use of a symbol, a TODO, a deprecated API across *all* repos? `haw
grep` fans `git grep` across every cloned repo at once:

```bash
haw grep TODO
```

```console
hello-world
  README:12:  TODO: add usage examples

spoon-knife
  index.html:8:  <!-- TODO: link the guide -->
```

Results are grouped per repo, just like `run`. It searches tracked files via Git, so
it's fast and ignores your build artifacts for free. Scope it to one stack with
`--stack <name>` when the fleet is large.

## 🎚️ 4. Control the blast radius: parallelism and groups

Two levers keep fleet-wide commands under control.

**Parallelism — `-j`.** By default `haw` runs up to `min(cores, 8)` repos at once. Cap it
when a task is heavy (or a CI runner is small):

```bash
haw test -j 4     # at most 4 repos building/testing at a time
haw run -j 1 'git fetch'   # fully serial
```

**Groups — `--group`.** Remember the `groups = [...]` labels from your manifest? They
exist precisely so you can act on a slice of the fleet. In `my-first-stack`, `hello-world`
is in group `core` and `spoon-knife` is in `web`:

```bash
haw status --group core
```

```console
REPO          BRANCH   HEAD       DIRTY  DRIFT
hello-world   master   7fd1a60b   -      -
```

Only the `core` repo. The same `--group` filter works on `sync`, `status`, and `run` (and
`build`/`test`), and it's *repeatable* — pass it twice to select two groups. An empty
filter means everything; a filter excludes ungrouped repos.

```bash
haw run --group core 'git log -1 --oneline'   # only core repos
```

## 🗺️ 5. When to reach for each

A quick map so you pick the right verb without thinking:

| You want to… | Use | Why |
|---|---|---|
| run an ad-hoc command everywhere | `haw run '<cmd>'` | one-off, not part of the manifest |
| build the whole product | `haw build` | runs each repo's declared `build =`, CI-ready exit code |
| test the whole product | `haw test` | runs each repo's declared `test =`, fails on any failure |
| find text across all repos | `haw grep <pat>` | fleet-wide `git grep`, grouped output |
| limit to part of the fleet | add `--group <g>` | act on a labeled slice |
| tame a heavy or CI run | add `-j <n>` | cap concurrent repos |

The rule of thumb: **`run` for one-off commands, `build`/`test` for the commands your repos
declare.** The declared ones are the ones you'll want identical locally and in CI.

<div class="your-turn">
<strong>🙌 Your turn</strong>
<p>Put the fleet through its paces in <code>my-first-stack</code>:</p>
<ul>
<li>Run <code>haw run 'git log -1 --oneline'</code> and confirm you get one line per repo, grouped by name.</li>
<li>Run <code>haw grep TODO</code> across the fleet. Then narrow it: <code>haw run --group core 'git log -1 --oneline'</code> — only the <code>core</code> repo should answer.</li>
<li>Force it fully serial with <code>haw run -j 1 'git status -s'</code> and watch the repos process one at a time instead of in parallel.</li>
</ul>
</div>

## ✅ Recap

- `haw run '<cmd>'` runs any command in every repo in parallel, output grouped per repo.
- Declare `build =` / `test =` per repo in the manifest; `haw build` / `haw test` fan them
  out and **exit non-zero on any failure** — so they double as CI steps.
- `haw grep <pattern>` is a fleet-wide `git grep`.
- `-j N` caps parallelism; `--group G` (repeatable) scopes commands to labeled repos.

## 👉 Next

Running commands is table stakes. Now the signature move — shipping *one feature across
many repos* as a single coordinated changeset →
[4. Cross-repo changesets](04-cross-repo-changesets.md).
