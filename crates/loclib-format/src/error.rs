use std::io;
use std::path::PathBuf;

/// What can go wrong laying out or writing a file system.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The file system can't be made as asked: a sector or cluster size it doesn't take, or a
    /// volume size it can't describe.
    #[error("{0}")]
    Invalid(String),

    /// Reading or writing the device failed.
    #[error("{}: {source}", path.display())]
    Io {
        /// What was being read or written (`volume` for the volume being formatted).
        path: PathBuf,
        /// What the operating system said.
        source: io::Error,
    },

    /// The [`Ctx`](loclib_progress::Ctx) was cancelled.
    #[error(transparent)]
    Cancelled(#[from] loclib_progress::Cancelled),
}

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Error::Invalid(message.into())
    }
}

/// A [`std::result::Result`] with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
