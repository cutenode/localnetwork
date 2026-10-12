use super::*;

fn snap(fp: u8, id: &str) -> SnapshotInfo {
    SnapshotInfo {
        id: Hash::of(id.as_bytes()),
        short_id: id.into(),
        library_id: Uuid::nil(),
        parents: vec![],
        created: Utc::now(),
        message: None,
        source: None,
        fingerprint: Hash::of(&[fp]),
        summary: Default::default(),
        files: 0,
        bytes: 0,
        labels: Vec::new(),
        beside: false,
    }
}

#[test]
fn standing_counts_newer_snapshots() {
    let history = vec![snap(3, "c"), snap(2, "b"), snap(1, "a")];
    assert!(matches!(standing(&history, &Hash::of(&[3])), Standing::UpToDate));
    assert!(matches!(standing(&history, &Hash::of(&[1])), Standing::Behind { snapshots: 2, .. }));
    assert!(matches!(standing(&history, &Hash::of(&[9])), Standing::Unsaved));
}

#[test]
fn roles_parse_by_name() {
    assert_eq!(Role::parse("Archive").unwrap(), Role::Archive);
    assert!(Role::parse("spare").is_err());
    // `gig` is the old name of `set`: read in requests and the registry, written as `set`.
    assert_eq!(Role::parse("GIG").unwrap(), Role::Set);
    assert_eq!(serde_json::from_str::<Role>("\"gig\"").unwrap(), Role::Set);
    assert_eq!(serde_json::to_string(&Role::Set).unwrap(), "\"set\"");
}

#[test]
fn windows_labels_the_drive_letter_with_set_volume() {
    let cmd = windows_label_command(Path::new(r"E:\"), &volume_label("DJ's USB")).unwrap();
    assert_eq!(cmd.args.last().unwrap(), "Set-Volume -DriveLetter E -NewFileSystemLabel 'DJ''S USB'");
    assert!(windows_label_command(Path::new(r"E:\Music"), "X").is_err(), "only a drive's root");
}

/// Under the test switch the volume holding a drive folder is this machine's own: its label
/// stays, and the drive keeps its name too.
#[test]
fn the_test_switch_leaves_volume_labels_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(dir.path()).unwrap();
    d.adopt(Some("Main")).unwrap();
    crate::testing::volumes_on_this_thread(Some(Vec::new()));
    let renamed = rename(None, &mut d, "Club", true);
    // A path that isn't there: were the switch ignored, no tool could find a volume for it.
    let labelled = set_volume_label(&dir.path().join("gone"), "Club");
    crate::testing::volumes_on_this_thread(None);
    for refused in [renamed.map(|r| r.name), labelled] {
        assert!(
            matches!(&refused, Err(Error::Invalid(m)) if m.starts_with("LOCALLIBRARY_VOLUMES is set")),
            "{refused:?}"
        );
    }
    assert_eq!(Device::open(dir.path()).unwrap().name(), "Main");
}

#[test]
fn same_disk_needs_a_serial() {
    let a = Hardware { model: "Cruzer".into(), serial: None, volume_uuid: Some("A".into()), size: 64 };
    assert!(!a.same_disk(&a.clone()));
    let b = Hardware { serial: Some("4C530001".into()), ..a.clone() };
    assert!(b.same_disk(&b.clone()));
    assert!(!b.same_disk(&Hardware { size: 32, ..b.clone() }));
    assert!(a.same_volume(&Hardware { volume_uuid: Some("a".into()), ..a.clone() }));
    assert!(!plausible_serial("0000000000"));
    assert!(plausible_serial("4C530001123456789012"));
}
