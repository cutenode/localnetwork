//! A set of settings files (a drive's PIONEER folder, or rekordbox's local settings folder): what
//! they hold, and what making them match a set of values would change.

use indexmap::IndexMap;
use serde::Serialize;

use crate::{Choice, Error, SCHEMA, SettingDef, SettingValue, SettingsBlob, SettingsFile, validate};

#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SettingsSnapshot {
    pub values: IndexMap<String, SettingValue>,
    pub missing_files: Vec<SettingsFile>,
    pub problems: Vec<String>,
    /// Bytes outside every known setting that differ from what rekordbox 6 writes: settings newer
    /// rekordbox versions added (rekordbox 7.2 writes six). Preserved by `apply`, but not managed
    /// by profiles until their meaning is known.
    pub undecoded: Vec<UndecodedByte>,
}

impl SettingsSnapshot {
    /// Add one file's values, its undecoded bytes, and a problem if its checksum is wrong.
    /// Missing files and files that can't be read are the caller's to note.
    pub fn insert(&mut self, blob: &SettingsBlob) {
        let file = blob.file;
        if !blob.checksum_ok() {
            self.problems.push(format!("{}: checksum mismatch", file.file_name()));
        }
        for def in SCHEMA.iter().filter(|d| d.file == file) {
            self.values.insert(def.key.to_string(), blob.get(def));
        }
        let known: Vec<usize> = SCHEMA.iter().filter(|d| d.file == file).map(|d| d.offset).collect();
        let defaults = file.default_data();
        for (offset, (&value, &expected)) in blob.data().iter().zip(&defaults).enumerate() {
            if value != expected && !known.contains(&offset) {
                self.undecoded.push(UndecodedByte { file, offset, value, expected });
            }
        }
    }
}

/// A data byte of a settings file that no known setting covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct UndecodedByte {
    pub file: SettingsFile,
    /// Offset in the file's data section (after the 104-byte header).
    pub offset: usize,
    pub value: u8,
    /// What rekordbox 6 writes there.
    pub expected: u8,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SettingChange {
    pub key: String,
    pub file: SettingsFile,
    /// `None` when the file did not exist yet.
    pub from: Option<SettingValue>,
    pub to: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ApplyReport {
    pub changes: Vec<SettingChange>,
    pub files_written: Vec<SettingsFile>,
    pub files_created: Vec<SettingsFile>,
    pub dry_run: bool,
}

/// Settings files' new bytes.
pub type NewFiles = Vec<(SettingsFile, Vec<u8>)>;

/// What making a set of settings files match `desired` (setting key -> value, in any form
/// [`validate`] takes) would change, and the files' new bytes. Keys not in `desired` keep their
/// current value. `existing` reads a file's current contents (`None` when there's no such file);
/// missing files are created only when `desired` touches them, or when `create_missing` is set.
/// A stale checksum is repaired even if no value changed.
pub fn changes<E: From<Error>>(
    mut existing: impl FnMut(SettingsFile) -> Result<Option<SettingsBlob>, E>,
    desired: &IndexMap<String, String>,
    create_missing: bool,
) -> Result<(ApplyReport, NewFiles), E> {
    let mut report = ApplyReport::default();
    let mut files = Vec::new();
    for file in SettingsFile::ALL {
        let wanted: Vec<(&SettingDef, &Choice)> = desired
            .iter()
            .map(|(k, v)| validate(k, v))
            .collect::<Result<Vec<_>, Error>>()?
            .into_iter()
            .filter(|(d, _)| d.file == file)
            .collect();
        let existing = existing(file)?;
        if existing.is_none() && wanted.is_empty() && !create_missing {
            continue;
        }
        let created = existing.is_none();
        let mut blob = existing.clone().unwrap_or_else(|| SettingsBlob::default_for(file));
        for (def, choice) in wanted {
            let before = existing.as_ref().map(|b| b.get(def));
            if blob.set(def, choice) || created {
                report.changes.push(SettingChange { key: def.key.into(), file, from: before, to: choice.id.into() });
            }
        }
        // Repair a stale checksum even if no value changed.
        let repaired = !created && existing.as_ref().is_some_and(|b| !b.checksum_ok());
        if repaired {
            blob.update_checksum();
        }
        if created || existing.as_ref() != Some(&blob) {
            files.push((file, blob.as_bytes().to_vec()));
            if created {
                report.files_created.push(file);
            }
            report.files_written.push(file);
        }
    }
    Ok((report, files))
}

#[cfg(test)]
mod tests;
