use std::io;
use std::path::PathBuf;

/// What can go wrong finding, formatting or checking a disk.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The file system can't be made as asked (a sector or cluster size it doesn't take, a
    /// volume size it can't describe), or a path isn't what's needed.
    #[error("{0}")]
    Invalid(String),

    /// Reading or writing a device failed.
    #[error("{}: {source}", path.display())]
    Io {
        /// What was being read or written: a device, or `volume` for the volume being formatted.
        path: PathBuf,
        /// What the operating system said.
        source: io::Error,
    },

    /// A system tool's output couldn't be read.
    #[error("could not parse {}: {message}", path.display())]
    Parse {
        /// What was read: `lsblk output`, `diskutil output`...
        path: PathBuf,
        /// What was wrong with it.
        message: String,
    },

    /// A disk or volume isn't there.
    #[error("not found: {0}")]
    NotFound(String),

    /// A system tool (diskutil, mkfs, PowerShell...) couldn't be started, or failed.
    #[error("{command} failed: {message}")]
    External {
        /// The command line that ran, or the program that couldn't start.
        command: String,
        /// The tool's own message.
        message: String,
    },

    /// The work needs administrator rights (root, an elevated prompt).
    #[error("{0}")]
    Permission(String),

    /// The [`Ctx`](loclib_progress::Ctx) was cancelled.
    #[error(transparent)]
    Cancelled(#[from] loclib_progress::Cancelled),
}

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Error::Invalid(message.into())
    }

    pub(crate) fn parse(path: impl Into<PathBuf>, message: impl ToString) -> Self {
        Error::Parse { path: path.into(), message: message.to_string() }
    }
}

/// A [`std::result::Result`] with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
