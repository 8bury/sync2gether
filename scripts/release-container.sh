#!/usr/bin/env bash
# Ambiente de build e validação usado localmente e pelo GitHub Actions.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
kind=${1:?Uso: bash scripts/release-container.sh arch|deb}
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
export CARGO_TARGET_DIR="$PWD/target/release-$kind"
export CARGO_HOME="$PWD/target/cargo-$kind"
export RUSTUP_HOME="$PWD/target/rustup-$kind"
export PATH="$CARGO_HOME/bin:$PATH"
mkdir -p target

case "$kind" in
  arch)
    pacman -Syu --noconfirm --needed rustup git pkgconf wayland libxkbcommon libxkbcommon-x11 mesa mpv ffmpeg pulseaudio xorg-server-xvfb xorg-xauth
    rustup toolchain install 1.98.0 --profile minimal --component rustfmt --component clippy
    rustup default 1.98.0
    ;;
  deb)
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y --no-install-recommends build-essential curl ca-certificates git pkg-config libwayland-dev libxkbcommon-dev libxkbcommon-x11-0 libgl1-mesa-dev libmpv-dev ffmpeg pulseaudio pulseaudio-utils xvfb xauth
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o target/rustup-init.sh
    bash target/rustup-init.sh -y --profile minimal --default-toolchain 1.98.0
    rustup component add rustfmt clippy
    ;;
  *) echo "Formato desconhecido: $kind" >&2; exit 1 ;;
esac

bash scripts/check.sh
bash scripts/test-mvp.sh
env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET xvfb-run -a -s '-screen 0 1280x720x24' \
  env LIBGL_ALWAYS_SOFTWARE=1 bash scripts/test-player-gl.sh

cargo build --release --locked
"$CARGO_TARGET_DIR/release/sync2gether" --version
if [[ "$kind" == arch ]]; then
  # makepkg recusa root. O usuário só recebe acesso ao diretório de empacotamento.
  id release-builder &>/dev/null || useradd --system --create-home release-builder
  mkdir -p target/packaging/arch target/dist
  chown -R release-builder target/packaging/arch target/dist
  runuser -u release-builder -- env CARGO_TARGET_DIR="$CARGO_TARGET_DIR" bash scripts/package.sh arch
  pacman -U --noconfirm target/dist/sync2gether-*.pkg.tar.zst
else
  bash scripts/package.sh deb
  apt-get install -y ./target/dist/sync2gether_*_amd64.deb
fi
sync2gether --version
ldd /usr/bin/sync2gether
