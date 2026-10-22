//! Files edits keep on a drive for its journal to name, by hash, below its state folder
//! (`.locallibrary/<folder>/<hash>.<ext>`): the images artwork edits set and the waveforms
//! analysis edits write, so replaying the journal after rekordbox exports makes the same edit.
//! The journal keeps those its entries name ([`Staged::prune`]) and takes them along when its
//! library goes to another drive ([`Staged::copy`]).

use rustc_hash::FxHashSet;
use std::path::PathBuf;

use crate::Result;
use crate::device::Device;
use crate::fsutil;
use crate::hash::Hash;
use crate::progress::Ctx;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Staged {
    folder: &'static str,
    ext: &'static str,
}

/// Artwork edits' images: JPEGs of the larger size players show.
pub(crate) const IMAGES: Staged = Staged { folder: "artwork", ext: "jpg" };

/// Analysis edits' waveforms: three bands packed ([`crate::anlz::Bands`]), 150 columns a second.
pub(crate) const WAVEFORMS: Staged = Staged { folder: "waveforms", ext: "bands" };

impl Staged {
    pub(crate) fn path(self, device: &Device, hash: &Hash) -> PathBuf {
        device.state_dir().join(self.folder).join(format!("{hash}.{}", self.ext))
    }

    /// Keep only the files in `keep`.
    pub(crate) fn prune(self, device: &Device, keep: &FxHashSet<Hash>) -> Result<()> {
        let Ok(entries) = std::fs::read_dir(device.state_dir().join(self.folder)) else { return Ok(()) };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let named = name.strip_suffix(&format!(".{}", self.ext)).and_then(|h| h.parse::<Hash>().ok());
            if !named.is_some_and(|h| keep.contains(&h)) {
                fsutil::remove_if_exists(&e.path())?;
            }
        }
        Ok(())
    }

    /// Give `target` the files in `hashes` that `source` has and it lacks.
    pub(crate) fn copy(self, source: &Device, target: &Device, hashes: &FxHashSet<Hash>) -> Result<()> {
        for hash in hashes {
            let (from, to) = (self.path(source, hash), self.path(target, hash));
            if from.is_file() && !to.is_file() {
                fsutil::copy_file(&from, &to, None, Some(*hash), true, &Ctx::silent())?;
            }
        }
        Ok(())
    }
}
