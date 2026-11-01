//! The `rekordbox-xml` library source (`library/read`): a rekordbox XML file's collection as a
//! library to build a drive from, a page at a time. Each track is its audio file (`Location`)
//! with its tags, key, tempo, cues and the beat grid its `TEMPO`s make; the playlists and what was
//! left out come with the last page. The host checks every file is in a folder it was granted,
//! and leaves the others out, so the source answers every track the file has.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::UNIX_EPOCH;

use loclib_plugin::{Host, LibraryPage, ReadLibrary, RpcError, SourceCue, SourcePlaylist, SourceTrack, params};
use loclib_tracklist::{MAX_BEATS, XmlLibrary, XmlPlaylist, XmlTrack, read_xml_library};
use serde::Deserialize;

use crate::xml_file;

/// The most tracks on a page.
const PER_PAGE: usize = 500;
/// A page ends once its tracks have this many beats and cues: a few MiB of JSON.
const PAGE_ITEMS: usize = 100_000;
/// The most the playlists' files come to: 100,000 entries naming files by 200-byte paths are
/// 20 MB.
const PLAYLIST_BYTES: usize = 64 << 20;

/// The source's options.
#[derive(Deserialize)]
struct Options {
    /// The rekordbox XML file.
    file: PathBuf,
}

/// What a read's pages left out so far, by why.
#[derive(Default)]
struct LeftOut {
    no_file: usize,
    no_length: usize,
    cut: usize,
    entries: usize,
    past_bytes: usize,
}

/// The file as it was when a read of it began: its length and modification time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    mtime_ns: u128,
}

impl Stamp {
    fn of(file: &Path) -> Result<Stamp, RpcError> {
        let meta = std::fs::metadata(file).map_err(|e| RpcError::failed("io", format!("{}: {e}", file.display())))?;
        let since = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok());
        Ok(Stamp { len: meta.len(), mtime_ns: since.map_or(0, |d| d.as_nanos()) })
    }

    /// The cursor of the page from track `i`: `i:len:mtime_ns`.
    fn cursor(self, i: usize) -> String {
        format!("{i}:{}:{}", self.len, self.mtime_ns)
    }
}

/// The track a cursor's page starts at, and the file's stamp when its read began.
fn from_cursor(c: &str) -> Option<(usize, Stamp)> {
    let mut parts = c.split(':');
    let i = parts.next()?.parse().ok()?;
    let stamp = Stamp { len: parts.next()?.parse().ok()?, mtime_ns: parts.next()?.parse().ok()? };
    parts.next().is_none().then_some((i, stamp))
}

/// A read under way: its file's collection, kept between pages so the file is read once, and
/// what its pages have left out so far.
struct Reading {
    file: PathBuf,
    stamp: Stamp,
    library: XmlLibrary,
    left_out: LeftOut,
}

/// The read under way. One is kept at a time, dropped with its last page; another read's page
/// reads its file again (and says what's left out of the pages from there).
static READING: Mutex<Option<Reading>> = Mutex::new(None);

/// `library/read`: the page of the collection from `cursor`, in a blob when it's big. The file is
/// read once for all its pages, and a file saved since the first is refused (`conflict`), as its
/// tracks may have moved. What was left out is said with the last page.
pub(crate) fn read_library(req: ReadLibrary, host: &Host) -> Result<LibraryPage, RpcError> {
    let Options { file } = params(req.options)?;
    let stamp = Stamp::of(&file)?;
    let shown = file.display();
    let from = match req.cursor.as_deref() {
        None => 0,
        Some(c) => {
            let (i, began) = from_cursor(c)
                .ok_or_else(|| RpcError::invalid_params(format!("{c:?} isn't a cursor of {shown}'s collection")))?;
            if began != stamp {
                return Err(RpcError::failed("conflict", format!("{shown} changed while it was read: build again")));
            }
            i
        }
    };
    let mut reading = READING.lock().unwrap_or_else(PoisonError::into_inner);
    let kept = reading.as_ref().is_some_and(|r| r.file == file && r.stamp == stamp);
    if req.cursor.is_none() || !kept {
        let library = xml_file::read(&file, read_xml_library)?;
        *reading = Some(Reading { file: file.clone(), stamp, library, left_out: LeftOut::default() });
    }
    let Reading { library, left_out, .. } = reading.as_mut().expect("read just now");
    if from > library.tracks.len() {
        return Err(RpcError::invalid_params(format!("{shown}'s collection has no track {from}")));
    }
    let mut page = LibraryPage::default();
    let mut items = 0;
    for (i, t) in library.tracks.iter().enumerate().skip(from) {
        host.ctx().check()?;
        if page.tracks.len() == PER_PAGE || items >= PAGE_ITEMS {
            page.next = Some(stamp.cursor(i));
            break;
        }
        if let Some(track) = track(t, left_out) {
            items += 1 + track.beats.len() + track.cues.len();
            page.tracks.push(track);
        }
    }
    if page.next.is_none() {
        page.playlists = playlists(library, left_out);
        page.warnings = std::mem::take(&mut library.warnings);
        page.warnings.extend(left_out.warnings());
        *reading = None;
    }
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

/// The playlists by their tracks' files. An entry without one (a track the collection lacks, or
/// one with no `Location`) is left out, and so is one past PLAYLIST_BYTES of files named, as a
/// file naming a track with a long path many times would have them.
fn playlists(library: &XmlLibrary, left_out: &mut LeftOut) -> Vec<SourcePlaylist> {
    let mut bytes = 0;
    let playlist = |p: &XmlPlaylist| {
        let mut tracks = Vec::new();
        for e in &p.entries {
            match library.file(e) {
                None => left_out.entries += 1,
                Some(f) if bytes + f.len() > PLAYLIST_BYTES => left_out.past_bytes += 1,
                Some(f) => {
                    bytes += f.len();
                    tracks.push(PathBuf::from(f));
                }
            }
        }
        SourcePlaylist { path: p.path.clone(), tracks }
    };
    library.playlists.iter().map(playlist).collect()
}

impl LeftOut {
    fn warnings(&self) -> Vec<String> {
        let said = [
            (self.no_file, "track(s) left out: no audio file (Location)".to_string()),
            (self.no_length, "beat grid(s) left out: no length (TotalTime) to end at".into()),
            (self.cut, format!("beat grid(s) cut at {MAX_BEATS} beats")),
            (self.entries, "playlist entr(ies) left out: not a collection track with an audio file".into()),
            (
                self.past_bytes,
                format!(
                    "playlist entr(ies) left out: the playlists name more than {} MiB of files",
                    PLAYLIST_BYTES >> 20
                ),
            ),
        ];
        said.into_iter().filter(|(n, _)| *n > 0).map(|(n, what)| format!("{n} {what}")).collect()
    }
}

#[cfg(test)]
mod tests;
