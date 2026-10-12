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
