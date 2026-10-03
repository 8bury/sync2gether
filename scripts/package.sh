#!/usr/bin/env bash
# Execute dentro da distribuição de destino, após cargo build --release --locked.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

kind=${1:?Uso: bash scripts/package.sh arch|deb}
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)
binary="${CARGO_TARGET_DIR:-target}/release/sync2gether"
output="$PWD/target/dist"
work="$PWD/target/packaging/$kind"
test -x "$binary"
test "$("$binary" --version)" = "sync2gether $version"
mkdir -p "$output" "$work"

case "$kind" in
  arch)
    test "$(uname -m)" = x86_64
    test "$version" = "$(sed -n "s/^pkgver=//p" packaging/arch/PKGBUILD)"
    install -m755 "$binary" "$work/sync2gether"
    install -m644 packaging/sync2gether.desktop packaging/arch/PKGBUILD "$work/"
    (cd "$work" && PKGDEST="$output" makepkg --force --nodeps)
    ;;
  deb)
    test "$(dpkg --print-architecture)" = amd64
    root="$work/sync2gether_${version}_amd64"
    mkdir -p "$root/DEBIAN"
    install -Dm755 "$binary" "$root/usr/bin/sync2gether"
    install -Dm644 packaging/sync2gether.desktop \
      "$root/usr/share/applications/sync2gether.desktop"
    size=$(du -sk "$root/usr" | cut -f1)
    cat > "$root/DEBIAN/control" <<EOF
Package: sync2gether
Version: $version
Section: video
Priority: optional
Architecture: amd64
Maintainer: 8bury <8bury@users.noreply.github.com>
Installed-Size: $size
Depends: libc6 (>= 2.39), libgcc-s1, libmpv2, libxkbcommon0, libwayland-client0, libwayland-cursor0, libwayland-egl1, libgl1, libegl1, libx11-6, libxcursor1, libxi6, libxrandr2, libxcb1, xdg-desktop-portal
Recommends: xdg-desktop-portal-gtk | xdg-desktop-portal-kde
Homepage: https://github.com/8bury/sync2gether
Description: Watch local movies in sync over LAN or VPN
 Two participants play identical local files using libmpv, with host-coordinated
 playback, room discovery and automatic guest reconnection.
EOF
    dpkg-deb --root-owner-group --build "$root" "$output/sync2gether_${version}_amd64.deb"
    ;;
  *) echo "Formato desconhecido: $kind" >&2; exit 1 ;;
esac
