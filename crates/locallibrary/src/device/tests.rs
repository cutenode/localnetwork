use super::*;

/// Regression: on Linux, `/media/<user>`, where udisks mounts drives, was a blank volume in
/// `/media`.
#[test]
fn mount_folders_are_not_volumes() {
    let dir = tempfile::tempdir().unwrap();
    let (media, user) = (dir.path().join("media"), dir.path().join("media/sam"));
    for v in [user.join("STICK"), media.join("usb0")] {
        std::fs::create_dir_all(v).unwrap();
    }
    let mut found = in_mount_folders(&[user.clone(), media.clone()]);
    found.sort();
    assert_eq!(found, [user.join("STICK"), media.join("usb0")]);
}

fn entry(fp: u8) -> LineageEntry {
    LineageEntry {
        fingerprint: Hash::of(&[fp]),
        at: Utc::now(),
        op: LineageOp::Observed,
        snapshot: None,
        source: None,
        tracks: 0,
    }
}

#[test]
fn lineage_dedupes_head_and_caps() {
    let mut s = DeviceState::new("x");
    s.record(entry(1));
    s.record(entry(1));
    assert_eq!(s.lineage.len(), 1);
    for i in 0..=255u8 {
        s.record(entry(i));
    }
    // 1 + 256 entries recorded; the oldest one is dropped.
    assert_eq!(s.lineage.len(), MAX_LINEAGE);
    assert_eq!(s.lineage[0].fingerprint, Hash::of(&[0]));
    assert_eq!(s.head().unwrap().fingerprint, Hash::of(&[255]));
}

#[test]
fn parses_rekordbox_playlist_sync() {
    let xml = "<?xml version=\"1.0\"?>\r\n<Sync DBID=\"-1940252023\" AutomaticSync=\"1\" AllPlaylists=\"0\">\
                   <Playlists><NODE Id=\"0\" ParentId=\"0\"/><NODE Id=\"B181727E\" ParentId=\"0\"/></Playlists></Sync>";
    let s = RekordboxSync::parse("onelibrary", xml).unwrap();
    assert_eq!(
        s,
        RekordboxSync { library: "onelibrary", automatic: true, collection: Some(2_354_715_273), playlists: 2 }
    );
    assert!(RekordboxSync::parse("onelibrary", "<Other/>").is_none());
}

/// What a run that didn't finish says, and what finishes it.
#[test]
fn an_unfinished_run_says_what_it_was_and_what_to_do() {
    let pending = |op, description: &str, backup: Option<&str>| PendingOp {
        op,
        started: Utc::now(),
        description: description.into(),
        backup: backup.map(String::from),
        player: None,
    };
    let pack = pending(LineageOp::Pack, "pack from Main", None);
    assert!(pack.describe().starts_with("a `locallibrary pack` (pack from Main) didn't finish (started "));
    assert_eq!(pack.remedy(), "pack it again to finish it");
    let repair = pending(LineageOp::Repair, "", Some("20261005-213000"));
    assert!(repair.describe().starts_with("a `locallibrary drive verify --repair` didn't finish"));
    assert_eq!(repair.remedy(), "`locallibrary undo` puts back the backup it made");
    let edit = pending(LineageOp::Edit, "", None);
    assert!(edit.describe().starts_with("a locallibrary operation didn't finish"));
    assert_eq!(edit.remedy(), "run it again, or make any other change to the drive, to finish it");
    assert_eq!(pending(LineageOp::Push, "", None).remedy(), "sync onto it again to finish it");
    for op in [
        LineageOp::Sync,
        LineageOp::Pull,
        LineageOp::Merge,
        LineageOp::Clean,
        LineageOp::Convert,
        LineageOp::Artwork,
        LineageOp::Restore,
    ] {
        assert!(pending(op, "", None).describe().starts_with("a `locallibrary "), "{op:?}");
    }
}

/// A drive that copied another's whole library takes over its history, capped like any other.
#[test]
fn inheriting_a_lineage_ends_with_the_new_state() {
    let source: Vec<LineageEntry> = (0..=255u8).map(entry).collect();
    let mut s = DeviceState::new("Backup");
    s.inherit_with(&source, LineageEntry { op: LineageOp::Sync, ..entry(255) });
    assert_eq!(s.lineage.len(), MAX_LINEAGE);
    assert_eq!(s.head().unwrap().op, LineageOp::Sync, "the source's head is replaced");
    s.inherit_with(&source, entry(7));
    assert_eq!((s.lineage.len(), s.lineage[0].fingerprint), (MAX_LINEAGE, Hash::of(&[1])));
    assert_eq!(s.head().unwrap().fingerprint, Hash::of(&[7]));
}

/// A formatted drive gets its identity back, but none of the states it held before.
#[test]
fn a_formatted_drive_gets_its_identity_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(dir.path()).unwrap();
    assert!(matches!(Device::open_export(dir.path()), Err(Error::NotAnExport(_))));
    d.save_state().unwrap();
    assert!(!dir.path().join(paths::STATE_DIR).join("state.json").exists(), "nothing to save");
    let mut old = DeviceState::new("Main");
    old.record(entry(1));
    d.reset_identity(&old).unwrap();
    let d = Device::open(dir.path()).unwrap();
    assert_eq!((d.id(), d.name()), (Some(old.id), "Main".to_string()));
    assert!(d.state().unwrap().lineage.is_empty());
    assert!(format!("{d:?}").contains(&old.id.to_string()));
}

#[test]
fn adopt_persists_identity() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(dir.path()).unwrap();
    assert!(!d.is_adopted());
    let id = d.adopt(Some("Backup A")).unwrap().id;
    let d2 = Device::open(dir.path()).unwrap();
    assert_eq!(d2.id(), Some(id));
    assert_eq!(d2.name(), "Backup A");
}
