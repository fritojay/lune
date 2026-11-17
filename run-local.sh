#!/usr/bin/env bash
# Launches the desktop app from source without needing `just` (same as
# `just run`). Extra arguments are passed to the app.
set -euo pipefail

cd "$(dirname "$0")"
exec cargo run --locked -p lune-app --bin lune -- "$@"
