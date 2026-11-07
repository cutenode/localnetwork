//! The demos plugin's `clean` on drives: the plugin served over pipes, reading the drive as the
//! host reads it for a plugin, and its edits planned and applied as the host plans a plugin's.

use crate::common;

use std::fs;
use std::path::Path;

use locallibrary::api::{self, DriveRef, LibraryCache, LibraryRequest, UndoRequest};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions, EditPlan};
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::tags::TagChange;
use locallibrary::{Error, backup, paths};
use loclib_plugin::{Incoming, OpPlan, PROTOCOL_VERSION, Peer, RpcError, method};
use rekordbox_pdb::{Database, NewTrack, PdbEditor};
use serde_json::{Value, json};

pub const SAMPLES: &str = "/Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES";

/// What the demos plugin's `clean` plans on the drive at `root`, as the host plans it.
pub fn clean_plan(root: &Path) -> EditPlan {
    try_clean_plan(root).unwrap()
}

/// [`clean_plan`], or why the plugin or the host couldn't plan it.
pub fn try_clean_plan(root: &Path) -> Result<EditPlan, String> {
    let (plugin_in, to_plugin) = std::io::pipe().unwrap();
    let (from_plugin, plugin_out) = std::io::pipe().unwrap();
    let served = std::thread::spawn(move || locallibrary_plugin_demos::serve(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let to = host.clone();
    let answering = std::thread::spawn(move || {
        for msg in incoming {
            let Incoming::Request(req) = msg else { continue };
            to.respond(&req.id, read(&req.method, req.params));
        }
    });
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "locallibrary", "version": "0.1.0" } });
    host.request::<_, Value>(method::HELLO, &hello).unwrap();
    let op = json!({ "operation": "clean", "context": { "drive": common::path(root) } });
    let plan: Result<OpPlan, RpcError> = host.request(method::PLAN_OP, &op);
    host.close();
    served.join().unwrap();
    answering.join().unwrap();
    let plan = plan.map_err(|e| e.message)?;
    let edits: Vec<Edit> = plan.edits.into_iter().map(|e| serde_json::from_value(e).unwrap()).collect();
    let device = Device::open_export(root).map_err(|e| e.to_string())?;
    let mut planned =
        edit::plan(&device, &edits, &EditOptions::default(), &Ctx::silent()).map_err(|e| e.to_string())?;
    planned.warnings.extend(plan.warnings);
    Ok(planned)
}

/// The library read the plugin may make, as the host answers it.
fn read(method: &str, params: Value) -> Result<Value, RpcError> {
    assert_eq!(method, "drive.library", "the only read it makes");
    let req: LibraryRequest = serde_json::from_value(params).unwrap();
    let page =
        api::drive_library_cached(LibraryCache::none(), &req).map_err(|e| RpcError::failed("io", e.to_string()))?;
    Ok(serde_json::to_value(page).unwrap())
}

/// `clean_plan`, applied.
pub fn clean(root: &Path) -> (EditPlan, edit::EditReport) {
    let plan = clean_plan(root);
    let report = edit::apply(&plan, &mut Device::open_export(root).unwrap(), &Ctx::silent()).unwrap();
    (plan, report)
}

/// Add a track the way rekordbox exports one, with audio and analysis files. Returns its id.
fn export(root: &Path, artist: &str, title: &str, album: Option<&str>, file: &str, anlz: u32) -> u32 {
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    let analyze = format!("/PIONEER/USBANLZ/P0AA/{anlz:08X}/ANLZ0000.DAT");
    let id = ed
        .add_track(&NewTrack {
            artist: Some(artist.into()),
            album: album.map(Into::into),
            genre: album.map(|_| "Loop Samples".into()),
            analyze_path: analyze.clone(),
            file_size: 600,
            ..NewTrack::new(title, file)
        })
        .unwrap();
    ed.save(&pdb_path).unwrap();
    for (key, bytes) in [
        (paths::key(file), common::placeholder_audio(file)),
        (paths::key(&analyze), common::synthetic_anlz(file)),
        (paths::key(&analyze.replace(".DAT", ".EXT")), common::synthetic_anlz(file)),
    ] {
        let p = paths::to_fs(root, &key);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
    id
}

/// The demo tracks go from both databases with their entries, files and the album and genre
/// only they used (entries renumbered without gaps); the DJ's track by rekordbox, its artist and
/// its files stay; undoing brings the tracks back, without the files the backup left out.
#[test]
fn demo_tracks_go_from_both_databases_and_come_back_on_undo_without_their_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let album = Some("GROOVE CIRCUIT FACTORY SAMPLES");
    let house = export(&root, "rekordbox", "House 1", album, &format!("{SAMPLES}/House1.wav"), 1);
    export(&root, "rekordbox", "House 1", album, &format!("{SAMPLES}/House1-1.wav"), 2);
    let techno = export(&root, "rekordbox", "Techno 1", album, &format!("{SAMPLES}/Techno1.wav"), 3);
    // Same artist, but not a demo track: stays, and keeps the "rekordbox" artist row alive.
    export(&root, "rekordbox", "Keeper", None, "/Contents/rekordbox/Keeper.mp3", 4);
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    let playlist = ed.database().unwrap().playlist_tree.iter().find(|n| n.name == "aab").unwrap().id;
    ed.add_to_playlist(playlist, techno, Some(5)).unwrap();
    ed.add_to_playlist(playlist, house, None).unwrap();
    ed.save(&pdb_path).unwrap();
    common::add_onelibrary(&root);
    let kept_before: Vec<_> = common::library_files(&root)
        .into_iter()
        .filter(|(k, _)| !k.contains("GROOVE") && !k.contains("P0AA") && !paths::DATABASE_FILES.contains(&k.as_str()))
        .collect();

    let (plan, report) = clean(&root);
    assert_eq!(plan.tracks_removed.len(), 3);
    assert_eq!(plan.files.len(), 9, "audio, .DAT and .EXT each: {:?}", plan.files);
    assert_eq!(plan.playlists_changed.iter().map(|p| (p.path.as_str(), p.removed)).collect::<Vec<_>>(), [("aab", 2)]);
    assert_eq!(report.files_deleted, 9);

    common::sqlcipher_reads(&root);
    for lib in [common::library(&root), common::plus_library(&root)] {
        assert_eq!(lib.tracks.len(), 13);
        assert!(lib.tracks.iter().any(|t| t.title == "Keeper"));
        assert_eq!(lib.find_playlist("aab").unwrap().track_ids.len(), 12);
    }
    let db = Database::from_bytes(fs::read(&pdb_path).unwrap()).unwrap();
    let mut indexes: Vec<u32> =
        db.playlist_entries.iter().filter(|e| e.playlist_id == playlist).map(|e| e.entry_index).collect();
    indexes.sort_unstable();
    assert_eq!(indexes, (1..=12).collect::<Vec<_>>(), "entries renumbered without gaps");
    assert!(db.artists.iter().any(|a| a.name == "rekordbox"));
    assert!(db.albums.iter().all(|a| a.name != "GROOVE CIRCUIT FACTORY SAMPLES"));
    assert!(db.genres.iter().all(|g| g.name != "Loop Samples"));
    let one = OneLibrary::open_root(&root).unwrap();
    let count = |sql: &str| one.connection().query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(count("SELECT count(*) FROM album WHERE name = 'GROOVE CIRCUIT FACTORY SAMPLES'"), 0);
    assert_eq!(count("SELECT count(*) FROM genre WHERE name = 'Loop Samples'"), 0);
    assert_eq!(count("SELECT count(*) FROM artist WHERE name = 'rekordbox'"), 1);
    // An independent parser agrees.
    let pdb = fs::read(&pdb_path).unwrap();
    assert_eq!(common::rekordcrate_rows(&pdb, rekordcrate::pdb::PageType::Tracks), 13);

    // The files are gone, empty folders pruned, everything else untouched.
    assert!(!root.join("Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES").exists());
    assert!(!root.join("PIONEER/USBANLZ/P0AA/00000001").exists());
    assert!(root.join("PIONEER/USBANLZ/P0AA/00000004/ANLZ0000.DAT").is_file());
    let kept_after: Vec<_> = common::library_files(&root)
        .into_iter()
        .filter(|(k, _)| !k.contains("P0AA") && !paths::DATABASE_FILES.contains(&k.as_str()))
        .collect();
    assert_eq!(kept_after, kept_before);
    assert!(clean_plan(&root).is_noop(), "nothing left to remove");

    let mut d = Device::open_export(&root).unwrap();
    backup::restore(&mut d, report.backup.as_deref(), false).unwrap();
    for lib in [common::library(&root), common::plus_library(&root)] {
        assert_eq!(lib.tracks.len(), 16);
        for t in &lib.tracks {
            let sample = t.file_path.starts_with(SAMPLES);
            assert_eq!(d.path(&paths::key(&t.file_path)).is_file(), !sample, "{}", t.file_path);
        }
    }
}

/// A drive holding one demo sample, in the demo export's playlists' company.
fn drive_with_sample(dir: &Path) -> std::path::PathBuf {
    let root = common::bigger_drive(dir, "Main");
    export(&root, "rekordbox", "House 1", None, &format!("{SAMPLES}/House1.wav"), 1);
    root
}

/// Every file below `dir`.
fn files_below(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut left = vec![dir.to_path_buf()];
    while let Some(d) = left.pop() {
        for e in fs::read_dir(d).unwrap().map(Result::unwrap) {
            match e.file_type().unwrap().is_dir() {
                true => left.push(e.path()),
                false => found.push(e.path().display().to_string()),
            }
        }
    }
    found
}

/// A clean's backup holds the databases: it names the files it left out, and none of them is
/// anywhere in it. The plan says what undo can't do before anything is deleted.
#[test]
fn a_clean_backs_up_only_the_databases() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive_with_sample(dir.path());
    let (plan, report) = clean(&root);
    assert_eq!(plan.files.len(), 3, "audio, .DAT and .EXT: {:?}", plan.files);
    assert_eq!(plan.unbacked, plan.files);
    assert_eq!(plan.changes, ["delete 1 track(s) (rekordbox's next export brings their files back, undo can't)"]);

    let d = Device::open_export(&root).unwrap();
    let b = backup::list(&d).unwrap().remove(0);
    assert_eq!(Some(&b.id), report.backup.as_ref());
    assert_eq!(b.audio, 0);
    assert!(b.files.iter().all(|f| paths::DATABASE_FILES.contains(&f.as_str())), "{:?}", b.files);
    assert_eq!(b.unbacked, plan.files);
    let held = files_below(&backup::dir(&d, &b.id));
    assert!(!held.iter().any(|f| f.contains("GROOVE") || f.contains("P0AA")), "{held:?}");
}

/// A sample too big for the drive to copy doesn't stop a clean: its plan counts only the
/// databases. The same tracks deleted the usual way are backed up, so that plan doesn't fit.
#[test]
fn a_clean_runs_on_a_drive_too_full_to_copy_the_samples() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive_with_sample(dir.path());
    let file = format!("{SAMPLES}/House1.wav");
    let sample = paths::to_fs(&root, &paths::key(&file));
    let free = locallibrary::fsutil::available_space(&root).unwrap();
    let too_big = 2 * free + (1 << 30);
    fs::File::options().write(true).open(&sample).unwrap().set_len(too_big).unwrap();

    let plan = clean_plan(&root);
    assert!(plan.fits && plan.needed_bytes < too_big, "needs {} of {too_big}", plan.needed_bytes);

    let mut d = Device::open_export(&root).unwrap();
    let plain = Edit::DeleteTracks { tracks: TrackSelector::paths([file]), keep_files: false };
    let usual = edit::plan(&d, &[plain], &EditOptions::default(), &Ctx::silent()).unwrap();
    assert!(!usual.fits && usual.needed_bytes > too_big, "needs {}", usual.needed_bytes);
    let refused = edit::apply(&usual, &mut d, &Ctx::silent()).unwrap_err();
    assert!(matches!(refused, Error::NoSpace { .. }), "{refused}");

    // Small again: a copy of the sparse sample, were one to creep back, would fill the real disk.
    fs::File::options().write(true).open(&sample).unwrap().set_len(11).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert!(!sample.exists());
}

/// Undoing a clean needs no force: the plan names the tracks that come back without their audio,
/// and stepping back past it to an older backup does too. Nothing brings the files back, and a
/// clean takes the tracks away again.
#[test]
fn undoing_a_clean_says_what_it_cant_restore() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive_with_sample(dir.path());
    common::apply(&root, &[Edit::CreatePlaylist { path: "Before".into(), folder: false }]);
    let (plan, _) = clean(&root);
    let name = "rekordbox - House 1";
    let undo = || UndoRequest { drive: Some(common::path(&root)), backup: None, force: false };

    let planned = api::undo_plan(&undo()).unwrap();
    assert_eq!(planned.plan.unbacked_audio, [name]);
    assert!(planned.plan.missing_audio.is_empty(), "{:?}", planned.plan.missing_audio);
    let warned = format!(
        "1 track(s) come back without their audio until rekordbox's next export (e.g. {name}): it isn't in any backup"
    );
    assert_eq!(planned.warnings, [warned]);
    let backup = Some(planned.plan.backup.id.clone());
    let preview =
        api::restore_preview(&api::RestoreBackup { drive: common::path(&root), backup, force: false }).unwrap();
    assert_eq!((preview.unbacked_audio.as_slice(), preview.missing_audio.len()), ([name.to_string()].as_slice(), 0));
    planned.apply(None, &Ctx::silent()).unwrap();
    assert!(plan.files.iter().all(|f| !paths::to_fs(&root, f).exists()), "{:?}", plan.files);

    // The backup before the clean lists the track too.
    let older = api::undo_plan(&undo()).unwrap();
    assert_eq!(
        (older.plan.unbacked_audio.as_slice(), older.plan.missing_audio.len()),
        ([name.to_string()].as_slice(), 0)
    );
    older.apply(None, &Ctx::silent()).unwrap();
    assert!(common::library(&root).find_playlist("Before").is_none());

    let again = clean_plan(&root);
    assert_eq!((again.tracks_removed.len(), again.files.len()), (1, 0), "{:?}", again.files);
    edit::apply(&again, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
    assert!(!common::library(&root).tracks.iter().any(|t| t.file_path.starts_with(SAMPLES)));
}

/// `edit.plan` and `edit.apply` take no `delete_unbacked`, whatever the drive: nothing changes.
#[test]
fn edit_plan_refuses_delete_unbacked() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive_with_sample(dir.path());
    let file = format!("{SAMPLES}/House1.wav");
    let tracks = TrackSelector::paths([file.clone()]);
    let req = api::EditRequest {
        drive: common::path(&root),
        edits: vec![Edit::DeleteUnbacked { tracks }],
        options: EditOptions::default(),
    };
    let before = common::Before::take(&root);
    let said = "delete_unbacked is only for plugins with the library:delete-unbacked capability";
    let planned = api::edit_plan(&req, &Ctx::silent()).map(drop).unwrap_err();
    assert!(planned.to_string().contains(said), "{planned}");
    let applied = api::edit_apply(&req, &Ctx::silent()).map(drop).unwrap_err();
    assert!(applied.to_string().contains(said), "{applied}");
    before.untouched(&root).unwrap();
    assert!(paths::to_fs(&root, &paths::key(&file)).exists());
}

/// A clean is recorded as it was made, so replaying it after an export leaves the audio out of
/// that backup too, also when a My Tag edit makes the plan one of steps.
#[test]
fn replaying_a_clean_leaves_the_audio_out_of_its_backup_too() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive_with_sample(dir.path());
    let file = format!("{SAMPLES}/House1.wav");
    let (_, report) = clean(&root);
    let d = Device::open_export(&root).unwrap();
    let recorded = edit::journal::list(&d).unwrap();
    let one = recorded.iter().find(|e| e.backup == report.backup).unwrap();
    assert!(matches!(one.edits.as_slice(), [Edit::DeleteUnbacked { .. }]), "{:?}", one.edits);
    let stored: Value = serde_json::from_slice(&fs::read(d.state_dir().join("edits.json")).unwrap()).unwrap();
    assert_eq!(stored["version"], 5);

    export(&root, "rekordbox", "House 1", None, &file, 1);
    let replay = api::edits_replay_apply(&DriveRef { drive: common::path(&root) }, &Ctx::silent()).unwrap();
    assert!(replay.backup.is_some());
    let d = Device::open_export(&root).unwrap();
    let b = backup::list(&d).unwrap().remove(0);
    assert_eq!((b.id.as_str(), b.audio, b.unbacked.len()), (replay.backup.as_deref().unwrap(), 0, 3));
    assert!(!paths::to_fs(&root, &paths::key(&file)).exists());

    export(&root, "rekordbox", "House 1", None, &file, 1);
    common::add_onelibrary(&root);
    let keeper = d.library().unwrap().tracks[0].file_path.clone();
    let tag = Edit::MyTag(TagChange::Add { tag: "Genre/Deep".into(), tracks: vec![keeper] });
    let delete = Edit::DeleteUnbacked { tracks: TrackSelector::paths([file.clone()]) };
    let mut d = Device::open_export(&root).unwrap();
    let plan = edit::plan(&d, &[tag, delete], &EditOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(plan.unbacked.len(), 3, "{:?}", plan.unbacked);
    let report = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let b = backup::list(&d).unwrap().remove(0);
    assert_eq!((Some(&b.id), b.audio, &b.unbacked), (report.backup.as_ref(), 0, &plan.unbacked));
    assert!(!paths::to_fs(&root, &paths::key(&file)).exists());
}

/// A demo track only OneLibrary lists (rekordbox exported it there alone) goes from it, with its
/// file; export.pdb is left as it was.
#[test]
fn a_demo_track_only_onelibrary_lists_goes_too() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let file = format!("{SAMPLES}/Breaks2.wav");
    let breaks = export(&root, "rekordbox", "Breaks 2", None, &file, 1);
    common::add_onelibrary(&root);
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    ed.remove_tracks(&[breaks]).unwrap();
    ed.save(&pdb_path).unwrap();
    let pdb_before = common::library(&root);

    let (plan, _) = clean(&root);
    assert_eq!(plan.tracks_removed.iter().map(|t| t.file_path.as_str()).collect::<Vec<_>>(), [file.as_str()]);
    assert_eq!(plan.changes, ["delete 1 track(s) (rekordbox's next export brings their files back, undo can't)"]);
    assert!(plan.files.iter().any(|f| *f == paths::key(&file)), "{:?}", plan.files);
    assert!(plan.onelibrary_changes);
    assert_eq!(common::plus_library(&root).tracks.len(), 12);
    assert_eq!(common::library(&root).tracks, pdb_before.tracks);
    assert!(!root.join(paths::key(&file)).exists());
}

/// Two demo rows of one file both go; a demo track sharing its file with one of the DJ's stays,
/// with the file, and the plan says so.
#[test]
fn demo_rows_go_by_file_unless_a_track_of_the_djs_shares_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let file = format!("{SAMPLES}/House1.wav");
    export(&root, "rekordbox", "House 1", None, &file, 1);
    export(&root, "rekordbox", "House 2", None, &file, 2);
    let mine = "/Contents/Mine/edit.mp3";
    export(&root, "rekordbox", "Breaks 1", None, mine, 3);
    export(&root, "Me", "My edit", None, mine, 3);

    let (plan, _) = clean(&root);
    assert_eq!(plan.tracks_removed.len(), 2);
    let shared =
        "1 demo track(s) share their audio file with another track and stay: deleting them would delete it too";
    assert!(plan.warnings.iter().any(|w| w == shared), "{:?}", plan.warnings);
    let titles: Vec<String> = common::library(&root).tracks.into_iter().map(|t| t.title).collect();
    assert_eq!(titles.len(), 14);
    assert!(titles.contains(&"Breaks 1".to_string()) && titles.contains(&"My edit".to_string()));
    assert!(!root.join(paths::key(&file)).exists());
    assert!(root.join(paths::key(mine)).is_file());
}

/// On a drive with OneLibrary alone, the demo tracks go from it, and no export.pdb appears; on
/// one without demo tracks nothing is planned.
#[test]
fn a_onelibrary_only_drive_and_one_without_demo_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let file = format!("{SAMPLES}/Breaks1.wav");
    export(&root, "rekordbox", "Breaks 1", None, &file, 1);
    common::onelibrary_only(&root);
    let (plan, _) = clean(&root);
    assert_eq!(plan.tracks_removed.len(), 1);
    common::sqlcipher_reads(&root);
    assert_eq!(common::plus_library(&root).tracks.len(), 12);
    assert!(!root.join(paths::key(&file)).exists());
    assert!(!root.join(paths::EXPORT_PDB).exists(), "no export.pdb appears");

    let demo = common::demo_drive(dir.path(), "Demo");
    let plan = clean_plan(&demo);
    assert!(plan.is_noop() && !plan.destructive());
}
