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

/// A file named otherwise is flagged (the devices look for their own name), a folder is
/// refused, and the guide's socket is passed on.
#[test]
fn update_files_are_known_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(dir.path()).unwrap();
    let err = plan(&d, dir.path(), false).unwrap_err();
    assert!(matches!(&err, Error::Invalid(m) if m.ends_with("is not a file")), "a folder: {err}");
    let renamed = dir.path().join("cdj3000x-update.upd");
    std::fs::write(&renamed, b"firmware").unwrap();
    let p = plan(&d, &renamed, false).unwrap();
    assert!(p.model.is_none());
    assert!(p.problems[0].contains("isn't named like an AlphaTheta update file"), "{:?}", p.problems);
    let file = dir.path().join("CDJ3000Xv150.UPD");
    std::fs::write(&file, b"firmware").unwrap();
    let p = plan(&d, &file, false).unwrap();
    assert!(
        p.notes.iter().any(|n| n == "insert the drive in the CDJ-3000X USB1 (Type-A) or USB2 (Type-C) socket"),
        "{:?}",
        p.notes
    );
    // Only update files in the drive's root count, and not macOS's `._` shadows of them.
    std::fs::write(dir.path().join("._CDJ3000Xv150.UPD"), b"").unwrap();
    std::fs::create_dir(dir.path().join("OLD.UPD")).unwrap();
    assert_eq!(existing_updates(dir.path()), ["CDJ3000Xv150.UPD", "cdj3000x-update.upd"]);
}

#[test]
fn removing_from_a_drive_without_updates_removes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(dir.path()).unwrap();
    assert!(remove(&mut d, &Ctx::silent()).unwrap().is_empty());
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
