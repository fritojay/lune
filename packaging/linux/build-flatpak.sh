#!/usr/bin/env bash
# Builds dist/Lune-<version>.flatpak, a single-file bundle that installs
# on any Linux with Flatpak (the KDE runtime comes from Flathub).
#
# Needs flatpak and flatpak-builder (Fedora: sudo dnf install flatpak-builder).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

APP_ID=io.github.fritojay.Lune
FLATHUB=https://dl.flathub.org/repo/flathub.flatpakrepo

command -v flatpak-builder > /dev/null || {
    echo "flatpak-builder not found (Fedora: sudo dnf install flatpak-builder)" >&2
    exit 1
}

VERSION="$(packaging/check-version.py)"

flatpak remote-add --user --if-not-exists flathub "$FLATHUB"
flatpak-builder --user --install-deps-from=flathub --force-clean --repo=repo \
    build-dir "packaging/linux/$APP_ID.yml"

BUNDLE="dist/Lune-$VERSION.flatpak"
mkdir -p dist
flatpak build-bundle --runtime-repo="$FLATHUB" repo "$BUNDLE" "$APP_ID"

echo "Built: $ROOT/$BUNDLE"
echo "Install: flatpak install --user $BUNDLE"
echo "Run:     flatpak run $APP_ID"
