#!/usr/bin/env bash
# Builds gtk4-layer-shell into ~/.local, unless it's there: the desktop app links it (desktop/build.rs)
# to float above the other windows on Wayland desktops, and no distro packages it yet. Needs meson,
# ninja and GTK 4's headers. Used by install.sh and CI.
set -euo pipefail

VERSION="1.3.0"
SHA256="1ebb01ab14e98afd1727f68f64981c37bd23305b1f131f5667c02b94cf593192"
PREFIX="$HOME/.local"

if [ -f "$PREFIX/lib/libgtk4-layer-shell.so.$VERSION" ]; then
    echo "found $VERSION"
    exit 0
fi
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL -o "$tmp/src.tar.gz" "https://github.com/wmww/gtk4-layer-shell/archive/refs/tags/v$VERSION.tar.gz"
echo "$SHA256  $tmp/src.tar.gz" | sha256sum -c --quiet -
tar -xzf "$tmp/src.tar.gz" -C "$tmp"
meson setup "$tmp/build" "$tmp/gtk4-layer-shell-$VERSION" --prefix "$PREFIX" --libdir lib --buildtype release \
    -Dintrospection=false -Dvapi=false >/dev/null
meson install -C "$tmp/build" --quiet
echo "installed $VERSION"
