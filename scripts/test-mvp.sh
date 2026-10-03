#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
if [[ ${SYNC2GETHER_TEST_AUDIO:-0} != 1 ]]; then
  exec bash scripts/with-test-audio.sh bash scripts/test-mvp.sh "$@"
fi
mkdir -p .cache
ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i 'testsrc2=size=640x360:rate=24' \
  -f lavfi -i 'sine=frequency=440:sample_rate=48000' \
  -t 20 -c:v mpeg4 -q:v 5 -c:a pcm_s16le .cache/mvp-fixture.mkv
cp .cache/mvp-fixture.mkv .cache/mvp-different.mkv
cp .cache/mvp-fixture.mkv .cache/mvp-renamed.mkv
printf 'different synthetic fixture\n' >> .cache/mvp-different.mkv
cargo test --release --locked --test mvp -- --ignored --nocapture --test-threads=1
