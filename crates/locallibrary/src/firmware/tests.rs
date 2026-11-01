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

/// Staging an update on a drive with other files in its root: the plan lists the other update
/// files (`.UPD` in any case, not macOS's `._` shadows, not folders), flags them for a model that
/// wants one file unless they're to be replaced, and names the socket the guide asks for; applying
/// it leaves the file there whole and only the update files the plan listed gone; removing then
/// takes every update file and leaves the rest.
#[hegel::test(test_cases = 6)]
#[hegel::explicit_test_case(others = vec![0usize, 3], name = "DJM-A9_120.upd", replace = false)]
#[hegel::explicit_test_case(others = vec![0usize, 1, 2, 3, 4, 5], name = "CDJ3000Xv150.UPD", replace = true)]
#[hegel::explicit_test_case(others = Vec::<usize>::new(), name = "firmware.upd", replace = false)]
fn an_update_goes_in_alone_when_its_model_wants_that(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use std::collections::BTreeSet;
    // Files already in the root: the update files among them by index.
    const ROOT: [(&str, bool); 6] = [
        ("DJM-A9_110.upd", true),
        ("CDJ3Kv300.UPD", true),
        ("._CDJ3Kv300.UPD", false),
        ("notes.txt", false),
        ("xdjazv101.Upd", true),
        ("CDJ3000Xv150.UPD", true),
    ];
    let others: Vec<usize> =
        tc.draw(gs::vecs(gs::integers::<usize>().max_value(ROOT.len() - 1)).max_size(4).unique(true));
    let name = tc.draw(gs::sampled_from(vec![
        "DJM-A9_120.upd",
        "CDJ3Kv321.UPD",
        "CDJ3000Xv150.UPD",
        "OPUSQUADv130.UPD",
        "firmware.upd",
    ]));
    let replace = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("USB");
    std::fs::create_dir_all(drive.join("OLD.UPD")).unwrap();
    for &i in &others {
        std::fs::write(drive.join(ROOT[i].0), b"old").unwrap();
    }
    let file = dir.path().join(name);
    std::fs::write(&file, format!("firmware {name}")).unwrap();
    let mut d = Device::open(&drive).unwrap();

    let p = plan(&d, &file, replace).unwrap();
    let mut updates: Vec<&str> = others
        .iter()
        .map(|&i| ROOT[i])
        .filter(|(n, upd)| *upd && !n.eq_ignore_ascii_case(name))
        .map(|(n, _)| n)
        .collect();
    updates.sort();
    assert_eq!(p.existing, if replace { updates.clone() } else { Vec::new() });
    let model = model_for(name);
    assert!(model_for(&format!("{name}.zip")).is_none(), "only an update file has a model");
    assert_eq!(p.model.map(|m| m.model), model.map(|m| m.model));
    let one_only = model.is_some_and(|m| m.single_file) && !updates.is_empty() && !replace;
    assert_eq!(p.problems.iter().any(|m| m.contains("wants only one update file")), one_only, "{:?}", p.problems);
    assert_eq!(p.problems.iter().any(|m| m.contains("isn't named like")), model.is_none(), "{:?}", p.problems);
    assert_eq!(p.notes.iter().any(|n| n.starts_with("insert the drive")), model.is_some_and(|m| m.port.is_some()));

    let before: BTreeSet<String> =
        std::fs::read_dir(&drive).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into()).collect();
    // Refused while there are problems, unless forced; nothing changes on a refusal.
    let r = match apply(&p, &mut d, false, &Ctx::silent()) {
        Ok(r) => {
            assert!(p.problems.is_empty(), "{:?}", p.problems);
            r
        }
        Err(e) => {
            assert!(!p.problems.is_empty() && e.to_string().contains(&p.problems[0]), "{e}");
            let unchanged: BTreeSet<String> =
                std::fs::read_dir(&drive).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into()).collect();
            assert_eq!(unchanged, before);
            apply(&p, &mut d, true, &Ctx::silent()).unwrap()
        }
    };
    assert_eq!(std::fs::read(drive.join(name)).unwrap(), format!("firmware {name}").into_bytes());
    assert_eq!(r.removed, p.existing);
    let after: BTreeSet<String> =
        std::fs::read_dir(&drive).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into()).collect();
    let mut want = before.clone();
    want.retain(|n| !p.existing.contains(n));
    want.insert(name.to_string());
    assert_eq!(after.difference(&want).filter(|n| !n.starts_with('.')).count(), 0, "{after:?}");

    let removed = remove(&mut d, &Ctx::silent()).unwrap();
    let mut left = existing_updates(&drive);
    assert!(left.is_empty(), "{left:?}");
    left.extend(removed.iter().cloned());
    assert!(removed.contains(&name.to_string()) && removed.is_sorted(), "{removed:?}");
    assert!(drive.join("OLD.UPD").is_dir() && remove(&mut d, &Ctx::silent()).unwrap().is_empty());
    if others.contains(&3) {
        assert!(drive.join("notes.txt").is_file());
    }

    // What isn't a file is never planned.
    assert!(
        matches!(plan(&d, &drive.join("OLD.UPD"), replace), Err(Error::Invalid(m)) if m.ends_with("is not a file"))
    );
    assert!(matches!(plan(&d, &dir.path().join("gone.UPD"), replace), Err(Error::Io { .. })));
}

#[cfg(unix)]
use crate::format::tests::fake_tools;

/// An update's drive is judged by its disk as the system describes it: a partition table other
/// than MBR is a problem for every model, and so is a file system the model doesn't take updates
/// from, naming what it does take. When the system can't say, the plan asks to check by hand.
#[cfg(unix)]
#[hegel::test(test_cases = 20)]
fn an_update_drive_is_judged_by_its_disk(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = fake_tools::FakeTools::new();
    std::fs::create_dir(fake.dir().join("USB")).unwrap();
    let d = Device::open(fake.dir().join("USB")).unwrap();
    let name = tc.draw(gs::sampled_from(MODELS.iter().map(|m| format!("{}100.UPD", m.prefix)).collect::<Vec<_>>()));
    let file = fake.dir().join(&name);
    std::fs::write(&file, b"firmware").unwrap();
    let known = tc.draw(gs::weighted_booleans(0.85));
    let (fs, mbr) = (tc.draw(gs::sampled_from(vec!["vfat", "exfat"])), tc.draw(gs::booleans()));
    if known {
        fake.volume_at(d.root(), fs, mbr);
    }

    let p = plan(&d, &file, false).unwrap();
    let model = model_for(&name).unwrap();
    let kind = if fs == "exfat" { FsKind::Exfat } else { FsKind::Fat32 };
    let partitioned = p.problems.iter().any(|m| m.contains("isn't partitioned MBR"));
    let wrong_fs = p.problems.iter().find(|m| m.contains("takes updates from"));
    let unknown = p.notes.iter().any(|n| n.starts_with("couldn't read the drive's partition scheme"));
    assert_eq!((partitioned, unknown), (known && !mbr, !known), "{p:?}");
    match wrong_fs {
        Some(m) => {
            assert!(known && !model.filesystems.contains(&kind), "{m}");
            let takes = if model.filesystems.contains(&FsKind::Fat16) { "FAT or FAT32" } else { "FAT32" };
            assert_eq!(*m, format!("the {} takes updates from {takes} drives, and this one is {kind:?}", model.model));
        }
        None => assert!(!known || model.filesystems.contains(&kind), "{p:?}"),
    }
}
