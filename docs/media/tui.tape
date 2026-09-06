# Regenerate:  vhs docs/media/tui.tape   (run from the repo root)
Output docs/media/tui.gif

Set Shell "bash"
Set FontSize 18
Set Width 1180
Set Height 820
Set Padding 24
Set Theme "Catppuccin Mocha"
Set TypingSpeed 45ms

Hide
Type "export PATH=$PWD/ktctl/target/release:$PATH; PS1='$ '; clear"
Enter
Show

Sleep 600ms
Type "ktctl --fake"
Sleep 400ms
Enter
Sleep 1.8s

# Move across the bands
Right@500ms Right Right
Sleep 800ms

# Boost the selected band
Up@250ms Up Up Up Up Up Up
Sleep 1s

# Move back and cut another band
Left@500ms Left
Sleep 500ms
Down@250ms Down Down Down
Sleep 1s

# Nudge frequency, cycle preset, write to device
Type "]]]"
Sleep 800ms
Type "p"
Sleep 1s
Type "w"
Sleep 1.5s

# Quit
Type "q"
Sleep 800ms
