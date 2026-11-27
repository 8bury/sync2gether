#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
if [[ ${SYNC2GETHER_TEST_AUDIO:-0} != 1 ]]; then
  exec bash scripts/with-test-audio.sh bash scripts/test-browser.sh "$@"
fi
command -v firefox >/dev/null || { echo 'Instale o Firefox para este teste.' >&2; exit 1; }
mkdir -p .cache
ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i 'testsrc2=size=640x360:rate=24' \
  -f lavfi -i 'sine=frequency=440:sample_rate=48000' \
  -t 20 -c:v libx264 -profile:v baseline -pix_fmt yuv420p -g 24 \
  -c:a aac -b:a 96k -movflags +faststart .cache/browser-fixture.mp4
cat > .cache/browser-fixture-pt.srt <<'EOF'
1
00:00:00,500 --> 00:00:09,500
Legenda embutida SRT inicio

2
00:00:10,500 --> 00:00:19,500
Legenda embutida SRT meio
EOF
cat > .cache/browser-fixture-en.ass <<'EOF'
[Script Info]
ScriptType: v4.00+

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.50,0:00:09.50,Default,,0,0,0,,{\an8\i1}Embedded ASS start\NSecond line
Dialogue: 0,0:00:10.50,0:00:19.50,Default,,0,0,0,,{\b1}Embedded ASS middle
EOF
ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i 'testsrc2=size=640x360:rate=24' \
  -f lavfi -i 'sine=frequency=440:sample_rate=48000' \
  -f lavfi -i 'sine=frequency=880:sample_rate=48000' \
  -i .cache/browser-fixture-pt.srt \
  -i .cache/browser-fixture-en.ass \
  -map 0:v:0 -map 1:a:0 -map 2:a:0 -map 3:s:0 -map 4:s:0 \
  -t 20 -c:v libx264 -profile:v baseline -pix_fmt yuv420p -g 24 \
  -c:a:0 ac3 -b:a:0 192k -metadata:s:a:0 language=por -metadata:s:a:0 title=Portugues -disposition:a:0 default \
  -c:a:1 ac3 -b:a:1 192k -metadata:s:a:1 language=eng -metadata:s:a:1 title=English -disposition:a:1 0 \
  -c:s:0 subrip -metadata:s:s:0 language=por -metadata:s:s:0 title="Portugues SRT" -disposition:s:0 default \
  -c:s:1 ass -metadata:s:s:1 language=eng -metadata:s:s:1 title="English ASS" -disposition:s:1 0 \
  .cache/browser-fixture.mkv
ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i 'testsrc2=size=640x360:rate=24' \
  -f lavfi -i 'sine=frequency=440:sample_rate=48000' \
  -f lavfi -i 'sine=frequency=880:sample_rate=48000' \
  -map 0:v:0 -map 1:a:0 -map 2:a:0 \
  -t 20 -c:v libx264 -profile:v baseline -pix_fmt yuv420p -g 24 \
  -c:a:0 aac -b:a:0 96k -metadata:s:a:0 language=por -metadata:s:a:0 title=Portugues -disposition:a:0 default \
  -c:a:1 aac -b:a:1 96k -metadata:s:a:1 language=eng -metadata:s:a:1 title=English -disposition:a:1 0 \
  .cache/browser-fixture-aac.mkv
bash scripts/build-web.sh
cargo test --release --locked --test browser -- --ignored --nocapture --test-threads=1 "$@"
