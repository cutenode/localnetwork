//! A Traktor collection (`collection.nml`) read with loclib-interop: what it says about tracks, for
//! `import` (the `traktor` importer), and the library itself, for building a drive from (the
//! `traktor` source).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::UNIX_EPOCH;

use loclib_interop::Error;
use loclib_interop::traktor::{self, Collection, Entry, Marker, MarkerKind};
use loclib_model::harmony::Camelot;
use loclib_plugin::{
    Blob, Context, Host, ImportAnswer, Imported, ImportedCue, ImportedTrack, LibraryPage, ReadImport, ReadLibrary,
    RpcError, SourceCue, SourcePlaylist, SourceTrack, TrackMatch, params,
};
use loclib_progress::Ctx;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

/// The most an answer holds in a message; a bigger one goes in a blob. Half what a message may be
/// (16 MiB), for the rest of the message around it.
const MESSAGE: usize = 8 << 20;

/// How big a page of `library/read` gets.
#[derive(Clone, Copy, Debug)]
struct Limits {
    /// The most tracks it holds.
    tracks: usize,
    /// How many beats and cues it holds before it ends, after the track that brings it there.
    marks: usize,
}

/// A page's limits: a thousand tracks, and 100,000 beats and cues, as grids of long tracks at
/// fast tempos would otherwise make a page of a thousand tracks hundreds of millions of beats. A
/// track's own grid is at most a day's beats (see `Entry::beats`). This keeps each page small;
/// the host stops a whole read past 2^26 (some 67 million) beats and cues, hundreds of pages,
/// more than any library has.
const LIMITS: Limits = Limits { tracks: 1000, marks: 100_000 };

/// The source's options.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceOptions {
    /// The collection to read, absolute.
    collection: PathBuf,
}

/// A collection being read a page at a time: its file and the file's stamp (its size and when it
/// changed) as the first page found them, and the beat grids left out of the pages so far, by why.
struct Reading {
    file: PathBuf,
    stamp: String,
    collection: Collection,
    unlaid: Vec<(&'static str, usize)>,
}

/// The read under way, kept between its pages so the file is read once for them all.
static READING: Mutex<Option<Reading>> = Mutex::new(None);

/// `library/read`: a page of the collection's tracks, each by its audio file on this computer, and
/// on the last page its playlists and what the read left out. A track whose file isn't in a folder
/// granted is answered by its path alone, for the host to leave out and say so, without what it
/// holds.
pub(crate) fn read_library(req: ReadLibrary, host: &Host) -> Result<LibraryPage, RpcError> {
    let SourceOptions { collection: file } = params(req.options)?;
    let ctx = host.ctx();
    let stamp = stamp(&file, &ctx)?;
    // A cursor is the index of the page's first track and the file's stamp when the first page
    // was read: a save between pages would shift the tracks under it.
    let (from, read_at) = match req.cursor.as_deref() {
        None => (Some(0), stamp.as_str()),
        Some(cursor) => cursor.split_once(' ').map_or((None, ""), |(i, s)| (i.parse().ok(), s)),
    };
    if from.is_some() && read_at != stamp {
        let changed = format!("{} changed since its first page was read: read it again", file.display());
        return Err(RpcError::failed("conflict", changed));
    }
    let mut reading = READING.lock().unwrap_or_else(PoisonError::into_inner);
    let kept = reading.as_ref().is_some_and(|r| r.file == file && r.stamp == stamp);
    if req.cursor.is_none() || !kept {
        let collection = collection(&file, &ctx)?;
        *reading = Some(Reading { file: file.clone(), stamp: stamp.clone(), collection, unlaid: Vec::new() });
    }
    let Reading { collection, unlaid, .. } = reading.as_mut().expect("read just now");
    let entries = &collection.entries;
    let rest = from.and_then(|from| entries.get(from..)).ok_or_else(|| {
        let cursor = req.cursor.as_deref().unwrap_or_default();
        crate::invalid(format!("{cursor:?} isn't a page of {}", file.display()))
    })?;
    let from = entries.len() - rest.len();
    let startup = traktor::startup_disk();
    let granted = |path: &Path| req.roots.iter().any(|root| crate::resolved(path).starts_with(root));
    let tracks = page_tracks(rest, &startup, granted, LIMITS, unlaid);
    let to = from + tracks.len();
    let last = to == entries.len();
    let (mut playlists, mut warnings) = (Vec::new(), Vec::new());
    if last {
        // Each track by the path its page answered it by, by the key playlists name it by.
        let paths: FxHashMap<String, PathBuf> =
            entries.iter().map(|e| (e.location.key(), e.location.path(&startup))).collect();
        playlists.extend(collection.lists.iter().map(|list| SourcePlaylist {
            path: list.path.join("/"),
            tracks: list.keys.iter().filter_map(|key| paths.get(key).cloned()).collect(),
        }));
        // What the whole read left out, said once.
        warnings.extend(collection.warnings.iter().chain(&collection.playlist_warnings).cloned());
        warnings.extend(unlaid.iter().map(|(why, n)| format!("{n} beat grid(s) left out: {why}")));
        *reading = None;
    }
    let next = (!last).then(|| format!("{to} {stamp}"));
    let page = LibraryPage { tracks, playlists, blob: None, next, warnings };
    Ok(match spilled(&page, &req.context, "page.json")? {
        Some(blob) => LibraryPage { blob: Some(blob), next: page.next, ..LibraryPage::default() },
        None => page,
    })
}

/// The tracks of the page that starts with `rest`'s first, within `limits`: each whose file is
/// `granted` (in a folder the user granted) with all it holds, the others by path alone.
fn page_tracks(
    rest: &[Entry],
    startup: &str,
    granted: impl Fn(&Path) -> bool,
    limits: Limits,
    unlaid: &mut Vec<(&'static str, usize)>,
) -> Vec<SourceTrack> {
    let (mut tracks, mut marks) = (Vec::new(), 0);
    for e in rest.iter().take(limits.tracks) {
        // The page ends after the track that brings its beats and cues to the budget.
        if marks >= limits.marks {
            break;
        }
        let path = e.location.path(startup);
        let track = match granted(&path) {
            true => source_track(e, path, unlaid),
            false => SourceTrack { path, ..SourceTrack::default() },
        };
        marks += track.beats.len() + track.cues.len();
        tracks.push(track);
    }
    tracks
}

/// An entry as a source's track: its tags, key, tempo, cues (loops by where they end) and the
/// beat grid its grid markers lay down. A grid that can't be is left out, counted in `unlaid` by
/// why.
fn source_track(e: &Entry, path: PathBuf, unlaid: &mut Vec<(&'static str, usize)>) -> SourceTrack {
    let cues = cues(e).map(|m| SourceCue {
        hot_cue: m.hot_cue,
        start_ms: start_ms(m),
        end_ms: is_loop(m).then(|| (m.start_ms + m.len_ms).max(0.0).round() as u32),
        name: m.name.clone(),
        color: colour(m),
    });
    let beats = e.beats().unwrap_or_else(|why| {
        match unlaid.iter_mut().find(|(w, _)| *w == why) {
            Some((_, n)) => *n += 1,
            None => unlaid.push((why, 1)),
        }
        Vec::new()
    });
    SourceTrack {
        title: e.title.clone(),
        artist: e.artist.clone(),
        album: e.album.clone(),
        genre: e.genre.clone(),
        label: e.label.clone(),
        remixer: e.remixer.clone(),
        comment: e.comment.clone(),
        year: e.year,
        rating: e.rating,
        key: key(e),
        bpm: e.bpm,
        cues: cues.collect(),
        beats,
        path,
        ..SourceTrack::default()
    }
}

/// `import/read`: each track's key, tempo and cues, found by its file, artist, title and length.
/// Tracks the collection says none of those about are left out.
pub(crate) fn read_import(req: ReadImport, host: &Host) -> Result<ImportAnswer, RpcError> {
    let collection = collection(&req.file, &host.ctx())?;
    let startup = traktor::startup_disk();
    let mut left_out = LeftOut::default();
    let tracks = collection.entries.iter().filter_map(|e| imported(e, &startup, &mut left_out));
    let read = Imported {
        tracks: tracks.collect(),
        sessions: Vec::new(),
        warnings: collection.warnings.into_iter().chain(left_out.warnings()).collect(),
    };
    Ok(match spilled(&read, &req.context, "read.json")? {
        Some(blob) => Blob { blob }.into(),
        None => read.into(),
    })
}

/// `file`'s stamp: its size and when it changed.
fn stamp(file: &Path, ctx: &Ctx<'_>) -> Result<String, RpcError> {
    let meta =
        std::fs::metadata(file).map_err(|source| crate::failure(Error::Io { path: file.into(), source }, ctx))?;
    let changed = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).unwrap_or_default();
    Ok(format!("{}:{}", meta.len(), changed.as_nanos()))
}

/// The collection in `file`.
fn collection(file: &Path, ctx: &Ctx<'_>) -> Result<Collection, RpcError> {
    let bytes = std::fs::read(file).map_err(|source| crate::failure(Error::Io { path: file.into(), source }, ctx))?;
    traktor::read(&bytes).map_err(|e| crate::failure(e, ctx))
}

/// `answer` in a file `name` in the request's scratch folder, when it's too big for a message;
/// `None` when it isn't (or there's no folder to put it in).
fn spilled(answer: &impl Serialize, context: &Context, name: &str) -> Result<Option<String>, RpcError> {
    let json = serde_json::to_vec(answer).expect("an answer is JSON");
    let Some(scratch) = context.scratch.as_deref().filter(|_| json.len() > MESSAGE) else { return Ok(None) };
    let path = scratch.join(name);
    std::fs::write(&path, json).map_err(|e| RpcError::failed("io", format!("{}: {e}", path.display())))?;
    Ok(Some(name.into()))
}

/// What of a collection's markers the formats can't take, counted.
#[derive(Default)]
struct LeftOut {
    past_h: usize,
    unsized_loops: usize,
}

impl LeftOut {
    fn warnings(self) -> impl Iterator<Item = String> {
        [
            (self.past_h, "hot cue(s) past H left out"),
            (
                self.unsized_loops,
                "loop(s) left out: not a whole number of beats (or 1/2 to 1/32 of one) at the track's tempo",
            ),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
    }
}

/// What an entry says of its track for `import`, if anything: its key, tempo and cues.
fn imported(e: &Entry, startup: &str, left_out: &mut LeftOut) -> Option<ImportedTrack> {
    let cues: Vec<ImportedCue> = cues(e).filter_map(|m| imported_cue(m, e.bpm, left_out)).collect();
    let key = key(e);
    if key.is_none() && e.bpm.is_none() && cues.is_empty() {
        return None;
    }
    let text = |s: &str| (!s.is_empty()).then(|| s.to_string());
    let find = TrackMatch {
        path: Some(e.location.path(startup).to_string_lossy().into_owned()),
        isrc: None,
        artist: text(&e.artist),
        title: text(&e.title),
        duration: e.duration_secs.map(|s| s.round() as u32),
    };
    Some(ImportedTrack { find, key, bpm: e.bpm, cues, my_tags: Vec::new() })
}

/// A marker as `import` takes a cue: a hot cue `A` to `H` or a memory cue, a loop by its length in
/// beats at the track's tempo.
fn imported_cue(m: &Marker, bpm: Option<f64>, left_out: &mut LeftOut) -> Option<ImportedCue> {
    let hot_cue = match m.hot_cue {
        Some(h) if h > 7 => {
            left_out.past_h += 1;
            return None;
        }
        hot => hot.map(|h| char::from(b'A' + h).to_string()),
    };
    let loop_beats = match is_loop(m) {
        true => match bpm.and_then(|bpm| loop_beats(m.len_ms, bpm)) {
            None => {
                left_out.unsized_loops += 1;
                return None;
            }
            beats => beats,
        },
        false => None,
    };
    Some(ImportedCue {
        hot_cue,
        start_ms: start_ms(m),
        loop_beats,
        name: (!m.name.is_empty()).then(|| m.name.clone()),
        color: colour(m),
    })
}

/// A loop `len_ms` long at `bpm` in beats, as cue edits take one: a whole number up to 512, or 1/2
/// to 1/32 of a beat. Within 2%, for a loop set where the grid's tempo is a little off the track's.
fn loop_beats(len_ms: f64, bpm: f64) -> Option<f64> {
    let beats = len_ms * bpm / 60_000.0;
    let lengths = (1..=512).map(f64::from).chain((1..=5).map(|i| 0.5_f64.powi(i)));
    lengths.into_iter().find(|n| (beats / n - 1.0).abs() <= 0.02)
}

/// An entry's markers that are cues: all but grid markers off the pads, which are its beat grid.
fn cues(e: &Entry) -> impl Iterator<Item = &Marker> {
    e.markers.iter().filter(|m| m.kind != MarkerKind::Grid || m.hot_cue.is_some())
}

fn is_loop(m: &Marker) -> bool {
    m.kind == MarkerKind::Loop && m.len_ms > 0.0
}

/// Where a marker is, a marker before the start of the track at its start.
fn start_ms(m: &Marker) -> u32 {
    m.start_ms.max(0.0).round() as u32
}

/// A marker's colour, or the one Traktor shows its kind in.
fn colour(m: &Marker) -> Option<String> {
    m.color.clone().or_else(|| m.kind.colour().map(String::from))
}

/// An entry's key as a Camelot code, or as Traktor shows it when that's all it says.
fn key(e: &Entry) -> Option<String> {
    e.key.map(Camelot::code).or_else(|| (!e.key_text.is_empty()).then(|| e.key_text.clone()))
}

#[cfg(test)]
mod tests;
