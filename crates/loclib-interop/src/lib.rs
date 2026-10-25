#![doc = include_str!("../README.md")]

pub mod encode;
#[cfg(feature = "engine")]
pub mod engine;
#[cfg(feature = "rekordbox")]
pub mod rekordbox;
#[cfg(feature = "serato")]
pub mod serato;
#[cfg(feature = "traktor")]
pub mod traktor;

use std::path::PathBuf;

use loclib_model::analysis::TrackAnalysis;
use loclib_model::{Library, Track, paths};
use serde::{Deserialize, Serialize};

/// The DJ software to write for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Software {
    /// Native Instruments Traktor Pro: `collection.nml`.
    Traktor,
    /// Serato DJ: `_Serato_/Subcrates/*.crate`.
    Serato,
    /// Engine DJ: `Engine Library/Database2/m.db`.
    Engine,
    /// rekordbox: a `DJ_PLAYLISTS` XML file, for its Imported Library.
    Rekordbox,
}

impl Software {
    pub fn name(self) -> &'static str {
        match self {
            Software::Traktor => "Traktor",
            Software::Serato => "Serato",
            Software::Engine => "Engine DJ",
            Software::Rekordbox => "rekordbox",
        }
    }
}

/// What a writer reads: the library, where its audio will be, and each track's analysis.
pub struct Source<'a> {
    /// Every track, playlist and folder in it is written. Its playlist tree must end (see
    /// [`loclib_model::model::untangle`]).
    pub library: &'a Library,
    /// The folder holding the library's `Contents/` when the other software reads the files:
    /// tracks point at their `file_path` below it.
    pub root: PathBuf,
    /// A track's beat grid and cues; `None` leaves them out.
    pub analysis: Option<&'a dyn Fn(&Track) -> TrackAnalysis>,
}

impl Source<'_> {
    /// Where a track's audio will be.
    pub fn audio(&self, t: &Track) -> PathBuf {
        paths::to_fs(&self.root, &paths::key(&t.file_path))
    }
}

/// Which file a track's audio is, for keeping one track per file: its path below the library (the
/// parts [`paths::to_fs`] keeps), ignoring case, as the drives DJs use do (FAT32, exFAT, and APFS
/// and HFS+ as macOS formats them), so `/Music/a.mp3` and `/music/A.mp3` are one file.
#[cfg(any(feature = "traktor", feature = "serato", feature = "engine"))]
fn file_key(t: &Track) -> String {
    let key = paths::key(&t.file_path);
    key.split('/').filter(|c| paths::is_safe_component(c)).collect::<Vec<_>>().join("/").to_lowercase()
}

/// An export worked out in memory: every file and how to make it, and what the format leaves
/// out.
#[derive(Debug, Default)]
pub struct Export {
    pub files: Vec<File>,
    /// Files that must go when the export replaces what's there (a database's old WAL, which
    /// SQLite would replay into the new file).
    pub remove: Vec<PathBuf>,
    pub tracks: usize,
    pub playlists: usize,
    pub folders: usize,
    pub beat_grids: usize,
    /// Hot cues, memory cues and loops carried across.
    pub cues: usize,
    /// Bytes of audio copied (Serato with copies).
    pub copy_bytes: u64,
    /// What the format can't hold, and what to do with the files.
    pub notes: Vec<String>,
    /// Problems with this library: cues the format has no room for, track rows sharing a file...
    /// Whether the audio is where the files say isn't checked.
    pub warnings: Vec<String>,
}

/// A file an export writes.
#[derive(Debug)]
pub struct File {
    pub path: PathBuf,
    /// What it holds.
    pub what: String,
    pub bytes: u64,
    pub content: Content,
}

/// How a file is made.
#[derive(Debug)]
pub enum Content {
    Bytes(Vec<u8>),
    /// A copy of an audio file, with Serato's cue tag written into it when there's one (see
    /// `serato::write_markers`).
    TaggedCopy {
        from: PathBuf,
        markers: Option<Vec<u8>>,
    },
}

impl Export {
    /// An export of `lib`, counting its playlists and folders.
    #[cfg(any(feature = "traktor", feature = "serato", feature = "engine", feature = "rekordbox"))]
    fn new(lib: &Library) -> Export {
        Export {
            playlists: lib.playlists.iter().filter(|p| !p.is_folder).count(),
            folders: lib.playlists.iter().filter(|p| p.is_folder).count(),
            ..Default::default()
        }
    }

    #[cfg(any(feature = "traktor", feature = "serato", feature = "engine", feature = "rekordbox"))]
    fn file(&mut self, path: PathBuf, what: impl Into<String>, bytes: Vec<u8>) {
        self.files.push(File { path, what: what.into(), bytes: bytes.len() as u64, content: Content::Bytes(bytes) });
    }
}

/// What can go wrong writing a library for other software.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The [`Ctx`](loclib_progress::Ctx) was cancelled.
    #[error(transparent)]
    Cancelled(#[from] loclib_progress::Cancelled),

    /// The library can't be written as asked.
    #[error("{0}")]
    Invalid(String),

    /// A file couldn't be read or written.
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: std::io::Error },

    /// An audio file's tags can't be read or written.
    #[error("tags: {0}")]
    Tags(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
