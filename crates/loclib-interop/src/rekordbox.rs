//! rekordbox XML (`DJ_PLAYLISTS`), which rekordbox imports from Preferences > Advanced >
//! rekordbox xml: tracks with their details, beat grids, hot cues, memory cues, loops and
//! playlists. It turns a USB drive back into a computer library, to recover one or move it to a
//! new computer without the original.

use std::fmt::Write as _;
use std::path::Path;

use loclib_model::audio::Format;
use loclib_model::{Library, Playlist, Track, TrackColor};
use loclib_progress::{Ctx, Event, Phase};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::encode::xml_escape as esc;
use crate::{Export, Result, Source};

const PATH_ENCODE: &AsciiSet =
    &NON_ALPHANUMERIC.remove(b'/').remove(b'-').remove(b'_').remove(b'.').remove(b'~').remove(b':');

/// An audio file's absolute path as a `Location`: a `file://localhost/` URL.
fn location(audio: &Path) -> String {
    let s = audio.to_string_lossy().replace('\\', "/");
    let s = if s.starts_with('/') { s } else { format!("/{s}") };
    format!("file://localhost{}", utf8_percent_encode(&s, PATH_ENCODE))
}

fn kind(path: &str) -> &'static str {
    match Format::from_name(path) {
        Some(Format::Aac | Format::Mpeg4) => "M4A File",
        Some(Format::Wav) => "WAV File",
        Some(Format::Aiff) => "AIFF File",
        Some(Format::Flac) => "FLAC File",
        _ => "MP3 File",
    }
}

fn colour(c: TrackColor) -> &'static str {
    match c {
        TrackColor::Pink => "0xFF007F",
        TrackColor::Red => "0xFF0000",
        TrackColor::Orange => "0xFFA500",
        TrackColor::Yellow => "0xFFFF00",
        TrackColor::Green => "0x00FF00",
        TrackColor::Aqua => "0x25FDE9",
        TrackColor::Blue => "0x0000FF",
        TrackColor::Purple => "0x660099",
    }
}

/// `file`, holding `src`'s tracks, playlists and folders, and their beat grids and cues when it
/// has them. Its `PRODUCT` is locallibrary, at this crate's version.
pub fn build(src: &Source<'_>, file: &Path, ctx: &Ctx<'_>) -> Result<Export> {
    let lib = src.library;
    let mut export = Export::new(lib);
    export.tracks = lib.tracks.len();
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<DJ_PLAYLISTS Version=\"1.0.0\">\n");
    let _ = writeln!(
        x,
        "  <PRODUCT Name=\"locallibrary\" Version=\"{}\" Company=\"locallibrary\"/>",
        env!("CARGO_PKG_VERSION")
    );
    let _ = writeln!(x, "  <COLLECTION Entries=\"{}\">", lib.tracks.len());
    ctx.phase_start(Phase::Scan, lib.tracks.len() as u64, 0);
    for t in &lib.tracks {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Scan, path: t.file_path.clone(), bytes: 0 });
        track(&mut x, t, src, &mut export);
    }
    ctx.phase_end(Phase::Scan);
    x.push_str("  </COLLECTION>\n  <PLAYLISTS>\n");
    let top = lib.children(None);
    let _ = writeln!(x, "    <NODE Type=\"0\" Name=\"ROOT\" Count=\"{}\">", top.len());
    for p in top {
        node(&mut x, lib, p, 0);
    }
    x.push_str("    </NODE>\n  </PLAYLISTS>\n</DJ_PLAYLISTS>\n");
    export.notes = vec![
        "In rekordbox, pick the file in Preferences > Advanced > rekordbox xml > Imported Library: its tracks and \
         playlists show under rekordbox xml in the tree, to import from there."
            .into(),
        "History lists aren't exported; My Tags, artwork and waveforms aren't part of the file.".into(),
    ];
    export.file(file.to_path_buf(), "rekordbox XML: tracks, beat grids, cues and playlists", x.into_bytes());
    Ok(export)
}

fn track(x: &mut String, t: &Track, src: &Source<'_>, export: &mut Export) {
    let rating = [0, 51, 102, 153, 204, 255][t.rating.min(5) as usize];
    // rekordbox shows the year; fall back to the release date's when only that was stored.
    let year = match t.year {
        0 => t.release_date.get(..4).and_then(|y| y.parse::<u16>().ok()).unwrap_or(0),
        y => y,
    };
    let _ = write!(
        x,
        "    <TRACK TrackID=\"{}\" Name=\"{}\" Artist=\"{}\" Composer=\"{}\" Album=\"{}\" Grouping=\"\" Genre=\"{}\" \
         Kind=\"{}\" Size=\"{}\" TotalTime=\"{}\" DiscNumber=\"{}\" TrackNumber=\"{}\" Year=\"{}\" AverageBpm=\"{:.2}\" \
         DateAdded=\"{}\" BitRate=\"{}\" SampleRate=\"{}\" Comments=\"{}\" PlayCount=\"{}\" Rating=\"{rating}\" \
         Location=\"{}\" Remixer=\"{}\" Tonality=\"{}\" Label=\"{}\" Mix=\"{}\"",
        t.id,
        esc(&t.title),
        esc(&t.artist),
        esc(&t.composer),
        esc(&t.album),
        esc(&t.genre),
        kind(&t.file_path),
        t.file_size,
        t.duration_secs,
        t.disc_number,
        t.track_number,
        year,
        t.bpm,
        esc(&t.date_added),
        t.bitrate,
        t.sample_rate,
        esc(&t.comment),
        t.play_count,
        esc(&location(&src.audio(t))),
        esc(&t.remixer),
        esc(&t.key),
        esc(&t.label),
        esc(&t.mix_name),
    );
    if let Some(c) = t.color {
        let _ = write!(x, " Colour=\"{}\"", colour(c));
    }
    let analysis = src.analysis.map(|load| load(t));
    let Some(a) = analysis.filter(|a| !a.beats.is_empty() || !a.cues.is_empty()) else {
        x.push_str("/>\n");
        return;
    };
    x.push_str(">\n");
    if !a.beats.is_empty() {
        export.beat_grids += 1;
    }
    // One TEMPO element per tempo change.
    let mut last_bpm = None;
    for b in &a.beats {
        if last_bpm != Some(b.bpm) {
            let _ = writeln!(
                x,
                "      <TEMPO Inizio=\"{:.3}\" Bpm=\"{:.2}\" Metro=\"4/4\" Battito=\"{}\"/>",
                b.time_ms as f64 / 1000.0,
                b.bpm,
                b.beat_number.clamp(1, 4)
            );
            last_bpm = Some(b.bpm);
        }
    }
    for c in &a.cues {
        export.cues += 1;
        let num = c.hot_cue.map_or(-1, |h| h as i32);
        let kind = if c.end_ms.is_some() { 4 } else { 0 };
        let _ = write!(
            x,
            "      <POSITION_MARK Name=\"{}\" Type=\"{kind}\" Start=\"{:.3}\"",
            esc(&c.comment),
            c.start_ms as f64 / 1000.0
        );
        if let Some(end) = c.end_ms {
            let _ = write!(x, " End=\"{:.3}\"", end as f64 / 1000.0);
        }
        let _ = write!(x, " Num=\"{num}\"");
        if let (Some([r, g, b]), Some(_)) = (c.color, c.hot_cue) {
            let _ = write!(x, " Red=\"{r}\" Green=\"{g}\" Blue=\"{b}\"");
        }
        x.push_str("/>\n");
    }
    x.push_str("    </TRACK>\n");
}

/// A playlist or a folder (with what it holds), `depth` folders below `ROOT`.
fn node(x: &mut String, lib: &Library, p: &Playlist, depth: usize) {
    let indent = "  ".repeat(depth + 3);
    if p.is_folder {
        let kids = lib.children(Some(p.id));
        let _ = writeln!(x, "{indent}<NODE Type=\"0\" Name=\"{}\" Count=\"{}\">", esc(&p.name), kids.len());
        for k in kids {
            node(x, lib, k, depth + 1);
        }
    } else {
        let _ = writeln!(
            x,
            "{indent}<NODE Name=\"{}\" Type=\"1\" KeyType=\"0\" Entries=\"{}\">",
            esc(&p.name),
            p.track_ids.len()
        );
        for id in &p.track_ids {
            let _ = writeln!(x, "{indent}  <TRACK Key=\"{id}\"/>");
        }
    }
    let _ = writeln!(x, "{indent}</NODE>");
}

#[cfg(test)]
mod tests;
