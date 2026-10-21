//! Writing waveforms: the blue preview and detail (`PWAV` and `PWV2` in `.DAT`, `PWV3` in
//! `.EXT`), the colour ones (`PWV4`, `PWV5`, `.EXT`) and the 3-band ones (`PWV6`, `PWV7`, `.2EX`),
//! as [DJL's waveform tags](https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/anlz.html#waveform-preview-tag)
//! lay them out: after the lengths, previews (`PWAV`, `PWV2`) have their column count u32 and a
//! u32 (0x00010000 in rekordbox 6 exports); the others the bytes a column u32 and the count u32,
//! then (but `PWV6`) a u32 starting with the columns a second u16 (0x0096, 150). A file's header
//! fields are kept as it has them; new ones are rekordbox 6's.
//!
//! [`write_waveform`] writes a tag's columns as given, and [`waveform_from_bands`] works them out
//! from a three-band waveform ([`Bands`], 150 columns a second) for every tag whose bytes are
//! all understood: a blue column is its height in the low 5 bits and its whiteness (here the
//! highs' share) in the top 3; `PWV2` a height of 4 bits; `PWV5` red, green and blue (lows, mids
//! and highs against the loudest) in 3 bits each, then the height in 5 and 2 unused; 3-band
//! columns are the mids', highs' and lows' heights ([beat-link] draws `PWV6` from 0-63 and
//! `PWV7` from 0-127, so they're a quarter and half of a band's 0-255 here). `PWV4`'s first two
//! bytes a column aren't understood, so it's never worked out. These mappings are locallibrary's
//! own: players draw what they're given, but only real players show whether it looks right.
//!
//! [beat-link]: https://github.com/Deep-Symmetry/beat-link/blob/main/src/main/java/org/deepsymmetry/beatlink/data/WaveformDetail.java

use super::tags::{only, put, section};
use super::write::put_u32;
use super::*;

/// A waveform tag's layout: the bytes a column, the header fields after the lengths as rekordbox
/// 6 writes them (the count left 0), where in the section the count is, and a preview's columns.
struct Spec {
    tag: [u8; 4],
    entry: usize,
    header: &'static [u8],
    count_at: usize,
    preview: Option<usize>,
}

const SPECS: [Spec; 7] = [
    Spec { tag: *b"PWAV", entry: 1, header: &[0, 0, 0, 0, 0, 1, 0, 0], count_at: 12, preview: Some(400) },
    Spec { tag: *b"PWV2", entry: 1, header: &[0, 0, 0, 0, 0, 1, 0, 0], count_at: 12, preview: Some(100) },
    Spec { tag: *b"PWV3", entry: 1, header: &[0, 0, 0, 1, 0, 0, 0, 0, 0, 0x96, 0, 0], count_at: 16, preview: None },
    Spec { tag: *b"PWV4", entry: 6, header: &[0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0], count_at: 16, preview: Some(1200) },
    Spec { tag: *b"PWV5", entry: 2, header: &[0, 0, 0, 2, 0, 0, 0, 0, 0, 0x96, 3, 5], count_at: 16, preview: None },
    Spec { tag: *b"PWV6", entry: 3, header: &[0, 0, 0, 3, 0, 0, 0, 0], count_at: 16, preview: Some(1200) },
    Spec { tag: *b"PWV7", entry: 3, header: &[0, 0, 0, 3, 0, 0, 0, 0, 0, 0x96, 0, 0], count_at: 16, preview: None },
];

/// The columns a second of the detail waveforms (`PWV3`, `PWV5`, `PWV7`) written here.
pub const DETAIL_RATE: f64 = 150.0;

/// A 3-band column's height in `PWV6` and `PWV7`, as a fraction of a band's (0-255).
const PWV6_SHIFT: u32 = 2;
const PWV7_SHIFT: u32 = 1;

fn spec(tag: &[u8; 4]) -> Result<&'static Spec> {
    SPECS
        .iter()
        .find(|s| &s.tag == tag)
        .ok_or_else(|| Error::invalid(format!("{} isn't a waveform tag", String::from_utf8_lossy(tag))))
}

/// The waveform tags, in rekordbox's order within each file.
pub fn waveform_tags() -> impl Iterator<Item = &'static [u8; 4]> {
    SPECS.iter().map(|s| &s.tag)
}

/// The columns of an analysis file's waveform `tag` (a `PWV...` or `PWAV`), as stored: as many
/// as its header says, no more than are there.
pub fn waveform_entries(bytes: &[u8], tag: &[u8; 4]) -> Result<Option<Vec<u8>>> {
    let spec = spec(tag)?;
    let secs = sections(bytes)?;
    Ok(only(&secs, tag)?.and_then(|s| entries(bytes, s, spec.entry, Some(spec.count_at))).map(<[u8]>::to_vec))
}

/// A copy of an analysis file whose waveform `tag` holds `columns` (as stored, whole columns),
/// added where rekordbox puts it when it has none. The file's header fields are kept; the same
/// columns keep the file as it was, byte for byte.
pub fn write_waveform(bytes: &[u8], tag: &[u8; 4], columns: &[u8]) -> Result<Vec<u8>> {
    let spec = spec(tag)?;
    let name = String::from_utf8_lossy(tag);
    if columns.is_empty() || !columns.len().is_multiple_of(spec.entry) {
        return Err(Error::invalid(format!("{name} takes whole columns of {} bytes", spec.entry)));
    }
    let count = u32::try_from(columns.len() / spec.entry).map_err(|_| Error::invalid("too many columns"))?;
    let secs = sections(bytes)?;
    let header_len = 12 + spec.header.len();
    let kept = only(&secs, tag)?.filter(|s| s.header_len == header_len);
    let mut header = kept.map_or(spec.header.to_vec(), |s| bytes[s.start + 12..s.start + header_len].to_vec());
    put_u32(&mut header, spec.count_at - 12, count);
    let out = put(bytes, tag, Some(&section(tag, &header, columns)?))?;
    // Never a waveform the reader doesn't read back as given.
    if waveform_entries(&out, tag)?.as_deref() != Some(columns) {
        return Err(malformed(&format!("the {name} waveform written doesn't read back")));
    }
    Ok(out)
}

/// `bands` (at `rate` columns a second) at [`DETAIL_RATE`], as long.
pub fn at_detail_rate(bands: &Bands, rate: f64) -> Bands {
    let n = (bands.0.len() as f64 * DETAIL_RATE / rate).round() as usize;
    bands.pooled(n.max(1))
}

/// The columns of waveform `tag` worked out from `bands` (the detail, at [`DETAIL_RATE`]; previews
/// are pooled from it), as stored. `None` for `PWV4`, whose bytes aren't all understood.
pub fn waveform_from_bands(tag: &[u8; 4], bands: &Bands) -> Result<Option<Vec<u8>>> {
    let spec = spec(tag)?;
    let bands = match spec.preview {
        Some(n) => bands.pooled(n),
        None => bands.clone(),
    };
    let scaled = |v: u8, most: u32, of: u8| (u32::from(v) * most / u32::from(of.max(1))) as u8;
    let loudest = |c: &[u8; 3]| c[0].max(c[1]).max(c[2]);
    let blue = |c: &[u8; 3]| {
        let top = loudest(c);
        (scaled(c[2], 7, top) << 5) | scaled(top, 31, 255)
    };
    let columns: Vec<u8> = match tag {
        b"PWAV" | b"PWV3" => bands.0.iter().map(blue).collect(),
        b"PWV2" => bands.0.iter().map(|c| scaled(loudest(c), 15, 255)).collect(),
        b"PWV5" => bands
            .0
            .iter()
            .flat_map(|c| {
                let top = loudest(c);
                let [r, g, b] = c.map(|v| u16::from(scaled(v, 7, top)));
                let v = (r << 13) | (g << 10) | (b << 7) | (u16::from(scaled(top, 31, 255)) << 2);
                v.to_be_bytes()
            })
            .collect(),
        b"PWV6" => bands.0.iter().flat_map(|c| [c[1], c[2], c[0]].map(|v| v >> PWV6_SHIFT)).collect(),
        b"PWV7" => bands.0.iter().flat_map(|c| [c[1], c[2], c[0]].map(|v| v >> PWV7_SHIFT)).collect(),
        _ => return Ok(None),
    };
    Ok(Some(columns))
}

/// A 3-band waveform's stored columns (mids, highs, lows) as [`Bands`], for drawing.
pub(super) fn three_band(columns: &[u8], tag: &[u8; 4]) -> Bands {
    let shift = if tag == b"PWV6" { PWV6_SHIFT } else { PWV7_SHIFT };
    let up = |v: u8| (u32::from(v) << shift).min(255) as u8;
    Bands(columns.chunks_exact(3).map(|c| [up(c[2]), up(c[0]), up(c[1])]).collect())
}
