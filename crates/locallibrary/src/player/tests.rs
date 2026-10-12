use super::*;

#[test]
fn contact_playlist_names_have_no_slashes() {
    assert_eq!(contact_playlist(" sam@example.com "), "IF FOUND: sam@example.com");
    assert_eq!(contact_playlist("call 555/1234"), "IF FOUND: call 555-1234");
}

#[test]
fn names_and_colours_players_can_show() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(dir.path()).unwrap();
    let invalid = |name: Option<&str>, colour| match check(&d, name, colour) {
        Err(Error::Invalid(m)) => m,
        other => panic!("{other:?}"),
    };
    assert_eq!(invalid(None, None), "give a name, a colour, or both");
    for name in ["  ", "A\tB", &"x".repeat(MAX_NAME + 1)] {
        assert!(invalid(Some(name), None).starts_with("a player name is 1 to 32"), "{name:?}");
    }
    assert!(invalid(None, Some(9)).starts_with("the colour is a rekordbox colour code"));
    // A good name, on a drive without the database that keeps it.
    let err = check(&d, Some("BACKUP B"), Some(3)).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::NoOneLibrary), "{err}");
    assert!(set(&mut Device::open(dir.path()).unwrap(), Some("BACKUP B"), None).is_err());
}

/// The contact goes in a text file in the root; a drive without a library gets no playlist.
#[test]
fn a_contact_for_whoever_finds_the_drive() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(dir.path()).unwrap();
    assert!(set_contact(&mut d, " \n", true, &Ctx::silent()).is_err());
    let r = set_contact(&mut d, "sam@example.com", true, &Ctx::silent()).unwrap();
    assert_eq!((r.file.as_path(), r.playlist), (d.root().join(CONTACT_FILE).as_path(), None));
    let text = std::fs::read_to_string(&r.file).unwrap();
    assert!(text.contains("\r\n    sam@example.com\r\n"), "{text:?}");
}

#[test]
fn reads_rekordbox_device_records() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("PIONEER/DeviceLibBackup");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(
            p.join("rbDevLibBaInfo_815904346.json"),
            r#"{"uuid":"x","info":[{"device_id":"x","device_name":"BACKUP B","background_color":"3","background_color_libplus":"3","device_filesystem":"FAT32","backup_pc_id":"1"}]}"#,
        )
        .unwrap();
    let r = rekordbox_device(dir.path()).unwrap();
    assert_eq!(r.device_name, "BACKUP B");
    assert_eq!(r.background_color_libplus, "3");
    assert!(rekordbox_device(&dir.path().join("nothing")).is_none());
}
