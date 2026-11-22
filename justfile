set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# Build the whole workspace (debug)
build:
    cargo build --locked --workspace --all-targets

# Run all tests
test:
    cargo test --locked --workspace --all-features

# Launch the desktop app (extra args are passed through)
run *args:
    cargo run --locked -p lune-app --bin lune -- {{args}}

# Check formatting and run clippy with warnings as errors
lint:
    cargo fmt --all --check
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings

# Format all code
fmt:
    cargo fmt --all

# Build dist/Lune.app and the .dmg (on an Apple Silicon Mac)
bundle-mac:
    packaging/macos/build-app.sh

# Build dist/Lune and the portable .zip (on x64 Windows, in PowerShell)
[windows]
bundle-windows:
    pwsh -NoProfile -File packaging/windows/build-zip.ps1

# Set the release version (e.g. `just bump-version 0.1.2`), then commit and tag it
bump-version version:
    packaging/bump-version.py {{version}}

# Regenerate packaging/linux/cargo-sources.json after a Cargo.lock change
cargo-sources:
    packaging/linux/cargo-sources.sh

# Build the dist/*.flatpak bundle (on Linux with flatpak-builder)
bundle-linux:
    packaging/linux/build-flatpak.sh
