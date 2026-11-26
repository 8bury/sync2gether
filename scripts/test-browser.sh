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
bash scripts/build-web.sh
cargo test --release --locked --test browser -- --ignored --nocapture --test-threads=1 "$@"
