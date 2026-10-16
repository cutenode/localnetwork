//! A track's BPM, album artist, ISRC and release date: written to both databases and read back,
//! and refused when they aren't values the databases keep.

use crate::common;

use locallibrary::Library;
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions, TrackFields};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;

use super::{both_databases, ids, pdb_library};

/// The details as each database reads them for the track of `file`.
fn details(root: &std::path::Path, file: &str) -> Vec<(f64, String, String, String, String)> {
    let of = |lib: Library| {
        let t = lib.tracks.into_iter().find(|t| t.file_path == file).unwrap();
        (t.bpm, t.album, t.album_artist, t.isrc, t.release_date)
    };
    vec![of(pdb_library(root)), of(common::plus_library(root))]
}

#[test]
fn bpm_album_artist_isrc_and_release_date_are_written_to_both_databases() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    let exported = common::library_files(&root);
    let lib = pdb_library(&root);
    // Two tracks of one album: the album artist of one changes, the other keeps the album's.
    let album: Vec<&locallibrary::Track> = lib.tracks.iter().filter(|t| t.album == "Framed And Defamed").collect();
    let (t, other) = (album[0], album[1]);
    let others = details(&root, &other.file_path);
    let fields = TrackFields {
        bpm: Some(126.456),
        album_artist: Some("Various Artists".into()),
        isrc: Some("GBDUW0000059".into()),
        release_date: Some("2019-05-01".into()),
        ..Default::default()
    };
    let plan = common::apply(&root, &[Edit::EditTracks { tracks: ids(&[t.id]), fields }]);
    assert_eq!(
        plan.changes,
        [
            r#"set album artist "Various Artists", ISRC "GBDUW0000059", release date "2019-05-01", BPM 126.46 on 1 track(s)"#
        ]
    );
    assert_eq!(plan.tracks_changed, 1);
    let set = (126.46, t.album.clone(), "Various Artists".to_string(), "GBDUW0000059".into(), "2019-05-01".into());
    assert_eq!(details(&root, &t.file_path), [set.clone(), set]);
    assert_eq!(details(&root, &other.file_path), others);
    common::sqlcipher_reads(&root);

    // Empty, the album artist, ISRC and release date are cleared; a new album keeps the artist.
    let fields = TrackFields {
        album: Some("Elsewhere".into()),
        album_artist: Some(String::new()),
        isrc: Some(String::new()),
        release_date: Some(String::new()),
        ..Default::default()
    };
    common::apply(&root, &[Edit::EditTracks { tracks: ids(&[t.id]), fields }]);
    let cleared = (126.46, "Elsewhere".to_string(), String::new(), String::new(), String::new());
    assert_eq!(details(&root, &t.file_path), [cleared.clone(), cleared]);
    let fields =
        TrackFields { album: Some("Again".into()), album_artist: Some("Someone".into()), ..Default::default() };
    common::apply(&root, &[Edit::EditTracks { tracks: ids(&[t.id]), fields }]);
    let fields = TrackFields { album: Some("Later".into()), ..Default::default() };
    common::apply(&root, &[Edit::EditTracks { tracks: ids(&[t.id]), fields }]);
    assert!(details(&root, &t.file_path).iter().all(|d| (d.1.as_str(), d.2.as_str()) == ("Later", "Someone")));

    // rekordbox exports again: a replay sets them again.
    let edited = details(&root, &t.file_path);
    for (key, bytes) in &exported {
        std::fs::write(locallibrary::paths::to_fs(&root, key), bytes).unwrap();
    }
    assert_ne!(details(&root, &t.file_path), edited);
    common::replay(&root);
    assert_eq!(details(&root, &t.file_path), edited);
}

#[test]
fn details_the_databases_cant_keep_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    let d = Device::open_export(&root).unwrap();
    let t = pdb_library(&root).tracks.into_iter().find(|t| !t.album.is_empty()).unwrap();
    let refused = |fields: TrackFields| {
        let edit = Edit::EditTracks { tracks: TrackSelector::paths([t.file_path.clone()]), fields };
        edit::plan(&d, &[edit], &EditOptions::default(), &Ctx::silent()).unwrap_err().to_string()
    };
    let bpm = |b: f64| TrackFields { bpm: Some(b), ..Default::default() };
    let isrc = |i: &str| TrackFields { isrc: Some(i.into()), ..Default::default() };
    let date = |r: &str| TrackFields { release_date: Some(r.into()), ..Default::default() };
    for b in [0.0, 0.99, 1000.0, f64::NAN] {
        assert_eq!(refused(bpm(b)), "BPM goes from 1 to 999.99");
    }
    for i in ["gbduw0000059", "GB-DUW-00-00059", "GBDUW000005", "1BDUW0000059", "GBDUW00000A9", " "] {
        assert_eq!(refused(isrc(i)), format!("{i:?} isn't an ISRC (like GBDUW0000059)"));
    }
    for r in ["2019-13-01", "01/05/2019", "2019", "2019-5-1", "2019-05-1 ", "+019-05-01"] {
        assert_eq!(refused(date(r)), format!("{r:?} isn't a release date (YYYY-MM-DD)"));
    }
    let no_album = TrackFields { album: Some(" ".into()), album_artist: Some("Someone".into()), ..Default::default() };
    assert_eq!(
        refused(no_album),
        format!("{} has no album to keep an album artist with: give one too", t.display_name())
    );
    let long = TrackFields { album_artist: Some("x".repeat(5_000)), ..Default::default() };
    assert_eq!(refused(long), "the album artist name is too long for export.pdb (5000 characters)");
}

/// The journal says version 3 only once an edit needs it, so builds that read version 2 keep
/// editing drives whose edits don't.
#[test]
fn the_journal_says_version_3_only_for_edits_that_need_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    let version = || {
        let journal = std::fs::read(root.join(locallibrary::paths::STATE_DIR).join("edits.json")).unwrap();
        serde_json::from_slice::<serde_json::Value>(&journal).unwrap()["version"].clone()
    };
    let rating = TrackFields { rating: Some(4), ..Default::default() };
    common::apply(&root, &[Edit::EditTracks { tracks: ids(&[1]), fields: rating }]);
    assert_eq!(version(), 2);
    let bpm = TrackFields { bpm: Some(128.0), ..Default::default() };
    common::apply(&root, &[Edit::EditTracks { tracks: ids(&[1]), fields: bpm }]);
    assert_eq!(version(), 3);
    edit::journal::clear(&Device::open_export(&root).unwrap(), None).unwrap();
    assert_eq!(version(), 2);
}

/// Regression: clearing only the album artist ran export.pdb's blank-name check on the track's own
/// album, so an album named with spaces was dropped.
#[test]
fn clearing_the_album_artist_keeps_an_album_whatever_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let pdb_path = root.join(locallibrary::paths::EXPORT_PDB);
    let id = pdb_library(&root).tracks[0].id;
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    let album = ed.album_id_for(" ", 0).unwrap();
    ed.set_track_field(id, "album_id", album).unwrap();
    ed.save(&pdb_path).unwrap();

    let fields = TrackFields { album_artist: Some(String::new()), ..Default::default() };
    common::apply(&root, &[Edit::EditTracks { tracks: ids(&[id]), fields }]);
    assert_eq!(pdb_library(&root).track(id).unwrap().album, " ");
}
