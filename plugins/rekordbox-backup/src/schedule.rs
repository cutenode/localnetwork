//! The `schedule` service: while a host that stays open runs it (the player, the sidecar or the
//! `watch` agent), a backup without music whenever the newest in the backup folder is
//! `every_days` old, keeping the newest `keep` of those it made. It knows its own by the list it
//! keeps in its data folder, so it never removes a backup rekordbox or the user made, nor one of
//! its own that was replaced since (another size or time).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use loclib_plugin::{Host, HostNotify, RpcError, StartService};
use loclib_progress::{Cancelled, Ctx};
use serde::{Deserialize, Serialize};

use crate::backup::{self, Made};
use crate::{Settings, io, list};

/// How often it looks at how old the newest backup is.
const LOOK_EVERY: Duration = Duration::from_secs(60 * 60);

/// How long it waits at a time, between looks for a cancel.
const NAP: Duration = Duration::from_millis(200);

/// The list of its backups, in its data folder.
const MADE: &str = "made.json";

/// Runs until the host cancels it.
pub(crate) fn run(req: StartService, host: &Host) -> Result<(), RpcError> {
    let ctx = host.ctx();
    let settings = Settings::of(&req.context)?;
    let (scratch, data_dir) = (req.context.scratch.as_deref(), req.context.data_dir.as_deref());
    let mut told: Option<String> = None;
    loop {
        if let Some(every) = settings.every() {
            let message = match due(host, &settings, every, scratch, data_dir, &ctx) {
                Ok(Some(made)) => Some(format!("Backed up rekordbox's library to {}", made.zip.display())),
                Ok(None) => None,
                Err(_) if ctx.check().is_err() => return Err(RpcError::cancelled()),
                Err(e) => Some(format!("Couldn't back up rekordbox's library: {}", e.message)),
            };
            // A failure that lasts (the backup folder's drive unplugged) is told once.
            if let Some(message) = message.filter(|m| told.as_ref() != Some(m)) {
                host.notify(&HostNotify { message: message.clone(), operation: None, action: None });
                told = Some(message);
            }
        }
        wait(LOOK_EVERY, &ctx)?;
    }
}

/// Waits `time`, or until cancelled.
fn wait(time: Duration, ctx: &Ctx<'_>) -> Result<(), Cancelled> {
    let until = SystemTime::now() + time;
    while SystemTime::now() < until {
        ctx.check()?;
        std::thread::sleep(NAP);
    }
    Ok(())
}

/// A backup, when the newest in the backup folder is `every` old (or there's none), recorded in
/// `data_dir` and the oldest of its own beyond `keep` removed.
pub(crate) fn due(
    host: &Host,
    settings: &Settings,
    every: Duration,
    scratch: Option<&Path>,
    data_dir: Option<&Path>,
    ctx: &Ctx<'_>,
) -> Result<Option<Made>, RpcError> {
    if !is_due(&settings.destination()?, every) {
        return Ok(None);
    }
    let scratch = scratch.ok_or_else(|| RpcError::failed("incompatible", "no scratch folder"))?;
    let made = backup::make(host, settings, false, scratch, ctx)?;
    if let Some(dir) = data_dir {
        rotate(dir, &made.zip, settings.keep())?.into_iter().for_each(|w| ctx.warn(w));
    }
    Ok(Some(made))
}

/// Whether a backup is due: the newest in `dest` is `every` old, or there's none, and today's
/// isn't there. One of today's there already (rekordbox's, say, with `every_days` under 1) isn't
/// replaced, nor later taken for one of its own and removed.
fn is_due(dest: &Path, every: Duration) -> bool {
    let newest = list::find(dest).into_iter().filter_map(|f| f.modified).max();
    // One dated ahead (a clock set back) isn't old.
    let old = newest.is_none_or(|t| t.elapsed().is_ok_and(|age| age >= every));
    old && !backup::zip_in(dest).exists()
}

/// One of its backups, as it was when it made it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Kept {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

impl Kept {
    fn of(path: &Path) -> Option<Kept> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Kept { path: path.to_path_buf(), bytes: meta.len(), modified: meta.modified().ok()? })
    }

    /// Whether it's still the file it made.
    fn unchanged(&self) -> bool {
        Kept::of(&self.path).is_some_and(|now| now == *self)
    }
}

/// Adds `zip` to the list in `dir` and, with `keep`, removes the oldest of the list's backups
/// beyond that many, those still as it made them. One it can't remove stays listed, to try again
/// next time, and is said in what it answers.
fn rotate(dir: &Path, zip: &Path, keep: Option<usize>) -> Result<Vec<String>, RpcError> {
    let list = dir.join(MADE);
    let read = std::fs::read(&list).ok();
    let mut kept: Vec<Kept> = read.and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    // Only those still as it made them are its own: the rest it forgets.
    kept.retain(|k| k.path != zip && k.unchanged());
    kept.extend(Kept::of(zip));
    let mut failed = Vec::new();
    if let Some(keep) = keep {
        let gone = kept.len().saturating_sub(keep);
        let mut stuck = Vec::new();
        for old in kept.drain(..gone) {
            if let Err(e) = std::fs::remove_file(&old.path) {
                failed.push(format!("couldn't remove the old backup {}: {e}", old.path.display()));
                stuck.push(old);
            }
        }
        kept.splice(0..0, stuck);
    }
    let json = serde_json::to_vec_pretty(&kept).expect("the list is JSON");
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| io(dir, e))?;
    std::io::Write::write_all(&mut tmp, &json).map_err(|e| io(&list, e))?;
    tmp.persist(&list).map_err(|e| io(&list, e.error))?;
    Ok(failed)
}

#[cfg(test)]
mod tests;
