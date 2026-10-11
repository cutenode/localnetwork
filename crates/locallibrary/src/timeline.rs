//! Where rekordbox's timeline for a track (its cues, beat grid and waveforms) starts in the
//! track's decoded audio, for moving cues between two files of one recording.
//!
//! rekordbox times an MP3 from its first decoded frame, as symphonia decodes it, with no gapless
//! trimming of the encoder's delay, but some MP3s (all with a complete LAME tag, a third of those
//! from FFmpeg, few others) from one frame (1,152 samples) earlier: measured against the waveforms
//! of 1,072 MP3s of a rekordbox 7 export, 85% at the first frame and 14% a frame early, with no
//! tag field telling them apart. AAC decodes with its encoder's priming too (1,024 or 2,112
//! samples, as encoders write it), and how rekordbox times it hasn't been measured. So which it
//! is gets measured per track: its detail waveform against its decoded audio's level on each
//! timeline. Lossless audio starts at its first sample.

use crate::device::Device;
use crate::equipment::{Codec, FromPath};
use crate::model::Track;
use crate::progress::Ctx;
use crate::{Result, paths};

/// Why a lead can't be told (see [`lead`]).
pub(crate) const UNTOLD: &str = "where rekordbox's timeline starts in its AAC audio can't be told without its waveform";

/// Seconds from rekordbox's time 0 for `t` to its decoded audio's first sample (0 or negative:
/// rekordbox's timeline starts that much earlier). `None` for an AAC file without a waveform to
/// measure against; an MP3 without one is taken to start at its first frame.
pub(crate) fn lead(device: &Device, t: &Track, ctx: &Ctx<'_>) -> Result<Option<f64>> {
    let (leads, without_waveform): (&[usize], _) = match Codec::from_path(&t.file_path) {
        FromPath::Codec(Codec::Mp3) => (&[0, 1152], Some(0.0)),
        FromPath::Codec(Codec::Aac) => (&[0, 1024, 2112], None),
        FromPath::AacOrAlac if !crate::doctor::lossless(device, t) => (&[0, 1024, 2112], None),
        _ => return Ok(Some(0.0)),
    };
    let Some(detail) = crate::anlz::load(device, t).detail else { return Ok(without_waveform) };
    let file = device.checked_path(&paths::key(&t.file_path))?;
    let rate = crate::probe::format(&file)?.sample_rate;
    if rate == 0 {
        return Ok(without_waveform);
    }
    // A minute is plenty to tell a frame apart.
    let per_column = f64::from(rate) / detail.columns_per_sec.unwrap_or(150.0);
    let columns = detail.columns.len().min((60.0 * f64::from(rate) / per_column) as usize);
    let need = (columns as f64 * per_column) as usize;
    let mut level = Vec::with_capacity(need);
    loclib_mixid::audio::decode_until(&file, rate, ctx, &mut |s| {
        level.extend(s.iter().map(|v| f64::from(v.abs())));
        if level.len() >= need { std::ops::ControlFlow::Break(()) } else { std::ops::ControlFlow::Continue(()) }
    })?;
    let heights: Vec<f64> = detail.columns[..columns].iter().map(|c| f64::from(c.back.height)).collect();
    // The decoded level of each column when rekordbox's timeline starts `lead` samples early.
    let levels = |lead: usize| -> Vec<f64> {
        (0..columns)
            .map(|i| {
                let from = ((i as f64 * per_column) as usize).saturating_sub(lead);
                let to = (((i + 1) as f64 * per_column) as usize).saturating_sub(lead).min(level.len());
                level.get(from..to).map_or(0.0, |w| w.iter().sum::<f64>() / w.len().max(1) as f64)
            })
            .collect()
    };
    let best =
        leads.iter().copied().max_by(|a, b| pearson(&heights, &levels(*a)).total_cmp(&pearson(&heights, &levels(*b))));
    Ok(Some(-(best.unwrap_or(0) as f64) / f64::from(rate)))
}

/// Pearson's correlation of two series of one length (0 when either is flat).
fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let mean = |x: &[f64]| x.iter().sum::<f64>() / x.len().max(1) as f64;
    let (ma, mb) = (mean(a), mean(b));
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        ab += (x - ma) * (y - mb);
        aa += (x - ma).powi(2);
        bb += (y - mb).powi(2);
    }
    if aa == 0.0 || bb == 0.0 { 0.0 } else { ab / (aa * bb).sqrt() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without a waveform, an MP3 is taken to start at its first frame and lossless audio at its
    /// first sample; AAC can't be told.
    #[test]
    fn without_a_waveform() {
        let dir = tempfile::tempdir().unwrap();
        let device = Device::open(dir.path()).unwrap();
        let lead = |path: &str| {
            let t = Track { file_path: path.into(), ..Default::default() };
            lead(&device, &t, &Ctx::silent()).unwrap()
        };
        assert_eq!(lead("/Contents/a.mp3"), Some(0.0));
        assert_eq!(lead("/Contents/a.flac"), Some(0.0));
        assert_eq!(lead("/Contents/a.aac"), None);
    }
}
