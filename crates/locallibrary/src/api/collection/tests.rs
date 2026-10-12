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

/// rekordbox's collection with its audio on this computer (but one track's), exported (all of
/// it, a playlist, or one it lacks) to a folder that's there, to a new one the export makes, or
/// to one on a drive that isn't mounted: the last is refused and nothing made; a playlist the
/// collection lacks is refused, taking the new folder away again; otherwise the plan says what
/// it exports where, and applied (planned first, or in one go, with the store or without), the
/// folder holds exactly the tracks planned.
///
/// Opening the encrypted collection is slow, so the explicit cases take each branch once.
#[hegel::test(test_cases = 3)]
#[hegel::explicit_test_case(target = 1u8, playlists = vec!["Nowhere".to_string()], with_store = false, in_one_go = false)]
#[hegel::explicit_test_case(target = 0u8, playlists = vec!["Shows/Friday".to_string()], with_store = true, in_one_go = true)]
#[hegel::explicit_test_case(target = 1u8, playlists = Vec::<String>::new(), with_store = true, in_one_go = false)]
#[hegel::explicit_test_case(target = 2u8, playlists = Vec::<String>::new(), with_store = false, in_one_go = false)]
fn the_collection_exports_to_a_folder_or_says_why_not(tc: hegel::TestCase) {
    use hegel::generators as gs;
    // 0: a folder that's there; 1: a new one; 2: one whose parent isn't there.
    let target = tc.draw(gs::integers::<u8>().max_value(2));
    let playlists =
        tc.draw(gs::sampled_from(vec![vec![], vec!["Shows/Friday".to_string()], vec!["Nowhere".to_string()]]));
    let with_store = tc.draw(gs::booleans());
    let in_one_go = tc.draw(gs::booleans());

    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    std::fs::create_dir_all(&music).unwrap();
    let f = crate::masterdb::tests::sample();
    f.exec(&format!(
        "UPDATE djmdContent SET FolderPath = '{m}/omt.mp3' WHERE ID = '84935471';
         UPDATE djmdContent SET FolderPath = '{m}/ngo.mp3' WHERE ID = '205356547';
         UPDATE djmdContent SET FolderPath = '{m}/gone/Only Here.mp3' WHERE ID = '4';",
        m = music.display()
    ));
    std::fs::write(music.join("omt.mp3"), b"one more time").unwrap();
    std::fs::write(music.join("ngo.mp3"), b"never grow old").unwrap();
    let db = dir.path().join("master.db");
    std::fs::write(&db, f.encrypted(DEFAULT_KEY)).unwrap();
    let root = match target {
        0 => {
            std::fs::create_dir(dir.path().join("USB")).unwrap();
            dir.path().join("USB")
        }
        1 => dir.path().join("New"),
        _ => dir.path().join("Unplugged/USB"),
    };
    let req = CollectionExportRequest {
        collection: CollectionRef { db: Some(db), key: Some(DEFAULT_KEY.into()) },
        drive: root.display().to_string(),
        options: CollectionExportOptions { playlists: playlists.clone(), ..Default::default() },
    };
    let ctx = Ctx::silent();
    // What's read is the collection, deleted rows left out.
    let view = collection_library(&req.collection).unwrap();
    assert_eq!(view.library.tracks.len(), 3);
    let planned = collection_export_plan(&req, &ctx);
    if target == 2 {
        let e = planned.map(|p| p.noop).unwrap_err();
        assert!(
            matches!(&e, Error::NotFound(m) if m.ends_with("the folder to export into (is its drive mounted?)")),
            "{e}"
        );
        assert!(!dir.path().join("Unplugged").exists(), "nothing made");
        return;
    }
    if playlists == ["Nowhere"] {
        assert!(planned.is_err(), "a playlist the collection lacks");
        assert_eq!(root.exists(), target == 0, "the folder made for it is taken away again");
        return;
    }
    let plan = planned.unwrap();
    assert_eq!(plan.plan.made_folder, target == 1);
    let what = if playlists.is_empty() { "rekordbox's collection".to_string() } else { playlists.join(", ") };
    let line = format!(
        "export {what} to {} ({} track(s), {} playlist(s))",
        plan.plan.target, plan.plan.tracks, plan.plan.playlist_count
    );
    assert_eq!(plan.summary[0], line);
    let store = with_store.then(|| Store::open_or_init(dir.path().join("store")).unwrap());
    let report = match in_one_go {
        true => collection_export_apply(store.as_ref(), &req, &ctx).unwrap(),
        false => plan.apply(store.as_ref(), &ctx).unwrap(),
    };
    assert_eq!(report.tracks, plan.plan.tracks);
    assert!(plan.plan.tracks <= view.library.tracks.len());
    let exported = crate::device::Device::open(&root).unwrap().library().unwrap();
    assert_eq!(exported.tracks.len(), plan.plan.tracks);
    if let Some(store) = &store {
        let id = crate::device::Device::open(&root).unwrap().id();
        assert!(id.is_none_or(|id| store.drives().unwrap().contains_key(&id)), "registered");
    }
}
