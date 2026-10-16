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
//! a Vorbis comment or an MP4 atom, which isn't written: their copies get no cues. Neither do
//! copies of files whose content isn't the audio their names say; a warning names one.

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
        // Written out rather than joined, so a Mac volume reads the same when exported on Windows.
        return (rest.to_string(), PathBuf::from(format!("/Volumes/{volume}")));
    }
    (s.trim_start_matches('/').to_string(), PathBuf::from("/"))
}

/// Bytes of a crate file's name kept before ` (2)` and `.crate`: a file name takes 255, and a
/// subcrate's holds the names of every folder above it.
const NAME_BYTES: usize = 200;

/// Bytes a folder's name leaves for each level of crates below it (`%%` and the start of a
/// name), so cutting a deep folder's name never leaves its subcrates without one.
const LEVEL_BYTES: usize = 2 + 8;

/// Crate file names for every playlist and folder: names joined by `%%`, each made safe for a
/// file name (and without `%%` of its own); same-named crates get ` (2)`, ` (3)`... A crate's own
/// name is what's cut, on a character boundary, for the whole to fit in [`NAME_BYTES`] with room
/// left for each level below it. Serato nests a crate under the one its name starts with, so the
/// folders' names above it are never cut, and it keeps at least its first character even when
/// they leave no room.
fn crate_names(lib: &Library) -> Vec<(&Playlist, String)> {
    fn walk<'a>(
        lib: &'a Library,
        parent: Option<u32>,
        prefix: &str,
        levels_below: &rustc_hash::FxHashMap<u32, usize>,
        used: &mut rustc_hash::FxHashSet<String>,
        out: &mut Vec<(&'a Playlist, String)>,
    ) {
        for p in lib.children(parent) {
            let mut part = paths::sanitize_component(&p.name);
            while part.contains("%%") {
                part = part.replace("%%", "%");
            }
            let sep = if prefix.is_empty() { "" } else { "%%" };
            let below = levels_below.get(&p.id).copied().unwrap_or(0);
            let room = NAME_BYTES.saturating_sub(prefix.len() + sep.len() + below * LEVEL_BYTES);
            let first = part.chars().next().map_or(0, char::len_utf8);
            let cut = (0..=room.min(part.len())).rev().find(|&i| part.is_char_boundary(i)).unwrap_or(0);
            part.truncate(cut.max(first));
            let base = format!("{prefix}{sep}{part}");
            let mut name = base.clone();
            let mut n = 2;
            while !used.insert(name.to_lowercase()) {
                name = format!("{base} ({n})");
                n += 1;
            }
            out.push((p, name.clone()));
            if p.is_folder {
                walk(lib, Some(p.id), &name, levels_below, used, out);
            }
        }
    }
    // How many levels of crates each folder holds, from each node's walk up its lineage.
    let parents: rustc_hash::FxHashMap<u32, Option<u32>> = lib.playlists.iter().map(|p| (p.id, p.parent_id)).collect();
    let mut levels_below = rustc_hash::FxHashMap::default();
    for p in &lib.playlists {
        let (mut up, mut levels) = (p.parent_id, 1);
        while let Some(id) = up {
            let l = levels_below.entry(id).or_insert(0);
            *l = levels.max(*l);
            up = parents.get(&id).copied().flatten();
            levels += 1;
        }
    }
    let mut out = Vec::new();
    walk(lib, None, "", &levels_below, &mut Default::default(), &mut out);
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

/// The kind of audio a file's name says it holds, when that kind keeps an ID3v2 tag: copies of
/// the audio get Serato's tag only in these formats.
fn tag_kind(name: &str) -> Option<lofty::file::FileType> {
    use lofty::file::FileType;
    match name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("mp3") => Some(FileType::Mpeg),
        Some("aif" | "aiff") => Some(FileType::Aiff),
        Some("wav") => Some(FileType::Wav),
        _ => None,
    }
}

/// Copies of the audio for the crates to list, with Serato's cue tag in those that can hold one.
pub struct Copies<'a, E> {
    /// The folder the copies' `Contents/` goes in.
    pub root: PathBuf,
    /// A track's audio file, to copy. A track whose file isn't there gets no copy.
    pub audio: &'a dyn Fn(&Track) -> Result<PathBuf, E>,
}

/// A `.crate` file in `output`'s `_Serato_/Subcrates` for every playlist and folder in `src`, and
/// with `copies`, a copy of every track's audio (tagged with its cues when `src` has analysis and
/// the audio can take the tag, and planned at its size with the tag's: see [`tag_growth`]).
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
    let (mut with_cues, mut untaggable, mut left_out) = (0, 0, 0);
    // Tracks whose audio isn't what its name says (so lofty can't tag it), and why the first isn't.
    let (mut not_audio, mut first_reason) = (0, None);
    ctx.phase_start(Phase::Scan, lib.tracks.len() as u64, 0);
    let mut seen = rustc_hash::FxHashSet::default();
    for t in &lib.tracks {
        ctx.check().map_err(Error::from)?;
        ctx.emit(Event::Item { phase: Phase::Scan, path: t.file_path.clone(), bytes: 0 });
        if !seen.insert(crate::file_key(t)) {
            continue;
        }
        let key = paths::key(&t.file_path);
        let from = (copies.audio)(t)?;
        let Ok(meta) = std::fs::metadata(&from) else { continue };
        let to = located(t);
        let (markers, bytes) = match (tag_kind(&key), src.analysis) {
            (Some(_), Some(load)) => {
                let m = Markers::from_rekordbox(t.color, &load(t).cues);
                let encoded = m.encode();
                // Checked now so the plan shows which copies go untagged, before anything is
                // copied: one file that isn't real audio shouldn't stop every other copy.
                match tag_growth(&from, &from, &encoded) {
                    Ok(growth) => {
                        left_out += m.left_out;
                        export.cues += m.cues.len() + m.loops.len();
                        with_cues += usize::from(!m.cues.is_empty() || !m.loops.is_empty());
                        (Some(encoded), meta.len() + growth)
                    }
                    Err(Error::Tags(reason)) => {
                        not_audio += 1;
                        first_reason.get_or_insert(reason);
                        (None, meta.len())
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            (None, Some(_)) => {
                untaggable += 1;
                (None, meta.len())
            }
            _ => (None, meta.len()),
        };
        let what = match markers {
            Some(_) => "copy of the audio, with Serato's cue tag",
            None => "copy of the audio",
        };
        export.copy_bytes += meta.len();
        let content = Content::TaggedCopy { from, markers };
        export.files.push(File { path: to, what: what.into(), bytes, content });
    }
    ctx.phase_end(Phase::Scan);
    export.notes.push(format!(
        "The audio is copied into {} (the drive's files are never changed) and the crates list the copies; {with_cues} \
         of them get rekordbox's hot cues and loops in their Serato Markers2 tag.",
        output.join("Contents").display()
    ));
    if untaggable > 0 {
        export.warnings.push(format!(
            "{untaggable} track(s) aren't MP3, AIFF or WAV; their copies get no cues (FLAC and MP4 tags aren't written)"
        ));
    }
    if let Some(reason) = first_reason {
        export.warnings.push(format!(
            "{not_audio} track(s) can't take Serato's cue tag, so their copies get no cues (e.g. {reason})"
        ));
    }
    if left_out > 0 {
        export.warnings.push(format!(
            "{left_out} cue(s) left out: memory cues (Serato has none) and cues past the eighth pad or loop slot"
        ));
    }
    Ok(export)
}

/// A tag error about the file `named`.
fn tag_error(named: &Path, e: &dyn std::fmt::Display) -> Error {
    Error::Tags(format!("{}: {e}", named.display()))
}

/// The ID3v2 tag [`write_markers`] writes into the file at `path`, the kind of file it is, and the
/// options it's written with.
fn marked_tag(
    path: &Path,
    named: &Path,
    markers: &[u8],
) -> Result<(lofty::id3::v2::Id3v2Tag, lofty::file::FileType, lofty::config::WriteOptions)> {
    use lofty::TextEncoding;
    use lofty::config::{ParseOptions, WriteOptions};
    use lofty::file::{AudioFile, FileType};
    use lofty::id3::v2::{BinaryFrame, Frame, FrameFlags, FrameId, GeneralEncapsulatedObject, Id3v2Tag, Id3v2Version};
    use lofty::probe::Probe;
    use std::io::{Read, Seek};

    let io = |e| Error::Io { path: path.to_path_buf(), source: e };
    let mut f = std::fs::File::open(path).map_err(io)?;
    let Some(kind) = tag_kind(&named.to_string_lossy()) else {
        return Err(tag_error(named, &"not an MP3, AIFF or WAV file"));
    };
    let options = ParseOptions::new().read_properties(false);
    // lofty writes a tag where the format its content looks like keeps one, whatever the name
    // says, and fails on content that looks like no format.
    let probed = Probe::new(&mut f).options(options).guess_file_type().map_err(io)?.file_type();
    if probed != Some(kind) {
        return Err(tag_error(named, &"its content isn't the MP3, AIFF or WAV audio its name says"));
    }
    if kind != FileType::Mpeg {
        // lofty reads an AIFF or WAV file cut off short of the size its header gives, but won't
        // write into one.
        let mut header = [0; 8];
        f.read_exact(&mut header).map_err(io)?;
        let size = [header[4], header[5], header[6], header[7]];
        let size = if kind == FileType::Wav { u32::from_le_bytes(size) } else { u32::from_be_bytes(size) };
        if f.metadata().map_err(io)?.len() < u64::from(size) + 8 {
            return Err(tag_error(named, &"it's cut off: shorter than its header says"));
        }
        f.rewind().map_err(io)?;
    }
    let existing = match kind {
        FileType::Mpeg => {
            lofty::mpeg::MpegFile::read_from(&mut f, options).map_err(|e| tag_error(named, &e))?.id3v2().cloned()
        }
        FileType::Aiff => {
            lofty::iff::aiff::AiffFile::read_from(&mut f, options).map_err(|e| tag_error(named, &e))?.id3v2().cloned()
        }
        _ => lofty::iff::wav::WavFile::read_from(&mut f, options).map_err(|e| tag_error(named, &e))?.id3v2().cloned(),
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
    let bytes = object.as_bytes(WriteOptions::new()).map_err(|e| tag_error(named, &e))?;
    tag.insert(Frame::Binary(BinaryFrame::new(geob.clone(), bytes)));
    if kind != FileType::Mpeg {
        // lofty drops a footer from a tag it writes into an AIFF or WAV chunk; dropped here too,
        // the tag measures what's written.
        let mut flags = *tag.flags();
        flags.footer = false;
        tag.set_flags(flags);
    }
    Ok((tag, kind, WriteOptions::new().use_id3v23(v23)))
}

/// The most bytes [`write_markers`] adds to the file at `path`: the whole new tag, in its chunk for
/// AIFF and WAV. Exactly how many isn't known before writing, as the new tag replaces any the file
/// has, and lofty finds that one (with any padding or junk around it) only as it writes. Counting
/// none of it replaced keeps this an upper bound, so a copy planned at its size plus this fits.
pub fn tag_growth(path: &Path, named: &Path, markers: &[u8]) -> Result<u64> {
    use lofty::file::FileType;
    use lofty::tag::TagExt;

    let (tag, kind, options) = marked_tag(path, named, markers)?;
    let mut bytes = Vec::new();
    tag.dump_to(&mut bytes, options).map_err(|e| tag_error(named, &e))?;
    let len = bytes.len() as u64;
    // An `ID3 ` chunk's header, and the byte padding it to an even length.
    Ok(if kind == FileType::Mpeg { len } else { len + 8 + len % 2 })
}

/// Write `markers` as the `Serato Markers2` GEOB frame of the ID3v2 tag of the file at `path` (a
/// copy of `named`, an MP3, AIFF or WAV file by its name and content), keeping its other frames
/// and removing an older `Serato Markers_` frame, which Serato would read instead. Errors about
/// the content name `named`: `path` is often a temporary file.
pub fn write_markers(path: &Path, named: &Path, markers: &[u8]) -> Result<()> {
    use lofty::tag::TagExt;

    let (tag, _, options) = marked_tag(path, named, markers)?;
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| Error::Io { path: path.to_path_buf(), source: e })?;
    tag.save_to(&mut f, options).map_err(|e| tag_error(named, &e))
}

#[cfg(test)]
mod tests;
