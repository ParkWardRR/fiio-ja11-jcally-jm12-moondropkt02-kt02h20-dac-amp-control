# Regenerate:  vhs docs/media/cli.tape   (run from the repo root)
Output docs/media/cli.gif

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

Sleep 700ms
Type "ktctl --fake probe"
Sleep 400ms
Enter
Sleep 1.6s

Type "ktctl --fake peq get"
Sleep 400ms
Enter
Sleep 2.2s

Type "ktctl --fake peq set 2 --gain 4 --q 1.4"
Sleep 400ms
Enter
Sleep 1.8s

Type "ktctl --fake state"
Sleep 400ms
Enter
Sleep 2.2s
