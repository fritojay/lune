//! The settings file, owned by the UI thread: the only place that writes it.
//! The `Preferences` QML singleton exposes the preferences; `LibraryManager`
//! saves the folder list the library thread reports.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;
use lune_core::{SCROLL_SPEEDS, Settings, clamp_scroll_speed};

use crate::format::qml_int;
use crate::property::set;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        /// Clamped to `minScrollSpeed`..=`maxScrollSpeed`.
        #[qproperty(i32, scroll_speed, cxx_name = "scrollSpeed", READ, WRITE = set_scroll_speed, NOTIFY)]
        #[qproperty(i32, min_scroll_speed, cxx_name = "minScrollSpeed", READ, CONSTANT)]
        #[qproperty(i32, max_scroll_speed, cxx_name = "maxScrollSpeed", READ, CONSTANT)]
        #[qproperty(QString, error, READ, NOTIFY)]
        type Preferences = super::PreferencesRust;
    }

    unsafe extern "RustQt" {
        #[cxx_name = "setScrollSpeed"]
        fn set_scroll_speed(self: Pin<&mut Preferences>, speed: i32);

        #[qinvokable]
        #[cxx_name = "clearError"]
        fn clear_error(self: Pin<&mut Preferences>);
    }
}

pub struct PreferencesRust {
    scroll_speed: i32,
    min_scroll_speed: i32,
    max_scroll_speed: i32,
    error: QString,
}

impl Default for PreferencesRust {
    fn default() -> Self {
        Self {
            scroll_speed: with_store(|store| qml_int(store.settings().scroll_speed)),
            min_scroll_speed: qml_int(*SCROLL_SPEEDS.start()),
            max_scroll_speed: qml_int(*SCROLL_SPEEDS.end()),
            error: QString::default(),
        }
    }
}

impl qobject::Preferences {
    fn set_scroll_speed(mut self: Pin<&mut Self>, speed: i32) {
        let speed = scroll_speed_from_qml(speed);
        let saved = with_store(|store| store.update(|s| s.set_scroll_speed(speed)));
        set!(self, scroll_speed, scroll_speed_changed, qml_int(speed));
        if let Err(message) = saved {
            set!(self, error, error_changed, QString::from(&message));
        }
    }

    fn clear_error(mut self: Pin<&mut Self>) {
        set!(self, error, error_changed, QString::default());
    }
}

fn scroll_speed_from_qml(speed: i32) -> u32 {
    clamp_scroll_speed(u32::try_from(speed).unwrap_or(0))
}

/// The settings and the file they are saved in.
pub struct Store {
    path: PathBuf,
    settings: Settings,
    /// The file exists but couldn't be read, so `settings` are the defaults
    /// and saving them would replace the user's folders with none.
    unreadable: bool,
    /// The settings came from (or have been saved to) the file.
    from_file: bool,
    load_error: Option<String>,
}

impl Store {
    pub fn load(path: PathBuf) -> Self {
        let (settings, unreadable, from_file, load_error) = match Settings::load(&path) {
            Ok(Some(settings)) => (settings, false, true, None),
            Ok(None) => (Settings::default(), false, false, None),
            Err(err) => {
                let message = format!("{err}. The library folders are not loaded.");
                tracing::error!("{message}");
                (Settings::default(), true, false, Some(message))
            }
        };
        Self {
            path,
            settings,
            unreadable,
            from_file,
            load_error,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The folders to prune the library to, or `None` without a settings
    /// file to go by: the DB may then hold a library built with lune-cli, and
    /// only a file the user (or the app) wrote decides what to drop.
    pub fn startup_folders(&self) -> Option<Vec<PathBuf>> {
        self.from_file
            .then(|| self.settings.library_folders.clone())
    }

    /// Why the file couldn't be read, once.
    pub fn take_load_error(&mut self) -> Option<String> {
        self.load_error.take()
    }

    /// Applies `change`, which returns whether it changed anything, and saves
    /// if it did. The change is kept even when it can't be saved.
    pub fn update(&mut self, change: impl FnOnce(&mut Settings) -> bool) -> Result<(), String> {
        if !change(&mut self.settings) {
            return Ok(());
        }
        let saved = if self.unreadable {
            Err(format!(
                "The settings were not saved, because {} could not be read. Fix it and restart Lune.",
                self.path.display()
            ))
        } else {
            self.settings
                .save(&self.path)
                .map_err(|err| format!("Could not save the settings: {err}"))
        };
        match &saved {
            Ok(()) => self.from_file = true,
            Err(message) => tracing::error!("{message}"),
        }
        saved
    }
}

thread_local! {
    // Loaded on first use. Callers don't call into Qt while it is borrowed.
    static STORE: RefCell<Option<Store>> = const { RefCell::new(None) };
}

pub fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    STORE.with_borrow_mut(|store| {
        f(store.get_or_insert_with(|| Store::load(crate::config().config_path.clone())))
    })
}

#[cfg(test)]
#[path = "tests/preferences_test.rs"]
mod tests;
