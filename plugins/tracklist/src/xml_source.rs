//! The `rekordbox-xml` library source (`library/read`): a rekordbox XML file's collection as a
//! library to build a drive from, a page at a time. Each track is its audio file (`Location`)
//! with its tags, key, tempo, cues and the beat grid its `TEMPO`s make; the playlists come with
//! the last page. The host checks every file is in a folder it was granted, and leaves the others
//! out, so the source answers every track the file has.

use std::path::PathBuf;

use loclib_plugin::{Host, LibraryPage, ReadLibrary, RpcError, SourceCue, SourcePlaylist, SourceTrack, params};
use loclib_tracklist::{Entry, MAX_BEATS, XmlTrack};
use serde::Deserialize;

use crate::xml_file;

/// The most tracks on a page.
const PER_PAGE: usize = 500;
/// A page ends once its tracks have this many beats and cues: a few MiB of JSON.
const PAGE_ITEMS: usize = 100_000;

/// The source's options.
#[derive(Deserialize)]
struct Options {
    /// The rekordbox XML file.
    file: PathBuf,
}

/// What a page left out, by why.
#[derive(Default)]
struct LeftOut {
    no_file: usize,
    no_length: usize,
    cut: usize,
    entries: usize,
}

/// `library/read`: the page of the collection from `cursor` (an index into it), the file read
/// again for each, in a blob when it's big. Its reader's warnings come with the first page.
pub(crate) fn read_library(req: ReadLibrary, host: &Host) -> Result<LibraryPage, RpcError> {
    let Options { file } = params(req.options)?;
    let library = xml_file::read(&file)?;
    let from = match req.cursor.as_deref() {
        None => 0,
        Some(c) => c.parse().ok().filter(|&i| i <= library.tracks.len()).ok_or_else(|| {
            RpcError::invalid_params(format!("{c:?} isn't a cursor of {}'s collection", file.display()))
        })?,
    };
    let mut page = LibraryPage::default();
    let mut left_out = LeftOut::default();
    let mut items = 0;
    for (i, t) in library.tracks.iter().enumerate().skip(from) {
        host.ctx().check()?;
        if page.tracks.len() == PER_PAGE || items >= PAGE_ITEMS {
            page.next = Some(i.to_string());
            break;
        }
        if let Some(track) = track(t, &mut left_out) {
            items += 1 + track.beats.len() + track.cues.len();
            page.tracks.push(track);
        }
    }
    if page.next.is_none() {
        page.playlists =
            library.playlists.into_iter().map(|(path, entries)| playlist(path, entries, &mut left_out)).collect();
    }
    if from == 0 {
        page.warnings = library.warnings;
    }
    page.warnings.extend(left_out.warnings());
    Ok(match xml_file::spilled(&page, req.context.scratch.as_deref(), "page.json")? {
        Some(blob) => LibraryPage { blob: Some(blob), next: page.next, ..LibraryPage::default() },
        None => page,
    })
}

/// A collection track as the source answers it, or none without an audio file.
fn track(t: &XmlTrack, left_out: &mut LeftOut) -> Option<SourceTrack> {
    let e = &t.entry;
    let Some(path) = e.path.as_deref() else {
        left_out.no_file += 1;
        return None;
    };
    let (beats, cut) = t.beats();
    left_out.cut += usize::from(cut);
    left_out.no_length += usize::from(!t.tempos.is_empty() && e.duration_secs.is_none());
    let cues = t.marks.iter().map(|m| SourceCue {
        hot_cue: m.hot_cue,
        start_ms: m.start_ms,
        end_ms: m.end_ms,
        name: m.name.clone(),
        color: m.rgb.map(xml_file::hex),
    });
    Some(SourceTrack {
        path: path.into(),
        title: e.title.clone(),
        artist: e.artist.clone(),
        album: e.album.clone().unwrap_or_default(),
        genre: t.genre.clone(),
        label: t.label.clone(),
        remixer: t.remixer.clone(),
        composer: t.composer.clone(),
        comment: t.comment.clone(),
        year: t.year,
        rating: t.rating,
        key: t.key.clone(),
        bpm: t.bpm,
        cues: cues.collect(),
        beats,
        ..SourceTrack::default()
    })
}

/// A playlist by its tracks' files; an entry without one (a track the collection lacks, or
/// one with no `Location`) is left out.
fn playlist(path: String, entries: Vec<Entry>, left_out: &mut LeftOut) -> SourcePlaylist {
    let tracks: Vec<PathBuf> = entries.iter().filter_map(|e| e.path.as_deref().map(PathBuf::from)).collect();
    left_out.entries += entries.len() - tracks.len();
    SourcePlaylist { path, tracks }
}

impl LeftOut {
    fn warnings(&self) -> Vec<String> {
        let said = [
            (self.no_file, "track(s) left out: no audio file (Location)".to_string()),
            (self.no_length, "beat grid(s) left out: no length (TotalTime) to end at".into()),
            (self.cut, format!("beat grid(s) cut at {MAX_BEATS} beats")),
            (self.entries, "playlist entr(ies) left out: not a collection track with an audio file".into()),
        ];
        said.into_iter().filter(|(n, _)| *n > 0).map(|(n, what)| format!("{n} {what}")).collect()
    }
}

#[cfg(test)]
mod tests;
