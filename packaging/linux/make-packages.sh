#!/usr/bin/env bash
# Crea el paquete .deb para Ubuntu (doble clic para instalar).
set -euo pipefail
cd "$(dirname "$0")/../.."
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
mkdir -p dist

# --- .deb
D=dist/deb
rm -rf "$D"
mkdir -p "$D/DEBIAN" "$D/usr/bin" "$D/usr/share/applications" "$D/usr/share/icons/hicolor/256x256/apps" "$D/usr/share/metainfo"
cp target/release/eginbeharraren-deia target/release/eginbeharraren-deia-server "$D/usr/bin/"
cp packaging/linux/eginbeharraren-deia.desktop "$D/usr/share/applications/"
cp assets/icon-256.png "$D/usr/share/icons/hicolor/256x256/apps/eginbeharraren-deia.png"
# Catálogo de modelos de artista
mkdir -p "$D/usr/share/eginbeharraren-deia/modelos"
cp -r assets/modelos/. "$D/usr/share/eginbeharraren-deia/modelos/"
# Ficha del centro de software (para verlo entre las instaladas y desinstalarlo desde ahí)
sed -e "s/@VERSION@/$VERSION/" -e "s/@DATE@/$(date -u +%F)/" packaging/linux/io.github.tramankulu.eginbeharraren_deia.metainfo.xml \
  > "$D/usr/share/metainfo/io.github.tramankulu.eginbeharraren_deia.metainfo.xml"
sed "s/@VERSION@/$VERSION/" packaging/linux/control > "$D/DEBIAN/control"
# Tamaño instalado (lo muestra el centro de software)
echo "Installed-Size: $(du -sk "$D/usr" | cut -f1)" >> "$D/DEBIAN/control"
cp packaging/linux/postinst packaging/linux/postrm "$D/DEBIAN/"
chmod 755 "$D/DEBIAN/postinst" "$D/DEBIAN/postrm"
dpkg-deb --build --root-owner-group "$D" "dist/eginbeharraren-deia_${VERSION}_amd64.deb"
echo "Creados en dist/"
