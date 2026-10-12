use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// What can go wrong making a copy.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The source couldn't be read or decoded, as loclib-mixid's decoder reports it (a decoder
    /// that panicked on it is [`loclib_mixid::audio::CRASHED`]).
    #[error(transparent)]
    Decode(#[from] loclib_mixid::Error),

    /// The copy couldn't be written.
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },

    /// The audio doesn't fit the container (a WAV or AIFF over 4 GiB, more channels than FLAC's
    /// eight).
    #[error("could not encode {}: {message}", path.display())]
    Encode { path: PathBuf, message: String },

    /// The source's tags couldn't be written to the copy.
    #[error("could not copy the tags to {}: {message}", path.display())]
    Tags { path: PathBuf, message: String },

    /// A recipe no player takes.
    #[error("{0}")]
    Invalid(String),

    /// The source is shorter than one frame at the recipe's rate, so a copy would hold no audio:
    /// a file nothing reads back.
    #[error("{} is too short to copy at {rate} Hz: {frames} frame(s) at {source_rate} Hz", path.display())]
    TooShort { path: PathBuf, frames: u64, source_rate: u32, rate: u32 },

    /// The [`Ctx`](loclib_progress::Ctx) was cancelled.
    #[error(transparent)]
    Cancelled(#[from] loclib_progress::Cancelled),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors from what writes the copy, as this crate's, about `path`. A
/// `match` rather than `map_err` with a closure, which coverage would count as a function never
/// run.
pub(crate) trait Context<T> {
    fn encoding(self, path: &Path) -> Result<T>;
    fn tagging(self, path: &Path) -> Result<T>;
}

impl<T, E: fmt::Display> Context<T> for std::result::Result<T, E> {
    fn encoding(self, path: &Path) -> Result<T> {
        match self {
            Ok(v) => Ok(v),
            Err(e) => Err(Error::Encode { path: path.to_path_buf(), message: e.to_string() }),
        }
    }

    fn tagging(self, path: &Path) -> Result<T> {
        match self {
            Ok(v) => Ok(v),
            Err(e) => Err(Error::Tags { path: path.to_path_buf(), message: e.to_string() }),
        }
    }
}

/// [`Context`] for I/O, keeping the error itself.
pub(crate) trait IoContext<T> {
    fn at(self, path: &Path) -> Result<T>;
}

impl<T> IoContext<T> for io::Result<T> {
    fn at(self, path: &Path) -> Result<T> {
        match self {
            Ok(v) => Ok(v),
            Err(source) => Err(Error::Io { path: path.to_path_buf(), source }),
        }
    }
}
