//! Traktor Pro's collection file, `collection.nml`, which Traktor imports with "Import another
//! Collection" (right-click Track Collection in the browser tree), merging it into its own.
//!
//! NML is XML (`<NML VERSION="19">`, as Traktor Pro 3 and 4 write it); Native Instruments
//! doesn't document it, so this follows what Traktor writes and what converters that read it
//! agree on:
//!
//! * a track is an `ENTRY` in `COLLECTION`, found by its `LOCATION`: `VOLUME` (a Mac volume's
//!   name, a Windows drive letter like `E:`), `DIR` with every folder as `/:name` and a final
//!   `/:`, and `FILE`. Playlists name tracks by the three run together
//!   (`USB/:Contents/:Artist/:x.mp3`) in a `PRIMARYKEY`.
//! * `INFO` holds the details: `BITRATE` in bits per second, `PLAYTIME` in seconds, `FILESIZE` in
//!   kilobytes, `RANKING` 0-255 (51 a star), dates as `2026/9/30`, `COLOR` 1-7 (red, orange,
//!   yellow, green, blue, violet, magenta). `TEMPO BPM` is the tempo and `MUSICAL_KEY VALUE` the
//!   key as 0-11 for C to B major and 12-23 for C to B minor.
//! * `CUE_V2` elements are markers, `START` and `LEN` in milliseconds: `TYPE` 0 a cue, 4 a grid
//!   marker (with the tempo, the beat grid), 5 a loop; `HOTCUE` the pad (0-7), -1 for a cue
//!   without one. Traktor's grid has one tempo, so a rekordbox grid becomes one marker on its
//!   first downbeat; `LOCK` keeps Traktor from analysing it again.
//! * playlists are `NODE`s under `PLAYLISTS`: `TYPE="FOLDER"` with `SUBNODES`, or
//!   `TYPE="PLAYLIST"` with a `PLAYLIST` of `ENTRY`/`PRIMARYKEY`s.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;

use chrono::{Datelike, Timelike};

use super::{Built, InteropOptions, Software, Source, constant_grid};
use crate::Result;
use crate::anlz::TrackAnalysis;
use crate::harmony::Camelot;
use crate::model::{Library, Playlist, Track, TrackColor};
use crate::progress::{Ctx, Event, Phase};
use crate::xml::esc;

/// The volume Traktor on a Mac calls the startup disk, for audio outside `/Volumes`.
const STARTUP_VOLUME: &str = "Macintosh HD";

/// Where a file is, the way `LOCATION` says it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Location {
    volume: String,
    dir: String,
    file: String,
}

impl Location {
    /// `VOLUME` + `DIR` + `FILE`: what playlists name the track by.
    fn key(&self) -> String {
        format!("{}{}{}", self.volume, self.dir, self.file)
    }
}

/// An absolute path as Traktor locates it, and whether the volume is a guess (the startup disk).
fn location(path: &Path) -> (Location, bool) {
    let s = path.to_string_lossy().replace('\\', "/");
    let (volume, rest, guessed) = match s.as_bytes() {
        [letter, b':', ..] if letter.is_ascii_alphabetic() => (s[..2].to_ascii_uppercase(), s[2..].to_string(), false),
        _ => match s.strip_prefix("/Volumes/").and_then(|r| r.split_once('/')) {
            Some((volume, rest)) => (volume.to_string(), format!("/{rest}"), false),
            None => (STARTUP_VOLUME.to_string(), s.clone(), true),
        },
    };
    let mut parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    let file = parts.pop().unwrap_or_default().to_string();
    let dir = parts.iter().map(|p| format!("/:{p}")).collect::<String>() + "/:";
    (Location { volume, dir, file }, guessed)
}

/// `2026-09-30` (or a longer timestamp) as Traktor writes dates: `2026/9/30`.
fn date(s: &str) -> Option<String> {
    let mut it = s.get(..10)?.split('-').map(|p| p.parse::<u32>().ok());
    let (y, m, d) = (it.next()??, it.next()??, it.next()??);
    (y > 0 && (1..=12).contains(&m) && (1..=31).contains(&d)).then(|| format!("{y}/{m}/{d}"))
}

/// The semitone (0 = C) of a key's tonic.
fn semitone(c: Camelot) -> u8 {
    // Inverse of Camelot::parse: number - 1 = (semitone * 7 + offset) % 12, and 7 * 7 = 1 mod 12.
    let offset = if c.major { 7 } else { 4 };
    ((u32::from(c.number) - 1 + 12 - offset) * 7 % 12) as u8
}

/// `MUSICAL_KEY VALUE`: 0-11 for C to B major, 12-23 for C to B minor.
fn key_value(key: &str) -> Option<u8> {
    let c = Camelot::parse(key)?;
    Some(semitone(c) + if c.major { 0 } else { 12 })
}

fn colour(c: TrackColor) -> u8 {
    match c {
        TrackColor::Red => 1,
        TrackColor::Orange => 2,
        TrackColor::Yellow => 3,
        TrackColor::Green => 4,
        TrackColor::Aqua | TrackColor::Blue => 5,
        TrackColor::Purple => 6,
        TrackColor::Pink => 7,
    }
}

/// The `ENTRY` attributes saying when it was written, and that its grid is locked.
struct Stamps {
    modified: String,
    lock: String,
}

/// Counts for the plan.
#[derive(Default)]
struct Tally {
    grids: usize,
    cues: usize,
    varying_tempo: usize,
    extra_hot_cues: usize,
    startup_volume: bool,
}

pub(crate) fn build(src: &Source<'_>, output: &Path, opts: &InteropOptions, ctx: &Ctx<'_>) -> Result<Built> {
    let lib = &src.library;
    let mut tally = Tally::default();
    let mut keys: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    let now = chrono::Local::now();
    let stamps = Stamps {
        modified: format!(
            "MODIFIED_DATE=\"{}/{}/{}\" MODIFIED_TIME=\"{}\"",
            now.year(),
            now.month(),
            now.day(),
            now.num_seconds_from_midnight()
        ),
        lock: format!(" LOCK=\"1\" LOCK_MODIFICATION_TIME=\"{}\"", now.format("%Y-%m-%dT%H:%M:%S")),
    };

    let mut entries = String::new();
    let mut written = HashSet::new();
    ctx.phase_start(Phase::Scan, lib.tracks.len() as u64, 0);
    for t in &lib.tracks {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Scan, path: t.file_path.clone(), bytes: 0 });
        let (loc, guessed) = location(&src.audio(t));
        tally.startup_volume |= guessed;
        keys.insert(t.id, loc.key());
        // Two rows for one file are one track to Traktor.
        if !written.insert(loc.key()) {
            continue;
        }
        let analysis = (!opts.skip_analysis).then(|| TrackAnalysis::load_grid(src.device, t));
        entry(&mut entries, t, &loc, analysis.as_ref(), &stamps, &mut tally);
    }
    ctx.phase_end(Phase::Scan);

    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<NML VERSION=\"19\">");
    x.push_str("<HEAD COMPANY=\"www.native-instruments.com\" PROGRAM=\"Traktor\"></HEAD>\n");
    x.push_str("<MUSICFOLDERS></MUSICFOLDERS>\n");
    let _ = writeln!(x, "<COLLECTION ENTRIES=\"{}\">", written.len());
    x.push_str(&entries);
    x.push_str("</COLLECTION>\n<SETS ENTRIES=\"0\"></SETS>\n<PLAYLISTS>\n");
    let top = lib.children(None);
    let _ = writeln!(x, "<NODE TYPE=\"FOLDER\" NAME=\"$ROOT\"><SUBNODES COUNT=\"{}\">", top.len());
    for p in top {
        node(&mut x, lib, p, &keys);
    }
    x.push_str("</SUBNODES></NODE>\n</PLAYLISTS>\n</NML>\n");

    let mut built = Built::new(Software::Traktor, output);
    let plan = &mut built.plan;
    plan.tracks = written.len();
    plan.playlists = lib.playlists.iter().filter(|p| !p.is_folder).count();
    plan.folders = lib.playlists.iter().filter(|p| p.is_folder).count();
    plan.beat_grids = tally.grids;
    plan.cues = tally.cues;
    plan.notes = vec![
        "In Traktor, right-click Track Collection in the browser tree, choose Import another Collection and pick \
         collection.nml: its tracks and playlists are added to Traktor's collection."
            .into(),
        "A beat grid becomes one grid marker on the first downbeat with one tempo, locked so Traktor keeps it. \
         Cue colours aren't carried (Traktor colours cues by type)."
            .into(),
        "Traktor and rekordbox can place an MP3's cues and grid a few milliseconds apart (they handle the encoder's \
         delay differently); nothing corrects for it, so check an MP3's grid before relying on it."
            .into(),
        "History lists aren't exported; My Tags, artwork and waveforms aren't part of the file.".into(),
    ];
    if tally.startup_volume {
        plan.notes.push(format!(
            "Audio outside /Volumes is located on the startup disk as \"{STARTUP_VOLUME}\"; if yours is called \
             otherwise, Traktor's Relocate (or Consistency Check) finds the files again."
        ));
    }
    if tally.varying_tempo > 0 {
        plan.warnings.push(format!(
            "{} track(s) change tempo; their grid keeps the tempo of the first downbeat",
            tally.varying_tempo
        ));
    }
    if tally.extra_hot_cues > 0 {
        plan.warnings.push(format!("{} hot cue(s) past H left out (Traktor has 8 pads)", tally.extra_hot_cues));
    }
    if lib.tracks.len() > written.len() {
        plan.warnings.push(format!(
            "{} track row(s) share a file with another and are one track in Traktor",
            lib.tracks.len() - written.len()
        ));
    }
    plan.warnings.extend(src.missing_audio());
    let path = output.join("collection.nml");
    built.file(path, "Traktor collection: tracks, beat grids, cues and playlists", x.into_bytes());
    Ok(built)
}

fn entry(
    x: &mut String,
    t: &Track,
    loc: &Location,
    analysis: Option<&TrackAnalysis>,
    stamps: &Stamps,
    tally: &mut Tally,
) {
    let grid = analysis.and_then(|a| constant_grid(&a.beats, t.bpm));
    let lock = if grid.is_some() { stamps.lock.as_str() } else { "" };
    let _ = writeln!(x, "<ENTRY {}{lock} TITLE=\"{}\" ARTIST=\"{}\">", stamps.modified, esc(&t.title), esc(&t.artist));
    let _ = writeln!(
        x,
        "<LOCATION DIR=\"{}\" FILE=\"{}\" VOLUME=\"{}\" VOLUMEID=\"{}\"></LOCATION>",
        esc(&loc.dir),
        esc(&loc.file),
        esc(&loc.volume),
        esc(&loc.volume)
    );
    if !t.album.is_empty() || t.track_number > 0 {
        let number = if t.track_number > 0 { format!("TRACK=\"{}\" ", t.track_number) } else { String::new() };
        let _ = writeln!(x, "<ALBUM {number}TITLE=\"{}\"></ALBUM>", esc(&t.album));
    }
    x.push_str("<MODIFICATION_INFO AUTHOR_TYPE=\"user\"></MODIFICATION_INFO>\n");
    let mut info = String::new();
    let mut attr = |name: &str, value: String| {
        if !value.is_empty() {
            let _ = write!(info, " {name}=\"{}\"", esc(&value));
        }
    };
    if t.bitrate > 0 {
        attr("BITRATE", (u64::from(t.bitrate) * 1000).to_string());
    }
    attr("GENRE", t.genre.clone());
    attr("LABEL", t.label.clone());
    attr("COMMENT", t.comment.clone());
    attr("MIX", t.mix_name.clone());
    attr("REMIXER", t.remixer.clone());
    attr("KEY", t.key.clone());
    attr("PLAYCOUNT", t.play_count.to_string());
    if t.duration_secs > 0 {
        attr("PLAYTIME", t.duration_secs.to_string());
        attr("PLAYTIME_FLOAT", format!("{:.6}", f64::from(t.duration_secs)));
    }
    attr("RANKING", (u32::from(t.rating.min(5)) * 51).to_string());
    attr("IMPORT_DATE", date(&t.date_added).unwrap_or_default());
    let release = date(&t.release_date).or_else(|| (t.year > 0).then(|| format!("{}/1/1", t.year)));
    attr("RELEASE_DATE", release.unwrap_or_default());
    if t.file_size > 0 {
        attr("FILESIZE", t.file_size.div_ceil(1024).to_string());
    }
    if let Some(c) = t.color {
        attr("COLOR", colour(c).to_string());
    }
    let _ = writeln!(x, "<INFO{info}></INFO>");
    let bpm = grid.map_or(t.bpm, |g| g.0);
    if bpm > 0.0 {
        let _ = writeln!(x, "<TEMPO BPM=\"{bpm:.6}\" BPM_QUALITY=\"100.000000\"></TEMPO>");
    }
    if let Some(v) = key_value(&t.key) {
        let _ = writeln!(x, "<MUSICAL_KEY VALUE=\"{v}\"></MUSICAL_KEY>");
    }
    if let Some((_, start, varies)) = grid {
        tally.grids += 1;
        tally.varying_tempo += usize::from(varies);
        cue(x, "AutoGrid", 4, f64::from(start), 0.0, -1);
    }
    for c in analysis.map(|a| a.cues.as_slice()).unwrap_or_default() {
        let hot = match c.hot_cue {
            Some(h) if h > 7 => {
                tally.extra_hot_cues += 1;
                continue;
            }
            Some(h) => i32::from(h),
            None => -1,
        };
        let name = if c.comment.is_empty() { "n.n." } else { &c.comment };
        let (kind, len) = match c.end_ms {
            Some(end) => (5, f64::from(end.saturating_sub(c.start_ms))),
            None => (0, 0.0),
        };
        tally.cues += 1;
        cue(x, name, kind, f64::from(c.start_ms), len, hot);
    }
    x.push_str("</ENTRY>\n");
}

fn cue(x: &mut String, name: &str, kind: u8, start_ms: f64, len_ms: f64, hot: i32) {
    let _ = writeln!(
        x,
        "<CUE_V2 NAME=\"{}\" DISPL_ORDER=\"0\" TYPE=\"{kind}\" START=\"{start_ms:.6}\" LEN=\"{len_ms:.6}\" \
         REPEATS=\"-1\" HOTCUE=\"{hot}\"></CUE_V2>",
        esc(name)
    );
}

/// A stable id for a playlist, from its path: re-importing the same playlist gives the same one.
fn uuid(path: &str) -> String {
    let hash = blake3::hash(format!("locallibrary traktor playlist\0{path}").as_bytes());
    hash.as_bytes()[..16].iter().map(|b| format!("{b:02x}")).collect()
}

fn node(x: &mut String, lib: &Library, p: &Playlist, keys: &std::collections::HashMap<u32, String>) {
    if p.is_folder {
        let kids = lib.children(Some(p.id));
        let _ = writeln!(x, "<NODE TYPE=\"FOLDER\" NAME=\"{}\"><SUBNODES COUNT=\"{}\">", esc(&p.name), kids.len());
        for k in kids {
            node(x, lib, k, keys);
        }
        x.push_str("</SUBNODES></NODE>\n");
        return;
    }
    let entries: Vec<&String> = p.track_ids.iter().filter_map(|id| keys.get(id)).collect();
    let _ = writeln!(
        x,
        "<NODE TYPE=\"PLAYLIST\" NAME=\"{}\"><PLAYLIST ENTRIES=\"{}\" TYPE=\"LIST\" UUID=\"{}\">",
        esc(&p.name),
        entries.len(),
        uuid(&lib.playlist_path(p.id))
    );
    for k in entries {
        let _ = writeln!(x, "<ENTRY><PRIMARYKEY TYPE=\"TRACK\" KEY=\"{}\"></PRIMARYKEY></ENTRY>", esc(k));
    }
    x.push_str("</PLAYLIST></NODE>\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_split_volume_folders_and_file() {
        let (mac, guessed) = location(Path::new("/Volumes/USB/Contents/Daft Punk/One.mp3"));
        assert!(!guessed);
        assert_eq!(
            mac,
            Location { volume: "USB".into(), dir: "/:Contents/:Daft Punk/:".into(), file: "One.mp3".into() }
        );
        assert_eq!(mac.key(), "USB/:Contents/:Daft Punk/:One.mp3");
        let (win, _) = location(Path::new(r"e:\Contents\x.mp3"));
        assert_eq!((win.volume.as_str(), win.dir.as_str(), win.file.as_str()), ("E:", "/:Contents/:", "x.mp3"));
        let (home, guessed) = location(Path::new("/Users/me/Music/x.mp3"));
        assert!(guessed);
        assert_eq!(home.key(), "Macintosh HD/:Users/:me/:Music/:x.mp3");
        let (root, _) = location(Path::new("/Volumes/USB/x.mp3"));
        assert_eq!(root.dir, "/:");
    }

    #[test]
    fn keys_dates_and_colours() {
        assert_eq!(key_value("C"), Some(0));
        assert_eq!(key_value("8B"), Some(0));
        assert_eq!(key_value("Am"), Some(21));
        assert_eq!(key_value("8A"), Some(21));
        assert_eq!(key_value("F#m"), Some(18));
        assert_eq!(key_value("Bb"), Some(10));
        assert_eq!(key_value("1m"), Some(21), "Open Key 1m is A minor");
        assert_eq!(key_value(""), None);
        for n in 1..=12 {
            for major in [false, true] {
                let c = Camelot { number: n, major };
                assert_eq!(Camelot::parse(&c.note()), Some(c), "{}", c.note());
                assert_eq!(key_value(&c.note()), key_value(&c.code()));
            }
        }
        assert_eq!(date("2026-09-30"), Some("2026/9/30".into()));
        assert_eq!(date("2024-03-01 10:00:00"), Some("2024/3/1".into()));
        assert_eq!(date("2024"), None);
        assert_eq!(colour(TrackColor::Pink), 7);
        assert_eq!(uuid("Shows/Friday"), uuid("Shows/Friday"));
        assert_eq!(uuid("Shows/Friday").len(), 32);
    }
}
