use std::io;
use std::path::PathBuf;

/// What can go wrong opening a disk or probing it.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The disk couldn't be opened, written or read back.
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },

    /// The device can't be opened for writing without administrator rights.
    #[error("checking the capacity of {} needs administrator rights", path.display())]
    Permission { path: PathBuf },

    /// The operating system had no random numbers for the probe's nonce.
    #[error("no random numbers: {0}")]
    Random(getrandom::Error),

    /// The [`Ctx`](loclib_progress::Ctx) was cancelled.
    #[error(transparent)]
    Cancelled(#[from] loclib_progress::Cancelled),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
