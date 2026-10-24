use super::*;

/// A grid whose tempo changes is written at its first downbeat's tempo, and the export says so.
#[test]
fn a_changing_tempo_is_kept_at_the_first_downbeat_and_warned_about() {
    let track =
        Track { id: 1, title: "A".into(), file_path: "/Contents/a.mp3".into(), bpm: 126.0, ..Default::default() };
    let library = Library { tracks: vec![track], ..Default::default() };
    let beat = |n, bpm, ms| Beat { beat_number: n, bpm, time_ms: ms };
    let analysis =
        |_: &Track| TrackAnalysis { beats: vec![beat(1, 120.0, 40), beat(2, 124.0, 540)], ..Default::default() };
    let src = Source { library: &library, root: std::path::PathBuf::from("/Volumes/USB"), analysis: Some(&analysis) };
    let export = build(&src, Path::new("out"), &Ctx::silent()).unwrap();
    assert_eq!(export.beat_grids, 1);
    assert_eq!(export.warnings, ["1 track(s) change tempo; their grid keeps the tempo of the first downbeat"]);
    let crate::Content::Bytes(nml) = &export.files[0].content else { panic!("the NML is written out") };
    assert!(String::from_utf8_lossy(nml).contains("<TEMPO BPM=\"120.000000\""));
}

/// Rows whose paths differ only in case are one file, as Serato and Engine take them: one
/// entry, which every playlist names.
#[test]
fn case_variants_of_a_path_are_one_entry() {
    let track = |id, file_path: &str| Track { id, file_path: file_path.into(), ..Default::default() };
    let library = Library {
        tracks: vec![track(1, "/Music/a.mp3"), track(2, "/music/A.mp3")],
        playlists: vec![Playlist {
            id: 1,
            parent_id: None,
            name: "Set".into(),
            is_folder: false,
            sort_order: 0,
            track_ids: vec![2, 1],
        }],
        ..Default::default()
    };
    let src = Source { library: &library, root: std::path::PathBuf::from("/Volumes/USB"), analysis: None };
    let export = build(&src, Path::new("out"), &Ctx::silent()).unwrap();
    assert_eq!(export.tracks, 1);
    assert_eq!(export.warnings, ["1 track row(s) share a file with another and are one track in Traktor"]);
    let crate::Content::Bytes(nml) = &export.files[0].content else { panic!("the NML is written out") };
    let nml = String::from_utf8_lossy(nml);
    assert!(nml.contains("<COLLECTION ENTRIES=\"1\">"), "{nml}");
    assert_eq!(nml.matches("KEY=\"USB/:Music/:a.mp3\"").count(), 2, "{nml}");
}

#[test]
fn grids_start_on_the_first_downbeat() {
    let beat = |n, bpm, ms| Beat { beat_number: n, bpm, time_ms: ms };
    let beats = [beat(3, 128.0, 100), beat(4, 128.0, 569), beat(1, 128.0, 1038)];
    assert_eq!(constant_grid(&beats, 128.0), Some((128.0, 1038, false)));
    let changing = [beat(1, 120.0, 0), beat(2, 121.0, 500)];
    assert_eq!(constant_grid(&changing, 120.0), Some((120.0, 0, true)));
    assert_eq!(constant_grid(&[], 128.0), None);
}

/// rekordbox's eight colours on Traktor's seven (red 1 round to magenta 7): aqua and blue
/// are both its blue.
#[test]
fn colours_are_traktors_nearest() {
    use TrackColor::*;
    let traktor = [Red, Orange, Yellow, Green, Aqua, Blue, Purple, Pink].map(colour);
    assert_eq!(traktor, [1, 2, 3, 4, 5, 5, 6, 7]);
}

#[test]
fn locations_split_volume_folders_and_file() {
    let (mac, on_startup) = location(Path::new("/Volumes/USB/Contents/Daft Punk/One.mp3"), STARTUP_VOLUME);
    assert!(!on_startup);
    assert_eq!(mac, Location { volume: "USB".into(), dir: "/:Contents/:Daft Punk/:".into(), file: "One.mp3".into() });
    assert_eq!(mac.key(), "USB/:Contents/:Daft Punk/:One.mp3");
    let (win, _) = location(Path::new(r"e:\Contents\x.mp3"), STARTUP_VOLUME);
    assert_eq!((win.volume.as_str(), win.dir.as_str(), win.file.as_str()), ("E:", "/:Contents/:", "x.mp3"));
    let (home, on_startup) = location(Path::new("/Users/me/Music/x.mp3"), STARTUP_VOLUME);
    assert!(on_startup);
    assert_eq!(home.key(), "Macintosh HD/:Users/:me/:Music/:x.mp3");
    let (root, _) = location(Path::new("/Volumes/USB/x.mp3"), STARTUP_VOLUME);
    assert_eq!(root.dir, "/:");
}

/// The startup disk is the volume linked to `/`, whatever it's called.
#[cfg(unix)]
#[test]
fn startup_volume_is_the_one_linked_to_root() {
    let volumes = tempfile::tempdir().unwrap();
    std::fs::create_dir(volumes.path().join("USB")).unwrap();
    std::os::unix::fs::symlink("/tmp", volumes.path().join("Elsewhere")).unwrap();
    assert_eq!(startup_volume(volumes.path()), None);
    std::os::unix::fs::symlink("/", volumes.path().join("Studio SSD")).unwrap();
    assert_eq!(startup_volume(volumes.path()).as_deref(), Some("Studio SSD"));
    assert_eq!(startup_volume(&volumes.path().join("missing")), None);
    let (home, _) = location(Path::new("/Users/me/x.mp3"), "Studio SSD");
    assert_eq!(home.key(), "Studio SSD/:Users/:me/:x.mp3");
}

#[test]
fn keys_dates_and_colours() {
    assert_eq!(key_value("C"), Some(0));
    assert_eq!(key_value("8B"), Some(0));
    assert_eq!(key_value("Am"), Some(21));
    assert_eq!(key_value("8A"), Some(21));
    assert_eq!(key_value("F#m"), Some(18));
    assert_eq!(key_value("Bb"), Some(10));
    assert_eq!(key_value("1m"), Some(21), "Open Key 1m is A minor");
    assert_eq!(key_value(""), None);
    for n in 1..=12 {
        for major in [false, true] {
            let c = Camelot { number: n, major };
            assert_eq!(Camelot::parse(&c.note()), Some(c), "{}", c.note());
            assert_eq!(key_value(&c.note()), key_value(&c.code()));
        }
    }
    assert_eq!(date("2026-09-30"), Some("2026/9/30".into()));
    assert_eq!(date("2024-03-01 10:00:00"), Some("2024/3/1".into()));
    assert_eq!(date("2024"), None);
    assert_eq!(colour(TrackColor::Pink), 7);
    assert_eq!(uuid("Shows/Friday"), uuid("Shows/Friday"));
    assert_eq!(uuid("Shows/Friday").len(), 32);
}

/// A location read back is a path on this computer, in its separator throughout (`\` on
/// Windows): below `/Volumes`, `/` for the startup disk, and below a drive letter.
#[test]
fn locations_are_paths_again() {
    let loc = |volume: &str, dir: &str| Location { volume: volume.into(), dir: dir.into(), file: "x.mp3".into() };
    let native = |p: &str| p.replace('/', MAIN_SEPARATOR_STR);
    let path = |l: Location, startup: &str| l.path(startup).to_string_lossy().into_owned();
    let usb = loc("USB", "/:Contents/:Daft Punk/:");
    assert_eq!(path(usb, "Macintosh HD"), native("/Volumes/USB/Contents/Daft Punk/x.mp3"));
    let home = loc("Macintosh HD", "/:Users/:me/:");
    assert_eq!(path(home.clone(), "Macintosh HD"), native("/Users/me/x.mp3"));
    let elsewhere = path(home, "Studio SSD");
    assert_eq!(elsewhere, native("/Volumes/Macintosh HD/Users/me/x.mp3"), "not this computer's startup disk");
    assert_eq!(path(loc("E:", "/:Music/:"), "E:"), native("E:/Music/x.mp3"));
    assert_eq!(path(loc("USB", "/:"), "Macintosh HD"), native("/Volumes/USB/x.mp3"));
}

/// The startup disk is this Mac's, or the name `build` gives it elsewhere.
#[test]
fn the_startup_disk_is_named_as_build_names_it() {
    assert_eq!(startup_disk(), startup_volume(Path::new("/Volumes")).unwrap_or_else(|| STARTUP_VOLUME.into()));
    assert!(!startup_disk().is_empty());
}

/// Every `MUSICAL_KEY VALUE` is a key that's written as that value again.
#[test]
fn key_values_read_back() {
    for v in 0..24 {
        let key = key_of(v).unwrap();
        assert_eq!((key_value(&key.code()), key.major), (Some(v), v < 12));
    }
    assert_eq!(key_of(21).map(Camelot::code).as_deref(), Some("8A"));
    assert_eq!(key_of(24), None);
}
