//! Writing a backup's zip: into a temporary file beside where it goes, put in place once whole,
//! so a failure or a cancel leaves what was there (an older backup of the same day, or nothing).
//! It holds sign-in tokens unless they were stripped, so only its owner may read it.

use std::fs::File;
use std::io::{BufWriter, ErrorKind, Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

use chrono::{Datelike, Local, TimeZone, Timelike, Utc};
use loclib_plugin::RpcError;
use loclib_progress::{Ctx, Event, Phase};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::entries::{Body, Entry};
use crate::io;

/// Bytes copied at a time, between checks for a cancel.
pub(crate) const CHUNK: usize = 1 << 20;

/// Permissions of an entry made for the backup (`contents.xml`), as a file made by its owner.
const MADE_MODE: u32 = 0o644;

/// Writes `entries`, in order, as the zip `into`, replacing a file there once it's whole;
/// answers its size.
pub(crate) fn write(entries: &[Entry], into: &Path, ctx: &Ctx<'_>) -> Result<u64, RpcError> {
    let dir = into.parent().unwrap_or(Path::new("."));
    let sizes: Vec<u64> = entries.iter().map(|e| e.body.len()).collect();
    let needed: u64 = sizes.iter().sum();
    // At most what it holds, deflated: rekordbox's own came to 80% of it.
    if let Ok(available) = fs4::available_space(dir)
        && needed > available
    {
        let message = format!("{} needs up to {needed} bytes free, and has {available}", dir.display());
        return Err(RpcError::failed("no_space", message));
    }
    ctx.phase_start(Phase::Write, entries.len() as u64, needed);
    tidy(dir);
    // Readable by its owner alone (tempfile makes it so on Unix), as it may hold tokens.
    let tmp = tempfile::Builder::new().prefix(PART.0).suffix(PART.1).tempfile_in(dir).map_err(|e| io(dir, e))?;
    let mut zip = ZipWriter::new(BufWriter::new(tmp.as_file()));
    let mut buf = vec![0u8; CHUNK];
    for (e, bytes) in entries.iter().zip(sizes) {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Write, path: e.name.clone(), bytes });
        // Opened before its entry starts: a file rekordbox removed since it was listed (an
        // analysis redone while it runs) is left out, but not master.db, the backup's core.
        let file = match &e.body {
            Body::File(path) => match File::open(path) {
                Ok(file) => Some((path, file)),
                Err(err) if err.kind() == ErrorKind::NotFound && e.name != "master.db" => {
                    ctx.warn(format!("{} went while the backup was made, so it's left out", e.name));
                    continue;
                }
                Err(err) => return Err(io(path, err)),
            },
            _ => None,
        };
        let meta = match (&file, &e.body) {
            (Some((path, file)), _) => Some(file.metadata().map_err(|err| io(path, err))?),
            (None, Body::Changed(path, _)) => std::fs::metadata(path).ok(),
            _ => None,
        };
        let (time, mode) = stamp(meta.as_ref());
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .last_modified_time(time)
            .unix_permissions(mode)
            .large_file(bytes >= u64::from(u32::MAX));
        zip.start_file(e.name.as_str(), options).map_err(|err| zip_error(into, err))?;
        match (file, &e.body) {
            (Some((path, mut file)), _) => loop {
                ctx.check()?;
                let n = file.read(&mut buf).map_err(|err| io(path, err))?;
                if n == 0 {
                    break;
                }
                zip.write_all(&buf[..n]).map_err(|err| io(into, err))?;
                ctx.emit(Event::Bytes { phase: Phase::Write, bytes: n as u64 });
            },
            // A file's entry was opened above.
            (None, Body::Changed(_, made) | Body::Made(made)) => {
                zip.write_all(made).map_err(|err| io(into, err))?;
                ctx.emit(Event::Bytes { phase: Phase::Write, bytes: made.len() as u64 });
            }
            (None, Body::File(_)) => {}
        }
    }
    ctx.check()?;
    let out = zip.finish().map_err(|err| zip_error(into, err))?;
    let file = out.into_inner().map_err(|err| io(into, err.into_error()))?;
    file.sync_all().map_err(|err| io(into, err))?;
    let written = file.metadata().map_err(|err| io(into, err))?.len();
    ctx.phase_end(Phase::Write);
    tmp.persist(into).map_err(|err| io(into, err.error))?;
    Ok(written)
}

/// How a zip being written is named, before and after tempfile's random part.
const PART: (&str, &str) = (".rekordbox_bak_", ".zip.part");

/// How old a zip being written must be before it's taken for one a killed backup left: no backup
/// takes this long, so it isn't one under way in another process.
const STALE: Duration = Duration::from_secs(24 * 60 * 60);

/// Removes from `dir` the zips a backup left half-written when its process was killed (the app
/// quit during a scheduled one, the power went): each may hold tokens.
fn tidy(dir: &Path) {
    let Ok(listed) = std::fs::read_dir(dir) else { return };
    for e in listed.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let part = name.starts_with(PART.0) && name.ends_with(PART.1);
        let modified = e.metadata().and_then(|m| m.modified());
        if part && modified.is_ok_and(|t| t.elapsed().is_ok_and(|age| age > STALE)) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// An entry's time and permissions: those of the file it's from (`meta`), or now and
/// [`MADE_MODE`] for one made.
fn stamp(meta: Option<&std::fs::Metadata>) -> (DateTime, u32) {
    let Some(meta) = meta else { return (dos_time(SystemTime::now()), MADE_MODE) };
    let time = meta.modified().unwrap_or_else(|_| SystemTime::now());
    #[cfg(unix)]
    let mode = std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o7777;
    #[cfg(not(unix))]
    let mode = MADE_MODE;
    (dos_time(time), mode)
}

/// `SOURCE_DATE_EPOCH` (seconds), when it's set: every time a backup records is then that one, in
/// UTC, so two built from the same files are the same to the byte.
pub(crate) fn fixed_epoch() -> Option<i64> {
    std::env::var("SOURCE_DATE_EPOCH").ok()?.trim().parse().ok()
}

/// `time` as a zip's DOS time, in local time as rekordbox writes it (or [`fixed_epoch`]'s, in
/// UTC), held to what a DOS time can say (1980 to 2107).
pub(crate) fn dos_time(time: SystemTime) -> DateTime {
    let naive = match fixed_epoch() {
        Some(secs) => Utc.timestamp_opt(secs, 0).single().map(|t| t.naive_utc()),
        None => Some(chrono::DateTime::<Local>::from(time).naive_local()),
    };
    let fits = naive.and_then(|t| {
        let year = u16::try_from(t.year()).ok()?;
        let at = (t.month() as u8, t.day() as u8, t.hour() as u8, t.minute() as u8, t.second() as u8);
        DateTime::from_date_and_time(year, at.0, at.1, at.2, at.3, at.4).ok()
    });
    fits.unwrap_or_else(|| match naive.is_some_and(|t| t.year() > 2107) {
        true => DateTime::from_date_and_time(2107, 12, 31, 23, 59, 58).expect("the last DOS time"),
        false => DateTime::default(),
    })
}

/// Today, as a backup's name has it (`20261110`): in local time, or [`fixed_epoch`]'s in UTC.
pub(crate) fn today() -> String {
    match fixed_epoch() {
        Some(secs) => {
            let at = SystemTime::UNIX_EPOCH + Duration::from_secs(secs.max(0) as u64);
            chrono::DateTime::<Utc>::from(at).format("%Y%m%d").to_string()
        }
        None => Local::now().format("%Y%m%d").to_string(),
    }
}

fn zip_error(path: &Path, e: zip::result::ZipError) -> RpcError {
    match e {
        zip::result::ZipError::Io(e) => io(path, e),
        e => RpcError::failed("io", format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests;
