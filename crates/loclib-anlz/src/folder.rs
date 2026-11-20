//! Where rekordbox puts a track's analysis files on a drive.

use std::fmt;

/// The folder rekordbox gives the analysis files of the audio file at a path, as `export.pdb`
/// stores it (`/Contents/...`): `PIONEER/USBANLZ/Pxxx/xxxxxxxx`, relative to the drive's root,
/// kept as its two numbers. [`fmt::Display`] writes it as that path.
///
/// It isn't free to choose. An XDJ-RX3 looks for a track's analysis in this folder, whatever
/// path the track's `export.pdb` row gives, and finds none anywhere else: a track whose files are
/// elsewhere loads with no waveforms and no beat grid.
///
/// The folder is a hash of the path's UTF-16 code units (surrogate pairs as two):
///
/// 1. Each code unit `c` goes in as `h = (h * 0x5BC9 + c) * 0x93B5 + c`, wrapping at 32 bits,
///    from `h = 0`.
/// 2. `r = h % 200003` is the second folder, as eight hex digits.
/// 3. `Pxxx` is seven of `r`'s bits, from bit 0, 2, 6, 7, 9, 13 and 16 in turn, as three hex
///    digits.
///
/// That places every analysed track of three rekordbox exports: 2,244 of 2,244 on a rekordbox
/// 7.2.14 drive, and all 3,886 and 2 of the `large` and `demo_tracks` exports in locallibrary's
/// test fixtures. Two paths can hash alike; rekordbox then numbers the second track's files
/// `ANLZ0001.DAT` and up in the same folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnalysisFolder {
    bucket: u8,
    hash: u32,
}

impl AnalysisFolder {
    /// The folder for `file_path`.
    pub fn of(file_path: &str) -> AnalysisFolder {
        AnalysisFolder::of_parts(&[file_path])
    }

    /// The folder for the path `parts` make one after another, without joining them: `["/", key]`
    /// for a path kept without its leading slash.
    pub fn of_parts(parts: &[&str]) -> AnalysisFolder {
        let units = parts.iter().flat_map(|p| p.encode_utf16()).map(u32::from);
        let h = units.fold(0u32, |h, c| h.wrapping_mul(0x5BC9).wrapping_add(c).wrapping_mul(0x93B5).wrapping_add(c));
        let hash = h % 200_003;
        let bucket =
            [0, 2, 6, 7, 9, 13, 16].iter().enumerate().fold(0u8, |p, (i, bit)| p | (((hash >> bit) & 1) as u8) << i);
        AnalysisFolder { bucket, hash }
    }

    /// The `Pxxx` folder's number.
    pub fn bucket(self) -> u8 {
        self.bucket
    }

    /// The `xxxxxxxx` folder's number.
    pub fn hash(self) -> u32 {
        self.hash
    }
}

impl fmt::Display for AnalysisFolder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PIONEER/USBANLZ/P{:03X}/{:08X}", self.bucket, self.hash)
    }
}
