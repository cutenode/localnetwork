//! What players show for a drive: its Device Name and background colour, and an "if found"
//! contact.
//!
//! Players show the Device Name set in rekordbox, not the volume label; a CDJ-3000 shows "NO
//! DEVICE NAME" until one is set. With identical sticks in two players, the name and colour are
//! how DJs tell them apart over Pro DJ Link. rekordbox keeps them in two places on the drive:
//!
//! * OneLibrary's `property` table (`deviceName`, `backGroundColorType`), which the newer
//!   players read;
//! * `PIONEER/DeviceLibBackup/rbDevLibBaInfo_*.json` (`device_name`, `background_color`,
//!   `background_color_libplus`), rekordbox's own record of the device, which locallibrary never
//!   copies between drives.
//!
//! Where players reading `export.pdb` find the name isn't publicly documented. [`set`] writes
//! OneLibrary's values; rekordbox's next export to the drive writes the values rekordbox has for
//! the device again.
//!
//! Because the name and colour belong to the drive, not to the library, OneLibrary's content
//! fingerprint leaves them out (see [`crate::onelibrary::content_hash`]), and a sync, push or pack
//! that replaces `exportLibrary.db` puts the target's own values back ([`stash`], [`preserve`]),
//! even when it finishes a run that was cut off. [`set`] also remembers them in the drive's
//! identity (`.locallibrary/state.json`), so `locallibrary refresh` can put them back after
//! rekordbox's next export.

use std::path::Path;

use rusqlite::OptionalExtension;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::commit::{self, Changes, NewDatabases, NewFile, Options};
use crate::device::{Device, LineageOp};
use crate::error::{Action, Reason};
use crate::onelibrary::{self, OneLibrary};
use crate::paths;
use crate::progress::Ctx;
use crate::{Error, Result};

/// `property` columns that describe the drive rather than the library.
pub const PER_DRIVE_COLUMNS: [&str; 2] = ["deviceName", "backGroundColorType"];

/// Longest name accepted (what players display of a longer one is untested).
pub const MAX_NAME: usize = 32;

/// The name and colour code in a OneLibrary database.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct DbIdentity {
    pub name: Option<String>,
    /// rekordbox's background colour code (0: none).
    pub colour: Option<i64>,
}

/// rekordbox's record of the device.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct RekordboxDevice {
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub device_name: String,
    #[serde(default)]
    pub background_color: String,
    #[serde(default)]
    pub background_color_libplus: String,
    #[serde(default)]
    pub device_filesystem: String,
    #[serde(default)]
    pub backup_pc_name: String,
    #[serde(default)]
    pub backup_date: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlayerIdentity {
    pub drive: String,
    /// From OneLibrary (`None`: the drive has no readable OneLibrary database).
    pub onelibrary: Option<DbIdentity>,
    /// rekordbox's own record, if rekordbox set the drive up.
    pub rekordbox: Option<RekordboxDevice>,
}

fn db_identity_of(one: &OneLibrary) -> Result<DbIdentity> {
    let row = one
        .connection()
        .query_row("SELECT deviceName, backGroundColorType FROM property LIMIT 1", [], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<i64>>(1)?))
        })
        .optional()?;
    Ok(match row {
        Some((name, colour)) => DbIdentity { name: name.filter(|n| !n.is_empty()), colour },
        None => DbIdentity::default(),
    })
}

/// The name and colour in the OneLibrary database below `root`, if it has one.
pub fn db_identity(root: &Path) -> Option<DbIdentity> {
    if !paths::to_fs(root, paths::LIBRARY_PLUS_DB).is_file() {
        return None;
    }
    OneLibrary::open_root(root).ok().and_then(|one| db_identity_of(&one).ok())
}

/// rekordbox's `DeviceLibBackup` record.
pub fn rekordbox_device(root: &Path) -> Option<RekordboxDevice> {
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        info: Vec<RekordboxDevice>,
    }
    let dir = paths::to_fs(root, "PIONEER/DeviceLibBackup");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("rbDevLibBaInfo_")))
        .collect();
    files.sort();
    files.into_iter().find_map(|p| {
        let f: File = serde_json::from_slice(&std::fs::read(p).ok()?).ok()?;
        f.info.into_iter().next()
    })
}

pub fn read(device: &Device) -> PlayerIdentity {
    PlayerIdentity {
        drive: device.name(),
        onelibrary: db_identity(device.root()),
        rekordbox: rekordbox_device(device.root()),
    }
}

/// Write the name and colour into a drive's OneLibrary database. A backup is taken before the
/// database is replaced (`locallibrary undo` takes it back).
pub fn set(device: &mut Device, name: Option<&str>, colour: Option<i64>) -> Result<PlayerIdentity> {
    check(device, name, colour)?;
    let _lock = commit::lock(device, None)?;
    // Edited and read back in memory first: a database that can't take the change leaves no
    // backup and nothing marked as interrupted.
    let want = DbIdentity { name: name.map(|n| n.trim().to_string()), colour };
    let bytes = with_identity(&OneLibrary::open(device)?, &want, true)?;
    let changes = Changes { databases: NewDatabases { plus: Some(bytes), ..Default::default() }, ..Default::default() };
    let mut opts = Options::new(LineageOp::Edit, "set the player name and colour").backup("player name and colour");
    // The name and colour aren't part of the library's content (its fingerprint leaves them out),
    // so the drive's lineage has nothing new to record.
    opts.lineage = false;
    commit::commit(device, changes, opts, &Ctx::silent())?;
    let now = read(device);
    // Remembered on the drive, for `refresh` to put back after rekordbox's next export.
    if device.is_adopted() {
        device.state_mut()?.player = now.onelibrary.clone();
        device.save_state()?;
    }
    Ok(now)
}

/// After `undo` put a backup back on a drive that remembers a name and colour: when that changed
/// the ones it shows (`shown`, read with [`db_identity`] before), as undoing [`set`] does, remember
/// the ones it shows now instead, or `refresh` would set the undone ones again. A drive left with
/// neither remembers nothing.
pub fn after_undo(device: &mut Device, shown: Option<DbIdentity>) -> Result<()> {
    if device.state().is_none_or(|s| s.player.is_none()) {
        return Ok(());
    }
    let now = db_identity(device.root());
    if now == shown {
        return Ok(());
    }
    device.state_mut()?.player = now.filter(|n| n.name.is_some() || n.colour.is_some_and(|c| c != 0));
    device.save_state()
}

/// Whether [`set`] can give a drive this name and colour.
pub fn check(device: &Device, name: Option<&str>, colour: Option<i64>) -> Result<()> {
    if name.is_none() && colour.is_none() {
        return Err(Error::invalid("give a name, a colour, or both"));
    }
    if let Some(n) = name {
        let n = n.trim();
        if n.is_empty() || n.chars().count() > MAX_NAME || n.chars().any(char::is_control) {
            return Err(Error::invalid(format!("a player name is 1 to {MAX_NAME} printable characters")));
        }
    }
    if let Some(c) = colour
        && !(0..=8).contains(&c)
    {
        return Err(Error::invalid("the colour is a rekordbox colour code from 0 (none) to 8"));
    }
    if !device.has_library_plus() {
        let drive = device.root().display().to_string();
        return Err(Error::invalid(format!(
            "{} has no OneLibrary database (exportLibrary.db), where the name is kept for newer players; \
             converting the drive adds one. Older players read the name from a place that isn't documented: set it in rekordbox",
            device.name()
        ))
        .because(Reason::NoOneLibrary)
        .with("drive", &drive)
        .action(Action::call(
            "convert.plan",
            serde_json::json!({ "drive": drive, "to": "one_library" }),
            "Add a OneLibrary database",
        )));
    }
    Ok(())
}

/// The database `one` with the values in `want` set (fields that are `None` stay as they are;
/// with `keep_others` false, they are cleared), encrypted and read back.
fn with_identity(one: &OneLibrary, want: &DbIdentity, keep_others: bool) -> Result<Vec<u8>> {
    let current = db_identity_of(one)?;
    let name = if keep_others { want.name.clone().or(current.name) } else { want.name.clone() };
    let colour = if keep_others { want.colour.or(current.colour) } else { want.colour };
    write_identity(one, &DbIdentity { name, colour })?;
    let bytes = one.to_encrypted()?;
    if db_identity_of(&OneLibrary::from_encrypted(&bytes, None)?)? != db_identity_of(one)? {
        return Err(Error::OneLibrary("the edited database failed verification; drive left unchanged".into()));
    }
    Ok(bytes)
}

fn write_identity(one: &OneLibrary, identity: &DbIdentity) -> Result<()> {
    one.connection().execute(
        "UPDATE property SET deviceName = ?1, backGroundColorType = ?2",
        rusqlite::params![identity.name.clone().unwrap_or_default(), identity.colour.unwrap_or(0)],
    )?;
    Ok(())
}

/// Give `to`, a OneLibrary database built to replace `from`, the name and colour `from` has.
pub(crate) fn keep_identity(from: &OneLibrary, to: &OneLibrary) -> Result<()> {
    write_identity(to, &db_identity_of(from)?)
}

/// The name and colour to put back with [`preserve`] once a sync, push or pack (marked with
/// [`Device::begin`]) that `replaces` `device`'s `exportLibrary.db` is done. They're kept in the
/// drive's pending marker, so a run cut off before then (after taking the database off first, or
/// after swapping the new one in) leaves them for the run that finishes the job: the drive's
/// database may no longer hold them, and that run may have no database left to replace.
pub fn stash(device: &mut Device, replaces: bool) -> Result<Option<DbIdentity>> {
    let kept = device.state().and_then(|s| s.pending.as_ref()).and_then(|p| p.player.clone());
    if kept.is_some() || !replaces {
        return Ok(kept);
    }
    let now = db_identity(device.root());
    if now.is_some()
        && let Some(p) = device.state_mut()?.pending.as_mut()
    {
        p.player.clone_from(&now);
        device.save_state()?;
    }
    Ok(now)
}

/// After a sync, push or pack replaced `exportLibrary.db`, put back the name and colour the
/// target had (`before`, from [`stash`]). The caller holds the drive's lock from before it read
/// them until this is done, so no other write comes between. Returns whether anything was
/// written.
pub fn preserve(device: &Device, before: Option<&DbIdentity>) -> Result<bool> {
    let Some(before) = before else { return Ok(false) };
    let Some(now) = db_identity(device.root()) else { return Ok(false) };
    if &now == before {
        return Ok(false);
    }
    onelibrary::write(device, &with_identity(&OneLibrary::open(device)?, before, false)?)?;
    Ok(true)
}

// ---------------------------------------------------------------------------------------------
// "If found"
// ---------------------------------------------------------------------------------------------

pub const CONTACT_FILE: &str = "IF FOUND.txt";

/// The playlist name that carries a contact (playlist paths split on `/`, so it's replaced).
pub fn contact_playlist(contact: &str) -> String {
    format!("IF FOUND: {}", contact.trim().replace('/', "-"))
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ContactReport {
    pub file: std::path::PathBuf,
    pub playlist: Option<String>,
}

/// The edit making the contact's playlist `name` (see [`contact_playlist`]).
pub(crate) fn contact_edit(name: &str) -> crate::edit::Edit {
    crate::edit::Edit::CreatePlaylist { path: name.into(), folder: false }
}

/// Put the owner's contact where a finder will see it: a text file in the drive's root and, with
/// `playlist`, a top-level playlist named after it (the first thing on the player's browse
/// screen). The playlist is an ordinary edit, recorded so `edits replay` restores it after
/// rekordbox's next export.
pub fn set_contact(device: &mut Device, contact: &str, playlist: bool, ctx: &Ctx<'_>) -> Result<ContactReport> {
    let contact = contact.trim();
    if contact.is_empty() || contact.chars().any(char::is_control) {
        return Err(Error::invalid("give an email address, phone number or name to contact"));
    }
    let text = format!(
        "This USB drive belongs to a DJ. If you found it, please get in touch:\r\n\r\n    {contact}\r\n\r\nThank you!\r\n"
    );
    let file = device.root().join(CONTACT_FILE);
    {
        let _lock = commit::lock(device, None)?;
        let changes = Changes { files: vec![NewFile::bytes(CONTACT_FILE, text.into_bytes())], ..Default::default() };
        let mut opts = Options::new(LineageOp::Edit, "write the contact").backup("before writing the contact");
        opts.lineage = false;
        commit::commit(device, changes, opts, ctx)?;
    }
    let playlist = if playlist && device.has_export() {
        let name = contact_playlist(contact);
        let edits = [contact_edit(&name)];
        let plan = crate::edit::plan(device, &edits, &Default::default(), ctx)?;
        crate::edit::apply(&plan, device, ctx)?;
        Some(name)
    } else {
        None
    };
    Ok(ContactReport { file, playlist })
}

#[cfg(test)]
mod tests;
