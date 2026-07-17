# Renders demo/haw-cli.gif — the colorful CLI walkthrough.
# Regenerate locally with: vhs demo/cli.tape   (needs the release binary built)

Output demo/haw-cli.gif

Set FontSize 18
Set Width 1100
Set Height 620
Set Padding 16
Set Theme "Catppuccin Mocha"
Set TypingSpeed 70ms

Hide
Type `export PATH="$PWD/target/release:$PATH" CLICOLOR_FORCE=1 && . demo/setup.sh`
Enter
Sleep 2s
Show

Sleep 1s

Type "haw sync --stack gateway"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "haw tree"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "echo tweak >> hal/README.md"
Sleep 400ms
Enter
Sleep 1s
Enter
Sleep 500ms

Type "haw status"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "haw change start FEAT-42 --repos kernel,app-mqtt"
Sleep 400ms
Enter
Sleep 3s
Enter
Sleep 500ms

Type "haw change status FEAT-42"
Sleep 400ms
Enter
Sleep 4s

Sleep 2s
