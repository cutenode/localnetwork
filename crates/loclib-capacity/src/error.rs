use std::io;
use std::path::PathBuf;

/// What can go wrong opening a disk or probing it.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The disk couldn't be opened, written or read back.
    #[error("{}: {source}", path.display())]
    Io {
        /// The device's path, or `disk` for a [`RawDisk`](crate::RawDisk) passed to
        /// [`probe`](crate::probe).
        path: PathBuf,
        /// The underlying I/O error.
        source: io::Error,
    },

    /// The device can't be opened for writing without administrator rights.
    #[error("checking the capacity of {} needs administrator rights", path.display())]
    Permission {
        /// The device's path.
        path: PathBuf,
    },

    /// The operating system had no random numbers for the probe's nonce.
    #[error("no random numbers: {0}")]
    Random(getrandom::Error),

    /// The [`Ctx`](loclib_progress::Ctx) was cancelled.
    #[error(transparent)]
    Cancelled(#[from] loclib_progress::Cancelled),
}

/// A `Result` whose error defaults to this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
