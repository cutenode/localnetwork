//! Export a drive's library as rekordbox XML (`DJ_PLAYLISTS`).
//!
//! rekordbox can import this file (Preferences > Advanced > rekordbox xml), which turns a USB
//! drive back into a computer library: tracks with metadata, beat grids, hot cues, memory cues,
//! loops and playlists. Useful to recover a library from a drive, or to move a library to a new
//! computer without the original.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use schemars::JsonSchema;
use serde::Serialize;

pub(crate) use loclib_interop::encode::xml_escape as esc;

use crate::anlz;
use crate::device::Device;
use crate::model::{Library, Track, TrackColor};
use crate::paths;
use crate::progress::{Ctx, Event, Phase};
use crate::{Error, Result};

#[derive(Debug, Clone, Default)]
pub struct XmlOptions {
    /// Where the audio files will be when rekordbox imports the XML. Defaults to the drive's
    /// current mount point. Point it at a folder you copied `Contents/` into to import from there.
    pub location_root: Option<PathBuf>,
    /// Omit beat grids and cues (metadata and playlists only).
    pub skip_analysis: bool,
    /// Only these playlists and folders (paths like `Folder/Name`, or unique names) and their
    /// tracks. Everything when empty.
    pub only: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct XmlReport {
    pub tracks: usize,
    pub playlists: usize,
    pub tracks_with_beatgrid: usize,
    pub cues: usize,
}

const PATH_ENCODE: &AsciiSet =
    &NON_ALPHANUMERIC.remove(b'/').remove(b'-').remove(b'_').remove(b'.').remove(b'~').remove(b':');

fn location(root: &Path, file_path: &str) -> String {
    let abs = paths::to_fs(root, &paths::key(file_path));
    let s = abs.to_string_lossy().replace('\\', "/");
    let s = if s.starts_with('/') { s } else { format!("/{s}") };
    format!("file://localhost{}", utf8_percent_encode(&s, PATH_ENCODE))
}

fn kind(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "mp3" => "MP3 File",
        "m4a" | "mp4" | "aac" => "M4A File",
        "wav" => "WAV File",
        "aif" | "aiff" => "AIFF File",
        "flac" => "FLAC File",
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

/// Render the drive's library as rekordbox XML.
pub fn export(device: &Device, opts: &XmlOptions, ctx: &Ctx<'_>) -> Result<(String, XmlReport)> {
    let library = subset(crate::convert::combined_library(device)?, &opts.only)?;
    let root = opts.location_root.clone().unwrap_or_else(|| device.root().to_path_buf());
    let mut report = XmlReport { tracks: library.tracks.len(), ..Default::default() };
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<DJ_PLAYLISTS Version=\"1.0.0\">\n");
    let _ = writeln!(
        x,
        "  <PRODUCT Name=\"locallibrary\" Version=\"{}\" Company=\"locallibrary\"/>",
        env!("CARGO_PKG_VERSION")
    );
    let _ = writeln!(x, "  <COLLECTION Entries=\"{}\">", library.tracks.len());
    ctx.phase_start(Phase::Scan, library.tracks.len() as u64, 0);
    for t in &library.tracks {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Scan, path: t.file_path.clone(), bytes: 0 });
        write_track(
            &mut x,
            t,
            &root,
            if opts.skip_analysis { None } else { Some(anlz::load_grid(device, t)) },
            &mut report,
        );
    }
    ctx.phase_end(Phase::Scan);
    x.push_str("  </COLLECTION>\n  <PLAYLISTS>\n");
    write_playlists(&mut x, &library, &mut report);
    x.push_str("  </PLAYLISTS>\n</DJ_PLAYLISTS>\n");
    Ok((x, report))
}

/// The playlists and folders in `only` (with the folders above them, so their paths stay the same)
/// and the tracks they hold. The whole library when `only` is empty.
pub(crate) fn subset(mut library: Library, only: &[String]) -> Result<Library> {
    if only.is_empty() {
        return Ok(library);
    }
    let mut keep = rustc_hash::FxHashSet::default();
    for query in only {
        let p = library.find_playlist(query).ok_or_else(|| Error::NotFound(format!("playlist {query:?}")))?;
        // The node and everything below it...
        let mut queue = vec![p.id];
        while let Some(id) = queue.pop() {
            if keep.insert(id) {
                queue.extend(library.children(Some(id)).iter().map(|c| c.id));
            }
        }
        // ...and the folders above it.
        let mut parent = p.parent_id;
        while let Some(id) = parent.filter(|id| keep.insert(*id)) {
            parent = library.playlist(id).and_then(|f| f.parent_id);
        }
    }
    library.playlists.retain(|p| keep.contains(&p.id));
    let used: rustc_hash::FxHashSet<u32> = library.playlists.iter().flat_map(|p| p.track_ids.iter().copied()).collect();
    library.tracks.retain(|t| used.contains(&t.id));
    Ok(library)
}

fn write_track(x: &mut String, t: &Track, root: &Path, analysis: Option<anlz::TrackAnalysis>, report: &mut XmlReport) {
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
        esc(&location(root, &t.file_path)),
        esc(&t.remixer),
        esc(&t.key),
        esc(&t.label),
        esc(&t.mix_name),
    );
    if let Some(c) = t.color {
        let _ = write!(x, " Colour=\"{}\"", colour(c));
    }
    let Some(a) = analysis.filter(|a| !a.beats.is_empty() || !a.cues.is_empty()) else {
        x.push_str("/>\n");
        return;
    };
    x.push_str(">\n");
    if !a.beats.is_empty() {
        report.tracks_with_beatgrid += 1;
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
        report.cues += 1;
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

fn write_playlists(x: &mut String, lib: &Library, report: &mut XmlReport) {
    let top = lib.children(None);
    let _ = writeln!(x, "    <NODE Type=\"0\" Name=\"ROOT\" Count=\"{}\">", top.len());
    fn node(x: &mut String, lib: &Library, p: &crate::model::Playlist, depth: usize, report: &mut XmlReport) {
        let indent = "  ".repeat(depth + 3);
        if p.is_folder {
            let kids = lib.children(Some(p.id));
            let _ = writeln!(x, "{indent}<NODE Type=\"0\" Name=\"{}\" Count=\"{}\">", esc(&p.name), kids.len());
            for k in kids {
                node(x, lib, k, depth + 1, report);
            }
            let _ = writeln!(x, "{indent}</NODE>");
        } else {
            report.playlists += 1;
            let _ = writeln!(
                x,
                "{indent}<NODE Name=\"{}\" Type=\"1\" KeyType=\"0\" Entries=\"{}\">",
                esc(&p.name),
                p.track_ids.len()
            );
            for id in &p.track_ids {
                let _ = writeln!(x, "{indent}  <TRACK Key=\"{id}\"/>");
            }
            let _ = writeln!(x, "{indent}</NODE>");
        }
    }
    for p in top {
        node(x, lib, p, 0, report);
    }
    x.push_str("    </NODE>\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_are_file_urls() {
        let root = Path::new("/Volumes/USB");
        assert_eq!(
            location(root, "/Contents/Daft Punk/Ärm & Co/01 x.mp3"),
            "file://localhost/Volumes/USB/Contents/Daft%20Punk/%C3%84rm%20%26%20Co/01%20x.mp3"
        );
    }

    /// Each element's attributes, by element name, in order, as quick-xml reads them.
    fn elements(xml: &str) -> Vec<(String, rustc_hash::FxHashMap<String, String>)> {
        let mut reader = quick_xml::Reader::from_str(xml);
        let mut out = Vec::new();
        loop {
            match reader.read_event().unwrap() {
                quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                    let attrs = e
                        .attributes()
                        .map(|a| a.unwrap())
                        .map(|a| {
                            (String::from_utf8(a.key.as_ref().to_vec()).unwrap(), a.unescape_value().unwrap().into())
                        })
                        .collect();
                    out.push((String::from_utf8(e.name().as_ref().to_vec()).unwrap(), attrs));
                }
                quick_xml::events::Event::Eof => return out,
                _ => {}
            }
        }
    }

    /// A track's colour, kind and year, a TEMPO for each tempo change, and its cues: loops with
    /// their ends, hot cues with their numbers and colours, memory cues without either.
    #[test]
    fn tracks_carry_their_colour_kind_tempos_and_cues() {
        let mut t = loclib_model::test_util::track(7, "A & B", "Title", "/Contents/a.flac");
        (t.color, t.year, t.release_date, t.rating) = (Some(TrackColor::Aqua), 0, "2019-05-01".into(), 9);
        let beat = |time_ms, bpm, beat_number| anlz::Beat { beat_number, bpm, time_ms };
        let cue = |hot_cue, start_ms, end_ms, color| anlz::Cue {
            hot_cue,
            start_ms,
            end_ms,
            comment: "Drop".into(),
            color,
            color_id: 0,
            loop_beats: None,
        };
        let analysis = anlz::TrackAnalysis {
            beats: vec![beat(0, 120.0, 1), beat(500, 120.0, 2), beat(1000, 122.0, 7)],
            cues: vec![cue(Some(0), 1500, Some(3500), Some([255, 0, 0])), cue(None, 250, None, Some([0, 255, 0]))],
            ..Default::default()
        };
        let (mut x, mut report) = (String::new(), XmlReport::default());
        write_track(&mut x, &t, Path::new("/Volumes/USB"), Some(analysis), &mut report);
        let e = elements(&x);
        let names: Vec<&str> = e.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["TRACK", "TEMPO", "TEMPO", "POSITION_MARK", "POSITION_MARK"]);
        let track = &e[0].1;
        let said = |k: &str| track[k].as_str();
        assert_eq!(
            [said("Kind"), said("Colour"), said("Year"), said("Rating"), said("Artist")],
            ["FLAC File", "0x25FDE9", "2019", "255", "A & B"]
        );
        assert_eq!(
            (e[2].1["Bpm"].as_str(), e[2].1["Inizio"].as_str(), e[2].1["Battito"].as_str()),
            ("122.00", "1.000", "4")
        );
        let (hot, memory) = (&e[3].1, &e[4].1);
        let hot: Vec<&str> = ["Type", "Start", "End", "Num", "Red", "Green"].iter().map(|k| hot[*k].as_str()).collect();
        assert_eq!(hot, ["4", "1.500", "3.500", "0", "255", "0"]);
        assert_eq!((memory["Type"].as_str(), memory["Num"].as_str(), memory.get("Red")), ("0", "-1", None));
        assert_eq!((report.tracks_with_beatgrid, report.cues), (1, 2));
    }

    #[test]
    fn kinds_and_colours_are_rekordboxs_own() {
        let kinds: Vec<&str> = ["a.MP3", "a.m4a", "a.aac", "a.wav", "a.aif", "a.aiff", "a.flac"].map(kind).into();
        assert_eq!(kinds, ["MP3 File", "M4A File", "M4A File", "WAV File", "AIFF File", "AIFF File", "FLAC File"]);
        use TrackColor::*;
        let colours = [Pink, Red, Orange, Yellow, Green, Aqua, Blue, Purple].map(colour);
        assert_eq!(
            colours,
            ["0xFF007F", "0xFF0000", "0xFFA500", "0xFFFF00", "0x00FF00", "0x25FDE9", "0x0000FF", "0x660099"]
        );
    }

    /// Exporting only some playlists keeps them, everything in a chosen folder, the folders
    /// above them, and their tracks.
    #[test]
    fn a_subset_keeps_its_folders_and_tracks() {
        use crate::model::Playlist;
        let node = |id, parent_id, name: &str, track_ids: Vec<u32>| Playlist {
            id,
            parent_id,
            name: name.into(),
            is_folder: track_ids.is_empty(),
            sort_order: id,
            track_ids,
        };
        let library = Library {
            tracks: (1..=4)
                .map(|id| loclib_model::test_util::track(id, "A", "T", &format!("/Contents/{id}.mp3")))
                .collect(),
            playlists: vec![
                node(1, None, "Shows", vec![]),
                node(2, Some(1), "Friday", vec![1]),
                node(3, Some(1), "Saturday", vec![2]),
                node(4, None, "All", vec![1, 2, 3]),
            ],
            ..Default::default()
        };
        let kept = |only: &[&str]| {
            let s = subset(library.clone(), &only.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap();
            let playlists: Vec<u32> = s.playlists.iter().map(|p| p.id).collect();
            (playlists, s.tracks.iter().map(|t| t.id).collect::<Vec<u32>>())
        };
        assert_eq!(kept(&["Shows"]), (vec![1, 2, 3], vec![1, 2]));
        assert_eq!(kept(&["Friday"]), (vec![1, 2], vec![1]));
        assert_eq!(kept(&[]), (vec![1, 2, 3, 4], vec![1, 2, 3, 4]));
        assert!(matches!(subset(library, &["Sunday".into()]), Err(Error::NotFound(_))));
    }
}
