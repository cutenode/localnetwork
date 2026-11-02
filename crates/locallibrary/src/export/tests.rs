use super::*;
use loclib_model::test_util::track;

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

/// Splitting a folder gives each playlist below it a file of its own, in folders as its folders
/// are, never two named alike in any case, each holding its playlist's tracks; a folder that holds
/// itself (a library built by hand can) still ends. Each format is told by its extension in any
/// case.
#[hegel::test(test_cases = 100)]
fn split_folders_give_each_playlist_a_file_of_its_own(tc: hegel::TestCase) {
    use hegel::generators as gs;
    for f in [Format::M3u8, Format::Csv, Format::Txt] {
        let ext: String =
            f.extension().chars().map(|c| if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c }).collect();
        assert_eq!(Format::from_path(Path::new(&format!("set.{ext}"))), Some(f));
    }
    let names = vec!["Friday", "friday", "FRIDAY", "Sat/urday", "Sat:urday", "Peak"];
    let tracks: Vec<Track> =
        (1..=3).map(|id| track(id, "A", &format!("T{id}"), &format!("/Contents/{id}.mp3"))).collect();
    let mut playlists = vec![crate::model::Playlist {
        id: 1,
        parent_id: None,
        name: "Sets".into(),
        is_folder: true,
        sort_order: 0,
        track_ids: vec![],
    }];
    let mut folders = vec![1u32];
    for id in 2..tc.draw(gs::integers::<u32>().min_value(2).max_value(9)) {
        let is_folder = tc.draw(gs::weighted_booleans(0.3));
        playlists.push(crate::model::Playlist {
            id,
            parent_id: Some(tc.draw(gs::sampled_from(folders.clone()))),
            name: tc.draw(gs::sampled_from(names.clone())).into(),
            is_folder,
            sort_order: id,
            track_ids: if is_folder {
                vec![]
            } else {
                tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(3)).max_size(3))
            },
        });
        if is_folder {
            folders.push(id);
        }
    }
    let looped = folders.len() > 1 && tc.draw(gs::booleans());
    if looped {
        playlists[0].parent_id = Some(*folders.last().unwrap());
    }
    let lib = Library { tracks, playlists, ..Default::default() };
    let picked = select(&lib, Some("Sets"), true, "x").unwrap();
    let mut files = FxHashSet::default();
    assert!(picked.iter().all(|s| files.insert(s.file.to_string_lossy().to_lowercase())), "two files named alike");
    if !looped {
        // Every playlist below once (folders nest as folders), each with its own tracks.
        let ids = |ts: &[&Track]| ts.iter().map(|t| t.id).collect::<Vec<_>>();
        let mut want: Vec<Vec<u32>> =
            lib.playlists.iter().filter(|p| !p.is_folder).map(|p| ids(&lib.tracks_by_id(&p.track_ids))).collect();
        let mut got: Vec<Vec<u32>> = picked.iter().map(|s| ids(&s.tracks)).collect();
        want.sort();
        got.sort();
        assert_eq!(got, want);
        for s in &picked {
            assert_eq!(
                s.file.components().count(),
                lib.find_playlist(&s.name).map_or(0, |p| lib.playlist_names(p.id).len() - 1)
            );
        }
    }
}
