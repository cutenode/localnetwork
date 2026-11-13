#![doc = include_str!("../README.md")]

pub mod fingerprint;
pub mod index;
pub mod matcher;
mod refine;

use std::path::Path;

use loclib_progress::Ctx;

// Decoding is loclib-audio's, and so is the error: nothing past decoding fails here but an
// argument out of range.
pub use index::{LibraryIndex, TrackPrint, fingerprint_file};
use loclib_audio as audio;
pub use loclib_audio::{Error, Result, SourceFormat};
pub use matcher::Found;
pub use refine::{REFINE_MIN_CORRELATION, Refined, refine};

use fingerprint::{Analyzer, FrameLevels, Peak};

/// A recording decoded and fingerprinted, ready for [`matcher::find`].
pub struct Heard {
    pub peaks: Vec<Peak>,
    /// Each frame's loudness, for drawing the recording.
    pub levels: Vec<FrameLevels>,
    pub frames: u32,
}

impl From<Analyzer> for Heard {
    fn from(analyzer: Analyzer) -> Heard {
        let frames = analyzer.frames();
        let (peaks, levels) = analyzer.finish();
        Heard { peaks, levels, frames }
    }
}

/// Decode and fingerprint the recording at `path`. Another source (an editor's project) is heard
/// by pushing it into an [`Analyzer`] made with [`fingerprint::QUERY`] and levels, then taking
/// [`Heard::from`] it.
pub fn listen(path: &Path, ctx: &Ctx<'_>) -> Result<(SourceFormat, Heard)> {
    let mut analyzer = Analyzer::new(fingerprint::QUERY, true);
    let format = audio::decode(path, fingerprint::RATE, ctx, &mut |s| analyzer.push(s))?;
    Ok((format, analyzer.into()))
}

#[cfg(test)]
mod tests;
