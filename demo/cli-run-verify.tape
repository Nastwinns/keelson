# Elearning: run + verify — parallel fan-out and the CI drift gate.
# Regenerate locally with: vhs demo/cli-run-verify.tape   (needs the release binary built)

Output demo/cli-run-verify.gif

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

Type "# Start from a clean, synced fleet."
Enter
Type "haw status"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# `run` executes one command in every repo, in parallel."
Enter
Type "haw run 'git rev-parse --short HEAD'"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# `verify` asserts the on-disk tree matches haw.lock. Clean tree -> exit 0."
Enter
Type "haw verify"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# Now dirty one repo to simulate uncommitted drift."
Enter
Type "echo tweak >> hal/README.md"
Sleep 400ms
Enter
Sleep 1s
Enter
Sleep 500ms

Type "# `status` flags hal as DIRTY."
Enter
Type "haw status"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "# `verify` now exits 3 — the CI gate that fails a build on drift."
Enter
Type `haw verify; echo "exit code: $?"`
Sleep 400ms
Enter
Sleep 4s

Sleep 2s
