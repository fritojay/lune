use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not read tags from {path}: {source}")]
    Tags {
        path: PathBuf,
        #[source]
        source: lofty::error::FileParseError,
    },

    #[error("the tag parser panicked on {0}")]
    TagPanic(PathBuf),

    #[error("could not make a thumbnail of the cover art in {path}: {source}")]
    Image {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },

    #[error("the image decoder panicked on the cover art in {0}")]
    ImagePanic(PathBuf),

    #[error("could not read the settings in {path}: {message}")]
    Settings { path: PathBuf, message: String },

    #[error("invalid window state in {path}: {message}")]
    WindowState { path: PathBuf, message: String },

    #[error("could not watch the library folders: {0}")]
    Watch(String),

    #[error("library root is not a directory: {0}")]
    NotADirectory(PathBuf),

    #[error("path is not valid UTF-8: {0}")]
    NonUtf8Path(PathBuf),

    #[error("database schema version {found} is newer than this build supports ({supported})")]
    SchemaTooNew { found: i64, supported: i64 },

    #[error("database schema version {0} is invalid")]
    SchemaInvalid(i64),

    #[error("the library already has {0} tracks; synthetic data needs an empty library")]
    NotEmpty(u64),

    #[error("another Lune is already using the library at {0}")]
    LibraryInUse(PathBuf),

    #[error("could not determine the user's home directory")]
    NoHomeDir,
}

pub type Result<T> = std::result::Result<T, CoreError>;

impl CoreError {
    /// Another connection held the database for longer than the busy timeout.
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Db(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::DatabaseBusy)
    }
}
