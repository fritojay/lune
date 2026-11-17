#!/usr/bin/env bash
# Builds dist/Lune.app (Apple Silicon only, Qt bundled by macdeployqt,
# ad-hoc signed) and dist/Lune-<version>.dmg.
#
# Needs an Apple Silicon Mac with the Xcode command line tools and Qt 6.11
# from aqtinstall with QMAKE pointing at it (DEVELOPMENT.md).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

[ "$(uname -s)-$(uname -m)" = Darwin-arm64 ] || {
    echo "build-app.sh builds the Apple Silicon app on an Apple Silicon Mac" >&2
    exit 1
}

SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT

: "${QMAKE:?set QMAKE to the qmake of Qt 6.11, see DEVELOPMENT.md}"
QT_BINS="$("$QMAKE" -query QT_INSTALL_BINS)"
QT_LIBS="$("$QMAKE" -query QT_INSTALL_LIBS)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
DIST="$ROOT/dist"
APP="$DIST/Lune.app"

VERSION="$(packaging/check-version.py)"

# Build for the same minimum macOS as the Qt being bundled, as recorded in
# QtCore's LC_BUILD_VERSION load command.
QTCORE="$QT_LIBS/QtCore.framework/QtCore"
MIN_MACOS="$(otool -arch arm64 -l "$QTCORE" | awk '$1 == "minos" { print $2; exit }')"
[ -n "$MIN_MACOS" ] || { echo "no minimum macOS version in $QTCORE" >&2; exit 1; }
export MACOSX_DEPLOYMENT_TARGET="$MIN_MACOS"

echo "Lune $VERSION, macOS $MIN_MACOS+, Qt $("$QMAKE" -query QT_VERSION)"

TARGET=aarch64-apple-darwin
cargo build --locked --release -p lune-app --bin lune --target "$TARGET"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$TARGET_DIR/$TARGET/release/lune" "$APP/Contents/MacOS/lune"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@MIN_MACOS@/$MIN_MACOS/g" \
    packaging/macos/Info.plist > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

# Icon: every size macOS asks for, from the 1024 px render of the SVG.
ICONSET="$SCRATCH/lune.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" packaging/icon/lune-1024.png \
        --out "$ICONSET/icon_${size}x${size}.png" > /dev/null
    double=$((size * 2))
    sips -z "$double" "$double" packaging/icon/lune-1024.png \
        --out "$ICONSET/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/lune.icns"

# Copies the Qt frameworks, plugins, and the QML modules the app's QML files import.
"$QT_BINS/macdeployqt" "$APP" -qmldir="$ROOT/crates/app/qml" -verbose=1

# The official Qt binaries are universal. Keep only their arm64 code.
find "$APP/Contents" -type f -print0 | while IFS= read -r -d '' file; do
    archs="$(lipo -archs "$file" 2>/dev/null)" || continue
    case " $archs " in
        *" x86_64 "*)
            lipo -thin arm64 "$file" -output "$file.thin"
            # cp onto the original keeps its permissions (executables stay executable).
            cp "$file.thin" "$file"
            rm "$file.thin"
            ;;
    esac
done

# The build links Qt through an rpath into the Qt install.
# Drop it, so the app can only load the Qt inside the bundle.
BIN="$APP/Contents/MacOS/lune"
otool -l "$BIN" | awk '/cmd LC_RPATH/ { getline; getline; print $2 }' | while read -r rpath; do
    case "$rpath" in
        @*) ;;
        *) install_name_tool -delete_rpath "$rpath" "$BIN" ;;
    esac
done
if otool -l "$BIN" | awk '/cmd LC_RPATH/ { getline; getline; print $2 }' | grep -qv '^@'; then
    echo "an absolute rpath is left in $BIN" >&2
    exit 1
fi

# Ad-hoc signature (no notarization): last, since every step above
# changes the bundle. Signed inside out (loose libraries and plugins, then
# frameworks, then the app) rather than with the deprecated --deep, which is
# the order notarization will need.
find "$APP/Contents" -path '*.framework' -prune -o -type f -print0 |
    while IFS= read -r -d '' file; do
        if lipo -archs "$file" > /dev/null 2>&1 && [ "$file" != "$BIN" ]; then
            codesign --force --sign - "$file"
        fi
    done
find "$APP/Contents/Frameworks" -maxdepth 1 -name '*.framework' -print0 |
    xargs -0 -n 1 codesign --force --sign -
codesign --force --sign - "$APP"
codesign --verify --deep --strict "$APP"

STAGING="$SCRATCH/dmg"
mkdir "$STAGING"
cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"
DMG="$DIST/Lune-$VERSION.dmg"
rm -f "$DMG"
hdiutil create -volname "Lune" -srcfolder "$STAGING" -format UDZO -ov "$DMG" > /dev/null

echo "Built:"
echo "  $APP"
echo "  $DMG"
