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
ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i 'testsrc2=size=640x360:rate=24' \
  -f lavfi -i 'sine=frequency=440:sample_rate=48000' \
  -f lavfi -i 'sine=frequency=880:sample_rate=48000' \
  -map 0:v:0 -map 1:a:0 -map 2:a:0 \
  -t 20 -c:v libx264 -profile:v baseline -pix_fmt yuv420p -g 24 \
  -c:a:0 ac3 -b:a:0 192k -metadata:s:a:0 language=por -metadata:s:a:0 title=Portugues -disposition:a:0 default \
  -c:a:1 ac3 -b:a:1 192k -metadata:s:a:1 language=eng -metadata:s:a:1 title=English -disposition:a:1 0 \
  .cache/browser-fixture.mkv
bash scripts/build-web.sh
cargo test --release --locked --test browser -- --ignored --nocapture --test-threads=1 "$@"
