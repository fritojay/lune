//! The app's desktop entry name. Linux shells match a window to its
//! installed `.desktop` file (name and icon) by the Wayland app ID, which
//! Qt takes from this; without it, Qt sends the executable name.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use cxx_qt_lib::QString;

pub const APP_ID: &str = "io.github.fritojay.Lune";

#[cxx_qt::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("lune-app/src/desktop_entry.h");
        #[namespace = "lune"]
        #[cxx_name = "setDesktopFileName"]
        fn set_desktop_file_name(name: &QString);

        #[namespace = "lune"]
        #[cxx_name = "desktopFileName"]
        fn desktop_file_name() -> QString;
    }
}

pub fn register() {
    ffi::set_desktop_file_name(&QString::from(APP_ID));
    tracing::debug!(name = %ffi::desktop_file_name(), "desktop entry");
}

#[cfg(test)]
#[path = "tests/desktop_entry_test.rs"]
mod tests;
