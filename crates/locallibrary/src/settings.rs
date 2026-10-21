//! rekordbox "My Settings" on drives: `MYSETTING.DAT`, `MYSETTING2.DAT`, `DJMMYSETTING.DAT` and
//! `DEVSETTING.DAT` in `PIONEER/`.
//!
//! Players load these when the drive is inserted (quantize, auto cue, jog mode, language, mixer
//! curves, waveform colours...). rekordbox writes them on export; locallibrary lets you define
//! them once in a [`SettingsProfile`] and apply it to every drive, with optional per-drive
//! overrides.
//!
//! The files themselves (the schema of every setting, decoding and byte-level patching) are
//! [`loclib_mysettings`]'s, re-exported here; this module reads and writes them on drives and
//! in rekordbox's settings folder, and keeps the profile.

use indexmap::IndexMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use loclib_mysettings::{
    ApplyReport, Category, Choice, NewFiles, SCHEMA, SettingChange, SettingDef, SettingValue, SettingsBlob,
    SettingsFile, SettingsSnapshot, UndecodedByte, crc16_xmodem, find,
};

use crate::error::IoContext;
use crate::fsutil;
use crate::paths;
use crate::{Error, Result};

/// [`loclib_mysettings::validate`], failing with locallibrary's [`Error`]: an unknown key's
/// carries the action that lists the settings.
pub fn validate(key: &str, value: &str) -> Result<(&'static SettingDef, &'static Choice)> {
    Ok(loclib_mysettings::validate(key, value)?)
}

/// Read one settings file. A damaged file's error names its path.
pub fn read_blob(file: SettingsFile, path: &Path) -> Result<SettingsBlob> {
    let bytes = fsutil::read(path).at(path)?;
    SettingsBlob::parse(file, bytes).map_err(|e| match e {
        loclib_mysettings::Error::Parse { message, .. } => Error::parse(path, message),
        e => e.into(),
    })
}

/// Where each settings file lives for a given source.
pub trait SettingsLocation {
    fn settings_path(&self, file: SettingsFile) -> PathBuf;
}

/// Drive root: files live in `PIONEER/`.
pub struct DriveSettings<'a>(pub &'a Path);

impl SettingsLocation for DriveSettings<'_> {
    fn settings_path(&self, file: SettingsFile) -> PathBuf {
        paths::to_fs(self.0, file.key())
    }
}

/// A plain directory containing the `.DAT` files (e.g. rekordbox's local settings folder).
pub struct FlatSettings<'a>(pub &'a Path);

impl SettingsLocation for FlatSettings<'_> {
    fn settings_path(&self, file: SettingsFile) -> PathBuf {
        self.0.join(file.file_name())
    }
}

/// rekordbox's own settings folder on this computer (holds the "My Settings" it exports).
pub fn rekordbox_settings_dir() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    #[cfg(target_os = "macos")]
    let dir = base.home_dir().join("Library/Application Support/Pioneer/rekordbox6");
    #[cfg(windows)]
    let dir = base.config_dir().join("Pioneer").join("rekordbox6");
    #[cfg(all(unix, not(target_os = "macos")))]
    let dir = base.config_dir().join("Pioneer").join("rekordbox6");
    dir.is_dir().then_some(dir)
}

pub fn read_all(loc: &impl SettingsLocation) -> SettingsSnapshot {
    let mut snap = SettingsSnapshot::default();
    for file in SettingsFile::ALL {
        let path = loc.settings_path(file);
        if !path.is_file() {
            snap.missing_files.push(file);
            continue;
        }
        match read_blob(file, &path) {
            Ok(blob) => snap.insert(&blob),
            Err(e) => snap.problems.push(e.to_string()),
        }
    }
    snap
}

/// Make the settings at `loc` match `desired` (canonical key -> value id). Keys not in
/// `desired` keep their current value. Missing files are created only when `desired` touches
/// them, or when `create_missing` is set.
pub fn apply(
    loc: &impl SettingsLocation,
    desired: &IndexMap<String, String>,
    create_missing: bool,
    dry_run: bool,
) -> Result<ApplyReport> {
    let (mut report, files) = changes(loc, desired, create_missing)?;
    report.dry_run = dry_run;
    if !dry_run {
        for (file, bytes) in files {
            fsutil::write_atomic(&loc.settings_path(file), &bytes)?;
        }
    }
    Ok(report)
}

/// [`apply`] to a drive: the files are written through the drive's write routine, backed up
/// first so `restore` undoes it.
pub fn apply_to_drive(
    device: &mut crate::device::Device,
    desired: &IndexMap<String, String>,
    create_missing: bool,
    ctx: &crate::progress::Ctx<'_>,
) -> Result<ApplyReport> {
    let _lock = crate::commit::lock(device, None)?;
    for f in SettingsFile::ALL {
        device.checked_path(f.key())?; // PIONEER/ must not be a symlink out of the drive
    }
    let (report, files) = changes(&DriveSettings(device.root()), desired, create_missing)?;
    if files.is_empty() {
        return Ok(report);
    }
    let files = files.into_iter().map(|(f, bytes)| crate::commit::NewFile::bytes(f.key(), bytes)).collect();
    let changes = crate::commit::Changes { files, ..Default::default() };
    let mut opts = crate::commit::Options::new(crate::device::LineageOp::Edit, "write My Settings")
        .backup("before writing My Settings");
    opts.lineage = false;
    crate::commit::commit(device, changes, opts, ctx)?;
    Ok(report)
}

/// What [`apply`] would change, and the files' new bytes.
pub fn changes(
    loc: &impl SettingsLocation,
    desired: &IndexMap<String, String>,
    create_missing: bool,
) -> Result<(ApplyReport, NewFiles)> {
    loclib_mysettings::changes(
        |file| {
            let path = loc.settings_path(file);
            path.is_file().then(|| read_blob(file, &path)).transpose()
        },
        desired,
        create_missing,
    )
}

// ---------------------------------------------------------------------------------------------
// Profile: settings defined once, applied to many drives
// ---------------------------------------------------------------------------------------------

/// One setting in the settings profile's base, before and after importing.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct BaseChange {
    pub key: String,
    /// `None`: not set.
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SettingsProfile {
    /// Applied to every drive.
    #[serde(default)]
    pub base: IndexMap<String, String>,
    /// Per-drive deviations, keyed by drive id.
    #[serde(default)]
    pub drives: IndexMap<Uuid, DriveOverrides>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DriveOverrides {
    /// Drive name when the override was last edited (for display only).
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub values: IndexMap<String, String>,
}

impl SettingsProfile {
    /// Base values with the drive's overrides on top.
    pub fn effective(&self, drive: Option<Uuid>) -> IndexMap<String, String> {
        let mut out = self.base.clone();
        if let Some(o) = drive.and_then(|d| self.drives.get(&d)) {
            out.extend(o.values.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        out.sort_unstable_keys();
        out
    }

    /// Set a value in the base profile, or as an override for `drive`. Values are validated
    /// and stored in canonical form.
    pub fn set(&mut self, drive: Option<(Uuid, &str)>, key: &str, value: &str) -> Result<()> {
        let (def, choice) = validate(key, value)?;
        let (k, v) = (def.key.to_string(), choice.id.to_string());
        match drive {
            None => {
                self.base.insert_sorted(k, v);
            }
            Some((id, name)) => {
                let o = self.drives.entry(id).or_default();
                o.name = name.to_string();
                o.values.insert_sorted(k, v);
            }
        }
        Ok(())
    }

    pub fn unset(&mut self, drive: Option<Uuid>, key: &str) -> Result<bool> {
        let def = find(key).ok_or_else(|| Error::invalid(format!("unknown setting {key:?}")))?;
        Ok(match drive {
            None => self.base.shift_remove(def.key).is_some(),
            Some(id) => {
                let removed = self.drives.get_mut(&id).is_some_and(|o| o.values.shift_remove(def.key).is_some());
                self.drives.retain(|_, o| !o.values.is_empty());
                removed
            }
        })
    }

    /// Set every known value read from a source (drive or rekordbox) in the base, keeping the
    /// rest. Returns how many were set.
    pub fn import_base(&mut self, snapshot: &SettingsSnapshot) -> usize {
        let mut n = 0;
        for (k, v) in &snapshot.values {
            if let SettingValue::Known(id) = v {
                self.base.insert_sorted(k.clone(), id.clone());
                n += 1;
            }
        }
        n
    }
}

/// How a drive's current settings deviate from what the profile wants.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Drift {
    pub key: String,
    pub expected: String,
    pub actual: Option<SettingValue>,
    /// Whether the expectation comes from a per-drive override.
    pub overridden: bool,
}

pub fn drift(profile: &SettingsProfile, drive: Option<Uuid>, current: &SettingsSnapshot) -> Vec<Drift> {
    let overrides = drive.and_then(|d| profile.drives.get(&d));
    profile
        .effective(drive)
        .into_iter()
        .filter_map(|(key, expected)| {
            let actual = current.values.get(&key).cloned();
            let matches = matches!(&actual, Some(SettingValue::Known(v)) if *v == expected);
            (!matches).then(|| Drift {
                overridden: overrides.is_some_and(|o| o.values.contains_key(&key)),
                key,
                expected,
                actual,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
