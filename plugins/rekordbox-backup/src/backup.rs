//! `backup`: a backup of rekordbox's library as rekordbox's Backup Library writes one. Its plan
//! (the dry run) reads rekordbox's own files and says what would be written; the backup itself
//! works from a copy of `master.db` taken at one moment, so the tracks it lists and the database
//! it holds agree, whatever rekordbox does meanwhile.

use std::path::{Path, PathBuf};

use loclib_plugin::{
    CollectionLocations, CollectionTracks, CommandPlan, ExportedFile, Host, Pair, RpcError, RunCommand, View,
};
use loclib_progress::{Ctx, WarningLimiter};
use serde_json::{Value, json};

use crate::contents::{self, Contents};
use crate::entries::{self, Body, Entry, Folders};
use crate::music::{self, Copied};
use crate::write::{self, today};
use crate::{Credentials, RESTORE, Settings, list, size, view};

/// The warnings of a kind a plan shows before it sums up the rest.
const SHOWN: usize = 5;

/// What a backup was made of: the library, as the host says where it is.
struct Library {
    at: CollectionLocations,
    running: bool,
    rb_version: u32,
    /// The backup folder, and the zip's place there.
    dest: PathBuf,
    zip: PathBuf,
}

impl Library {
    fn folders(&self) -> Folders<'_> {
        Folders::of(&self.at).expect("checked when read")
    }
}

/// Where the library is, and where its backup goes today.
fn library(host: &Host, settings: &Settings) -> Result<Library, RpcError> {
    let dest = settings.destination()?;
    let collection = host.collection();
    let at = collection.locations()?;
    let master_db = at.master_db.as_deref().filter(|_| at.master_db_exists);
    if master_db.is_none() || Folders::of(&at).is_none() {
        return Err(RpcError::failed("not_found", "rekordbox's library isn't on this computer"));
    }
    let status = collection.status()?;
    let Some(rb_version) = status.version.as_deref().and_then(contents::rb_version) else {
        let why = "can't tell which version of rekordbox this is, which a backup records for its restore";
        return Err(RpcError::failed("not_found", why));
    };
    let zip = zip_in(&dest);
    Ok(Library { at, running: status.running, rb_version, dest, zip })
}

/// Today's backup in the backup folder `dest`, as rekordbox names it.
pub(crate) fn zip_in(dest: &Path) -> PathBuf {
    dest.join(format!("rekordbox_bak_{}.zip", today()))
}

/// `backup`: its plan for a dry run, else the backup made, as a summary.
pub(crate) fn command(req: &RunCommand, host: &Host) -> Result<Value, RpcError> {
    let settings = Settings::of(&req.context)?;
    if req.dry_run {
        return Ok(plan(host, &settings)?.into());
    }
    let scratch =
        req.context.scratch.as_deref().ok_or_else(|| RpcError::failed("incompatible", "no scratch folder"))?;
    let made = make(host, &settings, settings.music, scratch, &host.ctx())?;
    Ok(view(made.view(&settings)))
}

/// What a backup would write: the zip (its size estimated, before compression) and each music
/// file to copy, with what the user should know first.
pub(crate) fn plan(host: &Host, settings: &Settings) -> Result<CommandPlan, RpcError> {
    let lib = library(host, settings)?;
    let tracks = host.collection().tracks(None)?;
    let strip = settings.credentials == Credentials::Strip;
    let master_db = lib.at.master_db.as_deref().expect("checked when read");
    let (entries, mut left_out) = entries::gather(&lib.folders(), master_db, &tracks.tracks, strip).map_err(invalid)?;
    let estimate: u64 = entries.iter().map(|e| e.body.len()).sum();
    let mut files = vec![ExportedFile {
        path: lib.zip.clone(),
        what: "rekordbox library backup (its size before compression)".into(),
        bytes: estimate,
    }];
    let mut warnings = Vec::new();
    if lib.running {
        warnings.push("rekordbox is running: what it changes after the backup starts isn't in it".into());
    }
    if lib.zip.exists() {
        warnings.push(format!("{} is there already, and is replaced", lib.zip.display()));
    }
    if !strip {
        let why = "the backup holds your rekordbox and Dropbox sign-in tokens, as rekordbox's own does: keep it \
private, or set credentials to strip";
        warnings.push(why.into());
    }
    let live = tracks.tracks.iter().filter(|t| !t.deleted).count();
    let mut summary = format!("Back up rekordbox's library ({live} tracks) to {}", lib.zip.display());
    if settings.music {
        let (copies, missing) = music::copies(&tracks.tracks);
        let root = lib.dest.join(music::FOLDER);
        let to_copy: Vec<_> = copies.iter().filter(|c| !c.there(&c.to(&root))).collect();
        summary += &format!(", and copy {} music files into {}", to_copy.len(), root.display());
        if to_copy.len() < copies.len() {
            summary += &format!(" ({} are there already)", copies.len() - to_copy.len());
        }
        let copied = to_copy.iter().map(|c| ExportedFile { path: c.to(&root), what: "music".into(), bytes: c.bytes });
        files.extend(copied);
        left_out.extend(missing);
    }
    let shown = left_out.len().min(SHOWN);
    warnings.extend(left_out.drain(..shown));
    if !left_out.is_empty() {
        warnings.push(format!("... and {} more left out", left_out.len()));
    }
    Ok(CommandPlan { summary, warnings, files })
}

/// A backup made.
#[derive(Debug)]
pub(crate) struct Made {
    pub zip: PathBuf,
    pub bytes: u64,
    pub entries: usize,
    pub tracks: usize,
    /// The music copied, when it was asked for.
    pub music: Option<Copied>,
}

impl Made {
    fn view(&self, settings: &Settings) -> View {
        let pair = |label: &str, value: Value| Pair { label: label.into(), value };
        let music = match &self.music {
            Some(m) if m.failed.is_empty() => format!("{} copied, {} there already", m.copied, m.kept),
            Some(m) => format!("{} copied, {} there already, {} failed", m.copied, m.kept, m.failed.len()),
            None => "not copied (the music setting)".into(),
        };
        let tokens = match settings.credentials {
            Credentials::Keep => "kept, as rekordbox keeps them",
            Credentials::Strip => "stripped: a restore signs in again",
        };
        let pairs = vec![
            pair("Backup", json!(self.zip.display().to_string())),
            pair("Size", json!(size(self.bytes))),
            pair("Files", json!(self.entries)),
            pair("Tracks", json!(self.tracks)),
            pair("Music", json!(music)),
            pair("Sign-in tokens", json!(tokens)),
            pair("Restoring", json!(RESTORE)),
        ];
        View::Kv { title: Some("rekordbox library backed up".into()), pairs }
    }
}

/// Makes a backup: a copy of `master.db` into `scratch` (removed again after), with `music` the
/// music folder, then the zip.
pub(crate) fn make(
    host: &Host,
    settings: &Settings,
    music: bool,
    scratch: &Path,
    ctx: &Ctx<'_>,
) -> Result<Made, RpcError> {
    let lib = library(host, settings)?;
    let mut warn = WarningLimiter::new(ctx, SHOWN);
    if lib.running {
        warn.warn("running", "rekordbox is running: what it changes from now on isn't in the backup");
    }
    let copy = scratch.join("master.db");
    // A service's scratch folder outlives one backup, and a copy is a new file.
    let _ = std::fs::remove_file(&copy);
    let made = make_from(host, settings, music, &lib, &copy, ctx, &mut warn);
    let _ = std::fs::remove_file(&copy);
    warn.finish();
    made
}

fn make_from(
    host: &Host,
    settings: &Settings,
    music: bool,
    lib: &Library,
    copy: &Path,
    ctx: &Ctx<'_>,
    warn: &mut WarningLimiter<'_, '_>,
) -> Result<Made, RpcError> {
    let strip = settings.credentials == Credentials::Strip;
    let collection = host.collection();
    let copied = collection.copy(copy, !strip)?;
    let tracks = collection.tracks(Some(copy))?;
    let (mut entries, left_out) = entries::gather(&lib.folders(), copy, &tracks.tracks, strip).map_err(invalid)?;
    left_out.into_iter().for_each(|w| warn.warn("left out", w));
    let (copies, missing) = match music {
        true => music::copies(&tracks.tracks),
        false => Default::default(),
    };
    missing.into_iter().for_each(|w| warn.warn("music", w));
    // The music first and the zip last: a cancel or a failure leaves the day's zip as it was, and
    // contents.xml maps only the copies that are there.
    let music = match music {
        true => Some(music::copy_all(&copies, &lib.dest.join(music::FOLDER), ctx)?),
        false => None,
    };
    let failed: Vec<&str> = music.iter().flat_map(|m| &m.failed).map(|(hex, _)| hex.as_str()).collect();
    for (hex, why) in music.iter().flat_map(|m| &m.failed) {
        warn.warn("music", format!("track {hex}'s music wasn't copied, so the backup doesn't map it: {why}"));
    }
    let there: Vec<music::Copy> = copies.into_iter().filter(|c| !failed.contains(&c.hex.as_str())).collect();
    let db_id = copied.db_id.or_else(|| tracks.db_id.clone()).unwrap_or_default();
    let contents = contents(lib, &tracks, &db_id, &there)?;
    entries.push(Entry { name: "contents.xml".into(), body: Body::Made(contents.to_xml()) });
    let bytes = write::write(&entries, &lib.zip, ctx)?;
    Ok(Made { zip: lib.zip.clone(), bytes, entries: entries.len(), tracks: contents.content_num, music })
}

/// The backup's `contents.xml`, for the tracks rekordbox hasn't deleted and the music `copies`.
fn contents(
    lib: &Library,
    tracks: &CollectionTracks,
    db_id: &str,
    copies: &[music::Copy],
) -> Result<Contents, RpcError> {
    let folders = contents::Folders::here()
        .ok_or_else(|| RpcError::failed("not_found", "can't tell where this account's home folder is"))?;
    let live: Vec<_> = tracks.tracks.iter().filter(|t| !t.deleted).collect();
    let directories = contents::directories(live.iter().map(|t| t.path.as_str()), &folders);
    Ok(Contents {
        roots: list::earlier_roots(&lib.dest, db_id, &directories),
        tracks: copies.iter().map(|c| (c.hex.clone(), c.name.clone())).collect(),
        db_id: db_id.into(),
        content_num: live.len(),
        rb_version: lib.rb_version,
        directories,
        folders,
    })
}

fn invalid(message: String) -> RpcError {
    RpcError::failed("invalid", message)
}
