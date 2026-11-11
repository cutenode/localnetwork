//! The music half of a backup: each track's audio copied into `rekordbox_bak/` beside the zip,
//! renamed after its id as rekordbox renames it (see [`crate::entries::music_name`]), with its
//! modification time. A copy already there with the same size and time is kept as it is, so a
//! second backup to the same folder copies only what changed.

use std::fs::{File, FileTimes};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use loclib_plugin::{CollectionTrack, RpcError};
use loclib_progress::{Ctx, Event, Phase};

use crate::entries::music_name;
use crate::write::CHUNK;
use crate::{contents, io};

/// The folder rekordbox's backup puts the music in, beside the zip.
pub(crate) const FOLDER: &str = "rekordbox_bak";

/// A track's audio, to copy.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Copy {
    /// Its id as `contents.xml` names it, 8 hex digits.
    pub hex: String,
    /// Its copy's place in the music folder (`/Artist/Album/HEXID_name.mp3`).
    pub name: String,
    pub from: PathBuf,
    pub bytes: u64,
    pub modified: Option<SystemTime>,
}

impl Copy {
    /// Where it goes below `root` (the music folder).
    pub(crate) fn to(&self, root: &Path) -> PathBuf {
        self.name.split('/').filter(|n| !n.is_empty()).fold(root.to_path_buf(), |p, n| p.join(n))
    }

    /// Whether the copy at `to` is this already: the same size, and the same modification time
    /// as far as its file system keeps one (FAT's to two seconds, exFAT's and HFS+'s coarser
    /// than APFS's), so a backup folder on such a drive isn't copied whole every time.
    pub(crate) fn there(&self, to: &Path) -> bool {
        let Ok(meta) = std::fs::metadata(to) else { return false };
        let close = match (meta.modified().ok(), self.modified) {
            (Some(a), Some(b)) => a.duration_since(b).or_else(|_| b.duration_since(a)).is_ok_and(|d| d <= SLACK),
            (a, b) => a == b,
        };
        meta.is_file() && meta.len() == self.bytes && close
    }
}

/// How far apart two modification times may be and still be one: FAT keeps them to 2 seconds.
const SLACK: Duration = Duration::from_secs(2);

/// The copies to make of `tracks`' audio (those rekordbox hasn't deleted, with a file on this
/// computer), in track order, and a line for each left out: its file is missing (rekordbox
/// leaves it out too, though it counts it), or its id isn't a number.
pub(crate) fn copies(tracks: &[CollectionTrack]) -> (Vec<Copy>, Vec<String>) {
    let (mut out, mut left_out) = (Vec::new(), Vec::new());
    for t in tracks.iter().filter(|t| !t.deleted) {
        let Some(meta) = std::fs::metadata(&t.path).ok().filter(std::fs::Metadata::is_file) else {
            // A streaming service's track has no file to copy, and isn't missing one.
            if contents::is_local(&t.path) {
                left_out.push(format!("track {}: {} isn't there, so its music isn't copied", t.id, t.path));
            }
            continue;
        };
        let Some(name) = music_name(&t.id, t.artist.as_deref(), t.album.as_deref(), &t.path) else {
            left_out.push(format!("track {}: its id isn't a number, so its music isn't copied", t.id));
            continue;
        };
        // The file's name starts with the id's 8 hex digits.
        let hex = name.rsplit('/').next().unwrap_or_default().chars().take(8).collect();
        out.push(Copy { hex, name, from: PathBuf::from(&t.path), bytes: meta.len(), modified: meta.modified().ok() });
    }
    (out, left_out)
}

/// What [`copy_all`] did.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Copied {
    pub copied: usize,
    pub kept: usize,
    /// The copies that couldn't be made, by id (8 hex digits), with why.
    pub failed: Vec<(String, String)>,
}

/// Copies each of `copies` into `root` (the music folder), each staged beside its place and put
/// there once whole, with its source's modification time; one there already is kept. A copy that
/// fails (its source unreadable, a name the drive refuses) is said and skipped, so the rest still
/// go; a cancel stops them all.
pub(crate) fn copy_all(copies: &[Copy], root: &Path, ctx: &Ctx<'_>) -> Result<Copied, RpcError> {
    let bytes = copies.iter().map(|c| c.bytes).sum();
    ctx.phase_start(Phase::Copy, copies.len() as u64, bytes);
    let mut done = Copied::default();
    let mut buf = vec![0u8; CHUNK];
    for c in copies {
        ctx.check()?;
        let to = c.to(root);
        ctx.emit(Event::Item { phase: Phase::Copy, path: c.name.clone(), bytes: c.bytes });
        if c.there(&to) {
            done.kept += 1;
            ctx.emit(Event::Bytes { phase: Phase::Copy, bytes: c.bytes });
            continue;
        }
        match copy(c, &to, root, &mut buf, ctx) {
            Ok(()) => done.copied += 1,
            Err(_) if ctx.check().is_err() => return Err(RpcError::cancelled()),
            Err(e) => done.failed.push((c.hex.clone(), e.message)),
        }
    }
    ctx.phase_end(Phase::Copy);
    Ok(done)
}

/// Copies `c` to `to`, staged beside it.
fn copy(c: &Copy, to: &Path, root: &Path, buf: &mut [u8], ctx: &Ctx<'_>) -> Result<(), RpcError> {
    let dir = to.parent().unwrap_or(root);
    std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    #[cfg_attr(not(unix), allow(unused_mut, reason = "only Unix sets its permissions"))]
    let mut builder = tempfile::Builder::new();
    // Made as any other file is (the umask decides), as rekordbox's copies are.
    #[cfg(unix)]
    builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o666));
    let mut tmp = builder.prefix(".rekordbox_bak_").tempfile_in(dir).map_err(|e| io(dir, e))?;
    let mut from = File::open(&c.from).map_err(|e| io(&c.from, e))?;
    loop {
        ctx.check()?;
        let n = from.read(buf).map_err(|e| io(&c.from, e))?;
        if n == 0 {
            break;
        }
        tmp.write_all(&buf[..n]).map_err(|e| io(to, e))?;
        ctx.emit(Event::Bytes { phase: Phase::Copy, bytes: n as u64 });
    }
    if let Some(modified) = c.modified {
        tmp.as_file().set_times(FileTimes::new().set_modified(modified)).map_err(|e| io(to, e))?;
    }
    tmp.as_file().sync_all().map_err(|e| io(to, e))?;
    tmp.persist(to).map_err(|e| io(to, e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests;
