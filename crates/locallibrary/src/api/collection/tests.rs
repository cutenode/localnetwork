use super::*;
use crate::masterdb::DEFAULT_KEY;

/// rekordbox's collection (the sample: three tracks, a folder with a playlist, an intelligent
/// playlist, a history list), encrypted as rekordbox keeps it.
fn collection(dir: &std::path::Path) -> CollectionRef {
    let db = dir.join("master.db");
    std::fs::write(&db, crate::masterdb::tests::sample().encrypted(DEFAULT_KEY)).unwrap();
    CollectionRef { db: Some(db), key: Some(DEFAULT_KEY.into()) }
}

#[test]
fn a_collection_that_isnt_there_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let req = CollectionRef { db: Some(dir.path().join("master.db")), key: None };
    let err = collection_info(&req).unwrap_err();
    assert!(matches!(&err, Error::NotFound(m) if m.contains("is rekordbox 6 or 7 installed?")), "{err}");
}

#[test]
fn the_collection_is_read_with_its_cues_and_my_tags() {
    let dir = tempfile::tempdir().unwrap();
    let req = collection(dir.path());
    let info = collection_info(&req).unwrap();
    assert_eq!((info.summary.tracks, info.history_lists), (3, 1));
    let view = collection_library(&req).unwrap();
    assert_eq!(view.library.tracks.len(), 3);
    assert_eq!(view.cues[&84935471].len(), 3);
    assert_eq!(view.tags.len(), 2);
}

/// What a drive lacks against the collection: every collection track, on a drive holding
/// none of them; a playlist neither has is refused.
#[test]
fn a_drive_is_compared_with_the_collection() {
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Main");
    let pdb = drive.join(crate::paths::EXPORT_PDB);
    std::fs::create_dir_all(pdb.parent().unwrap()).unwrap();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rekordcrate/demo_tracks");
    std::fs::copy(fixture.join(crate::paths::EXPORT_PDB), &pdb).unwrap();
    let diff = |playlist: Option<&str>| {
        collection_diff(&CollectionDiffRequest {
            collection: collection(dir.path()),
            drive: drive.display().to_string(),
            playlist: playlist.map(Into::into),
        })
    };
    let d = diff(None).unwrap();
    assert_eq!((d.matched_by_file, d.matched_by_details), (0, 0));
    assert_eq!(d.diff.added_tracks.len(), 3);
    let err = diff(Some("Nowhere")).unwrap_err();
    assert_eq!(err.to_string(), Error::NotFound("playlist \"Nowhere\" on either side".into()).to_string());
}
