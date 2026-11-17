# Contributing

Lune is a fast, read-only, cross-platform desktop music player in Rust. Targets: macOS and Linux (Fedora) now, Windows later. Library is mostly M4A, MP3, FLAC.

Read `DEVELOPMENT.md` (setup, build, run) before making changes. The rules below apply to every change.

## Non-negotiables

- Never write, rename, move, or delete music files. Open them read-only. Never call lofty write APIs. The app writes only its own SQLite DB, cache, and config.
- Audio callback: no allocation, locks, I/O, or logging. It only drains a lock-free ring buffer.
- Startup loads from SQLite. Scanning runs in the background. Lists are virtualized. Art is decoded off the UI thread.
- Gapless playback is required.
- No Qt types or Qt crate dependencies outside `crates/app`.
- UI is QML. Rust exposes list models and controller objects (player, search/filter, library manager, window layout) that QML only binds to and calls. No business logic in QML.

## Layout

Cargo workspace (packages are named `lune-core`, `lune-engine`, `lune-cli`, `lune-app`, because a package named `core` would shadow Rust's built-in `core`):
- `crates/core`: scanner, lofty tags (read-only), SQLite, grouping, search, queue model
- `crates/engine`: Symphonia, rubato, cpal, ring buffer, gapless, gain stage
- `crates/cli`: headless harness
- `crates/app`: CXX-Qt and QML

## Stack

symphonia (no system FFmpeg), cpal, rubato, rtrb, lofty, rusqlite (`bundled`, FTS5), notify, directories, souvlaki, tracing, thiserror, cxx-qt / cxx-qt-build / cxx-qt-lib (pinned exact versions).

## Portability

- Use `PathBuf`/`OsString`. No string-built paths. No `std::os::unix` in `core` or `engine`.
- Get DB, cache, and config paths from `directories`.
- Platform code (media keys, Now Playing) sits behind a trait.
- Do not unwrap path-to-string conversions.

## Build

- Qt is pinned to 6.11 (6.11.2) on both platforms: macOS uses official binaries via `aqtinstall` (not Homebrew `qt`), and Fedora 44 uses `qt6-qtbase-devel` and `qt6-qtdeclarative-devel`. CXX-Qt finds Qt via the `QMAKE` env var.
- Exact setup steps live in `DEVELOPMENT.md`.
- `justfile` targets: `build`, `test`, `run`, `lint`, `fmt`.
- `cargo fmt` and `cargo clippy -- -D warnings` must pass.
- The toolchain is pinned in `rust-toolchain.toml`. `rust-version` in `Cargo.toml` (the MSRV) is checked by CI.

## Tests

- Tests are part of every feature, not an afterthought. Derive them from the behavior being built.
- Cover edge cases, not just the happy path: empty and single-item inputs, boundaries, missing or malformed tags, unusual sample rates and formats, non-UTF-8 paths, and interactions such as editing the queue mid-playback.
- Test observable behavior through public APIs rather than implementation details, so tests survive refactors.
- Every bug fix comes with a test that fails without the fix.
- `just test` runs everything. Run one crate's tests with `cargo test -p lune-core` or `cargo test -p lune-engine`, and one test with `cargo test -p lune-engine a_queue_edit`.
- Unit tests live in `src/tests/<module>_test.rs`, not inline. The source file ends with `#[cfg(test)] #[path = "tests/<module>_test.rs"] mod tests;`, and the test file starts with `use super::*;`. `test_util.rs` lives in `src/tests/` too. If modules nest later, `src/tests/` mirrors the nesting. Because `tests` is still a child module, it can reach private items. Integration tests that use only the public API go in the crate's `tests/` directory.
- Tests generate their own fixtures. Use `test_util` in each crate: temp dirs, generated WAV files, and fake tags injected through `scan_using`. Don't commit media files, and don't depend on ffmpeg in tests.
- Engine timing tests drive the decoder and renderer on one thread (`decoder.rs` `Sim`) instead of relying on timing. Whole-player tests use the capture sink in fast mode (`test_util::play_to_end`), which writes exactly the frames played.
- `unwrap`/`expect` are fine in tests (`clippy.toml`). Test helpers that create or remove files need `#[expect(clippy::disallowed_methods, reason = "...")]`.

## Conventions

- No `unwrap()`/`expect()` in non-test code except true invariants (comment why).
- `thiserror` in libraries, `anyhow` in binaries only.
- Logging via `tracing`, never from the audio callback.
- Avoid extra comments. Only comment when something is done in a non-obvious way, and say why (a workaround, an invariant, a safety or realtime constraint). Don't write comments or doc comments that restate what the code or its name already says.
