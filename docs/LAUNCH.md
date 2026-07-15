# Keelson — Launch & community playbook

How to talk about Keelson on Reddit (and beyond) without getting downvoted or shadow-banned.
Reddit is allergic to marketing. The only frame that works: *"I built this to fix my own
problem — here's a gif."* This doc has the timing gate, the media assets to prepare, the
per-subreddit playbook, and copy-ready post drafts.

Nothing here is posted by the tooling — publishing to Reddit/HN is a human action. These are
drafts and a checklist for **you** to post when the gate is met.

---

## 0. Timing gate (do NOT post before this)

A project only gets one first impression per community. Posting a repo that doesn't run burns
it. Post only when **all** of these are true:

- [ ] Phase 1 runs: `haw init` → `sync` → `tree`/`status` → `change start` work end to end.
- [ ] `git init` + real commits + a public GitHub repo (README already ships).
- [ ] **A gif of the TUI cockpit** (the single most important asset — see §1).
- [ ] `cargo install hawser` works, or a one-line install is documented.
- [ ] README quick start copy-pastes and works on a clean machine.

Until then: build in private, tease nothing.

---

## 1. Media assets to prepare

Store under `media/` in the repo. The gif is the hero; everything else is secondary.

| Asset | Size | Use | Tool |
|-------|------|-----|------|
| `media/haw-tui.gif` | ~1200×700, <5 MB, <20 s loop | r/commandline, r/rust, README top | **vhs** (charmbracelet) |
| `media/keel-tree.png` | terminal screenshot | inline in posts | native screenshot |
| `media/keel-status.png` | terminal screenshot | inline | native |
| `media/social-card.png` | 1200×630 | HN/X link preview | any |

**Make the gif reproducible with `vhs`** (a tape file → identical gif every time):

```tape
# media/demo.tape  —  run: vhs media/demo.tape
Output media/haw-tui.gif
Set FontSize 16
Set Width 1200
Set Height 700
Set Theme "Catppuccin Mocha"
Type "keel"          Enter   Sleep 2s      # cockpit opens
Type "j"  Sleep 400ms  Type "j"  Sleep 800ms  # move cursor
Type "s"  Sleep 2s                          # sync the cursor repo
Type "c"  Sleep 1500ms                      # change menu
Type "q"  Sleep 500ms
```

Gif rules: <20 s, loops cleanly, big font (readable on mobile), shows the *value* (fleet
state + one action), not a menu tour. Keep it under ~5 MB so Reddit/GitHub inline it.

---

## 2. Reddit norms (break these = downvoted or banned)

- **9:1 rule.** For every self-promo link, ~9 non-promo comments/contributions. Don't drop a
  link into a sub you've never participated in.
- **No marketing voice.** "I built X because Y annoyed me" — never "Introducing X, the
  revolutionary…". Downvotes are instant.
- **Gif or it didn't happen.** For CLI/TUI subs, a static repo link underperforms 10×.
- **One or two subs at a time**, spaced by days. Simultaneous cross-posting = spam flag.
- **Use the right flair** (`project`, `tooling`, `show`) and the weekly thread when one exists.
- **Answer every comment** in the first 2 hours — engagement drives the ranking.
- **Read each sub's rules first**; some ban links outright and require the weekly thread.

Best windows (weekday mornings US-Eastern unless noted):
- r/rust: Tue–Thu, 8–11am ET. Weekly "what are you working on" thread also good.
- r/commandline: flexible, weekends fine (gif-driven).
- HN: Tue–Thu, 8–10am ET.

---

## 3. Per-subreddit playbook

| Sub | Angle | Format | Watch out |
|-----|-------|--------|-----------|
| **r/rust** | a Rust CLI+TUI you built; gitoxide, no unsafe | gif + short "why" + repo | Post in the weekly thread first if unsure |
| **r/commandline** | k9s-style multi-repo cockpit | **gif first**, one line of text | No gif = ignored |
| **r/ROS** + **r/robotics** | end the `vcstool`/`wstool` multi-repo mess | technical, show a `ros2.repos`→`keel.toml` import | Frame as helpful, not an ad |
| **r/embedded** | `west`/`repo` refugees: lockfile, no Python, no detached HEAD | technical, concrete pain | Zero marketing tone or instant downvote |
| **r/devops** | reproducible multi-repo composition + CI | use the self-promo thread if the sub has one | Strict on promo; check rules |
| **r/opensource** | project launch, MIT/Apache, contributions welcome | launch post + gif | Fine for a genuine launch |

---

## 4. Copy-ready post drafts

Replace `<REPO_URL>` and attach the gif. Keep bodies short — Reddit rewards brevity.

### r/rust — standalone

> **Title:** Keelson: reproducible multi-repo composition + cross-repo PR/MR orchestration, in Rust
>
> I got tired of managing a product split across ~10 git repos with bash scripts and
> submodules, so I built `haw`. A TOML manifest declares your repos and *stacks* (named
> compositions); a committed lockfile pins every repo to an exact SHA so CI reproduces the
> exact tree. On top: start a feature branch across N repos at once and watch the whole fleet
> in a k9s-style TUI.
>
> Rust, `gitoxide` for fast native introspection, `#![forbid(unsafe_code)]`, Linux/macOS/Windows.
>
> Gif of the cockpit ↑. Repo: `<REPO_URL>` — feedback very welcome, especially from anyone
> living the multi-repo life.

### r/rust — weekly "what are you working on" thread

> `haw` — a Rust CLI+TUI for composing a product out of many git repos: a manifest + a
> committed lockfile (reproducible checkouts) plus cross-repo branch/PR orchestration, with a
> k9s-style fleet dashboard (ratatui). Just got the TUI cockpit working — gif: `<REPO_URL>`.

### r/commandline — gif-first

> **Title:** k9s-style TUI for managing a fleet of git repos [gif]
>
> `haw` — one screen for N repos: branch, dirty, drift-vs-lockfile, ahead/behind; `:` command
> bar, `/` filter, single-key sync/switch. Rust + ratatui, cross-platform. `<REPO_URL>`

### r/ROS (and r/robotics)

> **Title:** A reproducible alternative to vcstool/wstool for multi-repo ROS workspaces
>
> ROS workspaces spread across many repos and `vcstool` has no lockfile — CI and teammates
> drift. I built `haw`: a TOML manifest + a **committed lockfile** pinning every repo to an
> exact SHA, an `import` from a `.repos` file, plus a TUI to see the whole workspace at once.
> Rust, no Python runtime. `<REPO_URL>` — would love ROS folks to tell me what's missing.

### r/embedded

> **Title:** For anyone stuck with `repo`/`west`: a Rust tool with a real lockfile and no Python
>
> Managing a firmware product from shared BSP/HAL/MCAL repos with `west`/`repo` means no
> lockfile, a Python runtime, and detached HEADs. `haw` gives you: a committed lockfile
> (exact SHA per repo → reproducible + auditable), plain full clones (no detached HEAD, no
> symlinks — Windows-safe), and a fleet TUI. `import --from west.yml` to try it on your tree.
> `<REPO_URL>`

### r/devops

> **Title:** keel — reproducible multi-repo composition with a committed lockfile (Rust)
>
> A manifest + lockfile so `haw sync --locked` reproduces the exact multi-repo tree in CI,
> `haw verify` gates drift, `--format json` for pipelines. Cross-repo PR/MR orchestration on
> GitHub *and* GitLab. `<REPO_URL>`. (Posting in the self-promo thread per the rules.)

### r/opensource

> **Title:** Keelson — multi-repo product composition + cross-repo MR orchestration (Rust, MIT/Apache)
>
> Open-source `haw`: compose a product from many git repos reproducibly (manifest +
> committed lockfile) and drive cross-repo feature branches + PR/MRs from one TUI. Dual
> MIT/Apache-2.0, contributions welcome. `<REPO_URL>`

### Bonus — Hacker News (Show HN)

> **Title:** Show HN: Keelson – reproducible multi-repo composition and PR orchestration in Rust
>
> (Body, 3–4 sentences: the problem, what keel does, why Rust/gitoxide/lockfile, link + gif.
> Answer every comment fast. Post Tue–Thu ~8–10am ET.)

Also: submit to **This Week in Rust** (free Rust reach), **lobste.rs** (tags `rust`,
`devops`), the **ratatui showcase**, and the **Zephyr/`west` community** (Discord + list).

---

## 5. After posting

- Reply to every comment in the first 2 hours; convert feedback into GitHub issues live.
- Pin a "Roadmap / what's next" comment linking [ARCHITECTURE.md](ARCHITECTURE.md).
- Track: GitHub stars/day, `cargo install` count, issues opened, which sub drove traffic.
- Space the next sub 2–3 days out; reuse the gif, re-angle the title per audience.

## 6. Pre-post checklist

- [ ] Gate §0 fully met (it runs, it's public, gif exists).
- [ ] Gif < 5 MB, readable on mobile, shows value in < 20 s.
- [ ] Repo has: working README quick start, LICENSE, a 30-second install path.
- [ ] Correct flair chosen; sub rules read; weekly/self-promo thread used if required.
- [ ] Title is "I built X because Y", not a product pitch.
- [ ] You have 2 free hours after posting to answer comments.
