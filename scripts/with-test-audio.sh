#!/usr/bin/env bash
# Saída de áudio virtual com relógio real, sem usar os alto-falantes.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
command -v pactl >/dev/null || {
  echo 'Instale pactl: libpulse no Arch ou pulseaudio-utils no Ubuntu.' >&2
  exit 1
}
sink="sync2gether_test_$$"
module=''
daemon=''
cleanup() {
  if [[ -n "$module" ]]; then pactl unload-module "$module" || true; fi
  if [[ -n "$daemon" ]]; then
    kill "$daemon" 2>/dev/null || true
    wait "$daemon" 2>/dev/null || true
  fi
}
trap cleanup EXIT
if ! pactl info >/dev/null 2>&1; then
  command -v pulseaudio >/dev/null || {
    echo 'Sem servidor de áudio. Instale PulseAudio para executar testes sem desktop.' >&2
    exit 1
  }
  audio_dir="$PWD/.cache/audio-$$"
  mkdir -p "$audio_dir"
  chmod 700 "$audio_dir"
  export PULSE_RUNTIME_PATH="$audio_dir"
  export PULSE_STATE_PATH="$audio_dir/state"
  export PULSE_SERVER="unix:$audio_dir/socket"
  pulseaudio -n --daemonize=no --exit-idle-time=-1 --use-pid-file=no \
    --log-target="file:$audio_dir/server.log" \
    --load="module-native-protocol-unix socket=$audio_dir/socket auth-anonymous=1" &
  daemon=$!
  for _ in {1..50}; do
    if pactl info >/dev/null 2>&1; then break; fi
    sleep 0.1
  done
  pactl info >/dev/null
fi
module=$(pactl load-module module-null-sink "sink_name=$sink" rate=48000 channels=2)
export PULSE_SINK="$sink"
# mpv tenta PipeWire antes de PulseAudio. Force o caminho que usa este sink.
export PIPEWIRE_REMOTE="sync2gether-test-isolated-$$"
export SYNC2GETHER_TEST_AUDIO=1
"$@"
