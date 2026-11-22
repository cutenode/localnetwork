use std::io;
use std::path::{Path, PathBuf};

/// What can go wrong reading or editing a rekordbox database.
///
/// New kinds of error may come: a `match` needs a `_` arm.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A file (or the temporary folder a working copy goes in) couldn't be read or written.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file or folder.
        path: PathBuf,
        /// What the system said.
        source: io::Error,
    },

    /// The OneLibrary database (`exportLibrary.db`) can't be read, edited or written.
    #[error("exportLibrary.db: {0}")]
    OneLibrary(String),

    /// A row an edit names isn't there.
    #[error("not found: {0}")]
    NotFound(String),

    /// A value an edit gives can't be used.
    #[error("{0}")]
    Invalid(String),

    /// An id an edit gives a new row is taken already.
    #[error("{0}")]
    Exists(String),
}

/// A result whose error is this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Attach a path to `io::Error`s.
pub(crate) trait IoContext<T> {
    fn at(self, path: impl AsRef<Path>) -> Result<T>;
}

impl<T> IoContext<T> for io::Result<T> {
    fn at(self, path: impl AsRef<Path>) -> Result<T> {
        self.map_err(|source| Error::Io { path: path.as_ref().to_path_buf(), source })
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::OneLibrary(e.to_string())
    }
}

impl From<loclib_sqlcipher::Error> for Error {
    fn from(e: loclib_sqlcipher::Error) -> Self {
        match e {
            // Here the key is rekordbox's own, so a page that fails is the file's fault.
            loclib_sqlcipher::Error::Authentication(pgno) => {
                Error::OneLibrary(format!("page {pgno} fails authentication (not a rekordbox database, or corrupt)"))
            }
            other => Error::OneLibrary(other.to_string()),
        }
    }
}
