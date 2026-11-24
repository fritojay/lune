mod album_grid_model;
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
mod preferences;
mod property;
mod queue_model;
mod session;
#[cfg(test)]
#[path = "tests/test_util.rs"]
mod test_util;
mod window_layout;

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::bail;
use clap::Parser;
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

/// Location of `qml/main.qml` inside the QML module compiled by `build.rs`.
const MAIN_QML: &str = "qrc:/qt/qml/com/lune/player/qml/main.qml";

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

/// Lune, a music player.
#[derive(Parser, Debug, PartialEq, Eq)]
#[command(name = "lune", version)]
struct Args {
    /// Library database (defaults to the platform data directory)
    #[arg(long = "db", value_name = "PATH")]
    db_path: Option<PathBuf>,
    /// Settings file (defaults to the platform config directory)
    #[arg(long = "config", value_name = "PATH")]
    config_path: Option<PathBuf>,
    /// Scroll the track list automatically, print timings and memory, and quit
    #[arg(long)]
    bench: bool,
}

fn config_from(args: Args, started: Instant) -> anyhow::Result<Config> {
    let Args {
        db_path,
        config_path,
        bench,
    } = args;
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

    if CONFIG.set(config_from(Args::parse(), started)?).is_err() {
        bail!("configuration was read before it was set");
    }

    let mut app = QGuiApplication::new();
    let Some(app_ref) = app.as_mut() else {
        anyhow::bail!("failed to create the Qt application");
    };
    app_ref.set_application_version(&QString::from(env!("CARGO_PKG_VERSION")));
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

#[cfg(test)]
#[path = "tests/main_test.rs"]
mod tests;
