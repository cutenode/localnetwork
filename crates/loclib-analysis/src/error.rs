use std::path::PathBuf;

/// What can go wrong analysing audio or reading annotations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A file couldn't be read or decoded (loclib-mixid's error, as its decoder reports it).
    #[error(transparent)]
    Audio(#[from] loclib_mixid::Error),

    /// The tempo and key analyzer or the loudness meter failed on audio that decoded.
    #[error("could not analyse {}: {message}", path.display())]
    Analyzer { path: PathBuf, message: String },

    /// An annotation isn't in the format its dataset uses.
    #[error("line {line}: {message}")]
    Annotation { line: usize, message: String },

    /// The [`Ctx`](loclib_progress::Ctx) was cancelled.
    #[error(transparent)]
    Cancelled(#[from] loclib_progress::Cancelled),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub(crate) fn analyzer(path: impl Into<PathBuf>, message: impl ToString) -> Error {
        Error::Analyzer { path: path.into(), message: message.to_string() }
    }

    pub(crate) fn annotation(line: usize, message: impl ToString) -> Error {
        Error::Annotation { line, message: message.to_string() }
    }
}
