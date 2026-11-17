//! The settings dialog's object: the library folders, scan status,
//! unavailable tracks, and errors. It runs the library thread
//! ([`LibraryService`]) and applies its updates on the Qt thread: the first
//! load (`loaded`) and every later change to the library (`libraryChanged`).

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::SystemTime;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QUrl, QVariant};

use lune_core::{DEFAULT_SCROLL_SPEED, SCROLL_SPEEDS, clamp_scroll_speed};

use crate::format::qml_int;
use crate::library_service::{Folder, Job, LibraryService, RESCAN_EVERY, Update};
use crate::{covers, format, session};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;
        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        /// `{ path, lastScanned }` per folder; `lastScanned` is epoch ms,
        /// absent if never scanned.
        #[qproperty(QList_QVariant, folders, READ, NOTIFY)]
        #[qproperty(bool, busy, READ, NOTIFY)]
        #[qproperty(QString, status, READ, NOTIFY)]
        #[qproperty(QString, error, READ, NOTIFY)]
        #[qproperty(i32, missing_count, cxx_name = "missingCount", READ, NOTIFY)]
        #[qproperty(i32, scroll_speed, cxx_name = "scrollSpeed", READ, NOTIFY)]
        #[qproperty(i32, min_scroll_speed, cxx_name = "minScrollSpeed", READ, CONSTANT)]
        #[qproperty(i32, max_scroll_speed, cxx_name = "maxScrollSpeed", READ, CONSTANT)]
        #[qproperty(
            bool,
            native_folder_dialog,
            cxx_name = "nativeFolderDialog",
            READ,
            CONSTANT
        )]
        type LibraryManager = super::LibraryManagerRust;
    }

    unsafe extern "RustQt" {
        /// The library and saved queue are loaded (once, at startup).
        #[qsignal]
        fn loaded(self: Pin<&mut LibraryManager>);

        /// The library changed after startup.
        #[qsignal]
        #[cxx_name = "libraryChanged"]
        fn library_changed(self: Pin<&mut LibraryManager>);

        /// The list of tracks whose files are missing changed.
        #[qsignal]
        #[cxx_name = "missingChanged"]
        fn missing_changed(self: Pin<&mut LibraryManager>);

        /// `folder` is a local `file:` URL, as a folder picker returns.
        #[qinvokable]
        #[cxx_name = "addFolder"]
        fn add_folder(self: Pin<&mut LibraryManager>, folder: &QUrl);

        #[qinvokable]
        #[cxx_name = "removeFolder"]
        fn remove_folder(self: Pin<&mut LibraryManager>, row: i32);

        #[qinvokable]
        fn rescan(self: Pin<&mut LibraryManager>);

        /// Deletes the unavailable track `id` from the library.
        #[qinvokable]
        #[cxx_name = "forgetMissing"]
        fn forget_missing(self: Pin<&mut LibraryManager>, id: i64);

        #[qinvokable]
        #[cxx_name = "forgetAllMissing"]
        fn forget_all_missing(self: Pin<&mut LibraryManager>);

        /// Clamped to `minScrollSpeed`..=`maxScrollSpeed`.
        #[qinvokable]
        #[cxx_name = "setScrollSpeed"]
        fn set_scroll_speed(self: Pin<&mut LibraryManager>, speed: i32);

        #[qinvokable]
        #[cxx_name = "clearError"]
        fn clear_error(self: Pin<&mut LibraryManager>);
    }

    impl cxx_qt::Threading for LibraryManager {}
    impl cxx_qt::Initialize for LibraryManager {}
}

#[derive(Default)]
pub struct LibraryManagerRust {
    folders: QList<QVariant>,
    busy: bool,
    status: QString,
    error: QString,
    missing_count: i32,
    scroll_speed: i32,
    min_scroll_speed: i32,
    max_scroll_speed: i32,
    native_folder_dialog: bool,
    folder_paths: Vec<PathBuf>,
    service: Option<LibraryService>,
}

impl cxx_qt::Initialize for qobject::LibraryManager {
    fn initialize(mut self: Pin<&mut Self>) {
        let qt_thread = self.qt_thread();
        let config = crate::config();
        let service = LibraryService::start(
            config.db_path.clone(),
            config.config_path.clone(),
            lune_core::default_thumbnail_dir().ok(),
            RESCAN_EVERY,
            SystemTime::now,
            move |update| {
                // An error means the object is gone (the app is quitting).
                let _ = qt_thread.queue(move |manager| manager.apply(update));
            },
        );
        self.as_mut().rust_mut().service = Some(service);
        self.as_mut().rust_mut().busy = true;
        self.as_mut().rust_mut().scroll_speed = qml_int(DEFAULT_SCROLL_SPEED);
        self.as_mut().rust_mut().min_scroll_speed = qml_int(*SCROLL_SPEEDS.start());
        self.as_mut().rust_mut().max_scroll_speed = qml_int(*SCROLL_SPEEDS.end());
        self.as_mut().rust_mut().native_folder_dialog = !in_flatpak(Path::new("/.flatpak-info"));
    }
}

impl qobject::LibraryManager {
    fn apply(mut self: Pin<&mut Self>, update: Update) {
        match update {
            Update::Loaded(catalog, queue) => {
                *session::lock_queue() = queue;
                session::set_catalog(catalog);
                self.loaded();
            }
            Update::Library(catalog) => {
                session::set_catalog(catalog);
                covers::covers().forget();
                self.library_changed();
            }
            Update::Missing(tracks) => {
                let count = i32::try_from(tracks.len()).unwrap_or(i32::MAX);
                session::set_missing(tracks);
                if self.missing_count != count {
                    self.as_mut().rust_mut().missing_count = count;
                    self.as_mut().missing_count_changed();
                }
                self.missing_changed();
            }
            Update::Folders(folders) => {
                let list: QList<QVariant> = folders.iter().map(folder_entry).collect();
                self.as_mut().rust_mut().folder_paths =
                    folders.into_iter().map(|f| f.path).collect();
                self.as_mut().rust_mut().folders = list;
                self.folders_changed();
            }
            Update::ScrollSpeed(speed) => self.show_scroll_speed(speed),
            Update::Status(status) => {
                let busy = status.is_some();
                if self.busy != busy {
                    self.as_mut().rust_mut().busy = busy;
                    self.as_mut().busy_changed();
                }
                self.as_mut().rust_mut().status = QString::from(&status.unwrap_or_default());
                self.status_changed();
            }
            Update::Error(message) => {
                self.as_mut().rust_mut().error = QString::from(&message);
                self.error_changed();
            }
        }
    }

    fn send(&self, job: Job) {
        if let Some(service) = &self.service {
            service.send(job);
        }
    }

    fn add_folder(self: Pin<&mut Self>, folder: &QUrl) {
        match folder.to_local_file() {
            Some(path) if !path.is_empty() => {
                self.send(Job::AddFolder(PathBuf::from(path.to_string())));
            }
            _ => tracing::warn!(url = %folder.to_qstring(), "not a local folder"),
        }
    }

    fn remove_folder(self: Pin<&mut Self>, row: i32) {
        let folder = usize::try_from(row)
            .ok()
            .and_then(|row| self.folder_paths.get(row))
            .cloned();
        if let Some(folder) = folder {
            self.send(Job::RemoveFolder(folder));
        }
    }

    fn rescan(self: Pin<&mut Self>) {
        self.send(Job::RescanAll);
    }

    fn forget_missing(self: Pin<&mut Self>, id: i64) {
        self.send(Job::ForgetMissing(vec![id]));
    }

    fn forget_all_missing(self: Pin<&mut Self>) {
        self.send(Job::ForgetAllMissing);
    }

    fn set_scroll_speed(mut self: Pin<&mut Self>, speed: i32) {
        let speed = scroll_speed_from_qml(speed);
        // Shown now: the library thread saves it only once a running scan ends.
        self.as_mut().show_scroll_speed(speed);
        self.send(Job::SetScrollSpeed(speed));
    }

    fn show_scroll_speed(mut self: Pin<&mut Self>, speed: u32) {
        let speed = qml_int(speed);
        if self.scroll_speed != speed {
            self.as_mut().rust_mut().scroll_speed = speed;
            self.scroll_speed_changed();
        }
    }

    fn clear_error(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().error = QString::default();
        self.error_changed();
    }
}

fn scroll_speed_from_qml(speed: i32) -> u32 {
    clamp_scroll_speed(u32::try_from(speed).unwrap_or(0))
}

fn folder_entry(folder: &Folder) -> QVariant {
    let mut entry = QMap::<QMapPair_QString_QVariant>::default();
    entry.insert(
        QString::from("path"),
        QVariant::from(&QString::from(&folder.path.display().to_string())),
    );
    if let Some(ns) = folder.last_scanned_ns {
        entry.insert(
            QString::from("lastScanned"),
            QVariant::from(&format::epoch_ms(ns)),
        );
    }
    QVariant::from(&entry)
}

// In a Flatpak the native picker is the portal's, which hands back a
// document-portal path (`/run/user/<uid>/doc/...`) for a folder the sandbox
// can only read. Those paths deliver no inotify events, so watching would
// silently stop. Qt's own dialog browses the sandbox and returns real paths.
fn in_flatpak(marker: &Path) -> bool {
    marker.exists()
}

#[cfg(test)]
#[path = "tests/library_manager_test.rs"]
mod tests;
