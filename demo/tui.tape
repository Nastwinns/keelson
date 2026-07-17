# Renders demo/haw-tui.gif — the k9s-style fleet cockpit.
# Driven by the built-in demo controller: no workspace, no git, no network.
# Regenerate locally with: vhs demo/tui.tape   (needs the release binary built)

Output demo/haw-tui.gif

Set FontSize 18
Set Width 1100
Set Height 620
Set Padding 16
Set Theme "Catppuccin Mocha"
Set TypingSpeed 70ms

Hide
Type `export PATH="$PWD/target/release:$PATH"`
Enter
Type "clear"
Enter
Show

Sleep 1s

Type "haw dash --demo"
Sleep 400ms
Enter
Sleep 3s

# Walk the fleet grid — clean, dirty, drift, a not-cloned repo.
Type "j"
Sleep 1s
Type "j"
Sleep 1.2s

# Drill into a repo: branch, SHA, status, log, diff, remotes — see the code.
Enter
Sleep 3s
Type "j"
Sleep 900ms
Type "j"
Sleep 1s
Type "b"
Sleep 1.2s

# Sort the fleet (caret ▲/▼), then mark repos for a bulk action.
Type ">"
Sleep 1.2s
Type ">"
Sleep 1.2s
Type " "
Sleep 800ms
Type "j"
Sleep 400ms
Type " "
Sleep 1.5s

# Fleet-wide open PR/MRs — github + gitlab, with review + CI state.
Type "m"
Sleep 2.5s

# Drill into a PR: reviewers, CI checks, body.
Enter
Sleep 3s
Type "b"
Sleep 1.2s

# Merge it — confirm-gated (reaches the network). Cancel here.
Type "M"
Sleep 2.5s
Escape
Sleep 1s
Type "b"
Sleep 1.2s

# Recent CI runs, then drill in to jobs + steps.
Type "i"
Sleep 2s
Enter
Sleep 3s
Type "b"
Sleep 1s
Type "b"
Sleep 1.2s

# Switch theme live — six built-in skins, NO_COLOR aware.
Type ":theme dracula"
Sleep 400ms
Enter
Sleep 2.5s

# Governance — registered plugins, SBOM/provenance, findings.
Type "v"
Sleep 2.5s
Type "b"
Sleep 1.2s

Type "q"
Sleep 1.5s
