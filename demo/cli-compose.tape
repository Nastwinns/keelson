# Elearning: composition lifecycle — how haw materializes a stack of repos.
# Regenerate locally with: vhs demo/cli-compose.tape   (needs the release binary built)

Output demo/cli-compose.gif

Set FontSize 18
Set Width 1100
Set Height 620
Set Padding 16
Set Theme "Catppuccin Mocha"
Set TypingSpeed 55ms

Hide
Type `export PATH="$PWD/target/release:$PATH" CLICOLOR_FORCE=1 && . demo/setup.sh`
Enter
Sleep 2s
Show

Sleep 1s

Type "# haw.toml declares repos + stacks. `tree` shows the intended composition."
Enter
Type "haw tree"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# `sync` clones every repo in a stack and writes haw.lock (reproducible)."
Enter
Type "haw sync --stack gateway"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# `status` is the fleet dashboard: branch / head / dirty / drift per repo."
Enter
Type "haw status"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# `lock` resolves each repo's rev (a branch) to an exact SHA in haw.lock."
Enter
Type "haw lock"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# `pin` snapshots whatever is checked out right now into the lock (no network)."
Enter
Type "haw pin"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# `switch` records another stack as current and materializes just its repos."
Enter
Type "haw switch sensor-node"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# `tree` again — sensor-node is a smaller slice of the same repos."
Enter
Type "haw tree"
Sleep 400ms
Enter
Sleep 3s

Sleep 2s
