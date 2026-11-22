//! What can go wrong measuring a drive.

use std::io;
use std::path::PathBuf;

use loclib_progress::Cancelled;

/// What can go wrong measuring a drive.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Reading or writing `path` failed.
    #[error("{}: {source}", path.display())]
    Io {
        /// What was being read or written.
        path: PathBuf,
        /// Why it failed.
        source: io::Error,
    },
    /// The drive failed as the caller reaches it (a [`crate::Drive`] method): its error, as given.
    #[error("{0}")]
    Drive(io::Error),
    /// The work was cancelled.
    #[error(transparent)]
    Cancelled(#[from] Cancelled),
}

/// A result whose error is this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Name the path a failed read or write was at.
pub(crate) trait IoContext<T> {
    /// The error, if any, as an [`Error::Io`] at `path`.
    fn at(self, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> IoContext<T> for io::Result<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T> {
        self.map_err(|source| Error::Io { path: path.into(), source })
    }
}
