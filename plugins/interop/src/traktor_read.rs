//! A Traktor collection (`collection.nml`) read with loclib-interop: what it says about tracks, for
//! `import` (the `traktor` importer), and the library itself, for building a drive from (the
//! `traktor` source).

use std::path::{Path, PathBuf};

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

/// How many of a collection's tracks a page of `library/read` holds.
const PAGE: usize = 1000;

/// The source's options.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceOptions {
    /// The collection to read, absolute.
    collection: PathBuf,
}

/// `library/read`: a page of the collection's tracks, each by its audio file on this computer, and
/// on the last page its playlists. A track whose file isn't in a folder granted is answered by its
/// path alone, for the host to leave out and say so, without what it holds.
pub(crate) fn read_library(req: ReadLibrary, host: &Host) -> Result<LibraryPage, RpcError> {
    let SourceOptions { collection: file } = params(req.options)?;
    let collection = collection(&file, &host.ctx())?;
    // A page has no warnings; what reading the collection left out goes in the log, once.
    if req.cursor.is_none() {
        collection.warnings.iter().for_each(|w| eprintln!("{}: {w}", file.display()));
    }
    let entries = &collection.entries;
    // A cursor that isn't a number is past the end, refused below.
    let from = req.cursor.as_deref().map_or(Ok(0), str::parse).unwrap_or(usize::MAX);
    let to = from.saturating_add(PAGE).min(entries.len());
    let page = entries.get(from..to).ok_or_else(|| {
        let cursor = req.cursor.unwrap_or_default();
        crate::invalid(format!("{cursor:?} isn't a page of {}", file.display()))
    })?;
    let startup = traktor::startup_disk();
    let tracks = page.iter().map(|e| {
        let path = e.location.path(&startup);
        match req.roots.iter().any(|root| crate::resolved(&path).starts_with(root)) {
            true => source_track(e, path),
            false => SourceTrack { path, ..SourceTrack::default() },
        }
    });
    let last = to == entries.len();
    let mut playlists = Vec::new();
    if last {
        // Each track by the path its page answered it by, by the key playlists name it by.
        let paths: FxHashMap<String, PathBuf> =
            entries.iter().map(|e| (e.location.key(), e.location.path(&startup))).collect();
        playlists.extend(collection.lists.iter().map(|list| SourcePlaylist {
            path: list.path.join("/"),
            tracks: list.keys.iter().filter_map(|key| paths.get(key).cloned()).collect(),
        }));
    }
    let page = LibraryPage { tracks: tracks.collect(), playlists, blob: None, next: (!last).then(|| to.to_string()) };
    Ok(match spilled(&page, &req.context, "page.json")? {
        Some(blob) => LibraryPage { blob: Some(blob), next: page.next, ..LibraryPage::default() },
        None => page,
    })
}

/// An entry as a source's track: its tags, key, tempo, cues (loops by where they end) and the
/// beat grid its grid markers lay down. A grid that can't be is left out, said in the log.
fn source_track(e: &Entry, path: PathBuf) -> SourceTrack {
    let cues = cues(e).map(|m| SourceCue {
        hot_cue: m.hot_cue,
        start_ms: start_ms(m),
        end_ms: is_loop(m).then(|| (m.start_ms + m.len_ms).max(0.0).round() as u32),
        name: m.name.clone(),
        color: colour(m),
    });
    let beats = e.beats().unwrap_or_else(|why| {
        eprintln!("{}: its beat grid is left out: {why}", path.display());
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
