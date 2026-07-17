# Elearning: cross-repo changesets — one feature branch across many repos.
# Regenerate locally with: vhs demo/cli-changeset.tape   (needs the release binary built)

Output demo/cli-changeset.gif

Set FontSize 18
Set Width 1100
Set Height 620
Set Padding 16
Set Theme "Catppuccin Mocha"
Set TypingSpeed 55ms

Hide
Type `export PATH="$PWD/target/release:$PATH" CLICOLOR_FORCE=1 && . demo/setup.sh && haw sync --stack gateway >/dev/null 2>&1`
Enter
Sleep 2s
Show

Sleep 1s

Type "# A changeset is one logical feature spanning several repos."
Enter
Type "# `change start` creates the SAME branch across only the repos you name."
Enter
Type "haw change start FEAT-42 --repos kernel,app-mqtt"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# `change status` is the per-repo dashboard: branch, on-it, dirty, HEAD, PR."
Enter
Type "haw change status FEAT-42"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# Prove it: only the named repos moved onto change/FEAT-42; hal stays on main."
Enter
Type "haw run 'git rev-parse --abbrev-ref HEAD'"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# `change list` records every changeset under .haw/changesets."
Enter
Type "haw change list"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# When a forge token is set, these open + merge cross-linked PR/MRs:"
Enter
Type "#   haw change request FEAT-42   # push branches, open PR/MRs"
Enter
Type "#   haw change land FEAT-42      # merge them in dependency order"
Enter
Sleep 4s

Sleep 2s
