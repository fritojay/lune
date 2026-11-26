//! The settings dialog's object: the library folders, scan status,
//! unavailable tracks, and errors. It runs the library thread
//! ([`LibraryService`]) and applies its updates on the Qt thread: the library
//! and missing tracks go to `session`, and the folder list is saved.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::SystemTime;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QUrl, QVariant};

use crate::library_service::{Folder, Job, LibraryService, RESCAN_EVERY, Update};
use crate::preferences::with_store;
use crate::property::set;
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
    native_folder_dialog: bool,
    folder_paths: Vec<PathBuf>,
    service: Option<LibraryService>,
}

impl cxx_qt::Initialize for qobject::LibraryManager {
    fn initialize(mut self: Pin<&mut Self>) {
        let qt_thread = self.qt_thread();
        let (folders, load_error) =
            with_store(|store| (store.startup_folders(), store.take_load_error()));
        let service = LibraryService::start(
            crate::config().db_path.clone(),
            folders,
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
        self.as_mut().rust_mut().error = QString::from(&load_error.unwrap_or_default());
        self.as_mut().rust_mut().native_folder_dialog = !in_flatpak(Path::new("/.flatpak-info"));
    }
}

impl qobject::LibraryManager {
    fn apply(mut self: Pin<&mut Self>, update: Update) {
        match update {
            Update::Loaded(catalog, queue, playlists) => {
                *session::lock_queue() = queue;
                session::set_playlists(playlists);
                session::set_catalog(catalog);
            }
            Update::Library(catalog) => {
                session::set_catalog(catalog);
                covers::covers().forget();
            }
            Update::Missing(tracks) => {
                let count = i32::try_from(tracks.len()).unwrap_or(i32::MAX);
                session::set_missing(tracks);
                set!(self, missing_count, missing_count_changed, count);
            }
            Update::Folders(folders) => {
                let list: QList<QVariant> = folders.iter().map(folder_entry).collect();
                let paths: Vec<PathBuf> = folders.into_iter().map(|f| f.path).collect();
                let saved =
                    with_store(|store| store.update(|s| replace(&mut s.library_folders, &paths)));
                self.as_mut().rust_mut().folder_paths = paths;
                set!(self, folders, folders_changed, list);
                if let Err(message) = saved {
                    set!(self, error, error_changed, QString::from(&message));
                }
            }
            Update::Status(status) => {
                set!(self, busy, busy_changed, status.is_some());
                set!(
                    self,
                    status,
                    status_changed,
                    QString::from(&status.unwrap_or_default())
                );
            }
            Update::Error(message) => set!(self, error, error_changed, QString::from(&message)),
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

    fn clear_error(mut self: Pin<&mut Self>) {
        set!(self, error, error_changed, QString::default());
    }
}

/// Sets `list` to `new`. Returns whether it changed.
fn replace(list: &mut Vec<PathBuf>, new: &[PathBuf]) -> bool {
    if list.as_slice() == new {
        return false;
    }
    new.clone_into(list);
    true
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
