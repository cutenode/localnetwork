//! A track's rekordbox analysis files (`ANLZ0000.DAT/.EXT/.2EX`) on a drive. The format itself,
//! reading and writing every section locallibrary deals with, is [`loclib_anlz`]'s, re-exported
//! here; this module finds a track's files on a [`Device`] and reads them ([`load`]).

use crate::device::Device;
use crate::model::Track;
use crate::paths;

pub use loclib_anlz::*;

/// A track's beat grid, cues, waveforms and phrases. Missing or unreadable files leave fields
/// empty.
pub fn load(device: &Device, track: &Track) -> TrackAnalysis {
    read_track(device, track, true)
}

/// Every waveform a track's analysis files hold ([`TrackWaves`]).
pub fn load_waves(device: &Device, track: &Track) -> TrackWaves {
    let Some(stem) = paths::key(&track.analyze_path).strip_suffix(".DAT").map(str::to_string) else {
        return TrackWaves::default();
    };
    let read = |ext: &str| {
        let path = device.checked_path(&format!("{stem}.{ext}")).ok()?;
        crate::fsutil::read(&path).ok()
    };
    let [dat, ext, two_ex] = ["DAT", "EXT", "2EX"].map(read);
    TrackWaves::read(dat.as_deref(), ext.as_deref(), two_ex.as_deref())
}

/// A track's beat grid and cues only, for bulk work such as XML export.
pub fn load_grid(device: &Device, track: &Track) -> TrackAnalysis {
    read_track(device, track, false)
}

fn read_track(device: &Device, track: &Track, waveforms: bool) -> TrackAnalysis {
    let mut a = TrackAnalysis::default();
    if track.analyze_path.is_empty() {
        return a;
    }
    let dat = paths::key(&track.analyze_path);
    let read = |key: &str| device.checked_path(key).ok().and_then(|p| crate::fsutil::read(&p).ok());
    let sibling = |ext: &str| dat.strip_suffix(".DAT").map(|s| format!("{s}.{ext}")).and_then(|k| read(&k));
    let ext = sibling("EXT");
    let dat = read(&dat);
    a.cues = track_cues(dat.as_deref(), ext.as_deref());
    if let Some(bytes) = dat {
        a.beats = beat_grid(&bytes).unwrap_or_default();
        if waveforms {
            a.preview = preview(&bytes).ok().flatten();
        }
    }
    if waveforms && let Some(bytes) = ext {
        // Colour waveforms live in .EXT; keep the .DAT's blue preview otherwise.
        if let Some(p) = preview(&bytes).ok().flatten() {
            a.preview = Some(p);
        }
        a.detail = detail(&bytes).ok().flatten();
        a.structure = song_structure(&bytes).ok().flatten();
    }
    // The 3-band waveforms (`.2EX`), only for a track with no others.
    if waveforms
        && (a.preview.is_none() || a.detail.is_none())
        && let Some(bytes) = sibling("2EX")
    {
        a.preview = a.preview.or_else(|| preview(&bytes).ok().flatten());
        a.detail = a.detail.or_else(|| detail(&bytes).ok().flatten());
    }
    a
}

#[cfg(test)]
mod tests;
