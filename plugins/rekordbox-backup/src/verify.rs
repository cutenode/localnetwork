//! `verify`: whether a backup is whole. Every entry of the zip is read (which checks each one's
//! CRC), its `contents.xml` is read and checked against the entries, its `master.db` is opened
//! through the host and its tracks counted, and when its music folder is beside it, every file
//! `contents.xml` names is looked for there.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use loclib_plugin::{Choice, ChoiceItem, Host, Pair, RpcError, RunCommand, Section, View, params};
use loclib_progress::{Ctx, Event, Phase};
use rustc_hash::FxHashSet;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::list::{self, MOST, value};
use crate::music::FOLDER;
use crate::write::CHUNK;
use crate::{RESTORE, Settings, contents, io, size, view};

/// The `contents.xml` entries a backup can't do without.
const REQUIRED: [&str; 5] = ["DB_ID", "CONTENT_NUM", "BACKUP_RB_VER", "BACKUP_FILE_VER", "DIRECTORY_ARRAY"];

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Params {
    /// The backup's zip; asked for from those in the backup folder when not given.
    file: Option<PathBuf>,
}

/// `verify`: the checks a backup passed, or which it failed.
pub(crate) fn command(req: &RunCommand, host: &Host) -> Result<Value, RpcError> {
    let Params { file } = params::<Option<Params>>(req.params.clone())?.unwrap_or_default();
    let Some(file) = file else { return ask(&Settings::of(&req.context)?) };
    let scratch =
        req.context.scratch.as_deref().ok_or_else(|| RpcError::failed("incompatible", "no scratch folder"))?;
    let report = check(&file, host, scratch, &host.ctx())?;
    if !report.problems.is_empty() {
        let message = format!("{} isn't whole: {}", file.display(), report.problems.join("; "));
        return Err(RpcError::failed("invalid", message));
    }
    let pairs = report.passed.into_iter().map(|(label, value)| Pair { label, value: json!(value) }).collect();
    let items = vec![
        Section { title: format!("{} is whole", file.display()), view: View::Kv { title: None, pairs } },
        Section { title: "Restoring".into(), view: View::Text { title: None, text: RESTORE.into() } },
    ];
    Ok(view(View::Sections { title: None, items }))
}

/// Which backup to verify, of those in the backup folder.
fn ask(settings: &Settings) -> Result<Value, RpcError> {
    let dest = settings.destination()?;
    let found = list::find(&dest);
    if found.is_empty() {
        return Err(RpcError::failed("not_found", format!("there are no backups to verify in {}", dest.display())));
    }
    let item = |f: &list::Found| ChoiceItem {
        label: f.name(),
        detail: Some(format!("{}, {}", f.date(), size(f.bytes))),
        value: json!(f.path),
    };
    let items = found.iter().map(item).collect();
    let choice = Choice { title: "Which backup?".into(), param: "file".into(), multi: false, items, more: Vec::new() };
    Ok(view(View::Choose(choice)))
}

/// What [`check`] found: the checks passed, each with what it saw, and the problems.
#[derive(Debug, Default)]
pub(crate) struct Report {
    pub passed: Vec<(String, String)>,
    pub problems: Vec<String>,
}

/// Checks the backup at `path`, its `master.db` unpacked into `scratch` (and removed after) for
/// the host to read.
pub(crate) fn check(path: &Path, host: &Host, scratch: &Path, ctx: &Ctx<'_>) -> Result<Report, RpcError> {
    let mut report = Report::default();
    let file = File::open(path).map_err(|e| io(path, e))?;
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(zip) => zip,
        Err(e) => {
            report.problems.push(format!("it isn't a zip that opens ({e})"));
            return Ok(report);
        }
    };
    let db = scratch.join("master.db");
    let read = read_entries(&mut zip, &db, ctx, &mut report.problems);
    let checked = read.and_then(|contents| check_contents(path, contents, &db, host, ctx, &mut report));
    let _ = std::fs::remove_file(&db);
    checked?;
    Ok(report)
}

/// Reads every entry of `zip` to its end, `master.db` into the file `db`; answers the
/// `contents.xml`, when it's there.
fn read_entries(
    zip: &mut zip::ZipArchive<File>,
    db: &Path,
    ctx: &Ctx<'_>,
    problems: &mut Vec<String>,
) -> Result<Option<Vec<u8>>, RpcError> {
    let mut names = Vec::with_capacity(zip.len());
    let mut contents = None;
    let mut buf = vec![0u8; CHUNK];
    ctx.phase_start(Phase::Verify, zip.len() as u64, 0);
    for i in 0..zip.len() {
        ctx.check()?;
        let mut entry = match zip.by_index(i) {
            Ok(entry) => entry,
            Err(e) => {
                problems.push(format!("its entry {i} can't be read ({e})"));
                continue;
            }
        };
        let name = entry.name().map(|n| n.into_owned()).unwrap_or_default();
        ctx.emit(Event::Item { phase: Phase::Verify, path: name.clone(), bytes: entry.size() });
        if entry.is_dir() {
            problems.push(format!("it has a folder entry, {name}, which rekordbox's backups don't"));
        }
        let mut out: Box<dyn Write> = match name.as_str() {
            "master.db" if names.is_empty() => {
                let _ = std::fs::remove_file(db);
                Box::new(File::create_new(db).map_err(|e| io(db, e))?)
            }
            _ => Box::new(std::io::sink()),
        };
        let mut kept = Vec::new();
        let read = loop {
            ctx.check()?;
            match entry.read(&mut buf) {
                Ok(0) => break Ok(()),
                Ok(n) => {
                    if name == "contents.xml" && (kept.len() as u64) < MOST {
                        kept.extend_from_slice(&buf[..n]);
                    }
                    out.write_all(&buf[..n]).map_err(|e| io(db, e))?;
                    ctx.emit(Event::Bytes { phase: Phase::Verify, bytes: n as u64 });
                }
                Err(e) => break Err(e),
            }
        };
        if let Err(e) = read {
            problems.push(format!("{name} is damaged ({e})"));
        } else if name == "contents.xml" {
            contents = Some(kept);
        }
        names.push(name);
    }
    ctx.phase_end(Phase::Verify);
    let mut seen = FxHashSet::default();
    if let Some(twice) = names.iter().find(|n| !seen.insert(n.as_str())) {
        problems.push(format!("it has {twice} twice"));
    }
    if names.first().map(String::as_str) != Some("master.db") {
        problems.push("master.db isn't its first entry".into());
    }
    if names.last().map(String::as_str) != Some("contents.xml") {
        problems.push("contents.xml isn't its last entry".into());
    }
    Ok(contents)
}

/// Checks `contents.xml` (read from the zip at `path`) against the backup's `master.db` (unpacked
/// to `db`) and its music folder.
fn check_contents(
    path: &Path,
    contents: Option<Vec<u8>>,
    db: &Path,
    host: &Host,
    ctx: &Ctx<'_>,
    report: &mut Report,
) -> Result<(), RpcError> {
    let problems = &mut report.problems;
    let values = match contents.as_deref().map(contents::parse) {
        Some(Ok(values)) => values,
        Some(Err(e)) => {
            problems.push(e);
            return Ok(());
        }
        None => {
            problems.push("it has no contents.xml that reads".into());
            return Ok(());
        }
    };
    let missing: Vec<&str> = REQUIRED.into_iter().filter(|k| value(&values, k).is_none()).collect();
    if !missing.is_empty() {
        problems.push(format!("contents.xml has no {}", missing.join(", ")));
    }
    let version = value(&values, "BACKUP_RB_VER").and_then(|v| v.parse().ok()).map(contents::version_text);
    let format = value(&values, "BACKUP_FILE_VER").unwrap_or("?");
    let said = value(&values, "CONTENT_NUM").and_then(|n| n.parse::<usize>().ok());
    let db_id = value(&values, "DB_ID");
    report
        .passed
        .push(("Zip".into(), format!("every entry reads ({})", size(std::fs::metadata(path).map_or(0, |m| m.len())))));
    report
        .passed
        .push(("contents.xml".into(), format!("rekordbox {}, format {format}", version.as_deref().unwrap_or("?"))));
    let mapped = values.iter().filter(|(n, _)| list::is_track(n)).map(|(n, v)| (n.as_str(), v.as_str()));
    // Only names three levels down in the music folder are looked for: a zip saying `/../x`
    // mustn't have verify say whether a file elsewhere exists.
    let (music, outside): (Vec<(&str, &str)>, Vec<_>) = mapped.partition(|(_, v)| in_music_folder(v));
    if !outside.is_empty() {
        problems.push(format!("contents.xml names {} music files outside the music folder", outside.len()));
    }
    let misnamed =
        music.iter().filter(|(n, v)| !v.rsplit('/').next().is_some_and(|f| f.starts_with(&format!("{n}_")))).count();
    if misnamed > 0 {
        problems.push(format!("contents.xml names {misnamed} music files that aren't named after their track"));
    }
    if db.is_file() {
        match host.collection().tracks(Some(db)) {
            Ok(tracks) => {
                let live = tracks.tracks.iter().filter(|t| !t.deleted).count();
                if said != Some(live) {
                    let said = said.map_or("no number".into(), |n| n.to_string());
                    problems.push(format!("its master.db has {live} tracks and contents.xml says {said}"));
                }
                if tracks.db_id.is_some() && tracks.db_id.as_deref() != db_id {
                    problems.push("its master.db is another library than contents.xml says (DB_ID)".into());
                }
                report.passed.push(("master.db".into(), format!("opens, {live} tracks")));
            }
            Err(_) if ctx.check().is_err() => return Err(RpcError::cancelled()),
            Err(e) => problems.push(format!("its master.db doesn't open ({})", e.message)),
        }
    } else {
        problems.push("it has no master.db".into());
    }
    let folder = path.parent().unwrap_or(Path::new(".")).join(FOLDER);
    let music_line = match (music.len(), folder.is_dir()) {
        (0, _) => "not in this backup".to_string(),
        (n, false) => format!("{n} files, but {} isn't beside the zip", folder.display()),
        (n, true) => {
            let gone = music.iter().filter(|(_, v)| {
                let to = v.split('/').filter(|p| !p.is_empty()).fold(folder.clone(), |p, n| p.join(n));
                !to.is_file()
            });
            let gone = gone.count();
            if gone > 0 {
                problems.push(format!("{gone} of its {n} music files aren't in {}", folder.display()));
            }
            format!("{n} files, all in {}", folder.display())
        }
    };
    report.passed.push(("Music".into(), music_line));
    Ok(())
}

/// Whether a music file's place in `contents.xml` is in the music folder, as rekordbox writes
/// them: `/<Artist>/<Album>/<file>`, each a plain name.
fn in_music_folder(place: &str) -> bool {
    let Some(rest) = place.strip_prefix('/') else { return false };
    let names: Vec<&str> = rest.split('/').collect();
    names.len() == 3 && names.iter().all(|n| !n.is_empty() && *n != "." && *n != ".." && !n.contains([':', '\\']))
}

#[cfg(test)]
mod tests;
