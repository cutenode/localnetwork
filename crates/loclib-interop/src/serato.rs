//! Serato DJ: crates, and (only when asked) cue points in tags of copies of the audio.
//!
//! **Crates** are files in a `_Serato_/Subcrates` folder at the root of the volume holding the
//! music (for the startup disk, `~/Music/_Serato_`), one per crate, the name only in the file
//! name; a subcrate's file name is its crates' names joined by `%%` (`Sets%%Friday.crate`). A
//! crate is tagged records (4 ASCII bytes, a big-endian u32 length, the data), as the mixxx wiki
//! and serato-tools document them: `vrsn` (UTF-16BE `1.0/Serato ScratchLive Crate`), `osrt`
//! (the sort column, `tvcn`, and `brev`, reversed), `ovct` per column (`tvcn` name, `tvcw`
//! width) and an `otrk` per track holding `ptrk`, its path in UTF-16BE relative to the volume's
//! root, `/`-separated, without a leading slash (`Contents/Artist/x.mp3`).
//!
//! **Cue points** live in the audio files' tags: an ID3v2 `GEOB` frame described
//! `Serato Markers2` (in MP3, AIFF and WAV files), documented by serato-tags: `01 01`, then
//! base64 (a line feed every 72 characters, `=` padding written as `A`) of `01 01` and entries
//! (a NUL-terminated name, a big-endian u32 length, the data): `COLOR` (the track colour), `CUE`
//! (pad, position in ms, colour, NUL-terminated name), `LOOP` (slot, start and end in ms, colour,
//! locked, name) and `BPMLOCK`, ending in a NUL byte; the frame is padded with NULs to 470 bytes.
//! Rewriting tags moves the audio data in a file, so tags are only written with [`Copies`], into
//! copies of the audio below the output folder, never into the files they're copied from; the
//! crates then list the copies. FLAC and MP4 files keep the same data in
//! a Vorbis comment or an MP4 atom, which isn't written: their copies get no cues.

use std::path::{Path, PathBuf};

use base64::prelude::*;
use loclib_model::analysis::Cue;
use loclib_model::{Library, Playlist, Track, TrackColor, paths};
use loclib_progress::{Ctx, Event, Phase};

use crate::{Content, Error, Export, File, Result, Source};

const VERSION: &str = "1.0/Serato ScratchLive Crate";

/// The columns a new crate shows, as Serato's own crates list them.
const COLUMNS: &[&str] = &["song", "artist", "bpm", "key", "album", "length", "comment"];

/// The 18 hot cue colours Serato offers, as it stores them (serato-tags' `docs/colors.md`): hues
/// 20° apart, starting at red. Serato DJ Pro and Lite show them a little differently.
const CUE_COLOURS: [[u8; 3]; 18] = [
    [0xcc, 0x00, 0x00],
    [0xcc, 0x44, 0x00],
    [0xcc, 0x88, 0x00],
    [0xcc, 0xcc, 0x00],
    [0x88, 0xcc, 0x00],
    [0x44, 0xcc, 0x00],
    [0x00, 0xcc, 0x00],
    [0x00, 0xcc, 0x44],
    [0x00, 0xcc, 0x88],
    [0x00, 0xcc, 0xcc],
    [0x00, 0x88, 0xcc],
    [0x00, 0x44, 0xcc],
    [0x00, 0x00, 0xcc],
    [0x44, 0x00, 0xcc],
    [0x88, 0x00, 0xcc],
    [0xcc, 0x00, 0xcc],
    [0xcc, 0x00, 0x88],
    [0xcc, 0x00, 0x44],
];

/// The colour Serato gives pads 1-8 by default.
const PAD_COLOURS: [[u8; 3]; 8] = [
    [0xcc, 0x00, 0x00],
    [0xcc, 0x88, 0x00],
    [0x00, 0x00, 0xcc],
    [0xcc, 0xcc, 0x00],
    [0x00, 0xcc, 0x00],
    [0xcc, 0x00, 0xcc],
    [0x00, 0xcc, 0xcc],
    [0x88, 0x00, 0xcc],
];

/// The colour saved loops get (ARGB).
const LOOP_COLOUR: [u8; 4] = [0x00, 0x27, 0xaa, 0xe1];

/// A track colour as Serato stores it (its picker's pastel; the library column shows it 0x666666
/// darker). White is no colour.
fn track_colour(c: Option<TrackColor>) -> [u8; 3] {
    match c {
        None => [0xff, 0xff, 0xff],
        Some(TrackColor::Pink) => [0xff, 0x99, 0xdd],
        Some(TrackColor::Red) => [0xff, 0x99, 0x99],
        Some(TrackColor::Orange) => [0xff, 0xbb, 0x99],
        Some(TrackColor::Yellow) => [0xff, 0xff, 0x99],
        Some(TrackColor::Green) => [0x99, 0xff, 0x99],
        Some(TrackColor::Aqua) => [0x99, 0xff, 0xff],
        Some(TrackColor::Blue) => [0x99, 0x99, 0xff],
        Some(TrackColor::Purple) => [0xdd, 0x99, 0xff],
    }
}

/// The Serato cue colour nearest `rgb` in hue; `None` for a grey, which has none.
fn cue_colour(rgb: [u8; 3]) -> Option<[u8; 3]> {
    let [r, g, b] = rgb.map(f64::from);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    if max - min < 16.0 {
        return None;
    }
    let d = max - min;
    let sector = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    let hue = sector * 60.0;
    Some(CUE_COLOURS[(hue / 20.0).round() as usize % 18])
}

/// A record: tag, big-endian length, data.
fn record(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
}

fn utf16be(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

/// A `.crate` file listing `tracks` (paths relative to their volume's root) in order.
pub fn crate_file(tracks: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    record(&mut out, b"vrsn", &utf16be(VERSION));
    // Sorted by "#": the crate's own order.
    let mut sort = Vec::new();
    record(&mut sort, b"tvcn", &utf16be("#"));
    record(&mut sort, b"brev", &[0]);
    record(&mut out, b"osrt", &sort);
    for c in COLUMNS {
        let mut col = Vec::new();
        record(&mut col, b"tvcn", &utf16be(c));
        record(&mut col, b"tvcw", &utf16be("0"));
        record(&mut out, b"ovct", &col);
    }
    for t in tracks {
        let mut otrk = Vec::new();
        record(&mut otrk, b"ptrk", &utf16be(t));
        record(&mut out, b"otrk", &otrk);
    }
    out
}

/// A path as a crate names it: relative to the root of its volume (`/Volumes/<name>`, a Windows
/// drive letter, or `/`), `/`-separated. Also returns the volume's root.
fn volume_relative(path: &Path) -> (String, PathBuf) {
    let s = path.to_string_lossy().replace('\\', "/");
    if let [letter, b':', ..] = s.as_bytes()
        && letter.is_ascii_alphabetic()
    {
        return (s[2..].trim_start_matches('/').to_string(), PathBuf::from(format!("{}\\", &s[..2])));
    }
    if let Some((volume, rest)) = s.strip_prefix("/Volumes/").and_then(|r| r.split_once('/')) {
        return (rest.to_string(), PathBuf::from("/Volumes").join(volume));
    }
    (s.trim_start_matches('/').to_string(), PathBuf::from("/"))
}

/// Bytes of a crate file's name kept before ` (2)` and `.crate`: a file name takes 255, and a
/// subcrate's holds the names of every folder above it.
const NAME_BYTES: usize = 200;

/// Crate file names for every playlist and folder: names joined by `%%`, each made safe for a
/// file name (and without `%%` of its own), cut to [`NAME_BYTES`]; same-named crates get ` (2)`,
/// ` (3)`...
fn crate_names(lib: &Library) -> Vec<(&Playlist, String)> {
    fn walk<'a>(
        lib: &'a Library,
        parent: Option<u32>,
        prefix: &str,
        used: &mut rustc_hash::FxHashSet<String>,
        out: &mut Vec<(&'a Playlist, String)>,
    ) {
        for p in lib.children(parent) {
            let mut part = paths::sanitize_component(&p.name);
            while part.contains("%%") {
                part = part.replace("%%", "%");
            }
            let mut base = if prefix.is_empty() { part } else { format!("{prefix}%%{part}") };
            if base.len() > NAME_BYTES {
                base.truncate((0..=NAME_BYTES).rev().find(|&i| base.is_char_boundary(i)).unwrap_or(0));
            }
            let mut name = base.clone();
            let mut n = 2;
            while !used.insert(name.to_lowercase()) {
                name = format!("{base} ({n})");
                n += 1;
            }
            out.push((p, name.clone()));
            if p.is_folder {
                walk(lib, Some(p.id), &name, used, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(lib, None, "", &mut Default::default(), &mut out);
    out
}

/// What a track's `Serato Markers2` holds, and the cues it had no room for.
#[derive(Debug, Default, PartialEq)]
pub struct Markers {
    /// (pad 0-7, position ms, stored colour, name)
    pub cues: Vec<(u8, u32, [u8; 3], String)>,
    /// (slot 0-7, start ms, end ms, name)
    pub loops: Vec<(u8, u32, u32, String)>,
    pub colour: [u8; 3],
    /// Memory cues (Serato has none) and cues past the eighth pad or loop slot.
    pub left_out: usize,
}

impl Markers {
    /// rekordbox's cues as Serato's: hot cues on their pads, hot cue loops in the same saved
    /// loop slots, memory loops in the slots left; memory cues have no equivalent.
    pub fn from_rekordbox(colour: Option<TrackColor>, cues: &[Cue]) -> Markers {
        let mut m = Markers { colour: track_colour(colour), ..Default::default() };
        let mut loop_slots = [false; 8];
        for c in cues {
            match (c.hot_cue, c.end_ms) {
                (Some(h), Some(end)) if h < 8 => {
                    m.loops.push((h, c.start_ms, end, c.comment.clone()));
                    loop_slots[h as usize] = true;
                }
                (Some(h), None) if h < 8 => {
                    let rgb = c.color.and_then(cue_colour).unwrap_or(PAD_COLOURS[h as usize]);
                    m.cues.push((h, c.start_ms, rgb, c.comment.clone()));
                }
                _ => {}
            }
        }
        for c in cues {
            match (c.hot_cue, c.end_ms) {
                (None, Some(end)) => match loop_slots.iter().position(|used| !used) {
                    Some(slot) => {
                        loop_slots[slot] = true;
                        m.loops.push((slot as u8, c.start_ms, end, c.comment.clone()));
                    }
                    None => m.left_out += 1,
                },
                (Some(h), _) if h >= 8 => m.left_out += 1,
                (None, None) => m.left_out += 1,
                _ => {}
            }
        }
        m.cues.sort_by_key(|c| c.0);
        m.loops.sort_by_key(|l| l.0);
        m
    }

    /// The `Serato Markers2` frame's data.
    pub fn encode(&self) -> Vec<u8> {
        fn entry(out: &mut Vec<u8>, name: &str, data: &[u8]) {
            out.extend_from_slice(name.as_bytes());
            out.push(0);
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(data);
        }
        let mut p = vec![1, 1];
        entry(&mut p, "COLOR", &[0, self.colour[0], self.colour[1], self.colour[2]]);
        for (pad, at, rgb, name) in &self.cues {
            let mut d = vec![0, *pad];
            d.extend_from_slice(&at.to_be_bytes());
            d.push(0);
            d.extend_from_slice(rgb);
            d.extend_from_slice(&[0, 0]);
            d.extend_from_slice(name.replace('\0', "").as_bytes());
            d.push(0);
            entry(&mut p, "CUE", &d);
        }
        for (slot, start, end, name) in &self.loops {
            let mut d = vec![0, *slot];
            d.extend_from_slice(&start.to_be_bytes());
            d.extend_from_slice(&end.to_be_bytes());
            d.extend_from_slice(&[0xff; 4]);
            d.extend_from_slice(&LOOP_COLOUR);
            d.extend_from_slice(&[0, 0]);
            d.extend_from_slice(name.replace('\0', "").as_bytes());
            d.push(0);
            entry(&mut p, "LOOP", &d);
        }
        entry(&mut p, "BPMLOCK", &[0]);
        p.push(0);
        let text = BASE64_STANDARD.encode(&p).replace('=', "A");
        let mut out = vec![1, 1];
        for (i, chunk) in text.as_bytes().chunks(72).enumerate() {
            if i > 0 {
                out.push(b'\n');
            }
            out.extend_from_slice(chunk);
        }
        if out.len() < 470 {
            out.resize(470, 0);
        }
        out
    }
}

/// Copies of the audio get tags when they're in these formats (ID3v2 containers).
fn taggable(path: &str) -> bool {
    matches!(
        path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref(),
        Some("mp3" | "aif" | "aiff" | "wav")
    )
}

/// Copies of the audio for the crates to list, with Serato's cue tag in those that can hold one.
pub struct Copies<'a, E> {
    /// The folder the copies' `Contents/` goes in.
    pub root: PathBuf,
    /// A track's audio file, to copy. A track whose file isn't there gets no copy.
    pub audio: &'a dyn Fn(&Track) -> Result<PathBuf, E>,
}

/// A `.crate` file in `output`'s `_Serato_/Subcrates` for every playlist and folder in `src`, and
/// with `copies`, a copy of every track's audio (tagged with its cues when `src` has analysis).
/// `copies.audio`'s errors come back as they are.
pub fn build<E: From<Error>>(
    src: &Source<'_>,
    output: &Path,
    copies: Option<&Copies<'_, E>>,
    ctx: &Ctx<'_>,
) -> Result<Export, E> {
    let lib = src.library;
    let mut export = Export::new(lib);
    // With copies, the crates list them instead of the audio they're copied from.
    let root = copies.map_or(&src.root, |c| &c.root);
    let located = |t: &Track| paths::to_fs(root, &paths::key(&t.file_path));

    let names = crate_names(lib);
    let mut volume = None;
    for (p, name) in &names {
        let tracks: Vec<String> = lib
            .tracks_by_id(&p.track_ids)
            .into_iter()
            .map(|t| {
                let (rel, vol) = volume_relative(&located(t));
                volume.get_or_insert(vol);
                rel
            })
            .collect();
        let what = if p.is_folder {
            format!("crate for the folder {}", lib.playlist_path(p.id))
        } else {
            format!("crate {} ({} tracks)", lib.playlist_path(p.id), tracks.len())
        };
        export.file(output.join("_Serato_/Subcrates").join(format!("{name}.crate")), what, crate_file(&tracks));
    }
    export.tracks = lib.tracks.len();
    let volume = volume.unwrap_or_else(|| volume_relative(&root.join("Contents")).1);
    let serato_home = if volume == Path::new("/") {
        "~/Music/_Serato_/Subcrates (Serato's folder for the startup disk)".to_string()
    } else {
        format!("{} on {}", Path::new("_Serato_").join("Subcrates").display(), volume.display())
    };
    export.notes.push(format!(
        "Copy the .crate files from {} into {serato_home}, next to any crates already there, then restart Serato: \
         crates belong to the volume that holds their music. Folders become crates holding their crates.",
        output.join("_Serato_/Subcrates").display()
    ));
    export.notes.push(
        "Crates only list tracks: Serato reads each file's own tags and analyses it again. Beat grids aren't carried."
            .into(),
    );

    let Some(copies) = copies else {
        export.notes.push(
            "Cue points live in the audio files' tags, which this export leaves alone; `Copies` writes them into copies \
             of the audio (in the output folder) for the crates to list instead."
                .into(),
        );
        return Ok(export);
    };

    // Copies of the audio with Serato's cue tag.
    let (mut tagged, mut untaggable, mut left_out) = (0, 0, 0);
    ctx.phase_start(Phase::Scan, lib.tracks.len() as u64, 0);
    let mut seen = rustc_hash::FxHashSet::default();
    for t in &lib.tracks {
        ctx.check().map_err(Error::from)?;
        ctx.emit(Event::Item { phase: Phase::Scan, path: t.file_path.clone(), bytes: 0 });
        let key = paths::key(&t.file_path);
        if !seen.insert(key.to_lowercase()) {
            continue;
        }
        let from = (copies.audio)(t)?;
        let Ok(meta) = std::fs::metadata(&from) else { continue };
        let to = located(t);
        let markers = match (taggable(&key), src.analysis) {
            (true, Some(load)) => {
                let m = Markers::from_rekordbox(t.color, &load(t).cues);
                left_out += m.left_out;
                export.cues += m.cues.len() + m.loops.len();
                tagged += 1;
                Some(m.encode())
            }
            (false, Some(_)) => {
                untaggable += 1;
                None
            }
            _ => None,
        };
        let what = match markers {
            Some(_) => "copy of the audio, with Serato's cue tag",
            None => "copy of the audio",
        };
        export.copy_bytes += meta.len();
        let content = Content::TaggedCopy { from, markers };
        export.files.push(File { path: to, what: what.into(), bytes: meta.len(), content });
    }
    ctx.phase_end(Phase::Scan);
    export.notes.push(format!(
        "The audio is copied into {} (the drive's files are never changed) and the crates list the copies; {tagged} \
         of them get rekordbox's hot cues and loops in their Serato Markers2 tag.",
        output.join("Contents").display()
    ));
    if untaggable > 0 {
        export.warnings.push(format!(
            "{untaggable} track(s) aren't MP3, AIFF or WAV; their copies get no cues (FLAC and MP4 tags aren't written)"
        ));
    }
    if left_out > 0 {
        export.warnings.push(format!(
            "{left_out} cue(s) left out: memory cues (Serato has none) and cues past the eighth pad or loop slot"
        ));
    }
    Ok(export)
}

/// Write `markers` as the `Serato Markers2` GEOB frame of the ID3v2 tag of the file at `path` (an
/// MP3, AIFF or WAV copy, the kind `named`'s extension says), keeping its other frames and
/// removing an older `Serato Markers_` frame, which Serato would read instead.
pub fn write_markers(path: &Path, named: &Path, markers: &[u8]) -> Result<()> {
    use lofty::TextEncoding;
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::AudioFile;
    use lofty::id3::v2::{BinaryFrame, Frame, FrameFlags, FrameId, GeneralEncapsulatedObject, Id3v2Tag, Id3v2Version};
    use lofty::tag::TagExt;

    let err = |e: Box<dyn std::error::Error>| Error::Tags(format!("{}: {e}", path.display()));
    let ext = named.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let options = ParseOptions::new().read_properties(false);
    let existing = {
        let mut f = std::fs::File::open(path).map_err(|e| Error::Io { path: path.to_path_buf(), source: e })?;
        match ext.as_str() {
            "mp3" => lofty::mpeg::MpegFile::read_from(&mut f, options).map_err(|e| err(e.into()))?.id3v2().cloned(),
            "aif" | "aiff" => {
                lofty::iff::aiff::AiffFile::read_from(&mut f, options).map_err(|e| err(e.into()))?.id3v2().cloned()
            }
            "wav" => lofty::iff::wav::WavFile::read_from(&mut f, options).map_err(|e| err(e.into()))?.id3v2().cloned(),
            _ => return Err(Error::Tags(format!("{}: not an MP3, AIFF or WAV file", path.display()))),
        }
    };
    let v23 = existing.as_ref().is_some_and(|t| t.original_version() == Id3v2Version::V3);
    let mut tag = existing.unwrap_or_else(Id3v2Tag::new);
    let geob = FrameId::Valid("GEOB".into());
    let serato = |f: &Frame<'_>| match f {
        Frame::Binary(b) if b.id() == &geob => GeneralEncapsulatedObject::parse(&b.data, FrameFlags::default())
            .ok()
            .and_then(|g| g.descriptor)
            .is_some_and(|d| d == "Serato Markers2" || d == "Serato Markers_"),
        _ => false,
    };
    tag.retain(|f| !serato(f));
    let object = GeneralEncapsulatedObject::new(
        TextEncoding::Latin1,
        Some("application/octet-stream".into()),
        Some(String::new()),
        Some("Serato Markers2".into()),
        markers.to_vec(),
    );
    let bytes = object.as_bytes(WriteOptions::new()).map_err(|e| err(e.into()))?;
    tag.insert(Frame::Binary(BinaryFrame::new(geob.clone(), bytes)));
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| Error::Io { path: path.to_path_buf(), source: e })?;
    tag.save_to(&mut f, WriteOptions::new().use_id3v23(v23)).map_err(|e| err(e.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Serato Markers2` of serato-tags' `data/id3/hotcues-with-names` test file (MIT), written by
    /// Serato DJ: white, three named hot cues, beat grid unlocked.
    const HOTCUES_WITH_NAMES: &str = "AQFDT0xPUgAAAAAEAP///0NVRQAAAAAaAAAAAAAAAMwAAAAASGVsbG8sIFdvcmxkIQBDVUUA\n\
        AAAAFQABAANVWADMiAAAAMOkw7bDvMOfAENVRQAAAAATAAIAAOpkAAAAzAAAw6nDqMOqAEJQ\nTUxPQ0sAAAAAAQAA";

    #[test]
    fn markers_encode_like_serato() {
        let m = Markers {
            colour: [0xff, 0xff, 0xff],
            cues: vec![
                (0, 0, [0xcc, 0x00, 0x00], "Hello, World!".into()),
                (1, 0x0003_5558, [0xcc, 0x88, 0x00], "äöüß".into()),
                (2, 0xea64, [0x00, 0x00, 0xcc], "éèê".into()),
            ],
            ..Default::default()
        };
        let mut want = vec![1, 1];
        want.extend_from_slice(HOTCUES_WITH_NAMES.as_bytes());
        want.resize(470, 0);
        assert_eq!(m.encode(), want);
    }

    #[test]
    fn rekordbox_cues_map_to_pads_and_loop_slots() {
        let cue = |hot_cue, start_ms, end_ms: Option<u32>, color| Cue {
            hot_cue,
            start_ms,
            end_ms,
            comment: String::new(),
            color,
            color_id: 0,
            loop_beats: None,
        };
        let m = Markers::from_rekordbox(
            Some(TrackColor::Red),
            &[
                cue(None, 100, None, None),
                cue(None, 200, Some(400), None),
                cue(Some(0), 1000, None, Some([0xe6, 0x28, 0x28])),
                cue(Some(4), 2000, Some(2500), None),
                cue(Some(9), 3000, None, None),
                cue(Some(2), 4000, None, None),
            ],
        );
        assert_eq!(m.colour, [0xff, 0x99, 0x99]);
        assert_eq!(m.cues, [(0, 1000, [0xcc, 0x00, 0x00], String::new()), (2, 4000, PAD_COLOURS[2], String::new())]);
        assert_eq!(m.loops, [(0, 200, 400, String::new()), (4, 2000, 2500, String::new())]);
        assert_eq!(m.left_out, 2, "the memory cue and pad 10");
    }

    /// Serato has eight saved loop slots: memory loops past them are left out, and each track
    /// colour is the pastel Serato's picker stores.
    #[test]
    fn loops_past_the_eighth_slot_are_left_out() {
        let memory_loop = |i: u32| Cue {
            hot_cue: None,
            start_ms: i * 1000,
            end_ms: Some(i * 1000 + 500),
            comment: String::new(),
            color: None,
            color_id: 0,
            loop_beats: None,
        };
        let m = Markers::from_rekordbox(None, &(0..9).map(memory_loop).collect::<Vec<_>>());
        assert_eq!(m.loops.iter().map(|l| l.0).collect::<Vec<_>>(), (0..8).collect::<Vec<u8>>());
        assert_eq!((m.left_out, m.colour), (1, [0xff, 0xff, 0xff]));
        use TrackColor::*;
        let pastels = [Pink, Red, Orange, Yellow, Green, Aqua, Blue, Purple].map(|c| track_colour(Some(c)));
        assert!(pastels.iter().all(|[r, g, b]| r.min(g).min(b) == &0x99 && r.max(g).max(b) == &0xff), "{pastels:x?}");
        assert_eq!(pastels.iter().collect::<rustc_hash::FxHashSet<_>>().len(), 8);
    }

    /// Crate names are made safe for a file name, `%%` (Serato's folder separator) can't come
    /// from a playlist's own name, long ones are cut on a character boundary, and same-named
    /// crates are told apart.
    #[test]
    fn crate_names_are_safe_unique_and_short_enough() {
        let node = |id, parent_id, name: &str, is_folder| Playlist {
            id,
            parent_id,
            name: name.into(),
            is_folder,
            sort_order: id,
            track_ids: Vec::new(),
        };
        let long = "ü".repeat(150);
        let lib = Library {
            playlists: vec![
                node(1, None, "50%% off", true),
                node(2, Some(1), "Set", false),
                node(3, None, "set", false),
                node(4, None, "Set", false),
                node(5, None, &long, false),
            ],
            ..Default::default()
        };
        let names: Vec<(u32, String)> = crate_names(&lib).into_iter().map(|(p, n)| (p.id, n)).collect();
        assert_eq!(
            &names[..4],
            [(1, "50% off".to_string()), (2, "50% off%%Set".into()), (3, "set".into()), (4, "Set (2)".into())]
        );
        assert!(names[4].1.len() <= NAME_BYTES && long.starts_with(&names[4].1), "{}", names[4].1);
    }

    #[test]
    fn crates_and_their_paths() {
        let c = crate_file(&["Contents/A/x.mp3".into()]);
        assert_eq!(&c[..8], b"vrsn\0\0\0\x38");
        assert_eq!(&c[8..10], &[0, b'1']);
        let otrk = c.windows(4).position(|w| w == b"otrk").unwrap();
        assert_eq!(&c[otrk + 4..otrk + 8], &(8 + 32u32).to_be_bytes());
        assert_eq!(&c[otrk + 8..otrk + 12], b"ptrk");
        assert_eq!(&c[otrk + 16..], utf16be("Contents/A/x.mp3").as_slice());

        assert_eq!(
            volume_relative(Path::new("/Volumes/USB/Contents/x.mp3")),
            ("Contents/x.mp3".into(), PathBuf::from("/Volumes/USB"))
        );
        assert_eq!(volume_relative(Path::new(r"E:\Contents\x.mp3")).0, "Contents/x.mp3");
        assert_eq!(volume_relative(Path::new("/Users/me/Music/x.mp3")), ("Users/me/Music/x.mp3".into(), "/".into()));
        assert_eq!(cue_colour([0xe6, 0x28, 0x28]), Some([0xcc, 0x00, 0x00]));
        assert_eq!(cue_colour([0x1d, 0xbe, 0xbd]), Some([0x00, 0xcc, 0xcc]));
        assert_eq!(cue_colour([0xff, 0x37, 0x6f]), Some([0xcc, 0x00, 0x44]), "rekordbox's pink-red");
        assert_eq!(cue_colour([0x30, 0x5a, 0xff]), Some([0x00, 0x44, 0xcc]));
        assert_eq!(cue_colour([0x80, 0x80, 0x84]), None);
        for (i, c) in CUE_COLOURS.iter().enumerate() {
            assert_eq!(cue_colour(*c), Some(*c), "{i}");
        }
    }
}
