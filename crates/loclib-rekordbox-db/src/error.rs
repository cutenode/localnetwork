use std::io;
use std::path::{Path, PathBuf};

/// What can go wrong reading or editing a rekordbox database.
///
/// New kinds of error may come: a `match` needs a `_` arm.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A file (or the temporary folder a working copy goes in) couldn't be read or written.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file or folder.
        path: PathBuf,
        /// What the system said.
        source: io::Error,
    },

    /// A file isn't what it should be: a `master.db` that can't be read, or rekordbox's settings
    /// that aren't well-formed XML.
    #[error("could not parse {}: {message}", path.display())]
    Parse {
        /// The file.
        path: PathBuf,
        /// What's wrong with it.
        message: String,
    },

    /// A `master.db` doesn't decrypt with the key given: it isn't a rekordbox 6 or 7 collection,
    /// or rekordbox changed its key ([`crate::masterdb::KEY_ENV`] gives another).
    #[error(
        "{} can't be decrypted with this key: it isn't a rekordbox 6/7 collection, or rekordbox changed its \
         key (give the right one, or set {})",
        .0.display(),
        crate::masterdb::KEY_ENV
    )]
    WrongKey(PathBuf),

    /// A `master.db` and its WAL kept changing while they were copied: rekordbox was saving.
    #[error("{} kept changing while it was read (is rekordbox saving?)", .0.display())]
    Changing(PathBuf),

    /// The OneLibrary database (`exportLibrary.db`) can't be read, edited or written.
    #[error("exportLibrary.db: {0}")]
    OneLibrary(String),

    /// A row an edit names isn't there.
    #[error("not found: {0}")]
    NotFound(String),

    /// A value an edit gives can't be used.
    #[error("{0}")]
    Invalid(String),

    /// An id an edit gives a new row is taken already.
    #[error("{0}")]
    Exists(String),

    /// A write of rekordbox's collection that wasn't made, and why; `master.db` is as it was.
    #[error("{0}")]
    Refused(Refusal),

    /// Renaming the new collection over `master.db` failed. `master.db` is most likely as it was,
    /// but on a drive that removes a file before renaming over it, it can be gone: the new
    /// collection and the backup of the old one are both kept.
    #[error(
        "{} couldn't be replaced ({source}): check it's still there. The edited collection is kept at {}, and the \
         backup from before at {}", master_db.display(), new.display(), backup.display()
    )]
    SwapFailed {
        /// The `master.db`.
        master_db: PathBuf,
        /// The new collection, beside it.
        new: PathBuf,
        /// The backup of `master.db` from right before.
        backup: PathBuf,
        /// What the system said.
        source: io::Error,
    },

    /// The edit was written, but rekordbox or its agent started (or wrote beside `master.db`) as it
    /// was swapped in, so rekordbox may have the old collection open: the backup from right before.
    #[error(
        "the edit was written, but rekordbox started as it was: quit rekordbox without saving changes and check its \
         collection (the backup from before is {})", .0.display()
    )]
    OpenedDuringSwap(PathBuf),
}

/// Why a write of rekordbox's collection (`master.db`) wasn't made. Nothing was written.
///
/// New reasons may come: a `match` needs a `_` arm.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Refusal {
    /// rekordbox is open.
    #[error("rekordbox is open: its collection is written only once rekordbox has quit")]
    RekordboxRunning,

    /// rekordbox's agent is still running, without rekordbox or with it.
    #[error(
        "rekordbox's agent (rekordboxAgent) is still running: it keeps rekordbox's collection open and writes to \
         it, so the collection is written only once it has quit too (it quits when rekordbox asks it to, or quit \
         it from its menu bar icon)"
    )]
    AgentRunning,

    /// Cloud Library Sync is on, or might be: what made it look so.
    #[error(
        "Cloud Library Sync looks to be on ({0}). locallibrary doesn't write a synced collection yet: what a write \
         has to change so it syncs, and isn't undone by the cloud's copy, hasn't been worked out"
    )]
    CloudSync(String),

    /// A file beside `master.db` (its WAL, shared memory or journal) holds changes rekordbox hasn't
    /// put into `master.db` yet.
    #[error(
        "{} holds changes rekordbox hasn't saved into master.db yet: open rekordbox and quit it again, then try \
         again", .0.display()
    )]
    Unsaved(PathBuf),

    /// `master.db` changed between its backup and the swap: rekordbox (or something else) wrote it.
    #[error("master.db changed while the edit was being made (did rekordbox start?), so it wasn't written")]
    Changed,

    /// Another write of the same collection is being made.
    #[error("another write of rekordbox's collection is being made")]
    Busy,

    /// Anything else that makes writing unsafe: what it is.
    #[error("{0}, so it wasn't written")]
    Odd(String),
}

impl Refusal {
    /// Whether the write can be made later as it is, once rekordbox and its agent have quit (or
    /// another write is done): they were running, the file changed underneath, or another write
    /// was being made. An edit refused so is worth keeping (staged) and trying again.
    pub fn waits(&self) -> bool {
        matches!(self, Refusal::RekordboxRunning | Refusal::AgentRunning | Refusal::Changed | Refusal::Busy)
    }
}

impl Error {
    pub(crate) fn parse(path: impl Into<PathBuf>, message: impl ToString) -> Self {
        Error::Parse { path: path.into(), message: message.to_string() }
    }
}

/// A result whose error is this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Attach a path to `io::Error`s.
pub(crate) trait IoContext<T> {
    fn at(self, path: impl AsRef<Path>) -> Result<T>;
}

impl<T> IoContext<T> for io::Result<T> {
    fn at(self, path: impl AsRef<Path>) -> Result<T> {
        self.map_err(|source| Error::Io { path: path.as_ref().to_path_buf(), source })
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::OneLibrary(e.to_string())
    }
}

impl From<loclib_sqlcipher::Error> for Error {
    fn from(e: loclib_sqlcipher::Error) -> Self {
        match e {
            // Here the key is rekordbox's own, so a page that fails is the file's fault.
            loclib_sqlcipher::Error::Authentication(pgno) => {
                Error::OneLibrary(format!("page {pgno} fails authentication (not a rekordbox database, or corrupt)"))
            }
            other => Error::OneLibrary(other.to_string()),
        }
    }
}
