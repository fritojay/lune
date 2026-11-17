mod album_model;
mod bench;
mod covers;
mod desktop_entry;
mod filter_model;
mod format;
mod library_manager;
mod library_service;
mod list_model;
mod media_controls;
mod missing_model;
mod player;
mod queue_model;
mod session;
#[cfg(test)]
#[path = "tests/test_util.rs"]
mod test_util;
mod window_layout;

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::{Context, bail};
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

/// Location of `qml/main.qml` inside the QML module compiled by `build.rs`.
const MAIN_QML: &str = "qrc:/qt/qml/com/lune/player/qml/main.qml";

const USAGE: &str = "\
usage: lune [--db <path>] [--config <path>] [--bench]

  --db <path>      library database (defaults to the platform data directory)
  --config <path>  settings file (defaults to the platform config directory)
  --bench      scroll the track list automatically, print timings and memory, and quit";

pub struct Config {
    pub db_path: PathBuf,
    pub config_path: PathBuf,
    pub window_state_path: PathBuf,
    pub bench: bool,
    pub started: Instant,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// QML-created objects have no constructor arguments, so they read the
/// command-line settings from here.
pub fn config() -> &'static Config {
    CONFIG.get_or_init(|| Config {
        db_path: PathBuf::new(),
        config_path: PathBuf::new(),
        window_state_path: PathBuf::new(),
        bench: false,
        started: Instant::now(),
    })
}

fn parse_args(started: Instant) -> anyhow::Result<Config> {
    let mut db_path = None;
    let mut config_path = None;
    let mut bench = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--db" {
            db_path = Some(PathBuf::from(args.next().context("--db needs a path")?));
        } else if arg == "--config" {
            config_path = Some(PathBuf::from(args.next().context("--config needs a path")?));
        } else if arg == "--bench" {
            bench = true;
        } else if arg == "--help" || arg == "-h" {
            println!("{USAGE}");
            std::process::exit(0);
        } else {
            bail!("unknown argument {}\n\n{USAGE}", arg.to_string_lossy());
        }
    }
    let db_path = match db_path {
        Some(path) => path,
        None => lune_core::default_db_path()?,
    };
    let config_path = match config_path {
        Some(path) => path,
        None => lune_core::default_config_path()?,
    };
    Ok(Config {
        window_state_path: lune_core::window_state_path(&db_path),
        db_path,
        config_path,
        bench,
        started,
    })
}

fn main() -> anyhow::Result<()> {
    let started = Instant::now();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    if CONFIG.set(parse_args(started)?).is_err() {
        bail!("configuration was read before it was set");
    }

    let mut app = QGuiApplication::new();
    // Fusion on every Linux desktop, so the look doesn't depend on the
    // platform theme: KDE's picks a style that can't load the icons in our
    // QML module and runs menu shortcuts into their text. A non-empty env var
    // still overrides this, for trying other styles; Qt ignores an empty one.
    #[cfg(target_os = "linux")]
    if std::env::var_os("QT_QUICK_CONTROLS_STYLE").is_none_or(|style| style.is_empty()) {
        cxx_qt_lib::QQuickStyle::set_style(&cxx_qt_lib::QString::from("Fusion"));
    }
    desktop_entry::register();
    let mut engine = QQmlApplicationEngine::new();

    let Some(mut engine_ref) = engine.as_mut() else {
        anyhow::bail!("failed to create the QML engine");
    };
    engine_ref
        .as_mut()
        .on_object_creation_failed(|_, url| {
            tracing::error!("failed to load QML: {url}");
            std::process::exit(1);
        })
        .release();
    engine_ref.load(&QUrl::from(MAIN_QML));

    let Some(app_ref) = app.as_mut() else {
        anyhow::bail!("failed to create the Qt application");
    };
    let code = app_ref.exec();
    session::finish_saving();
    if code != 0 {
        anyhow::bail!("Qt event loop exited with code {code}");
    }
    Ok(())
}
