# Elearning: collaborative merge — slice one big merge into reviewable units.
# Regenerate locally with: vhs demo/cli-merge.tape   (needs the release binary built)

Output demo/cli-merge.gif

Set FontSize 18
Set Width 1100
Set Height 620
Set Padding 16
Set Theme "Catppuccin Mocha"
Set TypingSpeed 55ms

Hide
Type `export PATH="$PWD/target/release:$PATH" CLICOLOR_FORCE=1 && . demo/setup.sh && haw sync --stack gateway >/dev/null 2>&1 && haw_demo_setup_merge`
Enter
Sleep 2s
Show

Sleep 1s

Type "# Setup (hidden): kernel now has main and feature/tweak, both editing src/driver.c."
Enter
Type "# `merge plan` starts the merge on an integration branch and SLICES the conflicts."
Enter
Type "haw merge plan feature/tweak --repo kernel"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# `merge status` lists each slice and whether it's resolved yet (· = pending)."
Enter
Type "haw merge status --repo kernel"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# Resolve the `src` slice by taking our side (or edit by hand, then resolve)."
Enter
Type "haw merge resolve src --take ours --repo kernel"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# `merge status` again — the slice is now checked off (✓)."
Enter
Type "haw merge status --repo kernel"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# `merge cleanup` seals it: one merge commit, fast-forwards main, drops the temp branch."
Enter
Type "haw merge cleanup --repo kernel -m 'merge feature/tweak into kernel'"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# (If things go sideways, `haw merge abort --repo kernel` restores the target branch.)"
Enter
Type "haw run 'git log --oneline -3'"
Sleep 400ms
Enter
Sleep 4s

Sleep 2s
