# Contributing

Lune is a fast, read-only, cross-platform desktop music player in Rust. Targets: macOS, Linux (Fedora), and Windows (x64). Library is mostly M4A, MP3, FLAC.

Read `DEVELOPMENT.md` (setup, build, run) before making changes. The rules below apply to every change.

## Non-negotiables

- Never write, rename, move, or delete music files. Open them read-only. Never call lofty write APIs. The app writes only its own SQLite DB, cache, config, and log.
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

symphonia (no system FFmpeg), cpal, rubato, rtrb, lofty, rusqlite (`bundled`, FTS5), notify, directories, souvlaki, tracing, thiserror, clap (derive, binaries' arguments), cxx-qt / cxx-qt-build / cxx-qt-lib (pinned exact versions).

## Portability

- Use `PathBuf`/`OsString`. No string-built paths. No `std::os::unix` in `core` or `engine`.
- Get DB, cache, and config paths from `lune_core` (`default_db_path` and friends, in `paths.rs`), which uses `directories` except in a portable Windows install (a `data` folder beside the exe).
- Platform code (media keys, Now Playing) sits behind a trait.
- Do not unwrap path-to-string conversions.

## Build

- Qt is pinned to 6.11 (6.11.2) on Linux and macOS, and to 6.10.3 on Windows until aqtinstall releases Qt 6.11 support for Windows: macOS and Windows use official binaries via `aqtinstall` (not Homebrew `qt` on macOS), and Fedora 44 uses `qt6-qtbase-devel` and `qt6-qtdeclarative-devel`. CXX-Qt finds Qt via the `QMAKE` env var.
- Exact setup steps live in `DEVELOPMENT.md`.
- `justfile` targets: `build`, `test`, `run`, `lint`, `fmt`.
- `cargo fmt` and `cargo clippy -- -D warnings` must pass.
- The toolchain is pinned in `rust-toolchain.toml`. `rust-version` in `Cargo.toml` (the MSRV) is checked by CI.

## Tests

- Tests are part of every feature, not an afterthought. Derive them from the behavior being built.
- Cover edge cases, not just the happy path: empty and single-item inputs, boundaries, missing or malformed tags, unusual sample rates and formats, non-UTF-8 paths, and interactions such as editing the queue mid-playback.
- Test observable behavior through public APIs rather than implementation details, so tests survive refactors.
- Prefer properties over examples: state a rule that holds for every input and let `proptest` generate the inputs. Common shapes are round trips (save then load), invariants (a sort is a permutation ordered by its key, widths fill the row), model tests (random edit sequences on `Queue` compared with plain lists), hostile input (any bytes never panic), and clamps.
- Before adding a `#[test]`, look for a property, model, or table that already states the rule, and extend it: a variant in its generator, a line in its oracle (e.g. `sort_key` in `browse_test.rs`), or a row in its table. Build generators from `ALL` arrays so new variants are covered without new tests. Shared generators (`arb_tracks`, `arb_grouping`, …) live in `test_util`.
- Keep example tests for what a generator can't express well: file-system and platform cases (links, non-UTF-8 paths), migrations, and slow end-to-end flows.
- Every bug fix comes with a test that fails without the fix. A pinned case in an existing table or a committed `proptest-regressions/` seed counts.
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
- The settings and window-state files must load files written by newer versions: an unknown key or a value this version can't read never makes the file unreadable, and an unreadable value falls back to its default. Settings keep unknown keys so saving writes them back. The exception is `library_folders`: an unreadable list is an error, so the app never saves over the user's folders.

## Adding a browsable field

A field the filter pane can group by (`Field` in `crates/core/src/browse.rs`):

1. Add the variant to `Field`, `Field::ALL`, and `field_count` in `browse_test.rs`, and give it an entry in `Field::def`: where its key comes from, its label, and its placeholder when the tag is missing. A text field only needs `key`, `label`, and `missing` on top of `FieldDef::TEXT`.
2. Add a `Grouping` variant that lists it (to `Grouping::ALL`, `Grouping::def`, and `grouping_count`), and its title in `titles` in `crates/app/qml/GroupingBar.qml`. The grouping bar lists `Grouping::ALL` through `SourcesModel`, so it gets a button without other changes.
3. If the data isn't on `ListTrack` yet: add it to `ListTrack` and `LIST_TRACK_COLUMNS` in `query.rs`, write a migration in `db.rs`, read it in `tags.rs`, and bump `READER_VERSION`.

The proptests in `browse_test.rs` run over `Grouping::ALL`, so the new grouping is tested without new tests.
