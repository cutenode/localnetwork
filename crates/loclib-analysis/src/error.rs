use std::path::PathBuf;

/// What can go wrong analysing audio or reading annotations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A file couldn't be read or decoded (loclib-audio's error).
    #[error(transparent)]
    Audio(#[from] loclib_audio::Error),

    /// The tempo and key analyzer or the loudness meter failed on audio that decoded.
    #[error("could not analyse {}: {message}", path.display())]
    Analyzer {
        /// The track.
        path: PathBuf,
        /// What the analyzer or meter reported.
        message: String,
    },

    /// An annotation isn't in the format its dataset uses.
    #[error("line {line}: {message}")]
    Annotation {
        /// The line it's on, from 1 (always 1 in a file of one annotation).
        line: usize,
        /// What's wrong with it.
        message: String,
    },

    /// The [`Ctx`](loclib_progress::Ctx) was cancelled.
    #[error(transparent)]
    Cancelled(#[from] loclib_progress::Cancelled),
}

/// A `Result` whose error defaults to this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    #[cfg(feature = "dsp")]
    pub(crate) fn analyzer(path: impl Into<PathBuf>, message: impl ToString) -> Error {
        Error::Analyzer { path: path.into(), message: message.to_string() }
    }

    pub(crate) fn annotation(line: usize, message: impl ToString) -> Error {
        Error::Annotation { line, message: message.to_string() }
    }
}
