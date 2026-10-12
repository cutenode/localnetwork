#![doc = include_str!("../README.md")]

pub mod audio;
mod error;
pub mod fingerprint;
pub mod index;
pub mod matcher;
mod refine;
#[cfg(any(test, feature = "test-util"))]
pub mod test_util;

use std::path::Path;

use loclib_progress::Ctx;

pub use audio::SourceFormat;
pub use error::{Error, Result};
pub use index::{LibraryIndex, TrackPrint, fingerprint_file};
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
