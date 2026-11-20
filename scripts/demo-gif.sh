#!/bin/sh
# Turns the raw recording from docs/demo-judge.tape (demo-judge.mp4) into
# docs/demo.gif. The agent's wait is a still screen lasting tens of seconds;
# this finds the longest still stretch and plays it at 15x, so the GIF shows a
# short pause instead of half a minute of nothing. Needs ffmpeg.
#
#   vhs docs/demo-judge.tape && sh scripts/demo-gif.sh
set -eu

in=${1:-demo-judge.mp4}
out=${2:-docs/demo.gif}

# The longest frozen stretch of at least 5 s: "start end".
freeze=$(ffmpeg -hide_banner -i "$in" -vf "freezedetect=n=0.001:d=5" -map 0:v -f null - 2>&1 |
  awk '/freeze_start/ {s=$NF} /freeze_end/ {e=$NF; if (e-s > best) {best=e-s; bs=s; be=e}}
       END {if (best > 0) print bs, be}')
if [ -z "$freeze" ]; then
  echo "demo-gif: no still stretch found in $in" >&2
  exit 1
fi
set -- $freeze
start=$1 end=$2

filter="[0:v]trim=0:$start,setpts=PTS-STARTPTS[a];
[0:v]trim=$start:$end,setpts=(PTS-STARTPTS)/15[b];
[0:v]trim=start=$end,setpts=PTS-STARTPTS[c];
[a][b][c]concat=n=3:v=1:a=0,fps=12,split[x][y];
[x]palettegen=stats_mode=diff[p];[y][p]paletteuse=dither=bayer:bayer_scale=4"
ffmpeg -hide_banner -loglevel error -y -i "$in" -filter_complex "$filter" "$out"
echo "wrote $out (sped up the still stretch from ${start}s to ${end}s)"
