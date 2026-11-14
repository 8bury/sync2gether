#!/usr/bin/env bash
# Executa um comando numa sessão sway headless: as janelas abertas por ele não
# aparecem na área de trabalho, não roubam o foco e não usam o X11 da sessão.
#
# Uso: bash scripts/headless.sh COMANDO [ARGS...]
#
# O comando recebe WAYLAND_DISPLAY e SWAYSOCK da sessão oculta (swaymsg e grim
# funcionam nela) e HEADLESS_SESSION=1. DISPLAY e a instância do Hyprland são
# removidos do ambiente. HEADLESS_SIZE muda a resolução (padrão 1920x1080).
set -euo pipefail
(($#)) || { echo "Uso: $0 COMANDO [ARGS...]" >&2; exit 2; }
command -v sway >/dev/null || {
  echo 'Instale sway para abrir janelas sem exibi-las: sudo pacman -S sway' >&2
  exit 1
}
runtime=${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR não definido}
dir=$(mktemp -d "$runtime/headless-sway-XXXXXX")
sway_pid=''
cleanup() {
  if [[ -n "$sway_pid" ]]; then
    kill "$sway_pid" 2>/dev/null || true
    wait "$sway_pid" 2>/dev/null || true
  fi
  rm -rf "$dir"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

cat > "$dir/config" <<CONFIG
xwayland disable
output HEADLESS-1 resolution ${HEADLESS_SIZE:-1920x1080} position 0 0
# Janelas flutuantes mantêm o tamanho pedido pelo aplicativo, como num desktop.
for_window [all] floating enable
default_border none
default_floating_border none
exec sh -c 'printf "%s\n%s\n" "\$WAYLAND_DISPLAY" "\$SWAYSOCK" > "$dir/env.tmp" && mv "$dir/env.tmp" "$dir/env"'
CONFIG

env -u DISPLAY -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u SWAYSOCK \
  -u HYPRLAND_INSTANCE_SIGNATURE \
  WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=1 WLR_LIBINPUT_NO_DEVICES=1 \
  sway --config "$dir/config" > "$dir/sway.log" 2>&1 &
sway_pid=$!

for _ in {1..100}; do
  [[ -f "$dir/env" ]] && break
  kill -0 "$sway_pid" 2>/dev/null || break
  sleep 0.1
done
if [[ ! -f "$dir/env" ]]; then
  echo 'A sessão sway headless não iniciou:' >&2
  cat "$dir/sway.log" >&2
  exit 1
fi
{ read -r wayland_display; read -r swaysock; } < "$dir/env"

status=0
env -u DISPLAY -u WAYLAND_SOCKET -u HYPRLAND_INSTANCE_SIGNATURE \
  WAYLAND_DISPLAY="$wayland_display" SWAYSOCK="$swaysock" \
  XDG_SESSION_TYPE=wayland HEADLESS_SESSION=1 \
  "$@" || status=$?
exit "$status"
