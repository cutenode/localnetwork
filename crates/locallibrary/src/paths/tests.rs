use super::*;

#[test]
fn keys_are_normalised() {
    // "é" as e + combining acute (NFD) must match the precomposed form (NFC).
    assert_eq!(key("/Contents/Cafe\u{301}/x.mp3"), "Contents/Caf\u{e9}/x.mp3");
    assert_eq!(key("PIONEER\\rekordbox\\export.pdb"), EXPORT_PDB);
    assert_eq!(device_path("Contents/a.mp3"), "/Contents/a.mp3");
}

#[test]
fn classification() {
    assert_eq!(classify(EXPORT_PDB), Some(FileClass::Database));
    assert_eq!(classify(LIBRARY_PLUS_DB), Some(FileClass::Database));
    assert_eq!(classify(LIBRARY_PLUS_WAL), Some(FileClass::Database));
    assert_eq!(classify(LIBRARY_PLUS_SHM), Some(FileClass::Transient));
    assert_eq!(classify("PIONEER/rekordbox/EXPORTLIBRARY.DB-SHM"), Some(FileClass::Transient));
    assert!(!FileClass::Transient.is_library());
    assert_eq!(classify("PIONEER/extracted/gcred.dat"), Some(FileClass::Private));
    assert!(!FileClass::Private.is_library());
    assert_eq!(classify("PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT"), Some(FileClass::Analysis));
    assert_eq!(classify("PIONEER/Artwork/00001/a1.jpg"), Some(FileClass::Artwork));
    assert_eq!(classify("PIONEER/MYSETTING.DAT"), Some(FileClass::Settings));
    assert_eq!(classify("PIONEER/DJMMYSETTING.DAT"), Some(FileClass::Settings));
    assert_eq!(classify("PIONEER/DeviceLibBackup/rbDevLibBaInfo_1.json"), Some(FileClass::DeviceIdentity));
    assert_eq!(classify("PIONEER/djprofile.nxs"), Some(FileClass::Other));
    assert_eq!(classify("Contents/A/B/x.mp3"), Some(FileClass::Audio));
    assert_eq!(classify("Contents/A/._x.mp3"), None);
    assert_eq!(classify("Contents/.DS_Store"), None);
    assert_eq!(classify("Contents/A/.locallibrary-tmp-x1y2"), None);
    assert_eq!(classify(".locallibrary/state.json"), None);
    assert_eq!(classify(".LOCALLIBRARY/state.json"), None);
    assert_eq!(classify("Music/x.mp3"), None);
    assert_eq!(classify("README.txt"), None);
}

#[test]
fn what_players_and_rekordbox_keep_for_a_drive_stays_with_it() {
    for key in [
        "PIONEER/LOG/0001.LOG",
        "PIONEER/TRASHBOX/x.dat",
        "PIONEER/cdj/settings.dat",
        "PIONEER/MPJ/a/b.dat",
        "PIONEER/rekordbox/RBFLTR.DAT",
        PLAYLIST_SYNC,
        PLAYLIST_SYNC_PLUS,
    ] {
        assert_eq!(classify(key), Some(FileClass::DriveState), "{key}");
    }
    assert!(!FileClass::DriveState.is_library());
    assert_eq!(classify("PIONEER/rekordbox/other.dat"), Some(FileClass::Database));
    assert_eq!(classify("PIONEER/LOGS/x"), Some(FileClass::Other));
}

#[test]
fn unsafe_keys() {
    for bad in [
        "",
        "../x",
        "Contents/../../x",
        "a//b",
        "./x",
        "C:/x",
        "a\\b",
        ".locallibrary/state.json",
        ".LOCALLIBRARY/state.json",
        ".LocalLibrary/index.json",
        ".locall\u{131}brary/state.json",
    ] {
        assert!(!is_safe_key(bad), "{bad:?}");
    }
    assert!(is_safe_key("Contents/A/x.mp3"));
    assert!(is_safe_key("Music/x.mp3"));
}

/// Every key below PIONEER/ is classed by where it is, whatever the case its files' names are
/// written in (FAT32 and exFAT keep case but don't tell by it): the OneLibrary index is
/// transient, the track filter and sync records stay with the drive, settings files are the
/// profile's, and anything under OS clutter or locallibrary's own folder isn't managed.
#[hegel::test(test_cases = 300)]
fn pioneer_keys_are_classed_by_where_they_are(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let shout = |s: &str, tc: &hegel::TestCase| -> String {
        s.chars()
            .map(|c| if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() })
            .collect()
    };
    let rows: [(&str, Option<FileClass>); 14] = [
        ("rekordbox/exportLibrary.db-shm", Some(FileClass::Transient)),
        ("rekordbox/rbfltr.dat", Some(FileClass::DriveState)),
        ("rekordbox/playlists3.sync", Some(FileClass::DriveState)),
        ("rekordbox/export.pdb", Some(FileClass::Database)),
        ("rekordbox/exportLibrary.db-wal", Some(FileClass::Database)),
        ("CDJ/x.dat", Some(FileClass::DriveState)),
        ("TRASHBOX/x", Some(FileClass::DriveState)),
        ("USBANLZ/P000/0001/ANLZ0000.DAT", Some(FileClass::Analysis)),
        ("Artwork/00001/a1.jpg", Some(FileClass::Artwork)),
        ("DeviceLibBackup/x.dat", Some(FileClass::DeviceIdentity)),
        ("extracted/gcred.dat", Some(FileClass::Private)),
        ("MYSETTING.DAT", Some(FileClass::Settings)),
        ("other/x", Some(FileClass::Other)),
        ("README.TXT", Some(FileClass::Other)),
    ];
    let (rest, want) = tc.draw(gs::sampled_from(rows.to_vec()));
    // The case-sensitive folders (rekordbox's own, USBANLZ...) are written as rekordbox writes them.
    let (dir, name) = rest.split_once('/').unwrap_or(("", rest));
    let fixed_dir = matches!(dir, "rekordbox" | "USBANLZ" | "Artwork" | "DeviceLibBackup" | "extracted" | "other");
    let dir = if fixed_dir { dir.to_string() } else { shout(dir, &tc) };
    let name = shout(name, &tc);
    let key = if dir.is_empty() { format!("PIONEER/{name}") } else { format!("PIONEER/{dir}/{name}") };
    assert_eq!(classify(&key), want, "{key}");
    let hidden = tc.draw(gs::sampled_from(vec![".DS_Store", "._x", ".locallibrary-tmp-1"]));
    assert_eq!(classify(&format!("{key}/{hidden}")), None);
    assert_eq!(classify(&format!("{}/{key}", shout(STATE_DIR, &tc))), None);
}
