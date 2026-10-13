use std::path::Path;

use super::*;
use crate::masterdb::{self, DEFAULT_KEY};
use crate::{anlz, pdb};

/// A rekordbox folder with the collection `masterdb`'s tests use: One More Time with
/// rekordbox's analysis files (the demo export's) and artwork, Never Grow Old in Dropbox
/// through Cloud Library Sync, and Only Here, whose audio isn't on this computer.
struct Computer {
    dir: tempfile::TempDir,
}

impl Computer {
    fn new() -> Computer {
        let dir = tempfile::tempdir().unwrap();
        let rb = dir.path().join("rekordbox");
        let music = dir.path().join("Music");
        let f = masterdb::tests::sample();
        f.exec(&format!(
            "UPDATE djmdContent SET FolderPath = '{}/One More Time.mp3' WHERE ID = '84935471';
                 UPDATE djmdContent SET FolderPath = '/contents_7/Floorplan/ngo.mp3' WHERE ID = '205356547';
                 UPDATE djmdContent SET FolderPath = '{}/Only Here.mp3' WHERE ID = '4';",
            music.display(),
            music.join("gone").display()
        ));
        std::fs::create_dir_all(&rb).unwrap();
        std::fs::write(rb.join("master.db"), f.encrypted(DEFAULT_KEY)).unwrap();
        std::fs::create_dir_all(&music).unwrap();
        std::fs::write(music.join("One More Time.mp3"), b"one more time").unwrap();
        let synced = dir.path().join("Dropbox/rekordbox/contents_7/Floorplan/ngo.mp3");
        std::fs::create_dir_all(synced.parent().unwrap()).unwrap();
        std::fs::write(&synced, b"never grow old").unwrap();
        let anlz = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/rekordcrate/demo_tracks/PIONEER/USBANLZ/P016/0000875E");
        let share = rb.join("share/PIONEER/USBANLZ/P016/0000875E");
        std::fs::create_dir_all(&share).unwrap();
        for e in std::fs::read_dir(anlz).unwrap() {
            let e = e.unwrap();
            std::fs::copy(e.path(), share.join(e.file_name())).unwrap();
        }
        let art = rb.join("share/PIONEER/Artwork/00001");
        std::fs::create_dir_all(&art).unwrap();
        std::fs::write(art.join("a1.jpg"), b"jpeg").unwrap();
        std::fs::write(art.join("a1_m.jpg"), b"medium jpeg").unwrap();
        Computer { dir }
    }

    fn collection(&self) -> Collection {
        masterdb::open(&self.dir.path().join("rekordbox/master.db"), DEFAULT_KEY).unwrap()
    }

    fn cloud(&self) -> CloudFolders {
        CloudFolders { dropbox: Some(self.dir.path().join("Dropbox")), google_drive: None }
    }

    /// An empty folder to export to.
    fn folder(&self, name: &str) -> Device {
        let root = self.dir.path().join(name);
        std::fs::create_dir_all(&root).unwrap();
        Device::open(&root).unwrap()
    }

    fn export(
        &self,
        target: &mut Device,
        opts: &CollectionExportOptions,
    ) -> (CollectionExportPlan, CollectionExportReport) {
        let plan = plan(&self.collection(), &self.cloud(), target, opts, &Ctx::silent()).unwrap();
        let report = apply(&plan, target, None, &Ctx::silent()).unwrap();
        (plan, report)
    }
}

#[test]
fn exports_the_collection_to_a_folder_players_read() {
    let pc = Computer::new();
    let mut target = pc.folder("Export");
    let (plan, report) = pc.export(&mut target, &CollectionExportOptions::default());
    assert_eq!(plan.missing, ["Somebody - Only Here"]);
    assert!(plan.warnings.iter().any(|w| w.contains("intelligent playlist")), "{:?}", plan.warnings);
    assert_eq!((report.tracks, report.playlists), (2, 1));

    let one = target.library_plus().unwrap();
    for lib in [pdb::read_library(&target.path(paths::EXPORT_PDB)).unwrap(), one.library().unwrap()] {
        let paths: Vec<&str> = lib.tracks.iter().map(|t| t.file_path.as_str()).collect();
        assert!(paths.contains(&"/Contents/Daft Punk/Discovery/One More Time.mp3"), "{paths:?}");
        assert!(paths.contains(&"/Contents/Floorplan/UnknownAlbum/ngo.mp3"), "{paths:?}");
        let friday = lib.find_playlist("Shows/Friday").unwrap();
        assert_eq!(friday.track_ids.len(), 2);
        assert!(lib.find_playlist("Recent").is_none(), "intelligent playlists stay behind");
        assert!(lib.history.is_empty());
        for t in &lib.tracks {
            let audio = std::fs::read(target.path(&paths::key(&t.file_path))).unwrap();
            assert!(audio == b"one more time" || audio == b"never grow old");
        }
    }
    let lib = target.library().unwrap();
    let t = lib.tracks.iter().find(|t| t.title == "One More Time").unwrap();
    let dat = std::fs::read(target.path(&paths::key(&t.analyze_path))).unwrap();
    assert_eq!(anlz::read_path(&dat).unwrap().as_deref(), Some(t.file_path.as_str()));
    for ext in ["EXT", "2EX"] {
        let other = t.analyze_path.replace(".DAT", &format!(".{ext}"));
        let bytes = std::fs::read(target.path(&paths::key(&other))).unwrap();
        assert_eq!(anlz::read_path(&bytes).unwrap().as_deref(), Some(t.file_path.as_str()));
    }
    let art = t.artwork_path.as_deref().unwrap();
    assert_eq!(art, "/PIONEER/Artwork/00001/a1.jpg");
    assert_eq!(std::fs::read(target.path(&medium_variant(&paths::key(art)))).unwrap(), b"medium jpeg");
    let ext = std::fs::read(target.path(&paths::key(&t.analyze_path.replace(".DAT", ".EXT")))).unwrap();
    assert_eq!(report.cues, anlz::read_cue_points(&dat, Some(&ext)).unwrap().len(), "OneLibrary's cue rows");

    let tags = crate::tags::read(&target).unwrap();
    let names: Vec<(&str, Vec<&str>)> =
        tags.categories.iter().map(|c| (c.name.as_str(), c.tags.iter().map(|t| t.name.as_str()).collect())).collect();
    assert_eq!(names, [("Genre", vec!["Deep"]), ("Situation", vec!["Peak"])]);
    assert_eq!(report.tagged_tracks, 2);
    assert_eq!(record(&target).unwrap().unwrap().collection, plan.collection);
}

#[test]
fn exporting_again_copies_only_what_changed() {
    let pc = Computer::new();
    let mut target = pc.folder("Export");
    let (first, _) = pc.export(&mut target, &CollectionExportOptions::default());
    assert!(first.unchanged == 0 && !first.copies.is_empty());
    let (again, report) = pc.export(&mut target, &CollectionExportOptions::default());
    assert!(again.copies.is_empty(), "{:?}", again.copies);
    assert_eq!((again.unchanged, report.copied), (first.copies.len(), 0));

    // Audio rekordbox's collection points at another file.
    std::fs::write(pc.dir.path().join("Music/One More Time.mp3"), b"remastered").unwrap();
    filetime::set_file_mtime(pc.dir.path().join("Music/One More Time.mp3"), filetime::FileTime::from_unix_time(1, 0))
        .unwrap();
    let (changed, _) = pc.export(&mut target, &CollectionExportOptions::default());
    assert_eq!(
        changed.copies.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
        ["Contents/Daft Punk/Discovery/One More Time.mp3"]
    );
}

#[test]
fn exports_chosen_playlists_and_remembers_them() {
    let pc = Computer::new();
    let mut target = pc.folder("Export");
    pc.export(&mut target, &CollectionExportOptions::default());
    let opts = CollectionExportOptions { playlists: vec!["Shows".into()], ..Default::default() };
    let (plan, _) = pc.export(&mut target, &opts);
    assert_eq!(plan.tracks, 2);
    assert!(plan.missing.is_empty(), "Only Here isn't in Shows");
    assert_eq!(record(&target).unwrap().unwrap().playlists, ["Shows"]);
    let (again, _) = pc.export(&mut target, &CollectionExportOptions::default());
    assert_eq!(again.playlists, ["Shows"], "the last selection");
    let err = plan_err(&pc, &target, &CollectionExportOptions { playlists: vec!["Nope".into()], ..Default::default() });
    assert!(err.to_string().contains("Nope"), "{err}");
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        match e.file_type().unwrap().is_dir() {
            true => copy_dir(&e.path(), &to.join(e.file_name())),
            false => {
                std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }
    }
}

fn plan_err(pc: &Computer, target: &Device, opts: &CollectionExportOptions) -> Error {
    plan(&pc.collection(), &pc.cloud(), target, opts, &Ctx::silent()).unwrap_err()
}

#[test]
fn a_library_not_exported_from_the_collection_is_only_replaced_when_asked() {
    let pc = Computer::new();
    let root = pc.dir.path().join("Stick");
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rekordcrate/demo_tracks"), &root);
    let mut target = Device::open(&root).unwrap();
    let err = plan_err(&pc, &target, &CollectionExportOptions::default());
    assert!(err.to_string().contains("wasn't exported from rekordbox's collection"), "{err}");
    let (plan, report) = pc.export(&mut target, &CollectionExportOptions { replace: true, ..Default::default() });
    assert!(plan.replaces_library);
    assert!(report.backup.is_some(), "what it replaced is backed up");
    assert_eq!(target.library().unwrap().tracks.len(), 2);
}

#[test]
fn loose_files_are_only_removed_when_asked_and_all_brings_everything_back() {
    let pc = Computer::new();
    let mut target = pc.folder("Stick");
    std::fs::create_dir_all(target.path("Contents/Mine")).unwrap();
    std::fs::write(target.path("Contents/Mine/demo.mp3"), b"mine").unwrap();
    let err = plan_err(&pc, &target, &CollectionExportOptions::default());
    assert!(err.to_string().contains("files the export would remove"), "{err}");
    assert!(target.path("Contents/Mine/demo.mp3").is_file());
    pc.export(&mut target, &CollectionExportOptions { replace: true, ..Default::default() });
    assert!(!target.path("Contents/Mine/demo.mp3").exists());

    pc.export(&mut target, &CollectionExportOptions { playlists: vec!["Shows".into()], ..Default::default() });
    let (all, _) = pc.export(&mut target, &CollectionExportOptions { all: true, ..Default::default() });
    assert!(all.playlists.is_empty());
    assert!(record(&target).unwrap().unwrap().playlists.is_empty(), "everything, from now on");
}

#[test]
fn changes_since_the_last_export_are_saved_first() {
    let pc = Computer::new();
    let mut target = pc.folder("Export");
    pc.export(&mut target, &CollectionExportOptions::default());
    // A player wrote to it: the analysis file of a track it played.
    let lib = target.library().unwrap();
    let dat = target.path(&paths::key(&lib.tracks[0].analyze_path));
    let mut bytes = std::fs::read(&dat).unwrap();
    bytes.push(0);
    std::fs::write(&dat, bytes).unwrap();

    let planned = plan(&pc.collection(), &pc.cloud(), &target, &Default::default(), &Ctx::silent()).unwrap();
    assert!(planned.unsaved.as_ref().is_some_and(|u| u.changed), "{:?}", planned.unsaved);
    let err = apply(&planned, &mut target, None, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("keeping it needs the store"), "{err}");
    let store = crate::store::Store::open_or_init(pc.dir.path().join("store")).unwrap();
    let report = apply(&planned, &mut target, Some(&store), &Ctx::silent()).unwrap();
    assert!(report.saved.is_some(), "the changed state is in the store");
    let again = plan(&pc.collection(), &pc.cloud(), &target, &Default::default(), &Ctx::silent()).unwrap();
    assert!(again.unsaved.is_none());
}

/// A OneLibrary on the target that can't be read may hold what a CDJ-3000X recorded there,
/// which the export would build over: it's saved in the store first, as pack saves one.
#[test]
fn a_onelibrary_that_cant_be_read_is_saved_first() {
    let pc = Computer::new();
    let mut target = pc.folder("Export");
    pc.export(&mut target, &CollectionExportOptions::default());
    std::fs::write(target.path(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();

    let planned = plan(&pc.collection(), &pc.cloud(), &target, &Default::default(), &Ctx::silent()).unwrap();
    assert!(planned.unsaved.as_ref().is_some_and(|u| u.unreadable_onelibrary), "{:?}", planned.unsaved);
    let err = apply(&planned, &mut target, None, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("keeping it needs the store"), "{err}");
    assert_eq!(std::fs::read(target.path(paths::LIBRARY_PLUS_DB)).unwrap(), b"not a database");
}

#[test]
fn an_unreadable_analysis_file_leaves_its_track_without_it() {
    let pc = Computer::new();
    let ext = pc.dir.path().join("rekordbox/share/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.EXT");
    std::fs::write(&ext, b"not an analysis file").unwrap();
    let mut target = pc.folder("Export");
    let (planned, _) = pc.export(&mut target, &CollectionExportOptions::default());
    assert_eq!(planned.unreadable, ["Daft Punk - One More Time"]);
    let lib = target.library().unwrap();
    let t = lib.tracks.iter().find(|t| t.title == "One More Time").unwrap();
    assert!(target.path(&paths::key(&t.analyze_path)).is_file(), "its .DAT still comes");
    assert!(!target.path(&paths::key(&t.analyze_path.replace(".DAT", ".EXT"))).exists());
}

#[test]
fn an_image_keeps_its_key_from_one_export_to_the_next() {
    let pc = Computer::new();
    let art = pc.dir.path().join("rekordbox/share/PIONEER/Artwork/00002");
    std::fs::create_dir_all(&art).unwrap();
    std::fs::write(art.join("b.jpg"), b"floorplan").unwrap();
    let with_art = |first: bool| {
        let mut c = pc.collection();
        for t in &mut c.library.tracks {
            t.artwork_path = match t.title.as_str() {
                "One More Time" if first => None,
                "Never Grow Old" => Some("/PIONEER/Artwork/00002/b.jpg".into()),
                _ => t.artwork_path.clone(),
            };
        }
        c
    };
    let mut target = pc.folder("Export");
    let first = plan(&with_art(true), &pc.cloud(), &target, &Default::default(), &Ctx::silent()).unwrap();
    apply(&first, &mut target, None, &Ctx::silent()).unwrap();
    let art_of = |target: &Device, title: &str| {
        target.library().unwrap().tracks.into_iter().find(|t| t.title == title).unwrap().artwork_path
    };
    assert_eq!(art_of(&target, "Never Grow Old").as_deref(), Some("/PIONEER/Artwork/00001/a1.jpg"));
    // One More Time, earlier in the collection, gets artwork: Never Grow Old's keeps its key.
    let second = plan(&with_art(false), &pc.cloud(), &target, &Default::default(), &Ctx::silent()).unwrap();
    assert!(second.copies.iter().all(|c| c.path != "PIONEER/Artwork/00001/a1.jpg"), "{:?}", second.copies);
    apply(&second, &mut target, None, &Ctx::silent()).unwrap();
    assert_eq!(art_of(&target, "Never Grow Old").as_deref(), Some("/PIONEER/Artwork/00001/a1.jpg"));
    assert_eq!(art_of(&target, "One More Time").as_deref(), Some("/PIONEER/Artwork/00001/a2.jpg"));
}

#[test]
fn an_export_cut_off_before_it_was_recorded_is_finished_without_asking() {
    let pc = Computer::new();
    let mut target = pc.folder("Export");
    pc.export(&mut target, &CollectionExportOptions::default());
    forget(&target).unwrap();
    target.begin(LineageOp::Export, "export rekordbox's collection", None).unwrap();
    let planned = plan(&pc.collection(), &pc.cloud(), &target, &Default::default(), &Ctx::silent()).unwrap();
    assert!(!planned.replaces_library);
    apply(&planned, &mut target, None, &Ctx::silent()).unwrap();
    assert!(target.finished().is_ok() && record(&target).unwrap().is_some());
}

#[test]
fn says_what_cloud_library_sync_has_still_to_bring() {
    let pc = Computer::new();
    let collection = {
        let mut c = pc.collection();
        c.synced_sizes.insert("/pioneer/usbanlz/p016/0000875e/anlz0000.ext".into(), 1);
        c
    };
    let target = pc.folder("Export");
    let planned = plan(&collection, &pc.cloud(), &target, &CollectionExportOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(planned.syncing, ["Daft Punk - One More Time"]);
    let gone =
        plan(&collection, &CloudFolders::default(), &target, &CollectionExportOptions::default(), &Ctx::silent())
            .unwrap();
    assert!(gone.missing.contains(&"Floorplan - Never Grow Old".to_string()), "{:?}", gone.missing);
}

/// A drawn collection (tracks whose audio may not be on this computer, some with rekordbox's
/// analysis files, playlists in and out of a folder, a chosen few of them or all) exports as a
/// library holding exactly the chosen tracks found here, their audio byte for byte, the
/// analysis naming each track's new path, and the playlists in their order without the tracks
/// left out. Exporting again copies nothing.
#[hegel::test(test_cases = 4, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_drawn_collection_exports_exactly_what_is_here(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let rb = dir.path().join("rekordbox");
    let music = dir.path().join("Music");
    std::fs::create_dir_all(&music).unwrap();
    let anlz = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/rekordcrate/demo_tracks/PIONEER/USBANLZ/P016/0000875E");
    let share = rb.join("share/PIONEER/USBANLZ/P016/0000875E");
    std::fs::create_dir_all(&share).unwrap();
    for e in std::fs::read_dir(anlz).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), share.join(e.file_name())).unwrap();
    }

    let f = masterdb::tests::Fixture::new();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(5));
    let mut here = Vec::new();
    for i in 0..n {
        let artist = tc.draw(gs::sampled_from(vec!["Daft Punk", "Ünï", "A/B"]));
        let file = music.join(format!("Track {i}.mp3"));
        f.track(&(i + 1).to_string(), &file.to_string_lossy(), artist, &format!("Track {i}"));
        if tc.draw(gs::booleans()) {
            f.exec(&format!(
                    "UPDATE djmdContent SET AnalysisDataPath = '/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT' WHERE ID = '{}'",
                    i + 1
                ));
        }
        let present = tc.draw(gs::weighted_booleans(0.8));
        if present {
            std::fs::write(&file, format!("audio of track {i}")).unwrap();
        }
        here.push(present);
    }
    f.exec("INSERT INTO djmdPlaylist (ID, Seq, Name, Attribute, ParentID) VALUES ('100', 0, 'Folder', 1, 'root')");
    let lists = tc.draw(gs::integers::<usize>().max_value(3));
    let mut names = Vec::new();
    for p in 0..lists {
        let in_folder = tc.draw(gs::booleans());
        let parent = if in_folder { "100" } else { "root" };
        f.exec(&format!(
                "INSERT INTO djmdPlaylist (ID, Seq, Name, Attribute, ParentID) VALUES ('{}', {p}, 'List {p}', 0, '{parent}')",
                200 + p
            ));
        for (e, k) in tc.draw(gs::vecs(gs::integers::<usize>().max_value(n - 1)).max_size(4)).into_iter().enumerate() {
            f.exec(&format!(
                "INSERT INTO djmdSongPlaylist (ID, PlaylistID, ContentID, TrackNo) VALUES ('{p}-{e}', '{}', '{}', {e})",
                200 + p,
                k + 1
            ));
        }
        names.push(if in_folder { format!("Folder/List {p}") } else { format!("List {p}") });
    }
    std::fs::create_dir_all(&rb).unwrap();
    std::fs::write(rb.join("master.db"), f.plain()).unwrap();
    let collection = masterdb::open(&rb.join("master.db"), DEFAULT_KEY).unwrap();
    let chosen: Vec<String> = names.iter().filter(|_| tc.draw(gs::booleans())).cloned().collect();

    let root = dir.path().join("Export");
    std::fs::create_dir_all(&root).unwrap();
    let mut target = Device::open(&root).unwrap();
    let opts = CollectionExportOptions { playlists: chosen.clone(), ..Default::default() };
    let cloud = CloudFolders::default();
    let planned = plan(&collection, &cloud, &target, &opts, &Ctx::silent()).unwrap();
    let report = apply(&planned, &mut target, None, &Ctx::silent()).unwrap();

    // What should be there: the chosen playlists' tracks (or all), less those not here.
    let lib = &collection.library;
    let shown: Vec<&crate::model::Playlist> = lib
        .playlists
        .iter()
        .filter(|p| !p.is_folder && (chosen.is_empty() || chosen.contains(&lib.playlist_path(p.id))))
        .collect();
    let used: FxHashSet<u32> = shown.iter().flat_map(|p| p.track_ids.iter().copied()).collect();
    let in_scope: Vec<&Track> = lib.tracks.iter().filter(|t| chosen.is_empty() || used.contains(&t.id)).collect();
    let present = |t: &Track| here[t.id as usize - 1];
    let mut missing: Vec<String> = in_scope.iter().filter(|t| !present(t)).map(|t| t.display_name()).collect();
    let mut said = planned.missing.clone();
    missing.sort();
    said.sort();
    assert_eq!(said, missing);
    assert_eq!(report.tracks, in_scope.len() - missing.len());

    let exported = target.library().unwrap();
    let mut titles: Vec<&str> = exported.tracks.iter().map(|t| t.title.as_str()).collect();
    let mut want: Vec<&str> = in_scope.iter().filter(|t| present(t)).map(|t| t.title.as_str()).collect();
    titles.sort();
    want.sort();
    assert_eq!(titles, want);
    for t in &exported.tracks {
        let i: usize = t.title.strip_prefix("Track ").unwrap().parse().unwrap();
        assert_eq!(
            std::fs::read(target.path(&paths::key(&t.file_path))).unwrap(),
            format!("audio of track {i}").into_bytes()
        );
        if !t.analyze_path.is_empty() {
            let dat = std::fs::read(target.path(&paths::key(&t.analyze_path))).unwrap();
            assert_eq!(anlz::read_path(&dat).unwrap().as_deref(), Some(t.file_path.as_str()));
        }
    }
    let title_of = |l: &Library, id: u32| l.track(id).unwrap().title.clone();
    for p in &shown {
        let path = lib.playlist_path(p.id);
        let got = exported.find_playlist(&path).unwrap_or_else(|| panic!("{path} wasn't exported"));
        let want: Vec<String> =
            p.track_ids.iter().filter(|id| here[**id as usize - 1]).map(|id| title_of(lib, *id)).collect();
        assert_eq!(got.track_ids.iter().map(|id| title_of(&exported, *id)).collect::<Vec<_>>(), want);
    }
    assert_eq!(exported.playlists.iter().filter(|p| !p.is_folder).count(), shown.len());
    assert_eq!(record(&target).unwrap().unwrap().playlists, chosen);

    let again = plan(&collection, &cloud, &target, &CollectionExportOptions::default(), &Ctx::silent()).unwrap();
    assert!(again.copies.is_empty() && again.deletes.is_empty(), "{:?} {:?}", again.copies, again.deletes);
    assert_eq!(again.playlists, chosen, "the last selection");
}

/// What a target can hold before an export, and how the export takes it: nothing (it's made), a
/// library not exported from the collection or the DJ's own files (refused unless replacing,
/// then backed up first), or an earlier export a player wrote to or whose OneLibrary can't be
/// read (saved in the store first, refused without one). A plan with no room is refused; the
/// collection's artwork and what Cloud Library Sync hasn't brought yet are said.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_export_takes_what_the_target_holds_as_it_should(tc: hegel::TestCase) {
    for variant in 0..5 {
        an_export_takes_what_the_target_holds_as_it_should_with(&tc, variant);
    }
}

/// One case of [`an_export_takes_what_the_target_holds_as_it_should`], `variant` choosing what's drawn.
#[hegel::test_helper]
fn an_export_takes_what_the_target_holds_as_it_should_with(tc: &hegel::TestCase, variant: u8) {
    use hegel::generators as gs;
    let pc = Computer::new();
    let mut target = pc.folder("Stick");
    let before = variant;
    match before {
        0 => {}
        1 => {
            std::fs::remove_dir(target.root()).unwrap();
            copy_dir(
                &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rekordcrate/demo_tracks"),
                target.root(),
            );
            target = Device::open(target.root()).unwrap();
        }
        2 => {
            std::fs::create_dir_all(target.path("Contents/Mine")).unwrap();
            std::fs::write(target.path("Contents/Mine/demo.mp3"), b"mine").unwrap();
        }
        3 => {
            pc.export(&mut target, &CollectionExportOptions::default());
            let lib = target.library().unwrap();
            let dat = target.path(&paths::key(&lib.tracks[0].analyze_path));
            let mut bytes = std::fs::read(&dat).unwrap();
            bytes.push(0);
            std::fs::write(&dat, bytes).unwrap();
        }
        _ => {
            pc.export(&mut target, &CollectionExportOptions::default());
            std::fs::write(target.path(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();
        }
    }
    let replace = tc.draw(gs::booleans());
    let store =
        tc.draw(gs::booleans()).then(|| crate::store::Store::open_or_init(pc.dir.path().join("store")).unwrap());
    let mut collection = pc.collection();
    let syncing = tc.draw(gs::booleans());
    if syncing {
        collection.synced_sizes.insert("/pioneer/usbanlz/p016/0000875e/anlz0000.ext".into(), 1);
    }
    let opts = CollectionExportOptions { replace, ..Default::default() };
    // Every file below the target and its contents, but for locallibrary's own (its scan index).
    let files = |root: &Path| -> Vec<(PathBuf, Vec<u8>)> {
        walkdir::WalkDir::new(root)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|e| e.file_name() != paths::STATE_DIR)
            .flatten()
            .filter(|e| e.file_type().is_file())
            .map(|e| (e.path().to_path_buf(), std::fs::read(e.path()).unwrap()))
            .collect()
    };
    let files_before = files(target.root());
    let planned = plan(&collection, &pc.cloud(), &target, &opts, &Ctx::silent());
    let not_ours = matches!(before, 1 | 2);
    let planned = match planned {
        Err(e) => {
            assert!(not_ours && !replace, "{e}");
            assert_eq!(e.reason(), Some(Reason::Exists), "{e}");
            let retry: Vec<_> = e.detail().actions.iter().map(|a| a.params.get("replace").cloned()).collect();
            assert_eq!(retry, [Some(serde_json::json!(true))]);
            return;
        }
        Ok(p) => p,
    };
    assert!(!not_ours || replace, "planned over what isn't the collection's export");
    assert_eq!(planned.replaces_library, not_ours);
    assert_eq!(planned.syncing.is_empty(), !syncing);
    assert_eq!(
        planned.warnings.iter().any(|w| w.contains("hasn't finished bringing")),
        syncing,
        "{:?}",
        planned.warnings
    );
    assert_eq!(planned.unsaved.is_some(), matches!(before, 3 | 4), "{:?}", planned.unsaved);

    let mut full = planned.clone();
    full.fits = false;
    assert!(matches!(apply(&full, &mut target, store.as_ref(), &Ctx::silent()), Err(Error::NoSpace { .. })));
    match apply(&planned, &mut target, store.as_ref(), &Ctx::silent()) {
        Err(e) => {
            assert!(planned.unsaved.is_some() && store.is_none(), "{e}");
            assert_eq!(e.reason(), Some(Reason::NotSaved), "{e}");
            let methods: Vec<_> = e.detail().actions.iter().map(|a| a.method.clone()).collect();
            assert_eq!(methods, [Some("pull".to_string())]);
            assert!(files(target.root()) == files_before, "refused, it leaves the target alone");
        }
        Ok(report) => {
            assert_eq!(report.saved.is_some(), planned.unsaved.is_some());
            assert_eq!(report.backup.is_some(), before != 0 && before != 2, "{report:?}");
            let lib = target.library().unwrap();
            let t = lib.tracks.iter().find(|t| t.title == "One More Time").unwrap();
            let art = t.artwork_path.as_deref().expect("its artwork");
            assert_eq!(std::fs::read(target.path(&paths::key(art))).unwrap(), b"jpeg");
            assert_eq!(std::fs::read(target.path(&medium_variant(&paths::key(art)))).unwrap(), b"medium jpeg");
            assert!(!target.path("Contents/Mine/demo.mp3").exists(), "the DJ's files go when replacing");
        }
    }
}

/// What an export plan says it leaves out and needs: analysis files that aren't rekordbox's
/// (drawn among a track's) are left behind with a warning, the track keeping the ones that read;
/// an image two tracks share is copied once; a library it replaces is said; `all` exports every
/// playlist whatever was chosen; and an export too big for the drive says so and is refused
/// when applied, the drive left as it was.
#[hegel::test(test_cases = 6)]
fn an_export_plan_says_what_it_leaves_out_and_what_it_needs(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let pc = Computer::new();
    let anlz = pc.dir.path().join("rekordbox/share/PIONEER/USBANLZ/P016/0000875E");
    let mut broken: Vec<&str> = ANALYSIS_EXTENSIONS.into_iter().filter(|_| tc.draw(gs::booleans())).collect();
    if broken.is_empty() {
        broken.push(tc.draw(gs::sampled_from(ANALYSIS_EXTENSIONS.to_vec())));
    }
    for ext in &broken {
        let junk = tc.draw(gs::text().alphabet("xyz \u{0}é").max_size(40));
        std::fs::write(anlz.join(format!("ANLZ0000.{ext}")), format!("not an analysis file: {junk}")).unwrap();
    }
    let mut collection = pc.collection();
    let image = collection.library.track(84935471).unwrap().artwork_path.clone();
    let shared = tc.draw(gs::booleans());
    if shared {
        collection.library.tracks.iter_mut().find(|t| t.id == 205356547).unwrap().artwork_path = image.clone();
    }
    let root = pc.dir.path().join("Stick");
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rekordcrate/demo_tracks"), &root);
    let target = Device::open(&root).unwrap();
    let chosen = tc.draw(gs::sampled_from(vec![vec![], vec!["Shows".to_string()], vec!["Nowhere".to_string()]]));
    let opts = CollectionExportOptions { replace: true, all: true, playlists: chosen, ..Default::default() };
    let planned = plan(&collection, &pc.cloud(), &target, &opts, &Ctx::silent()).unwrap();
    assert!(!planned.is_noop(), "the databases are always rebuilt");
    assert!(planned.playlists.is_empty(), "every playlist, whatever was chosen");
    assert_eq!(planned.unreadable, ["Daft Punk - One More Time"]);
    let says = |what: &str| planned.warnings.iter().any(|w| w.contains(what));
    assert!(says("1 track(s) have analysis files that can't be read (e.g. Daft Punk - One More Time)"));
    assert!(planned.replaces_library && says("holds a library or files the export replaces"), "{:?}", planned.warnings);
    let one = planned.library.tracks.iter().find(|t| t.title == "One More Time").unwrap();
    assert_eq!(one.analyze_path.is_empty(), broken.contains(&"DAT"), "{broken:?}: {}", one.analyze_path);
    let dir = one.file_path.clone();
    let analysis: Vec<&str> =
        planned.copies.iter().filter_map(|c| c.path.rsplit_once("/ANLZ0000.").map(|(_, ext)| ext)).collect();
    for ext in ANALYSIS_EXTENSIONS {
        assert_eq!(analysis.contains(&ext), !broken.contains(&ext), "{dir}: {ext} of {analysis:?}");
    }
    let images: Vec<&Option<String>> = planned.library.tracks.iter().map(|t| &t.artwork_path).collect();
    let art_copies = planned.copies.iter().filter(|c| c.path.starts_with("PIONEER/Artwork/")).count();
    assert_eq!(art_copies, 2, "the image and its larger copy, once: {:?}", planned.copies);
    if shared {
        assert!(images[0].is_some() && images[0] == images[1], "{images:?}");
    }

    // Sparse only on Unix: NTFS would have to find the room for it.
    #[cfg(unix)]
    {
        // Audio far bigger than any drive.
        let audio = pc.dir.path().join("Music/One More Time.mp3");
        std::fs::OpenOptions::new().write(true).open(&audio).unwrap().set_len(1 << 50).unwrap();
        let mut folder = pc.folder("Small");
        let planned =
            plan(&collection, &pc.cloud(), &folder, &CollectionExportOptions::default(), &Ctx::silent()).unwrap();
        assert!(!planned.fits && planned.needed_bytes > 1 << 50, "{planned:?}");
        assert!(planned.warnings.iter().any(|w| w.starts_with("the export needs about")), "{:?}", planned.warnings);
        assert!(matches!(apply(&planned, &mut folder, None, &Ctx::silent()), Err(Error::NoSpace { .. })));
        assert_eq!(std::fs::read_dir(folder.root()).unwrap().count(), 0, "the drive is left as it was");
    }
}
