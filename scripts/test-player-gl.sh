#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
# Na área de trabalho, a janela de teste abre numa sessão sway oculta.
# Em Xvfb (container/CI) não há WAYLAND_DISPLAY e o display atual é usado.
if [[ ${HEADLESS_SESSION:-0} != 1 && -n ${WAYLAND_DISPLAY:-} ]]; then
  exec bash scripts/headless.sh bash scripts/test-player-gl.sh "$@"
fi
if [[ ${SYNC2GETHER_TEST_AUDIO:-0} != 1 ]]; then
  exec bash scripts/with-test-audio.sh bash scripts/test-player-gl.sh "$@"
fi
mkdir -p .cache
cat > .cache/player-gl-subtitles.srt <<'SUBTITLES'
1
00:00:00,000 --> 00:00:15,000
Legenda sintetica de teste
SUBTITLES
ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i 'testsrc2=size=640x360:rate=60' \
  -f lavfi -i 'sine=frequency=440:sample_rate=48000' \
  -f lavfi -i 'sine=frequency=660:sample_rate=48000' \
  -i .cache/player-gl-subtitles.srt \
  -map 0:v -map 1:a -map 2:a -map 3:s \
  -t 16 -c:v mpeg4 -q:v 5 -c:a pcm_s16le -c:s srt \
  -metadata:s:a:0 language=por -metadata:s:a:1 language=eng \
  -metadata:s:s:0 language=por .cache/player-gl-fixture.mkv
cargo build --locked --features demo --example check-player-gl
# Usa a sessão headless ou o Xvfb. Não usa arquivos pessoais.
timeout 35s "${CARGO_TARGET_DIR:-target}/debug/examples/check-player-gl"
