use std::path::PathBuf;

/// What can go wrong reading or rendering a project.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A file isn't an Audacity project that can be read: not SQLite, missing a table, a damaged
    /// document, a sample block that's missing or in an unknown format.
    #[error("could not parse {}: {message}", path.display())]
    Parse {
        /// The project file.
        path: PathBuf,
        /// What's wrong with it.
        message: String,
    },

    /// The [`Ctx`](loclib_progress::Ctx) was cancelled.
    #[error(transparent)]
    Cancelled(#[from] loclib_progress::Cancelled),
}

/// A `Result` whose error is this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub(crate) fn parse(path: impl Into<PathBuf>, message: impl ToString) -> Self {
        Error::Parse { path: path.into(), message: message.to_string() }
    }
}
