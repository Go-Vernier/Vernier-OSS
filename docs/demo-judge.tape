# The README demo (docs/demo.gif): the blast radius of one file in
# robot-shop, then the same change judged by Claude Code.
#
# --judge needs a signed-in Claude Code, so this is recorded locally, not in
# CI. From the repository root, with vernier on PATH and robot-shop at
# corpus/robot-shop:
#
#   vhs docs/demo-judge.tape
#
# VHS writes the raw recording to demo-judge.mp4; scripts/demo-gif.sh then
# speeds up the agent's wait (about 30 s, a still screen) and writes
# docs/demo.gif.

Output demo-judge.mp4

Set Shell bash
Set FontSize 14
Set Width 1200
Set Height 900
Set Padding 20
Set TypingSpeed 45ms
Set Theme "Catppuccin Mocha"

Hide
Type "cd corpus && export PS1='$ ' && clear"
Enter
Show

Type "vernier robot-shop --files cart/server.js"
Sleep 400ms
Enter
Sleep 4.5s

Hide
Type "clear"
Enter
Show

Type "vernier robot-shop --files cart/server.js --judge"
Sleep 400ms
Enter
Wait@180s /^\$ *$/
Sleep 9s
