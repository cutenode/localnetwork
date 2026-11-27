use super::*;
use loclib_rekordbox_db::masterdb::{DEFAULT_KEY, KEY_ENV};
use loclib_rekordbox_db::test_util::{Fixture, sample};
use rusqlite::params;

/// rekordbox's collection (the sample: three tracks, a folder with a playlist, an intelligent
/// playlist, a history list), encrypted as rekordbox keeps it.
fn collection(dir: &std::path::Path) -> CollectionRef {
    let db = dir.join("master.db");
    std::fs::write(&db, sample().encrypted(DEFAULT_KEY)).unwrap();
    CollectionRef { db: Some(db), key: Some(DEFAULT_KEY.into()) }
}

#[test]
fn a_collection_that_isnt_there_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let req = CollectionRef { db: Some(dir.path().join("master.db")), key: None };
    let err = collection_info(&req).unwrap_err();
    assert!(matches!(&err, Error::NotFound(m) if m.contains("is rekordbox 6 or 7 installed?")), "{err}");
}

/// Without the collection, nothing is read from it, compared with it or exported from it; nor is
/// it compared with a folder without a library.
#[test]
fn what_needs_the_collection_says_when_it_isnt_there() {
    let dir = tempfile::tempdir().unwrap();
    let missing = CollectionRef { db: Some(dir.path().join("gone.db")), key: None };
    let not_there = |e: Error| matches!(&e, Error::NotFound(m) if m.contains("is rekordbox 6 or 7 installed?"));
    assert!(not_there(collection_library(&missing).unwrap_err()));
    let drive = dir.path().join("Main");
    let pdb = drive.join(crate::paths::EXPORT_PDB);
    std::fs::create_dir_all(pdb.parent().unwrap()).unwrap();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rekordcrate/demo_tracks");
    std::fs::copy(fixture.join(crate::paths::EXPORT_PDB), &pdb).unwrap();
    let empty = tempfile::tempdir().unwrap();
    let diff = |drive: &Path, collection: &CollectionRef| {
        let drive = drive.display().to_string();
        collection_diff(&CollectionDiffRequest { collection: collection.clone(), drive, playlist: None }).unwrap_err()
    };
    assert_eq!(diff(empty.path(), &collection(dir.path())).code(), "not_an_export");
    assert!(not_there(diff(&drive, &missing)));
    let ctx = Ctx::silent();
    let export = CollectionExportRequest {
        collection: missing.clone(),
        drive: drive.display().to_string(),
        options: CollectionExportOptions::default(),
    };
    assert!(not_there(collection_export_plan(&export, &ctx).map(|p| p.noop).unwrap_err()));
    assert!(not_there(collection_export_apply(None, &export, &ctx).unwrap_err()));
}

/// A folder named by a relative path whose parent isn't there isn't made: its drive isn't mounted.
#[test]
fn a_relative_folder_whose_parent_isnt_there_isnt_made() {
    let spec = "Unplugged drive/USB";
    let err = export_planned("collection export", spec, &CollectionExportOptions::default(), &Ctx::silent(), |_| {
        unreachable!("nothing is planned without the folder")
    })
    .map(|p| p.noop)
    .unwrap_err();
    assert!(matches!(&err, Error::NotFound(m) if m.starts_with("Unplugged drive, the folder to export into")), "{err}");
    assert!(!Path::new("Unplugged drive").exists());
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
fn prop_the_collection_exports_to_a_folder_or_says_why_not(tc: hegel::TestCase) {
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
    let f = sample();
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

/// A folder that can't be made for an export, as a file stands where its parent should be, is
/// refused as the error making it gave, and nothing is planned.
#[cfg(unix)]
#[test]
fn a_folder_under_a_file_cant_be_exported_into() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("file"), b"").unwrap();
    let spec = dir.path().join("file/USB").display().to_string();
    let err = export_planned("collection export", &spec, &CollectionExportOptions::default(), &Ctx::silent(), |_| {
        unreachable!("nothing is planned without the folder")
    })
    .unwrap_err();
    assert!(matches!(&err, Error::Io { path, .. } if path.ends_with("file/USB")), "{err}");
}

/// An intelligent playlist a filter can say is exported with a smart rule for the drive it went
/// to: the plan says so, and applying it with a store keeps the rule there for that drive alone,
/// unless the store has a rule for the same path already (one for another drive), which stays as
/// it was and is warned of.
#[hegel::test(test_cases = 2)]
#[hegel::explicit_test_case(taken = false)]
#[hegel::explicit_test_case(taken = true)]
fn prop_an_exports_smart_rules_are_kept_unless_another_rule_has_the_path(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let taken = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    std::fs::create_dir_all(&music).unwrap();
    let recent = r#"<NODE Id="1" LogicalOperator="1" AutomaticUpdate="1"><CONDITION PropertyName="genre" Operator="8" ValueUnit="" ValueLeft="house" ValueRight=""/></NODE>"#;
    let f = sample();
    f.exec(&format!(
        "UPDATE djmdContent SET FolderPath = '{m}/omt.mp3' WHERE ID = '84935471';
         UPDATE djmdContent SET FolderPath = '{m}/ngo.mp3' WHERE ID = '205356547';
         UPDATE djmdContent SET FolderPath = '{m}/gone/Only Here.mp3' WHERE ID = '4';
         UPDATE djmdPlaylist SET SmartList = '{recent}' WHERE ID = '12';",
        m = music.display()
    ));
    std::fs::write(music.join("omt.mp3"), b"one more time").unwrap();
    std::fs::write(music.join("ngo.mp3"), b"never grow old").unwrap();
    let db = dir.path().join("master.db");
    std::fs::write(&db, f.encrypted(DEFAULT_KEY)).unwrap();
    let root = dir.path().join("USB");
    std::fs::create_dir(&root).unwrap();
    let req = CollectionExportRequest {
        collection: CollectionRef { db: Some(db), key: Some(DEFAULT_KEY.into()) },
        drive: root.display().to_string(),
        options: CollectionExportOptions::default(),
    };
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let theirs = crate::smart::SmartPlaylist {
        path: "Recent".into(),
        filter: "bpm:120-130".into(),
        sort: None,
        limit: None,
        drives: vec![uuid::Uuid::new_v4()],
    };
    if taken {
        let mut all = store.smart_playlists().unwrap();
        all.set(theirs.clone(), &Default::default()).unwrap();
        store.save_smart_playlists(&all).unwrap();
    }
    let warnings = std::sync::Mutex::new(Vec::new());
    let sink = |e: &crate::progress::Event| {
        if let crate::progress::Event::Warning { message } = e {
            warnings.lock().unwrap().push(message.clone());
        }
    };
    let ctx = Ctx::new(&sink, crate::progress::CancelToken::new());
    let plan = collection_export_plan(&req, &ctx).unwrap();
    let line = "keep a smart rule for 1 intelligent playlist(s), so `smart apply` keeps them up to date";
    assert!(plan.summary.iter().any(|l| l == line), "{:?}", plan.summary);
    plan.apply(Some(&store), &ctx).unwrap();

    let id = crate::device::Device::open(&root).unwrap().id().expect("the export gave the drive an id");
    let kept = store.smart_playlists().unwrap();
    let rule = kept.get("Recent").unwrap();
    let said = warnings.lock().unwrap().clone();
    let warned = said
        .iter()
        .any(|w| w.starts_with("1 intelligent playlist(s) got no smart rule") && w.ends_with("(e.g. Recent)"));
    if taken {
        assert_eq!(rule, &theirs, "the rule there stays");
        assert!(warned, "{said:?}");
    } else {
        assert_eq!((rule.filter.as_str(), rule.drives.as_slice()), (r#"genre:"house""#, &[id][..]));
        assert!(!warned, "{said:?}");
    }
}

/// `bytes` as `master.db` in `dir`.
fn write(dir: &Path, bytes: &[u8]) -> PathBuf {
    let path = dir.join("master.db");
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn the_api_reads_the_collection_it_is_given() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &sample().encrypted(DEFAULT_KEY));
    let req = CollectionRef { db: Some(path), key: None };
    if std::env::var_os(KEY_ENV).is_none() {
        assert_eq!(collection_info(&req).unwrap().summary.tracks, 3);
    }
    let missing = CollectionRef { db: Some(dir.path().join("nope.db")), key: None };
    assert_eq!(collection_info(&missing).unwrap_err().code(), "not_found");
}

/// A test reads the collection it gives or names with `LOCALLIBRARY_MASTERDB`, wherever it is;
/// naming none, it's told how and nothing is read.
#[test]
fn a_test_reads_the_collection_it_names_and_is_told_how_without_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), &sample().encrypted(DEFAULT_KEY));
    let unnamed = CollectionRef { db: None, key: Some(DEFAULT_KEY.into()) };
    // SAFETY: nextest runs each test in a process of its own.
    unsafe { std::env::remove_var(masterdb::PATH_ENV) };
    let err = collection_info(&unnamed).unwrap_err();
    assert!(matches!(&err, Error::NotFound(m) if m.contains(masterdb::PATH_ENV)), "{err}");
    let given = CollectionRef { db: Some(path.clone()), ..unnamed.clone() };
    assert_eq!(collection_info(&given).unwrap().summary.tracks, 3);
    unsafe { std::env::set_var(masterdb::PATH_ENV, &path) };
    assert_eq!(collection_info(&unnamed).unwrap().summary.tracks, 3);
}

#[test]
fn the_collections_tracks_are_found_on_a_drive_by_file_or_details() {
    use loclib_model::test_util::track;
    let dir = tempfile::tempdir().unwrap();
    let c =
        loclib_rekordbox_db::masterdb::open(&write(dir.path(), &sample().encrypted(DEFAULT_KEY)), DEFAULT_KEY).unwrap();
    // Exported earlier: the same file under /Contents, Floorplan's track under another file
    // name (found by artist, title and length), plus a track the collection no longer has.
    let mut one_more_time =
        track(1, "Daft Punk", "One More Time", "/Contents/Daft Punk/Discovery/01 One More Time.mp3");
    one_more_time.duration_secs = 320;
    let mut never = track(2, "Floorplan", "Never Grow Old", "/Contents/Floorplan/UnknownAlbum/Never Grow Old.mp3");
    never.duration_secs = 412;
    let drive = Library {
        tracks: vec![one_more_time, never, track(3, "Old", "Gone", "/Contents/Old/Gone.mp3")],
        playlists: vec![crate::model::Playlist {
            id: 1,
            parent_id: None,
            name: "Friday".into(),
            is_folder: false,
            sort_order: 0,
            track_ids: vec![1, 2],
        }],
        ..Default::default()
    };
    let d = diff_drive(&c, &drive, "Main", None).unwrap();
    assert_eq!((d.matched_by_file, d.matched_by_details), (1, 1));
    assert!(d.uncertain.is_empty());
    let names = |v: &[TrackRef]| v.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&d.diff.added_tracks), ["Somebody - Only Here"]);
    assert_eq!(names(&d.diff.removed_tracks), ["Old - Gone"]);
    let changed: Vec<(&str, &str)> = d
        .diff
        .changed_tracks
        .iter()
        .flat_map(|c| c.fields.iter().map(move |f| (c.track.name.as_str(), f.field.as_str())))
        .filter(|(_, f)| *f == "rating")
        .collect();
    assert_eq!(changed, [("Daft Punk - One More Time", "rating")]);
    assert_eq!(d.diff.added_playlists, ["Recent", "Shows/", "Shows/Friday"]);
    assert_eq!(d.diff.removed_playlists, ["Friday"]);
    // One playlist only.
    let only = diff_drive(&c, &drive, "Main", Some("Shows")).unwrap();
    assert!(only.diff.added_tracks.is_empty());
    assert_eq!(only.diff.added_playlists, ["Shows/", "Shows/Friday"]);
    // One neither has would show as no differences.
    let err = diff_drive(&c, &drive, "Main", Some("nope/none")).unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err}");
}

/// A drive compared with the collection: tracks at the same file count as found by it, ones moved
/// on the drive are found by ISRC or artist, title and length, and ones only a title finds are
/// listed as uncertain and counted, with the ones the drive lacks, as missing from it. A playlist
/// neither has is refused.
#[hegel::test(test_cases = 15)]
fn prop_a_drive_differs_from_the_collection_in_what_it_lacks(tc: hegel::TestCase) {
    use hegel::generators as gs;
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum On {
        Same,
        Moved,
        Gone,
    }
    const TITLES: &[&str] = &["Aerodynamic", "Nightcall", "Voyager", "Harder", "Veridis"];
    const ARTISTS: &[&str] = &["Daft Punk", "Kavinsky", "Justice", "Moby", "Air"];
    let f = Fixture::new();
    let n = tc.draw(gs::integers::<usize>().min_value(2).max_value(TITLES.len()));
    let mut drawn = Vec::new();
    for i in 0..n {
        // The first track has every detail, the second none, the rest drawn.
        let (artist, isrc, extras) = match i {
            0 => (true, true, true),
            1 => (false, false, false),
            _ => (tc.draw(gs::booleans()), tc.draw(gs::booleans()), tc.draw(gs::booleans())),
        };
        let on = match i {
            0 | 1 => On::Moved,
            _ => tc.draw(gs::sampled_from(vec![On::Same, On::Moved, On::Gone])),
        };
        let id = (i + 1).to_string();
        f.track(&id, &format!("/Users/dj/Music/{}/{}.mp3", ARTISTS[i], TITLES[i]), ARTISTS[i], TITLES[i]);
        if !artist {
            f.conn.execute("UPDATE djmdContent SET ArtistID = NULL WHERE ID = ?1", [&id]).unwrap();
        }
        if isrc {
            f.conn
                .execute("UPDATE djmdContent SET ISRC = ?2 WHERE ID = ?1", params![id, format!("GBDUW00000{i:02}")])
                .unwrap();
        }
        if extras {
            f.exec(&format!("INSERT INTO djmdAlbum (ID, Name) VALUES ('{id}', 'Album {id}')"));
            f.conn
                .execute("UPDATE djmdContent SET AlbumID = ?1, Subtitle = 'Extended Mix' WHERE ID = ?1", [&id])
                .unwrap();
        }
        drawn.push((i as u32 + 1, on, artist || isrc));
    }
    let dir = tempfile::tempdir().unwrap();
    let c = loclib_rekordbox_db::masterdb::open(&write(dir.path(), &f.plain()), DEFAULT_KEY).unwrap();
    let mut drive = c.library.clone();
    drive.tracks.retain(|t| drawn.iter().any(|(id, on, _)| *id == t.id && *on != On::Gone));
    for t in &mut drive.tracks {
        if drawn.iter().any(|(id, on, _)| *id == t.id && *on == On::Moved) {
            t.file_path = format!("/Contents/Moved/{}.mp3", t.id);
        }
    }
    let d = diff_drive(&c, &drive, "Main", None).unwrap();
    let count = |want: &dyn Fn(On, bool) -> bool| drawn.iter().filter(|(_, on, known)| want(*on, *known)).count();
    assert_eq!(d.matched_by_file, count(&|on, _| on == On::Same));
    assert_eq!(d.matched_by_details, count(&|on, known| on == On::Moved && known));
    let uncertain: Vec<u32> = d.uncertain.iter().map(|t| t.id).collect();
    let loose: Vec<u32> =
        drawn.iter().filter(|(_, on, known)| *on == On::Moved && !known).map(|(id, ..)| *id).collect();
    assert_eq!(uncertain, loose);
    let mut missing: Vec<u32> = d.diff.added_tracks.iter().map(|t| t.id).collect();
    missing.sort_unstable();
    let mut lacking: Vec<u32> = drawn
        .iter()
        .filter(|(_, on, known)| *on == On::Gone || (*on == On::Moved && !known))
        .map(|(id, ..)| *id)
        .collect();
    lacking.sort_unstable();
    assert_eq!(missing, lacking);
    assert!(diff_drive(&c, &drive, "Main", Some("Nowhere")).is_err());
    // A drive with exactly the collection's tracks and playlists differs in nothing.
    let same = diff_drive(&c, &c.library, "Main", None).unwrap();
    assert_eq!(same.matched_by_file, c.library.tracks.len());
    let diff = &same.diff;
    assert!(diff.added_tracks.is_empty() && diff.removed_tracks.is_empty() && diff.changed_tracks.is_empty());
    assert!(diff.added_playlists.is_empty() && diff.removed_playlists.is_empty(), "{diff:?}");
}
