#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

preview=0
out=target/web
for arg in "$@"; do
  case $arg in
    --preview) preview=1 ;;
    -*) echo "Opção desconhecida: $arg" >&2; exit 2 ;;
    *) out=$arg ;;
  esac
done
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
if [[ $preview -eq 1 ]]; then
  cp web/preview/*.css web/preview/*.js "$out/"
  awk '{
    if ($0 ~ /<script type="module" src="main\.js"><\/script>/) {
      print "  <link rel=\"stylesheet\" href=\"preview.css\">"
      print "  <script type=\"module\" src=\"preview.js\"></script>"
    } else {
      print
    }
  }' "$out/index.html" > "$out/index.html.tmp"
  mv "$out/index.html.tmp" "$out/index.html"
fi
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
