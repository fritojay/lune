#!/usr/bin/env bash
# Regenerates cargo-sources.json, the crates the Flatpak build vendors, with
# flatpak-builder-tools' flatpak-cargo-generator.py. flatpak-builder builds
# without network access, so every crate in Cargo.lock is listed as a download
# with its checksum. Run it after any change to Cargo.lock.
#
#     cargo-sources.sh           rewrite cargo-sources.json
#     cargo-sources.sh --check   fail if it is out of date with Cargo.lock
#
# The generator is pinned by commit and checksum, and its Python packages by
# exact version; they are installed into a throwaway virtualenv.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/packaging/linux/cargo-sources.json"

GENERATOR_COMMIT=9d86d336a39b9af38b86f114b6ce5820fe260f91
GENERATOR_SHA256=0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380
GENERATOR_URL="https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/$GENERATOR_COMMIT/cargo/flatpak-cargo-generator.py"
PACKAGES=(
    aiohappyeyeballs==2.7.1
    aiohttp==3.14.5
    aiosignal==1.4.0
    attrs==26.1.0
    frozenlist==1.8.0
    idna==3.20
    multidict==7.2.1
    propcache==0.5.4
    PyYAML==6.0.3
    tomlkit==0.15.2
    yarl==1.25.1
)

case "${1-}" in
    "" | --check) ;;
    *)
        echo "usage: $0 [--check]" >&2
        exit 2
        ;;
esac

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

curl -fsSL "$GENERATOR_URL" -o "$WORK/flatpak-cargo-generator.py"
# Python rather than sha256sum, which macOS lacks.
python3 -I -c '
import hashlib, sys
path, want = sys.argv[1:]
with open(path, "rb") as f:
    got = hashlib.sha256(f.read()).hexdigest()
if got != want:
    sys.exit(f"flatpak-cargo-generator.py: sha256 {got}, expected {want}")
' "$WORK/flatpak-cargo-generator.py" "$GENERATOR_SHA256"

python3 -m venv "$WORK/venv"
"$WORK/venv/bin/pip" install --quiet --disable-pip-version-check --no-deps "${PACKAGES[@]}"
# -I: don't import modules from the current directory or the script's.
"$WORK/venv/bin/python" -I "$WORK/flatpak-cargo-generator.py" "$ROOT/Cargo.lock" -o "$WORK/cargo-sources.json"

if [[ "${1-}" == --check ]]; then
    if ! diff -q "$WORK/cargo-sources.json" "$OUT" > /dev/null; then
        echo "cargo-sources.json is out of date with Cargo.lock; run just cargo-sources" >&2
        exit 1
    fi
else
    cp "$WORK/cargo-sources.json" "$OUT"
fi
