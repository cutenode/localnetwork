use super::*;

#[test]
fn sync_manager_names() {
    assert_eq!(sync_manager_base("House1-1"), "House1");
    assert_eq!(sync_manager_base("House1-12"), "House1");
    assert_eq!(sync_manager_base("Track (2)"), "Track");
    assert_eq!(sync_manager_base("Track"), "Track");
    assert_eq!(sync_manager_base("-1"), "-1");
    assert_eq!(sync_manager_base("(1)"), "(1)");
    assert_eq!(sync_manager_base("Track (live)"), "Track (live)");
    assert_eq!(sync_manager_base("Track 2"), "Track 2");
}

/// Regression: tracks sharing a missing cover were reported as one, so `check` counted one
/// track without artwork for a whole album.
#[test]
fn each_track_missing_a_shared_file_is_reported_once() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(dir.path()).unwrap();
    let track = |id, path: &str| Track {
        artwork_path: Some("/PIONEER/Artwork/00001/a1.jpg".into()),
        ..loclib_model::test_util::track(id, "A", "T", path)
    };
    let lib = Library { tracks: vec![track(1, "/Contents/a.mp3"), track(2, "/Contents/b.mp3")], ..Default::default() };
    // Both databases list both tracks.
    let dbs = Databases { pdb: Some(lib.clone()), plus: Some(lib), plus_error: None, images: Vec::new() };
    let found = missing(&d, &dbs, &FileTree::default(), &Ctx::silent()).unwrap();
    let said: Vec<(Problem, u32, &str)> = found.iter().map(|b| (b.problem, b.track.id, b.database)).collect();
    assert_eq!(
        said,
        [
            (Problem::MissingAudio, 1, "device_library"),
            (Problem::MissingArtwork, 1, "device_library"),
            (Problem::MissingAudio, 2, "device_library"),
            (Problem::MissingArtwork, 2, "device_library"),
        ]
    );
}
