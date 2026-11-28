# Development

How to set up a machine, build Lune from `main`, and run it. Before changing code, read [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Current state

- **Playback and the library work headlessly** through `lune-cli`: gapless playback, scanning, and querying.
- **The desktop app (`lune`) browses and plays.** Quick-filter buttons along the top pick a grouping (Album Artist, Artist, Album, Year, Genre, File Type, Recently Added, Recently Played) and the filter pane lists its values; the recency groupings list albums newest first, each dated by its newest track; the main pane shows the selection grouped by album with covers; double-click plays, and the queue panel and now-playing bar work. The **Playlists** button lists playlists in the filter pane, where they can be made, renamed, and deleted; right-clicking tracks in the main pane or the queue adds them to a playlist, new or existing. A playlist shows as a track list; its tracks can be removed, and dragged or moved to the top or bottom while it is in its own order. Shift-click and Ctrl-click (⌘-click on macOS) select several tracks in the main pane and the queue. Add music folders in **Preferences** (File → Preferences on Linux and Windows, Lune → Preferences… or ⌘, on macOS): they are scanned in the background, rescanned at launch, rescanned quickly every 24 hours (skipping directories whose mtime hasn't changed), and watched for changes. Files that disappear are kept as unavailable rather than deleted, and are listed under **File → Unavailable Tracks**, where they can be removed one at a time or all at once. Queued tracks that go missing stay in the queue, marked with a warning icon, and playback skips them. Preferences also picks the audio output (the system default, or a device by name). If the output fails, for example when a device is unplugged, playback moves to the default and goes on from the same place, and moves back when the chosen device returns. A chosen device that is connected but won't open, for example because another app holds it, isn't watched for: the default plays until the output is opened again, such as after a failure or a change in Preferences. Media keys and the system Now Playing display (MPRIS on Linux, Now Playing on macOS, the media overlay on Windows) are wired up.

## Prerequisites

| Need | Fedora 44 | macOS | Windows (x64) |
|---|---|---|---|
| Rust | `rustup` (the version in `rust-toolchain.toml` installs itself) | same | same (MSVC toolchain) |
| Qt 6.11 | `qt6-qtbase-devel qt6-qtdeclarative-devel qt6-qtsvg-devel` | `aqt install-qt mac desktop 6.11.2 clang_64 -O ~/Qt` | `aqt install-qt windows desktop 6.10.3 win64_msvc2022_64 -O C:\Qt` |
| `QMAKE` env var | `/usr/bin/qmake6` | `$HOME/Qt/6.11.2/macos/bin/qmake` | `C:\Qt\6.10.3\msvc2022_64\bin\qmake.exe` |
| Other | `gcc gcc-c++ lld pkgconf-pkg-config git just alsa-lib-devel dbus-devel` | `xcode-select --install`, `brew install just aqtinstall` | Visual Studio 2022 (or Build Tools) with "Desktop development with C++", Python 3.11+, `pip install aqtinstall==3.3.0` |

Do not use Homebrew's `qt` on macOS. The exact commands are below. A Fedora dev container with everything preinstalled lives in the separate [lune-dev](https://github.com/fritojay/lune-dev) repo.

Check that Qt is found:

```sh
"$QMAKE" -query QT_VERSION   # must print 6.11.x
```

## Machine setup

CI (`.github/workflows/ci.yml`) follows the same steps.

### Pinned versions

| Component | Version | Notes |
|---|---|---|
| Qt | **6.11.2** (Windows: **6.10.3**) | The **minor** version (6.11) must match on Linux and macOS. Windows stays on 6.10 because aqtinstall 3.3.0 can't find Qt 6.11+ for Windows (Qt changed its download layout; the fix is merged upstream but not yet released). CI checks the exact version. |
| Rust | stable | Set by `rust-toolchain.toml`. Minimum is `rust-version` in `Cargo.toml`. |
| CXX-Qt | 0.10.0 | `cxx-qt`, `cxx-qt-lib`, `cxx-qt-build` are pinned with `=` in the root `Cargo.toml`. |
| aqtinstall (macOS, Windows) | 3.3.0 | Installs the official Qt binaries. |

Why not Homebrew's `qt` on macOS: Homebrew ships only the newest Qt (6.12 at the time of writing), has no `qt@6.11`, and `brew upgrade` can change it without warning. Fedora 44 ships 6.11. Using aqtinstall on macOS lets us pin the same version on both platforms.

### How CXX-Qt finds Qt

CXX-Qt's build script runs `qmake -query` to find Qt's headers and libraries. It uses the **`QMAKE` environment variable** if it is set, and otherwise the first `qmake` on `PATH`. Always set `QMAKE` explicitly so the wrong Qt never gets picked up:

```sh
"$QMAKE" -query QT_VERSION   # must print 6.11.x
```

If you change `QMAKE`, run `cargo clean -p lune-app` so the build script runs again.

### Fedora 44

```sh
sudo dnf install -y \
  gcc gcc-c++ lld pkgconf-pkg-config git just \
  qt6-qtbase-devel qt6-qtdeclarative-devel qt6-qtsvg-devel \
  alsa-lib-devel dbus-devel

# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Point CXX-Qt at the system Qt (add to ~/.bashrc)
export QMAKE=/usr/bin/qmake6
"$QMAKE" -query QT_VERSION   # expect 6.11.x
```

- `qmake6` must resolve to `/usr/bin/qmake6` from `qt6-qtbase-devel`. Do not use a Qt installed by other means.
- `lld` is used by CXX-Qt to link Qt's static initializers reliably. Without it, the build prints "Neither mold, lld, nor gold linkers were found".
- `alsa-lib-devel` is needed for cpal audio output. `dbus-devel` is needed for MPRIS (media keys and Now Playing).

### macOS (Apple Silicon or Intel)

```sh
xcode-select --install          # compilers and the SDK
brew install just aqtinstall    # do NOT install Homebrew's `qt` (or keep it off PATH)

# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Qt 6.11.2, official binaries (universal arm64 + x86_64)
aqt install-qt mac desktop 6.11.2 clang_64 -O ~/Qt

# Point CXX-Qt at it (add to ~/.zshrc)
export QMAKE="$HOME/Qt/6.11.2/macos/bin/qmake"
"$QMAKE" -query QT_VERSION      # expect 6.11.2
```

- The base install includes `qtbase`, `qtdeclarative` (QML/Quick), `qtsvg`, and `qttools` (including `macdeployqt`, used by `just bundle-mac`).
- If Homebrew's `qt` is also installed, `QMAKE` must still point at `~/Qt/6.11.2/macos/bin/qmake`.
- The binary gets an rpath to `~/Qt/6.11.2/macos/lib`, so `just run` works without `DYLD_FRAMEWORK_PATH`.

### Windows (x64)

In PowerShell:

```powershell
# Visual Studio 2022 or its Build Tools, with the "Desktop development with C++" workload
winget install Rustlang.Rustup Python.Python.3.13 Casey.Just
pip install aqtinstall==3.3.0

# Qt 6.10.3, official MSVC binaries (not 6.11: see Pinned versions)
aqt install-qt windows desktop 6.10.3 win64_msvc2022_64 -O C:\Qt

# Point CXX-Qt at it, and put Qt's DLLs on PATH for `just run` (set both permanently in System Properties)
$env:QMAKE = 'C:\Qt\6.10.3\msvc2022_64\bin\qmake.exe'
$env:PATH = "C:\Qt\6.10.3\msvc2022_64\bin;$env:PATH"
& $env:QMAKE -query QT_VERSION   # expect 6.10.3
```

- The base install includes `windeployqt`, used by `just bundle-windows`.
- `just` runs recipes with bash, so run it from Git Bash, or call the `cargo` commands in the `justfile` directly. `just bundle-windows` itself runs PowerShell.
- A debug build opens a console window next to the app, for the log. A release build doesn't.

### Dev container (Fedora)

See [lune-dev](https://github.com/fritojay/lune-dev).

### QML language server (optional)

`crates/app/.qmlls.ini` tells `qmlls` where CXX-Qt puts the generated QML modules. It depends on your machine's target directory, so it isn't committed (it's in `.gitignore`). Create it if you want QML completion in your editor:

```ini
[General]
buildDir="<your cargo target dir>/cxxqt/qml_modules"
no-cmake-calls=true
```

In the dev container, the target dir is `/home/dev/target`.

## Build from main

```sh
git clone <repo-url> lune
cd lune
git checkout main && git pull

just build   # debug build of the whole workspace
just test    # all tests
just lint    # fmt check + clippy -D warnings (must pass before committing)
```

Without `just`, use the commands in the `justfile` directly (for example `cargo build --workspace --all-targets`).

For a release build: `cargo build --release --workspace`. Binaries land in `target/release/` (`lune-cli`, `lune`).

## Run the desktop app

```sh
just run
# same as: ./run-local.sh, or cargo run -p lune-app --bin lune
```

In the dev container there is no display, so `QT_QPA_PLATFORM=offscreen` is set and no window appears. A clean exit means the QML loaded. Run on a host to see the window. There is no sound card in the container either, so playback is disabled there ("No audio output").

`--db <path>` and `--config <path>` point the app at another library and settings file, so you can try a test library without touching your own. The settings file is plain TOML (`library_folders = ["/path/to/music"]`) in the platform config directory (or the portable `data` folder, see [Windows `.zip`](#windows-zip)).

When stderr is not a terminal, Qt on Fedora sends its messages (including QML errors) to journald. Set `QT_FORCE_STDERR_LOGGING=1` to see them.

## Benchmark the track list

Generate a fake 100k-track library (no media files; the DB must be new or empty), then run the app's bench mode against it on a real display:

```sh
cargo run --release -p lune-cli --features synthetic -- gen-library /tmp/lune-bench.db --tracks 100000
cargo run --release -p lune-app --bin lune -- --db /tmp/lune-bench.db --bench
```

`--bench` scrolls the list for about 20 seconds (steady scrolling, fast flicks, then jumps like dragging the scroll bar), then the album grid for 5, prints the time to the first frame and to the filled list, frame-time statistics per phase, and memory, then quits. Don't touch the window while it runs. A "dropped" frame is one longer than 1.5 refresh intervals.

## Play music with the CLI

```sh
cargo run -p lune-cli -- play a.flac b.flac c.m4a
```

The files play in order, gaplessly. Type a command and press Enter:

| Key | Action | Key | Action |
|---|---|---|---|
| `p` | pause / resume | `n` | next track |
| `b` | previous track | `s <sec>` | seek to `<sec>` |
| `v <0-1>` | volume | `q` | quit |

Without a sound card (dev container, CI), render somewhere else:

```sh
cargo run -p lune-cli -- play --capture out.wav a.flac b.flac   # write what would be played
cargo run -p lune-cli -- play --null --fast a.flac b.flac       # decode only, as fast as possible
```

`--capture` refuses to overwrite an existing file. `--rate <hz>` sets the output rate for `--capture`/`--null` (default 48000). See `lune-cli play --help`.

## Scan and query a library

Music files are only ever read. The app writes only its own database. Links inside a library folder aren't followed (a folder that is itself a link is), and files whose names aren't valid UTF-8 are skipped. In the app, **File → Files Not Added** lists what the scans left out (these, files whose tags couldn't be read, and anything that couldn't be opened), each with the reason, and Preferences says how many there are.

```sh
cargo run -p lune-cli -- scan ~/Music          # incremental; re-run to pick up changes
cargo run -p lune-cli -- query stats
cargo run -p lune-cli -- query albums
cargo run -p lune-cli -- query album <id>
cargo run -p lune-cli -- query search <words..>
```

In the dev container, the host's `~/Test-Music` is mounted read-only at `/music`, so scan `/music` there.

The database lives in the platform data directory (`~/.local/share/lune/` on Linux, `~/Library/Application Support/io.github.fritojay.Lune/` on macOS, `%APPDATA%\fritojay\Lune\data\` on Windows). On Windows, a folder named `data` beside the executable takes the place of all three platform folders (data, config, cache), as in the Windows zip. Use `--db <path>` before the command to keep a separate one while developing:

```sh
cargo run -p lune-cli -- --db /tmp/dev.sqlite3 scan ~/Music
```

## Package the app

Packaging files live in `packaging/`. CI builds all three packages on a `v*` tag (and on a manual run of the Release workflow) and attaches them to a draft GitHub release.

### macOS `.app` and `.dmg`

```sh
just bundle-mac
open "dist/Lune.app"
```

This builds an Apple Silicon app with Qt bundled by `macdeployqt`, ad-hoc signed, plus `dist/Lune-<version>.dmg`. `QMAKE` must point at Qt 6.11 (see [Machine setup](#machine-setup)). An app built on your own Mac opens normally. A downloaded copy is quarantined because it isn't notarized: open it once, then allow it in System Settings > Privacy & Security > Open Anyway.

### Windows `.zip`

```powershell
just bundle-windows   # or: pwsh packaging/windows/build-zip.ps1
dist\Lune\lune.exe
```

This writes `dist\Lune\` (`lune.exe` with the Qt DLLs, plugins, and QML modules from `windeployqt`, and the MSVC runtime DLLs from Visual Studio's redistributable folder) and `dist\Lune-<version>-windows-x64.zip` of that folder. There is no installer: unzip it anywhere you can write to (not `Program Files`) and run `lune.exe`. It needs Windows 10 or later (x64). The install is self-contained: the zip has a `data` folder beside `lune.exe`, and the library database, settings, window state, cover cache, and log all go there, so nothing is written to the user profile and deleting the folder removes everything. To update, delete everything in the `Lune` folder except `data` (so no DLL or plugin of the old version is left behind), then unzip the new version where the old one was. The exe isn't signed, so the first launch of a downloaded copy shows SmartScreen's "Windows protected your PC": choose More info > Run anyway.

### Fedora Flatpak

```sh
sudo dnf install flatpak-builder   # once
just bundle-linux
flatpak install --user dist/Lune-<version>.flatpak
flatpak run io.github.fritojay.Lune
```

This writes `dist/Lune-<version>.flatpak`, a single file that installs on any Linux with Flatpak (the KDE runtime is fetched from Flathub). The first build downloads the KDE SDK, so it takes a while.

The Flatpak builds without network access from `packaging/linux/cargo-sources.json`. After any change to `Cargo.lock`, regenerate it with `just cargo-sources`, which runs flatpak-builder-tools' `flatpak-cargo-generator.py` at a pinned commit (it needs `curl` and `python3` with `venv`, and network access; CI checks the file). Its database and settings are under `~/.var/app/io.github.fritojay.Lune/`, separate from a non-Flatpak install. It is built with the Rust SDK extension's compiler, not the one `rust-toolchain.toml` pins; the build log prints its version.

The sandbox can read your Music folder and drives (`/run/media`, `/media`, `/mnt`) but not write them, and sees nothing else. The folder picker is Qt's own (not the system one) and shows only those places. To add music stored elsewhere, grant that folder once, read-only, then restart the app:

```sh
flatpak override --user --filesystem=/data/music:ro io.github.fritojay.Lune
```

A symlink in `~/Music` doesn't help: its target is still outside the sandbox.

### Releasing

The version is in `Cargo.toml` (`workspace.package`). `just bump-version <version>` (`packaging/bump-version.py`) sets it there and in `Cargo.lock`, and adds a matching `<release>` dated today at the top of `packaging/linux/io.github.fritojay.Lune.metainfo.xml`; then commit and tag `v<version>`. `packaging/check-version.py` checks they agree (CI runs it, and the Release workflow also checks the tag).

The app icon is a placeholder: `packaging/icon/lune.svg`. After changing it, re-render the PNG the macOS build uses: `rsvg-convert -w 1024 -h 1024 packaging/icon/lune.svg -o packaging/icon/lune-1024.png`, then the Windows icon from it, `packaging/windows/lune.ico` (16 to 256 px; for example with ImageMagick, `magick packaging/icon/lune-1024.png -define icon:auto-resize=256,128,64,48,32,24,16 packaging/windows/lune.ico`).

## Logging

Both binaries use `tracing`. Set `RUST_LOG`, for example `RUST_LOG=debug` or `RUST_LOG=lune_engine=trace`. The CLI defaults to `warn` and logs to stderr. The app logs warnings, errors, and panics unless `RUST_LOG` says otherwise: to its console if it has one (a Windows release build doesn't), and always to `lune.log` in its data folder (`~/.local/share/lune/` on Linux, `data\lune.log` in the Windows zip), emptied at each launch. The About dialog shows the file's path. If that file can't be written (the folder is read-only, or another running Lune has it), the app logs to `lune.log` in the temp folder.

## Running a subset of tests

```sh
cargo test -p lune-core
cargo test -p lune-engine
cargo test -p lune-engine a_queue_edit   # one test by name
```

Tests generate their own audio fixtures. No media files or ffmpeg are needed.

## Troubleshooting

- **Wrong Qt version linked**: check `"$QMAKE" -query QT_VERSION`, then `cargo clean -p lune-app`.
- **`failed to load QML: qrc:/...`**: the QML module did not compile into the binary. Check the warnings from `build.rs` (`cargo build -vv -p lune-app`).
- **"could not find the Qt platform plugin"** on Linux: install `qt6-qtbase-gui` (pulled in by `qt6-qtbase-devel`), or set `QT_QPA_PLATFORM=offscreen` for headless runs.
