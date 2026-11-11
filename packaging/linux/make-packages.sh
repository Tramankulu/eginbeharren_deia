#!/usr/bin/env bash
# Crea el paquete .deb para Ubuntu (doble clic para instalar).
set -euo pipefail
cd "$(dirname "$0")/../.."
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
mkdir -p dist

# --- .deb
D=dist/deb
rm -rf "$D"
mkdir -p "$D/DEBIAN" "$D/usr/bin" "$D/usr/share/applications" "$D/usr/share/icons/hicolor/256x256/apps"
cp target/release/eginbeharraren-deia target/release/eginbeharraren-deia-server "$D/usr/bin/"
cp packaging/linux/eginbeharraren-deia.desktop "$D/usr/share/applications/"
cp assets/icon-256.png "$D/usr/share/icons/hicolor/256x256/apps/eginbeharraren-deia.png"
# Catálogo de modelos de artista
mkdir -p "$D/usr/share/eginbeharraren-deia/modelos"
cp -r assets/modelos/. "$D/usr/share/eginbeharraren-deia/modelos/"
sed "s/@VERSION@/$VERSION/" packaging/linux/control > "$D/DEBIAN/control"
dpkg-deb --build --root-owner-group "$D" "dist/eginbeharraren-deia_${VERSION}_amd64.deb"
echo "Creados en dist/"
