//! The `rekordbox-xml` importer (`import/read`): each collection track of a rekordbox XML file,
//! found on a drive by its file, artist, title and length, with its key, tempo, hot cues, memory
//! cues and loops, which the host adds where the track has none.

use loclib_plugin::{Blob, Host, ImportAnswer, Imported, ImportedCue, ImportedTrack, ReadImport, RpcError, TrackMatch};
use loclib_tracklist::{Mark, XmlTrack, read_xml_collection};

use crate::xml_file;

/// `import/read`: what the file says of each collection track, in a blob when it's big.
pub(crate) fn read_import(req: ReadImport, host: &Host) -> Result<ImportAnswer, RpcError> {
    let library = xml_file::read(&req.file, read_xml_collection)?;
    host.ctx().check()?;
    let mut unsized_loops = 0;
    let tracks = library.tracks.into_iter().map(|t| imported(t, &mut unsized_loops)).collect();
    let mut warnings = library.warnings;
    if unsized_loops > 0 {
        warnings.push(format!(
            "{unsized_loops} loop(s) imported as cues: not 1-512 beats or 1/2 to 1/32 of one at the track's tempo"
        ));
    }
    let read = Imported { tracks, warnings, ..Imported::default() };
    Ok(match xml_file::spilled(&read, req.context.scratch.as_deref(), "read.json")? {
        Some(blob) => Blob { blob }.into(),
        None => read.into(),
    })
}

/// A collection track as the host finds it and what it adds; a loop whose length isn't one cue
/// edits take in beats comes as a cue, counted in `unsized_loops`.
fn imported(t: XmlTrack, unsized_loops: &mut usize) -> ImportedTrack {
    let cues = t
        .marks
        .iter()
        .map(|m| {
            let loop_beats = m.end_ms.and_then(|end| {
                let beats = tempo_at(&t, m).and_then(|bpm| loop_beats(end - m.start_ms, bpm));
                *unsized_loops += usize::from(beats.is_none());
                beats
            });
            ImportedCue {
                hot_cue: m.hot_cue.map(|slot| char::from(b'A' + slot).to_string()),
                start_ms: m.start_ms,
                loop_beats,
                name: Some(m.name.clone()).filter(|n| !n.is_empty()),
                color: m.rgb.map(xml_file::hex),
            }
        })
        .collect();
    let some = |s: String| Some(s).filter(|s| !s.is_empty());
    let e = t.entry;
    let find = TrackMatch {
        path: e.path,
        isrc: e.isrc,
        artist: some(e.artist),
        title: some(e.title),
        duration: e.duration_secs,
    };
    ImportedTrack { find, key: t.key, bpm: t.bpm, cues, my_tags: Vec::new() }
}

/// The tempo where `m` starts: its grid's there (or its first), else the track's.
fn tempo_at(t: &XmlTrack, m: &Mark) -> Option<f64> {
    let at = f64::from(m.start_ms);
    let tempo = t.tempos.iter().rev().find(|x| x.start_ms <= at).or(t.tempos.first());
    tempo.map(|x| x.bpm).or(t.bpm)
}

/// A loop `len_ms` long at `bpm` as the beats cue edits take, 1-512 or 1/2 to 1/32 of one, when
/// it's within 1% (or 2 ms) of one: the file has it to the millisecond.
fn loop_beats(len_ms: u32, bpm: f64) -> Option<f64> {
    let beats = f64::from(len_ms) * bpm / 60_000.0;
    let nearest = match beats >= 0.75 {
        true => beats.round().min(512.0),
        false => 2f64.powi(beats.log2().round().clamp(-5.0, -1.0) as i32),
    };
    let off_ms = (beats - nearest).abs() * 60_000.0 / bpm;
    (off_ms <= (f64::from(len_ms) / 100.0).max(2.0)).then_some(nearest)
}

#[cfg(test)]
pub(crate) mod tests;
