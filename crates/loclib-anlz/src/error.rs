/// What can go wrong reading or writing an analysis file.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The bytes aren't an analysis file, or a section in it runs short or past its end.
    #[error("could not parse ANLZ: {0}")]
    Malformed(String),

    /// Something the format can't hold (a beat grid out of order, a cue name too long), or a file
    /// the writers won't guess at (one holding a section twice).
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Error::Invalid(message.into())
    }
}
