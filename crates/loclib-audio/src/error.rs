use std::io;
use std::path::PathBuf;

/// What can go wrong reading or decoding audio, or in a sink that takes the samples.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A file couldn't be read.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// Why it couldn't be read.
        source: io::Error,
    },

    /// A file isn't audio the decoder can make sense of.
    #[error("could not parse {}: {message}", path.display())]
    Parse {
        /// The file.
        path: PathBuf,
        /// What's wrong with it.
        message: String,
    },

    /// An argument out of range.
    #[error("{0}")]
    Invalid(String),

    /// The [`Ctx`](loclib_progress::Ctx) was cancelled.
    #[error(transparent)]
    Cancelled(#[from] loclib_progress::Cancelled),
}

/// A `Result` whose error defaults to this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub(crate) fn parse(path: impl Into<PathBuf>, message: impl ToString) -> Self {
        Error::Parse { path: path.into(), message: message.to_string() }
    }
}
