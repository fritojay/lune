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
use cxx_qt_lib::{QString, QStringList};
use lune_core::{SCROLL_SPEEDS, Settings, clamp_scroll_speed};
use lune_engine::OutputDevice;

use crate::format::qml_int;
use crate::property::set;
use crate::session;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
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
        /// What `outputDevice` picks from: the system default first.
        #[qproperty(QStringList, output_devices, cxx_name = "outputDevices", READ, NOTIFY)]
        #[qproperty(i32, output_device, cxx_name = "outputDevice", READ, WRITE = set_output_device, NOTIFY)]
        type Preferences = super::PreferencesRust;
    }

    unsafe extern "RustQt" {
        #[cxx_name = "setScrollSpeed"]
        fn set_scroll_speed(self: Pin<&mut Preferences>, speed: i32);

        #[cxx_name = "setOutputDevice"]
        fn set_output_device(self: Pin<&mut Preferences>, index: i32);

        /// Lists the connected devices again (slow on some systems).
        #[qinvokable]
        #[cxx_name = "refreshOutputDevices"]
        fn refresh_output_devices(self: Pin<&mut Preferences>);

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
    output_devices: QStringList,
    output_device: i32,
    /// The device each of `output_devices` names; `None` is the default.
    output_ids: Vec<Option<String>>,
}

impl Default for PreferencesRust {
    fn default() -> Self {
        Self {
            scroll_speed: with_store(|store| qml_int(store.settings().scroll_speed)),
            min_scroll_speed: qml_int(*SCROLL_SPEEDS.start()),
            max_scroll_speed: qml_int(*SCROLL_SPEEDS.end()),
            error: QString::default(),
            output_devices: QStringList::default(),
            output_device: 0,
            output_ids: Vec::new(),
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

    fn set_output_device(mut self: Pin<&mut Self>, index: i32) {
        let Some(id) = usize::try_from(index)
            .ok()
            .and_then(|i| self.output_ids.get(i))
            .cloned()
        else {
            return;
        };
        let saved = with_store(|store| {
            store.update(|s| {
                let changed = s.output_device != id;
                s.output_device = id;
                changed
            })
        });
        if index != self.output_device {
            set!(self, output_device, output_device_changed, index);
            session::output_device_changed();
        }
        if let Err(message) = saved {
            set!(self, error, error_changed, QString::from(&message));
        }
    }

    fn refresh_output_devices(mut self: Pin<&mut Self>) {
        let saved = with_store(|store| store.settings().output_device.clone());
        let (choices, chosen) = device_choices(&lune_engine::output_devices(), saved.as_deref());
        let labels: QStringList = choices
            .iter()
            .map(|(_, label)| QString::from(label))
            .collect();
        self.as_mut().rust_mut().output_ids = choices.into_iter().map(|(id, _)| id).collect();
        set!(self, output_devices, output_devices_changed, labels);
        let chosen = i32::try_from(chosen).unwrap_or(0);
        set!(self, output_device, output_device_changed, chosen);
    }

    fn clear_error(mut self: Pin<&mut Self>) {
        set!(self, error, error_changed, QString::default());
    }
}

/// The outputs to choose from, as (device id, label), and which one is
/// chosen. A saved device that isn't connected stays listed, so looking at
/// the list doesn't change the choice.
fn device_choices(
    devices: &[OutputDevice],
    saved: Option<&str>,
) -> (Vec<(Option<String>, String)>, usize) {
    let mut choices = vec![(None, "System Default".to_owned())];
    choices.extend(devices.iter().map(|d| (Some(d.id.clone()), d.name.clone())));
    if let Some(saved) = saved
        && !devices.iter().any(|d| d.id == saved)
    {
        choices.push((
            Some(saved.to_owned()),
            "Chosen Device (not connected)".to_owned(),
        ));
    }
    let chosen = choices
        .iter()
        .position(|(id, _)| id.as_deref() == saved)
        .unwrap_or(0);
    (choices, chosen)
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
