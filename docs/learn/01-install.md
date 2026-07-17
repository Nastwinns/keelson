# 1. Installing haw

In this chapter you'll get the `haw` binary onto your machine, confirm it works, and turn
on tab completion so the shell helps you as you learn. It's short — we want you at a
prompt fast.

<img class="chapter-illus" src="../assets/img/home-settings.svg" alt="Setting up the haw tool on your machine">

*A one-time setup, then you're at the prompt for the rest of the course.*

<div class="objectives">
<strong>🎯 In this chapter, you'll learn to…</strong>
<ul>
<li>Install <code>haw</code> through the channel that fits your machine — Cargo, Homebrew, Scoop, or the static musl binary.</li>
<li>Confirm the binary is on your <code>PATH</code> and prints <code>haw 0.1.3</code>.</li>
<li>Turn on shell tab-completion so the shell fills in subcommands and flags as you learn.</li>
</ul>
</div>

The tool ships as a **single binary named `haw`**. There's no runtime, no interpreter,
nothing to keep updated alongside it. The current release is **v0.1.3**.

## 📦 1. Install it

Pick the line that matches your setup — all of them install the *same* `haw` binary.

```bash
cargo install hawser                              # Rust / crates.io (canonical)
brew install nastwinns/tap/hawser                 # macOS + Linux (Homebrew)
```

On Windows, use Scoop:

```powershell
scoop bucket add nastwinns https://github.com/Nastwinns/scoop-bucket
scoop install hawser
```

On a Linux server, container, or air-gapped host, the **static musl binary** is the
easiest choice — it's fully static (no glibc, no runtime), so one file just runs:

```bash
curl -sSL https://github.com/Nastwinns/hawser/releases/download/v0.1.3/haw-0.1.3-x86_64-unknown-linux-musl.tar.gz \
  | tar xz && sudo install haw /usr/local/bin/
```

<div class="callout tip">

**Tip:** `cargo install hawser` is the canonical Rust install. It builds from source
and drops `haw` into `~/.cargo/bin` — make sure that directory is on your `PATH`.

</div>

For every other channel (`.deb`/`.rpm`, AUR, Nix, Docker), plus **signature verification**
and the full **air-gap workflow**, see [Installing hawser](../INSTALL.md).

## ✅ 2. Verify it works

Whatever channel you used, confirm the binary is on your `PATH`:

```bash
haw --version
```

You should see the version print:

```console
haw 0.1.3
```

If you get "command not found", the install directory isn't on your `PATH` yet — for
`cargo`, that's `~/.cargo/bin`. Fix that and re-run.

Now peek at the full command surface — you don't need to read it all, just get a feel:

```bash
haw --help
```

You'll see the subcommands we'll cover: `sync`, `status`, `tree`, `run`, `build`, `test`,
`change`, `plugins`, and more. Every one is a single guessable word.

![The haw command-line tool in action](../assets/haw-cli.gif)

*A quick taste of `haw` at the command line — every subcommand is one guessable word.*

## ⌨️ 3. Turn on shell completions

This is a small quality-of-life win that pays off all course long: press `Tab` and the
shell fills in subcommands and flags for you.

`haw completions <shell>` prints a completion script to stdout. Redirect it to the right
place for your shell:

```bash
haw completions zsh  > ~/.zfunc/_haw                 # zsh
haw completions bash > /etc/bash_completion.d/haw    # bash
haw completions fish > ~/.config/fish/completions/haw.fish   # fish
```

<div class="callout tip">

**Tip:** For zsh, make sure `~/.zfunc` is on your `$fpath` (add
`fpath=(~/.zfunc $fpath)` before `compinit` in your `~/.zshrc`), then restart your
shell. Now `haw sy<Tab>` completes to `haw sync`.

</div>

## ✅ Recap

- `haw` is a single binary — install it with `cargo`, `brew`, `scoop`, or the static
  musl archive.
- `haw --version` should print `haw 0.1.3`; `haw --help` lists every command.
- `haw completions <shell>` gives you tab completion — set it up now, thank yourself
  later.
- The [full install matrix](../INSTALL.md) covers signed releases and air-gapped hosts.

<div class="your-turn">
<strong>🙌 Your turn</strong>
<p>Two-minute checkpoint — prove your setup before moving on:</p>
<ul>
<li>Run <code>haw --version</code> and confirm you see <code>haw 0.1.3</code>. If it says "command not found", the install dir isn't on your <code>PATH</code> yet — fix that first.</li>
<li>Run <code>haw --help</code> and skim the subcommand list. Spot <code>sync</code>, <code>status</code>, <code>run</code>, and <code>change</code> — you'll meet every one of them soon.</li>
<li>Set up completions for your shell, restart it, then type <code>haw sy</code> and press <code>Tab</code>. It should complete to <code>haw sync</code>.</li>
</ul>
</div>

## 👉 Next

Time for the fun part — let's build your first stack from real repositories and watch
`haw` sync them → [2. Your first stack](02-your-first-stack.md).
