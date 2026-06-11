#!/usr/bin/env bash
# Install Orpheus into the current user's GNOME session.
set -e
PREFIX="${PREFIX:-$HOME/.local}"
install -Dm755 latte "$PREFIX/bin/latte"
install -d "$PREFIX/share/orpheus"
cp -r lib docs "$PREFIX/share/orpheus/"
install -Dm644 orpheus.svg "$PREFIX/share/icons/hicolor/scalable/apps/orpheus.svg"
# point the launcher at the installed binary and run it from the data dir
sed "s|^Exec=.*|Exec=sh -c 'cd $PREFIX/share/orpheus \&\& exec $PREFIX/bin/latte gui'|" orpheus.desktop \
  > "$PREFIX/share/applications/orpheus.desktop"
update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
echo "Installed. Find 'Orpheus' in your app grid, or run: latte"
