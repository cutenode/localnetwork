//! Putting a player or mixer firmware update on a drive the device will accept.
//!
//! Every AlphaTheta update guide asks for one `.UPD` file, not renamed, in the drive's root, but
//! the rules differ by model (checked October 2026 against the update guides): the DJM-A9 needs
//! an MBR drive with only one update file on it, while the CDJ-3000X also accepts exFAT. The
//! mixers take FAT32 only, as the equipment table has it for their settings and firmware (see
//! [`crate::equipment`]). [`plan`] checks the file name, the drive's partition scheme and file
//! system, and other update files already there; [`apply`] copies the file (verified) to the root.
//! Firmware is never downloaded: get it from AlphaTheta's support site.
//!
//! The file goes in the drive's root, outside `PIONEER/` and `Contents/`, which is why this is
//! an explicit command.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

use crate::commit::{self, Changes, NewFile, Options};
use crate::device::{Device, LineageOp};
use crate::error::{Action, Reason};
use crate::format::{FsKind, Scheme};
use crate::fsutil;
use crate::progress::Ctx;
use crate::{Error, Result};

/// A device's update rules.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub struct FirmwareModel {
    pub model: &'static str,
    /// The update file's name starts with this (any case) and ends in `.UPD`.
    pub prefix: &'static str,
    pub filesystems: &'static [FsKind],
    /// Only one update file may be on the drive.
    pub single_file: bool,
    /// Which socket to use, where the guide says.
    pub port: Option<&'static str>,
}

const FAT: &[FsKind] = &[FsKind::Fat16, FsKind::Fat32];
const FAT_EXFAT: &[FsKind] = &[FsKind::Fat16, FsKind::Fat32, FsKind::Exfat];
/// The mixers take settings and firmware from FAT32 only (see [`crate::equipment`]).
const FAT32: &[FsKind] = &[FsKind::Fat32];

pub const MODELS: &[FirmwareModel] = &[
    FirmwareModel {
        model: "CDJ-3000X",
        prefix: "CDJ3000Xv",
        filesystems: FAT_EXFAT,
        single_file: false,
        port: Some("USB1 (Type-A) or USB2 (Type-C)"),
    },
    FirmwareModel { model: "CDJ-3000", prefix: "CDJ3Kv", filesystems: FAT, single_file: true, port: None },
    FirmwareModel { model: "XDJ-AZ", prefix: "XDJAZv", filesystems: FAT, single_file: false, port: Some("USB1") },
    FirmwareModel { model: "OPUS-QUAD", prefix: "OPUSQUADv", filesystems: FAT, single_file: false, port: Some("USB1") },
    FirmwareModel { model: "DJM-A9", prefix: "DJM-A9_", filesystems: FAT32, single_file: true, port: None },
    FirmwareModel { model: "DJM-V10", prefix: "DJM-V10_", filesystems: FAT32, single_file: false, port: None },
];

/// The model an update file is for, by its name.
pub fn model_for(file_name: &str) -> Option<&'static FirmwareModel> {
    let upper = file_name.to_ascii_uppercase();
    if !upper.ends_with(".UPD") {
        return None;
    }
    // Longest prefix first: CDJ3000Xv... is not CDJ3Kv..., but keep the rule general.
    let mut models: Vec<&FirmwareModel> = MODELS.iter().collect();
    models.sort_by_key(|m| std::cmp::Reverse(m.prefix.len()));
    models.into_iter().find(|m| upper.starts_with(&m.prefix.to_ascii_uppercase()))
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FirmwarePlan {
    pub drive: String,
    pub root: PathBuf,
    pub file: PathBuf,
    pub file_name: String,
    pub size: u64,
    pub model: Option<FirmwareModel>,
    /// Update files already in the root; removed with `replace`.
    pub existing: Vec<String>,
    /// What stops the device from taking the update. Applying refuses while there are any
    /// (unless forced).
    pub problems: Vec<String>,
    pub notes: Vec<String>,
}

/// `.UPD` files in a drive's root.
pub fn existing_updates(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(root)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| !n.starts_with("._") && n.to_ascii_uppercase().ends_with(".UPD"))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Check whether `file` can go on `device` for its model. `replace`: remove other update files.
pub fn plan(device: &Device, file: &Path, replace: bool) -> Result<FirmwarePlan> {
    let meta = std::fs::metadata(file).map_err(|e| Error::Io { path: file.to_path_buf(), source: e })?;
    if !meta.is_file() {
        return Err(Error::invalid(format!("{} is not a file", file.display())));
    }
    let file_name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let model = model_for(&file_name).copied();
    let mut problems = Vec::new();
    let mut notes = Vec::new();
    match &model {
        None => problems.push(format!(
            "{file_name:?} isn't named like an AlphaTheta update file this knows ({}); keep the name the download had",
            MODELS.iter().map(|m| format!("{}***.UPD", m.prefix)).collect::<Vec<_>>().join(", ")
        )),
        Some(m) => {
            if let Some(p) = m.port {
                notes.push(format!("insert the drive in the {} {} socket", m.model, p));
            }
        }
    }
    let existing: Vec<String> =
        existing_updates(device.root()).into_iter().filter(|n| !n.eq_ignore_ascii_case(&file_name)).collect();
    if model.is_some_and(|m| m.single_file) && !existing.is_empty() && !replace {
        problems.push(format!(
            "the {} wants only one update file on the drive, and it holds {}; replace them",
            model.unwrap().model,
            existing.join(", ")
        ));
    }
    match crate::format::disk_at(device.root()) {
        Ok(disk) => {
            let volume = disk.volumes.iter().find(|v| v.mount.as_deref() == Some(device.root()));
            if disk.scheme != Some(Scheme::Mbr) {
                problems.push(
                    "the drive isn't partitioned MBR (players don't read GPT drives); formatting it fixes that".into(),
                );
            }
            if let (Some(m), Some(fs)) = (&model, volume.and_then(|v| v.filesystem))
                && !m.filesystems.contains(&fs)
            {
                let takes = match (m.filesystems.contains(&FsKind::Fat16), m.filesystems.contains(&FsKind::Exfat)) {
                    (_, true) => "FAT, FAT32 or exFAT",
                    (true, false) => "FAT or FAT32",
                    (false, false) => "FAT32",
                };
                problems.push(format!("the {} takes updates from {takes} drives, and this one is {fs:?}", m.model));
            }
        }
        Err(_) => notes.push("couldn't read the drive's partition scheme and file system; check them yourself".into()),
    }
    if let Some(free) = fsutil::available_space(device.root())
        && free < meta.len()
    {
        problems.push(format!("not enough free space ({free} bytes free, {} needed)", meta.len()));
    }
    Ok(FirmwarePlan {
        drive: device.name(),
        root: device.root().to_path_buf(),
        file: file.to_path_buf(),
        file_name,
        size: meta.len(),
        model,
        existing: if replace { existing } else { Vec::new() },
        problems,
        notes,
    })
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FirmwareReport {
    pub written: PathBuf,
    pub removed: Vec<String>,
}

/// Copy the update file to the drive's root (removing the other update files the plan lists).
/// Nothing is backed up: update files can be downloaded again.
pub fn apply(plan: &FirmwarePlan, device: &mut Device, force: bool, ctx: &Ctx<'_>) -> Result<FirmwareReport> {
    if !plan.problems.is_empty() && !force {
        return Err(Error::conflict(Reason::FirmwareProblems, plan.problems.join("; "))
            .with("problems", &plan.problems)
            .action(Action::retry(serde_json::json!({ "force": true }), "Stage it anyway")));
    }
    let _lock = commit::lock(device, None)?;
    let present: Vec<String> = plan.existing.iter().filter(|n| device.root().join(n).is_file()).cloned().collect();
    let changes = Changes {
        files: vec![NewFile::copy(plan.file_name.clone(), plan.file.clone(), plan.size, None)],
        deletes: present.clone(),
        ..Default::default()
    };
    let mut opts = Options::new(LineageOp::Edit, format!("copy the update {}", plan.file_name));
    opts.lineage = false;
    commit::commit(device, changes, opts, ctx)?;
    Ok(FirmwareReport { written: device.root().join(&plan.file_name), removed: present })
}

/// Remove every update file from a drive's root (after updating).
pub fn remove(device: &mut Device, ctx: &Ctx<'_>) -> Result<Vec<String>> {
    let _lock = commit::lock(device, None)?;
    let removed = existing_updates(device.root());
    if removed.is_empty() {
        return Ok(removed);
    }
    let changes = Changes { deletes: removed.clone(), ..Default::default() };
    let mut opts = Options::new(LineageOp::Edit, "remove update files");
    opts.lineage = false;
    commit::commit(device, changes, opts, ctx)?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mixers take updates from what the equipment table says they read settings and
    /// firmware from.
    #[test]
    fn mixer_updates_agree_with_the_equipment_table() {
        for m in MODELS {
            let Some(e) = crate::equipment::find(m.model).filter(|e| e.is_mixer()) else { continue };
            for fs in [FsKind::Fat16, FsKind::Fat32, FsKind::Exfat, FsKind::Hfsplus] {
                assert_eq!(m.filesystems.contains(&fs), e.reads(fs) == Some(true), "{} on {fs:?}", m.model);
            }
        }
    }

    #[test]
    fn knows_models_by_file_name() {
        assert_eq!(model_for("CDJ3000Xv150.UPD").unwrap().model, "CDJ-3000X");
        assert_eq!(model_for("cdj3kv321.upd").unwrap().model, "CDJ-3000");
        assert_eq!(model_for("DJM-A9_120.upd").unwrap().model, "DJM-A9");
        assert_eq!(model_for("OPUSQUADv130.UPD").unwrap().model, "OPUS-QUAD");
        assert!(model_for("CDJ3000Xv150.zip").is_none());
        assert!(model_for("firmware.UPD").is_none());
    }

    #[test]
    fn stages_and_removes_update_files() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("USB");
        std::fs::create_dir_all(&drive).unwrap();
        std::fs::write(drive.join("DJM-A9_110.upd"), b"old").unwrap();
        let file = dir.path().join("DJM-A9_120.upd");
        std::fs::write(&file, b"new firmware").unwrap();
        let mut d = Device::open(&drive).unwrap();

        let p = plan(&d, &file, false).unwrap();
        assert_eq!(p.model.unwrap().model, "DJM-A9");
        assert!(p.problems.iter().any(|m| m.contains("only one update file")), "{:?}", p.problems);
        assert!(apply(&p, &mut d, false, &Ctx::silent()).is_err());

        let p = plan(&d, &file, true).unwrap();
        // A temp dir isn't a disk the system can describe, so only the file rules apply.
        assert!(p.problems.iter().all(|m| !m.contains("only one")), "{:?}", p.problems);
        let r = apply(&p, &mut d, true, &Ctx::silent()).unwrap();
        assert_eq!(r.removed, ["DJM-A9_110.upd"]);
        assert_eq!(std::fs::read(drive.join("DJM-A9_120.upd")).unwrap(), b"new firmware");
        assert_eq!(remove(&mut d, &Ctx::silent()).unwrap(), ["DJM-A9_120.upd"]);
        assert!(existing_updates(&drive).is_empty());
    }
}
