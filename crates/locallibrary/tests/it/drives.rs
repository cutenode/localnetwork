//! Drive management: the registry, recognising and copying drives, parallel sync, scrubbing and
//! repairing, space and player names.

mod api;
mod care;
mod provision;
mod recordings;
mod registry;
mod registry_api;
mod scrubbing;
mod which_drive;

use crate::common::{self, rot};

use locallibrary::compare::{self, Relation};
use locallibrary::device::{Device, LineageOp};
use locallibrary::fleet::{self, Role, Standing};
use locallibrary::progress::{CancelToken, Ctx, Event, Phase, Progress};
use locallibrary::scrub::{self, RepairSource, ScrubOptions};
use locallibrary::space::{self, SpaceKind};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, SyncOptions};
use locallibrary::{Error, paths, player};

fn ctx() -> Ctx<'static> {
    Ctx::silent()
}

fn pull(store: &Store, d: &mut Device, message: &str) {
    let opts = PullOptions { library: None, message: Some(message.into()), force: false };
    sync::pull(store, d, &opts, &ctx()).unwrap();
}

fn mirror(src: &mut Device, dst: &mut Device) {
    sync::sync_drive_to_drive(src, dst, &SyncOptions::default(), &ctx()).unwrap();
}

fn first_audio(d: &Device) -> String {
    paths::key(&d.library().unwrap().tracks[0].file_path)
}

#[test]
fn registry_knows_offline_drives_and_roles() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut main = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    main.adopt(Some("Main")).unwrap();
    pull(&store, &mut main, "first");
    std::fs::create_dir(dir.path().join("Backup")).unwrap();
    let mut backup = Device::open(dir.path().join("Backup")).unwrap();
    mirror(&mut main, &mut backup);
    backup.adopt(Some("Backup")).unwrap();
    sync::register_drive(&store, &backup, None).unwrap();

    // Main moves on and is pulled again; Backup isn't mounted any more.
    common::export_new_track(main.root(), "Artist", "New");
    let mut main = Device::open(main.root()).unwrap();
    pull(&store, &mut main, "second");
    let known = fleet::known(&store, &[], false).unwrap();
    let b = known.iter().find(|k| k.name == "Backup").unwrap();
    assert!(b.mounted.is_none());
    assert!(matches!(b.standing, Standing::Behind { snapshots: 1, .. }), "{:?}", b.standing);
    let m = known.iter().find(|k| k.name == "Main").unwrap();
    assert!(matches!(m.standing, Standing::UpToDate));

    // Roles: one primary per library; archive drives refuse to be overwritten.
    let main_id = main.id().unwrap();
    let backup_id = backup.id().unwrap();
    fleet::set_role(&store, backup_id, Some(Role::Primary)).unwrap();
    fleet::set_role(&store, main_id, Some(Role::Primary)).unwrap();
    let drives = store.drives().unwrap();
    assert_eq!(drives[&main_id].role, Some(Role::Primary));
    assert_eq!(drives[&backup_id].role, Some(Role::Backup));
    fleet::set_role(&store, backup_id, Some(Role::Archive)).unwrap();
    let refused = fleet::check_target(Some(&store), &backup, false).unwrap_err();
    assert_eq!((refused.code(), refused.reason()), ("conflict", Some(locallibrary::error::Reason::Archive)));
    assert!(fleet::check_target(Some(&store), &backup, true).is_ok());
    assert!(fleet::check_target(Some(&store), &main, false).is_ok());

    // Registering a drive again keeps what only the registry knows.
    sync::register_drive(&store, &backup, None).unwrap();
    assert_eq!(store.drives().unwrap()[&backup_id].role, Some(Role::Archive));

    // Retired drives are hidden and refused too; forgetting removes the record.
    fleet::set_role(&store, backup_id, None).unwrap();
    fleet::retire(&store, backup_id, false).unwrap();
    assert!(fleet::known(&store, &[], false).unwrap().iter().all(|k| k.id != backup_id));
    assert!(fleet::check_target(Some(&store), &backup, false).is_err());
    fleet::forget(&store, "backup").unwrap();
    assert!(fleet::find_record(&store, "Backup").is_err());

    // The logbook.
    fleet::record_write(&store, &main, 1000).unwrap();
    fleet::record_problem(&store, main_id, "damaged_files", "2 file(s)").unwrap();
    let log = &store.drives().unwrap()[&main_id].log;
    assert_eq!((log.bytes_written, log.writes), (1000, 1));
    assert_eq!(log.problems.len(), 1);
}

#[test]
fn recognises_a_drive_that_lost_its_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut main = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    main.adopt(Some("Main")).unwrap();
    pull(&store, &mut main, "first");
    let id = main.id().unwrap();

    std::fs::remove_dir_all(main.root().join(".locallibrary")).unwrap();
    let erased = Device::open(main.root()).unwrap();
    assert!(!erased.is_adopted());
    let r = fleet::recognise(&store, &erased, &ctx()).unwrap();
    assert_eq!(r.snapshot.as_ref().unwrap().library, "Main");
    assert_eq!(r.best().unwrap().id, id);

    let mut erased = erased;
    let info = fleet::adopt_as(&store, &mut erased, "Main", &[], &ctx()).unwrap();
    assert_eq!(info.id, Some(id));
    assert_eq!(info.name.as_deref(), Some("Main"));
    // Its lineage came back with it, so the store sees it as up to date.
    let snap = store.load_snapshot(&store.libraries().unwrap()[0].head.unwrap()).unwrap();
    let c = compare::compare(
        &compare::Side::from_snapshot(&store, &snap, "store").unwrap(),
        &compare::Side::from_device(&erased, &ctx()).unwrap(),
    );
    assert_eq!(c.relation, Relation::Identical);
}

#[test]
fn copies_of_a_drive_are_found_and_reidentified() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open(common::demo_drive(dir.path(), "A")).unwrap();
    a.adopt(Some("Main")).unwrap();
    common::copy_dir(a.root(), &dir.path().join("Copy"));
    let copy = Device::open(dir.path().join("Copy")).unwrap();
    let mounted = vec![Device::open(a.root()).unwrap(), copy];
    let dups = fleet::duplicates(&mounted, &|_| None);
    assert_eq!(dups.len(), 1);
    assert_eq!(dups[0].roots.len(), 2);

    let mut copy = Device::open(dir.path().join("Copy")).unwrap();
    let info = fleet::reidentify(None, &mut copy, None).unwrap();
    assert_ne!(info.id, a.id());
    assert_eq!(info.name.as_deref(), Some("Main (copy)"));
    // It still holds the same states, so the two compare as identical.
    let c = compare::compare_devices(&a, &copy, &ctx()).unwrap();
    assert_eq!(c.relation, Relation::Identical);
    let mounted = vec![Device::open(a.root()).unwrap(), copy];
    assert!(fleet::duplicates(&mounted, &|_| None).is_empty());
}

/// Renaming and re-identifying a drive save its identity file as it is once the drive is locked:
/// what another process recorded after this one opened the drive stays.
#[test]
fn renaming_and_reidentifying_keep_what_another_process_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let opened_then_edited = |name: &str| {
        let root = common::demo_drive(dir.path(), name);
        Device::open(&root).unwrap().adopt(Some(name)).unwrap();
        let opened = Device::open(&root).unwrap();
        common::apply(&root, &[locallibrary::edit::Edit::CreatePlaylist { path: "Mine".into(), folder: false }]);
        opened
    };
    let edited = |d: &Device| {
        Device::open(d.root()).unwrap().state().is_some_and(|s| s.lineage.iter().any(|e| e.op == LineageOp::Edit))
    };

    let mut a = opened_then_edited("A");
    fleet::rename(None, &mut a, "Club", false).unwrap();
    assert!(edited(&a), "the edit's entry is kept");
    let mut b = opened_then_edited("B");
    fleet::reidentify(None, &mut b, None).unwrap();
    assert!(edited(&b), "the edit's entry is kept");
}

/// Giving a drive a registered drive's identity is refused when another process gave it one of
/// its own after this one opened it.
#[test]
fn adopting_as_a_known_drive_sees_an_identity_given_meanwhile() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut main = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    main.adopt(Some("Main")).unwrap();
    pull(&store, &mut main, "first");
    let fresh = common::demo_drive(dir.path(), "Fresh");

    let mut opened = Device::open(&fresh).unwrap();
    Device::open(&fresh).unwrap().adopt(Some("Fresh")).unwrap();
    let err = fleet::adopt_as(&store, &mut opened, "Main", &[], &ctx()).unwrap_err();
    assert_eq!(err.reason(), Some(locallibrary::error::Reason::HasIdentity), "{err}");
    assert_eq!(Device::open(&fresh).unwrap().name(), "Fresh");
}

#[test]
fn syncs_several_drives_at_once_and_isolates_failures() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open(common::demo_drive(dir.path(), "A")).unwrap();
    let mut targets = Vec::new();
    for name in ["B", "C", "D"] {
        std::fs::create_dir(dir.path().join(name)).unwrap();
        targets.push(Device::open(dir.path().join(name)).unwrap());
    }
    // D is busy with another operation.
    let busy = Device::open(dir.path().join("D")).unwrap();
    let _held = busy.lock().unwrap();

    let results = sync::sync_drive_to_many(&mut a, &mut targets, &SyncOptions::default(), &ctx()).unwrap();
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok() && results[1].is_ok(), "{results:?}");
    assert!(matches!(results[2], Err(Error::Locked(_))), "{:?}", results[2]);
    for t in &targets[..2] {
        assert_eq!(common::library_files(a.root()), common::library_files(t.root()));
        let c = compare::compare_devices(&a, &Device::open(t.root()).unwrap(), &ctx()).unwrap();
        assert_eq!(c.relation, Relation::Identical);
    }
}

/// A target whose copy thread panics (here, in the progress it reports) fails with what the
/// panic said.
#[test]
fn a_sync_thread_that_panics_says_why() {
    struct PanicOnCopy;
    impl Progress for PanicOnCopy {
        fn on_event(&self, e: &Event) {
            if matches!(e, Event::Item { phase: Phase::Copy, .. }) {
                panic!("the progress bar fell over");
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open(common::demo_drive(dir.path(), "A")).unwrap();
    std::fs::create_dir(dir.path().join("B")).unwrap();
    let mut targets = vec![Device::open(dir.path().join("B")).unwrap()];
    let ctx = Ctx::new(&PanicOnCopy, CancelToken::new());
    let results = sync::sync_drive_to_many(&mut a, &mut targets, &SyncOptions::default(), &ctx).unwrap();
    let err = results[0].as_ref().unwrap_err().to_string();
    assert!(err.contains("a sync thread panicked: the progress bar fell over"), "{err}");
}

#[test]
fn scrub_finds_damage_and_repair_fixes_it_from_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    // Written well before the pull, so the snapshot tells a rewrite since by its later mtime.
    common::age(&root);
    let mut main = Device::open(root).unwrap();
    pull(&store, &mut main, "first");

    let clean = scrub::scrub(&main, Some(&store), &[], &ScrubOptions::default(), &ctx()).unwrap();
    assert!(clean.is_clean(), "{clean:?}");
    assert!(clean.snapshot.is_some(), "the drive's state is a snapshot, so it's the reference");
    assert_eq!(clean.unverified, 0);

    let audio = first_audio(&main);
    rot(&paths::to_fs(main.root(), &audio));
    let gone = main.library().unwrap().tracks[1].file_path.clone();
    std::fs::remove_file(paths::to_fs(main.root(), &paths::key(&gone))).unwrap();

    let r = scrub::scrub(&main, Some(&store), &[], &ScrubOptions::default(), &ctx()).unwrap();
    assert_eq!(r.damaged.len(), 1, "{r:?}");
    assert_eq!(r.damaged[0].path, audio);
    assert_eq!(r.damaged[0].repair, Some(RepairSource::Store));
    assert!(r.missing.iter().any(|m| m.path == paths::key(&gone)), "{:?}", r.missing);
    assert_eq!(r.tracks.len(), 2);

    let problems: Vec<_> = r.problems().cloned().collect();
    let rep = scrub::repair(&mut main, Some(&store), &[], &problems, &ctx()).unwrap();
    assert_eq!(rep.repaired.len(), 2, "{rep:?}");
    assert!(rep.failed.is_empty());
    let after = scrub::scrub(&main, Some(&store), &[], &ScrubOptions::default(), &ctx()).unwrap();
    assert!(after.is_clean(), "{after:?}");
    // Repaired files keep the modification time they had, so a sync sees them unchanged.
    let snap = store.load_snapshot(&store.libraries().unwrap()[0].head.unwrap()).unwrap();
    let entry = snap.files.iter().find(|f| f.path == audio).unwrap();
    let meta = std::fs::metadata(paths::to_fs(main.root(), &audio)).unwrap();
    assert!(locallibrary::fsutil::mtime_eq(entry.mtime, locallibrary::fsutil::mtime_secs(&meta)));
}

/// A snapshot an earlier version took lists what players keep for the drive (`PIONEER/LOG`) as
/// library files. Players clearing it since isn't a library file gone missing.
#[test]
fn verify_against_an_older_snapshot_leaves_out_what_players_keep() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut main = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    pull(&store, &mut main, "first");
    let mut old = store.load_snapshot(&store.libraries().unwrap()[0].head.unwrap()).unwrap();
    let hash = old.files[0].hash;
    old.files.push(locallibrary::tree::FileEntry {
        path: "PIONEER/LOG/0001.LOG".into(),
        size: 1,
        mtime: 0,
        class: paths::FileClass::Other,
        hash,
    });
    let old = store.save_snapshot(old).unwrap();

    let r = scrub::scrub(&main, Some(&store), &[], &ScrubOptions { snapshot: Some(old.id) }, &ctx()).unwrap();
    assert!(r.is_clean(), "{:?}", r.missing);
}

#[test]
fn scrub_without_a_store_uses_the_drives_own_records_and_a_sibling() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open(common::demo_drive(dir.path(), "A")).unwrap();
    std::fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();
    // The sync records the hash of every file it writes to B.
    mirror(&mut a, &mut b);
    // Wait out FAT's racy window so the records count.
    let audio = first_audio(&b);
    let path = paths::to_fs(b.root(), &audio);
    let old = filetime::FileTime::from_unix_time(filetime::FileTime::now().unix_seconds() - 60, 0);
    filetime::set_file_mtime(&path, old).unwrap();
    let r0 = scrub::scrub(&b, None, &[], &ScrubOptions::default(), &ctx()).unwrap();
    assert!(r0.is_clean());

    rot(&path);
    let siblings = vec![Device::open(a.root()).unwrap()];
    let r = scrub::scrub(&b, None, &siblings, &ScrubOptions::default(), &ctx()).unwrap();
    assert_eq!(r.damaged.len(), 1, "{r:?}");
    assert!(matches!(r.damaged[0].repair, Some(RepairSource::Drive { .. })));
    let problems: Vec<_> = r.problems().cloned().collect();
    let rep = scrub::repair(&mut b, None, &siblings, &problems, &ctx()).unwrap();
    assert_eq!(rep.repaired, vec![audio]);
    assert!(scrub::scrub(&b, None, &[], &ScrubOptions::default(), &ctx()).unwrap().is_clean());
}

#[test]
fn space_breakdown_sorts_every_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    std::fs::create_dir_all(root.join("Music")).unwrap();
    std::fs::write(root.join("Music/loose.mp3"), vec![0u8; 5000]).unwrap();
    std::fs::write(root.join("Contents/._x.mp3"), [0u8, 5, 0x16, 7]).unwrap();
    std::fs::create_dir_all(root.join(".Trashes/501")).unwrap();
    std::fs::write(root.join(".Trashes/501/old.mp3"), vec![0u8; 3000]).unwrap();
    std::fs::create_dir_all(root.join("PIONEER REC")).unwrap();
    std::fs::write(root.join("PIONEER REC/REC001.WAV"), vec![0u8; 7000]).unwrap();
    let d = Device::open(&root).unwrap();
    let r = space::breakdown(&d, &ctx()).unwrap();
    assert!(r.bytes(SpaceKind::Audio) > 0);
    assert!(r.bytes(SpaceKind::Analysis) > 0);
    assert!(r.bytes(SpaceKind::Databases) > 0);
    assert_eq!(r.bytes(SpaceKind::LooseAudio), 5000);
    assert_eq!(r.loose_audio[0].path, "Music/loose.mp3");
    assert_eq!(r.bytes(SpaceKind::Trash), 3000);
    assert_eq!(r.bytes(SpaceKind::Recordings), 7000);
    assert_eq!(r.bytes(SpaceKind::OsClutter), 4);
}

#[test]
fn player_names_are_per_drive_and_survive_sync() {
    let dir = tempfile::tempdir().unwrap();
    let a_root = common::demo_drive(dir.path(), "A");
    common::add_onelibrary(&a_root);
    let mut a = Device::open(&a_root).unwrap();
    std::fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();
    mirror(&mut a, &mut b);
    let before = a.scan_fingerprinted(None, &ctx()).unwrap().fingerprint;

    player::set(&mut a, Some("MAIN"), Some(3)).unwrap();
    player::set(&mut b, Some("BACKUP"), Some(6)).unwrap();
    let id = player::read(&a).onelibrary.unwrap();
    assert_eq!((id.name.as_deref(), id.colour), (Some("MAIN"), Some(3)));
    // The name is the drive's, not the library's.
    assert_eq!(a.scan_fingerprinted(None, &ctx()).unwrap().fingerprint, before);
    let c = compare::compare_devices(&a, &b, &ctx()).unwrap();
    assert_eq!(c.relation, Relation::Identical);

    // A sync that replaces exportLibrary.db keeps the target's own name.
    common::export_new_track(&a_root, "Artist", "New");
    common::add_onelibrary(&a_root);
    let mut a = Device::open(&a_root).unwrap();
    player::set(&mut a, Some("MAIN"), Some(3)).unwrap();
    mirror(&mut a, &mut b);
    let id = player::read(&b).onelibrary.unwrap();
    assert_eq!((id.name.as_deref(), id.colour), (Some("BACKUP"), Some(6)));
    let c = compare::compare_devices(&a, &b, &ctx()).unwrap();
    assert_eq!(c.relation, Relation::Identical);
}

/// Regression: a sync or pack onto a stick with no exportLibrary.db gave it the source's player
/// name and colour, as there was none to keep, so a set stick showed up on the players as the main
/// drive. It's left with none, and the plan says so.
#[test]
fn a_stick_without_onelibrary_gets_no_player_name() {
    use locallibrary::pack::{self, PackOptions};
    let dir = tempfile::tempdir().unwrap();
    let a_root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&a_root);
    let mut a = Device::open(&a_root).unwrap();
    player::set(&mut a, Some("MAIN"), Some(3)).unwrap();
    let said = |warnings: &[String], name: &str| {
        warnings
            .iter()
            .any(|w| w.starts_with(&format!("{name} has no exportLibrary.db, so it gets one without a player name")))
    };
    let unnamed = |root: &std::path::Path| {
        let id = player::db_identity(root).expect("a OneLibrary database");
        assert_eq!((id.name, id.colour.unwrap_or(0)), (None, 0), "{}", root.display());
    };

    std::fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();
    let (plan, _) = sync::plan_drive_to_drive(&a, &b, &SyncOptions::default(), &ctx()).unwrap();
    assert!(said(&plan.warnings, &b.name()), "{:?}", plan.warnings);
    mirror(&mut a, &mut b);
    unnamed(b.root());
    // Once it has one, a sync keeps what it has.
    let (plan, _) = sync::plan_drive_to_drive(&a, &b, &SyncOptions::default(), &ctx()).unwrap();
    assert!(!said(&plan.warnings, &b.name()), "{:?}", plan.warnings);

    let club_root = dir.path().join("Club");
    std::fs::create_dir(&club_root).unwrap();
    let mut club = Device::open(&club_root).unwrap();
    let opts = PackOptions { playlists: vec!["aab".into()], ..Default::default() };
    let plan = pack::plan(&a, &club, &opts, &ctx()).unwrap();
    assert!(said(&plan.warnings, &club.name()), "{:?}", plan.warnings);
    pack::apply(&plan, &a, &mut club, None, &ctx()).unwrap();
    unnamed(&club_root);
}

/// Regression: a sync with `--delete-first` takes the target's database off before copying, and
/// one cut off then left the run that finished the job no player name to put back: the drive got
/// its source's. The name is kept with the drive's pending marker until it's back.
#[test]
fn a_cut_off_sync_keeps_the_targets_player_name() {
    let dir = tempfile::tempdir().unwrap();
    let a_root = common::demo_drive(dir.path(), "A");
    common::add_onelibrary(&a_root);
    let mut a = Device::open(&a_root).unwrap();
    std::fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();
    mirror(&mut a, &mut b);
    player::set(&mut a, Some("MAIN"), Some(3)).unwrap();
    player::set(&mut b, Some("BACKUP"), Some(6)).unwrap();
    // A has a new track, and B a file A doesn't, so the sync deletes before it copies.
    common::export_new_track(&a_root, "Artist", "New");
    common::add_onelibrary(&a_root);
    let mut a = Device::open(&a_root).unwrap();
    player::set(&mut a, Some("MAIN"), Some(3)).unwrap();
    std::fs::write(b.root().join("Contents/stale.mp3"), "stale").unwrap();

    let token = CancelToken::new();
    let stop = |e: &Event| {
        if matches!(e, Event::Item { phase: Phase::Copy, .. }) {
            token.cancel();
        }
    };
    let opts = SyncOptions { delete_first: true, ..Default::default() };
    let cut = sync::sync_drive_to_drive(&mut a, &mut b, &opts, &Ctx::new(&stop, token.clone()));
    assert!(matches!(cut, Err(Error::Cancelled)), "{cut:?}");
    assert_eq!(player::db_identity(b.root()), None, "the database came off first");

    mirror(&mut a, &mut b);
    let id = player::read(&b).onelibrary.unwrap();
    assert_eq!((id.name.as_deref(), id.colour), (Some("BACKUP"), Some(6)));
    assert!(Device::open(b.root()).unwrap().state().unwrap().pending.is_none());
}

/// Regression: undoing `drive player` left the drive remembering the undone name, so `refresh`
/// set it again. It remembers the one the drive shows again, and an undo of something else
/// leaves what it remembers alone.
#[test]
fn undoing_the_player_name_forgets_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let mut d = Device::open(&root).unwrap();
    d.adopt(None).unwrap();
    player::set(&mut d, Some("MAIN"), Some(3)).unwrap();
    player::set(&mut d, Some("OTHER"), None).unwrap();
    let remembered = |d: &Device| Device::open(d.root()).unwrap().state().unwrap().player.clone().unwrap();

    locallibrary::backup::restore(&mut d, None, false).unwrap();
    let shown = player::db_identity(&root).unwrap();
    assert_eq!((shown.name.as_deref(), shown.colour), (Some("MAIN"), Some(3)));
    assert_eq!(remembered(&d), shown);

    // rekordbox exports its own name, then an edit is undone: the drive still remembers MAIN, for
    // `refresh` to put back.
    let one = locallibrary::onelibrary::OneLibrary::open_root(&root).unwrap();
    one.connection().execute("UPDATE property SET deviceName = 'REKORDBOX'", []).unwrap();
    std::fs::write(paths::to_fs(&root, paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    common::apply(&root, &[locallibrary::edit::Edit::CreatePlaylist { path: "Friday".into(), folder: false }]);
    let mut d = Device::open(&root).unwrap();
    locallibrary::backup::restore(&mut d, None, false).unwrap();
    assert_eq!(player::db_identity(&root).unwrap().name.as_deref(), Some("REKORDBOX"));
    assert_eq!(remembered(&d), shown);
}

/// Regression: setting the player name made its backup before editing the database, so a
/// database that couldn't take the name left a stray backup behind.
#[test]
fn a_player_name_the_database_cant_take_leaves_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    let one = locallibrary::onelibrary::OneLibrary::open_root(&root).unwrap();
    one.connection().execute("DROP TABLE property", []).unwrap();
    let db = root.join(paths::LIBRARY_PLUS_DB);
    std::fs::write(&db, one.to_encrypted().unwrap()).unwrap();
    let before = std::fs::read(&db).unwrap();

    let mut d = Device::open(&root).unwrap();
    assert!(player::set(&mut d, Some("MAIN"), Some(3)).is_err());
    assert!(locallibrary::backup::list(&d).unwrap().is_empty());
    assert!(d.state().is_none_or(|s| s.pending.is_none()), "nothing marked as interrupted");
    assert_eq!(std::fs::read(&db).unwrap(), before);

    // A database that takes it gets a backup, and nothing is left marked as interrupted.
    common::add_onelibrary(&root);
    player::set(&mut d, Some("MAIN"), Some(3)).unwrap();
    assert_eq!(locallibrary::backup::list(&d).unwrap().len(), 1);
    assert!(d.state().unwrap().pending.is_none());
}

/// A sync puts the target's player name back while it still holds the drive's lock, so nothing
/// else can write in between (it used to take the lock again, and fail if another write had).
#[test]
fn the_player_name_goes_back_under_the_lock_the_sync_holds() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let mut d = Device::open(&root).unwrap();
    player::set(&mut d, Some("MAIN"), Some(3)).unwrap();
    let before = player::db_identity(&root);
    player::set(&mut d, Some("OTHER"), None).unwrap();
    let _held = d.lock().unwrap();
    assert!(player::preserve(&d, before.as_ref()).unwrap());
    assert_eq!(player::db_identity(&root).unwrap().name.as_deref(), Some("MAIN"));
}
