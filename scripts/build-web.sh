#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

out=${1:-target/web}
relay_url=${SYNC2GETHER_WEB_RELAY_URL:-}
tools=target/tools
bindgen_version=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' Cargo.lock)

if ! "$tools/bin/wasm-bindgen" --version 2>/dev/null | grep -qx "wasm-bindgen $bindgen_version"; then
  CARGO_TARGET_DIR=target/tools-build cargo install --locked wasm-bindgen-cli \
    --version "$bindgen_version" --root "$tools"
fi

cargo build --locked --release -p sync2gether-web --target wasm32-unknown-unknown

rm -rf -- "$out"
mkdir -p -- "$out/pkg" "$out/fonts"
"$tools/bin/wasm-bindgen" --target web --no-typescript --out-dir "$out/pkg" \
  target/wasm32-unknown-unknown/release/sync2gether_web.wasm
cp web/static/*.html web/static/*.css web/static/*.js "$out/"
cp assets/fonts/Geist-Regular.ttf assets/fonts/Geist-Medium.ttf \
  assets/fonts/GeistMono-Regular.ttf assets/fonts/LICENSE "$out/fonts/"
case $relay_url in
  '' | ws://* | wss://*) ;;
  *) echo "SYNC2GETHER_WEB_RELAY_URL precisa começar com ws:// ou wss://" >&2; exit 2 ;;
esac
if [[ $relay_url == *[\"\\\']* ]]; then
  echo "SYNC2GETHER_WEB_RELAY_URL contém caracteres inválidos" >&2
  exit 2
fi
printf 'export const RELAY_URL = "%s";\n' "$relay_url" > "$out/config.js"
echo "Página gerada em $out"
