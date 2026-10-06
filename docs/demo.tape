# The README demo: the blast radius of one file in robot-shop, then the TUI.
# Recorded by .github/workflows/demo.yml, with vernier on PATH and
# instana/robot-shop cloned into ./robot-shop.

Output demo.gif

Set Shell bash
Set FontSize 16
Set Width 1200
Set Height 720
Set Padding 20
Set TypingSpeed 50ms
Set Theme "Catppuccin Mocha"

Hide
Type "export PS1='$ ' && clear"
Enter
Show

Type "vernier robot-shop --files cart/server.js"
Sleep 500ms
Enter
Sleep 4s

Type "vernier tui robot-shop"
Sleep 500ms
Enter
Sleep 2s
Type "2"
Sleep 1.5s
Type "j"
Sleep 400ms
Type "j"
Sleep 800ms
Enter
Sleep 3s
Type "+"
Sleep 2s
Type "q"
Sleep 500ms
