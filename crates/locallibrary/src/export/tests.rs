use super::*;
use loclib_model::test_util::track;

#[test]
fn relative_paths() {
    assert_eq!(
        relative(Path::new("/Volumes/USB/Lists"), Path::new("/Volumes/USB/Contents/A/x.mp3")).as_deref(),
        Some("../Contents/A/x.mp3")
    );
    assert_eq!(
        relative(Path::new("/Volumes/USB"), Path::new("/Volumes/USB/Contents/x.mp3")).as_deref(),
        Some("Contents/x.mp3")
    );
    assert_eq!(
        relative(Path::new("/Users/me"), Path::new("/Volumes/USB/x.mp3")).as_deref(),
        Some("../../Volumes/USB/x.mp3")
    );
}

#[test]
fn a_relative_path_needs_a_root_in_common() {
    assert_eq!(relative(Path::new("Lists"), Path::new("/Volumes/USB/x.mp3")), None);
}

#[test]
fn formats_are_told_by_their_extension() {
    for f in [Format::M3u8, Format::Csv, Format::Txt] {
        assert_eq!(Format::from_path(Path::new(&format!("set.{}", f.extension().to_uppercase()))), Some(f));
    }
    assert_eq!(Format::from_path(Path::new("set.wav")), None);
    assert_eq!(Format::from_path(Path::new("set")), None);
}

/// Playlists split into files of the same name get " (2)", " (3)"...
#[test]
fn split_playlists_named_alike_get_files_of_their_own() {
    let list = |id, parent_id, name: &str, is_folder| crate::model::Playlist {
        id,
        parent_id,
        name: name.into(),
        is_folder,
        sort_order: id,
        track_ids: if is_folder { vec![] } else { vec![1] },
    };
    let lib = Library {
        tracks: vec![track(1, "A", "B", "/Contents/b.mp3")],
        playlists: vec![
            list(1, None, "Sets", true),
            list(2, Some(1), "Friday", false),
            list(3, Some(1), "friday", false),
            list(4, Some(1), "FRIDAY", false),
        ],
        ..Default::default()
    };
    let files: Vec<PathBuf> = select(&lib, Some("Sets"), true, "x").unwrap().into_iter().map(|s| s.file).collect();
    let names: Vec<String> = files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["Friday", "friday (2)", "FRIDAY (3)"]);
}

#[test]
fn formats() {
    let mut a = track(1, "Daft Punk", "Around the World", "/Contents/Daft Punk/atw.mp3");
    a.duration_secs = 429;
    a.comment = "says \"around\", a lot".into();
    let b = track(2, "", "Untitled\nTwo", "/Contents/u.mp3");
    let locator = Locator { style: PathStyle::Drive, drive_root: "/Volumes/USB".into(), base: "/x".into() };
    assert_eq!(
        m3u8("Set", &[&a, &b], &locator),
        "#EXTM3U\n#PLAYLIST:Set\n#EXTINF:429,Daft Punk - Around the World\n/Contents/Daft Punk/atw.mp3\n\
             #EXTINF:300,Untitled Two\n/Contents/u.mp3\n"
    );
    assert_eq!(text(&[&a, &b]), "01. Daft Punk - Around the World\n02. Untitled Two\n");
    let csv = csv(&[&a], &locator);
    let row = csv.lines().nth(1).unwrap();
    assert!(row.starts_with("1,Around the World,Daft Punk,,,,128.00,,0,7:09,,0,,"), "{row}");
    assert!(row.ends_with(",\"says \"\"around\"\", a lot\",/Contents/Daft Punk/atw.mp3"), "{row}");
    assert_eq!(Format::from_path(Path::new("set.M3U")), Some(Format::M3u8));
}
