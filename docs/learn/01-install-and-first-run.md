# 1. Install and first run

In this chapter you'll get the `haw` binary onto your machine, confirm it works, turn on
tab completion — and then take your very first `haw` run, watching the keyboard cockpit
open. It's short: we want you at a prompt fast, and we want you to *see* where the course
is heading.

<img class="chapter-illus" src="../assets/img/home-settings.svg" alt="Setting up the haw tool on your machine">

*A one-time setup, then you're at the prompt for the rest of the course.*

<div class="objectives">
<strong>🎯 In this chapter, you'll learn to…</strong>
<ul>
<li>Install <code>haw</code> through the channel that fits your machine — Cargo, Homebrew, Scoop, or the static musl binary.</li>
<li>Confirm the binary is on your <code>PATH</code> and prints <code>haw 0.1.7</code>.</li>
<li>Turn on shell tab-completion so the shell fills in subcommands and flags as you learn.</li>
<li>Take a first <code>haw</code> run and watch the cockpit open — a taste of where you're headed.</li>
</ul>
</div>

The tool ships as a **single binary named `haw`**. There's no runtime, no interpreter,
nothing to keep updated alongside it. The current release is **v0.1.7**.

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
curl -sSL https://github.com/Nastwinns/hawser/releases/download/v0.1.7/haw-0.1.7-x86_64-unknown-linux-musl.tar.gz \
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
haw 0.1.7
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

## 🚁 4. Your first run — open the cockpit

Here's the reward. `haw` isn't only a batch of subcommands: run it with **no subcommand at
all** and it opens a full-screen, keyboard-driven **cockpit** for your fleet.

```bash
haw
```

You don't have a workspace yet, so there's nothing real to show. Good news: there's a
built-in **demo controller**, populated with canned repos, PRs, and CI runs, so every view
has something in it — no network, no setup:

```bash
haw dash --demo
```

A live fleet grid fills the terminal. Move with `↑`/`↓` (or `k`/`j`, Vim-style), press
`Enter` to drill into a repo, `Esc` to come back, `?` for the help overlay, and `q` to
quit.

![The hawser TUI cockpit — mission control for the fleet](../assets/haw-tui.gif)

*The cockpit you just opened: the fleet grid, drill-downs, and keyboard actions — all in the terminal.*

<div class="callout note">

**Just a taste for now.** Poke around, then quit with `q`. We'll give the cockpit a proper
guided tour in [Chapter 4](04-the-tui-cockpit.html) — first you need a real fleet to point
it at, which is what the next three chapters build.

</div>

## ✅ Recap

- `haw` is a single binary — install it with `cargo`, `brew`, `scoop`, or the static
  musl archive.
- `haw --version` should print `haw 0.1.7`; `haw --help` lists every command.
- `haw completions <shell>` gives you tab completion — set it up now, thank yourself
  later.
- Bare `haw` (or `haw dash --demo`) opens the cockpit — your first glimpse of mission
  control, explored in depth in Chapter 4.
- The [full install matrix](../INSTALL.md) covers signed releases and air-gapped hosts.

<div class="your-turn">
<strong>🙌 Your turn</strong>
<p>Two-minute checkpoint — prove your setup before moving on:</p>
<ul>
<li>Run <code>haw --version</code> and confirm you see <code>haw 0.1.7</code>. If it says "command not found", the install dir isn't on your <code>PATH</code> yet — fix that first.</li>
<li>Set up completions for your shell, restart it, then type <code>haw sy</code> and press <code>Tab</code>. It should complete to <code>haw sync</code>.</li>
<li>Run <code>haw dash --demo</code>, move around with <code>↑</code>/<code>↓</code>, press <code>Enter</code> to drill into a repo and <code>Esc</code> to back out, then <code>q</code> to quit. You just met the cockpit.</li>
</ul>
</div>

## 👉 Next

Now let's give `haw` something real to manage. First stop: the manifest — where you
*declare* your fleet → [2. The manifest](02-the-manifest.md).
