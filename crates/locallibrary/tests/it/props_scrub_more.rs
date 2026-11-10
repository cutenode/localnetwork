//! Scrubs of a drive that changed since it was saved (files checked against older snapshots, new
//! ones vouched for from then on), against a snapshot named outright, of files that can't be read,
//! and repairs that can't be made: no known-good copy, a file that isn't the library's, a sibling
//! that isn't there, or a copy that's damaged too.

// Some draws lock a file with Unix permissions to make it unreadable.
#![cfg(unix)]

use crate::{common, drawn};

use std::fs;
use std::os::unix::fs::PermissionsExt;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::onelibrary::{self, OneLibrary};
use locallibrary::paths::{self, FileClass};
use locallibrary::player;
use locallibrary::progress::Ctx;
use locallibrary::scrub::{self, FileProblem, ReferenceKind, RepairSource, ScrubOptions};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, SyncOptions};

#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn a_scrub_after_the_drive_changed_checks_what_the_store_knows(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let (root, _) = drawn::drive(&tc, dir.path(), "Main");
    // On OneLibrary too, at times: its database is then rewritten holding the same library.
    let on_onelibrary = tc.draw(gs::booleans());
    if on_onelibrary {
        common::add_onelibrary(&root);
    }
    common::age(&root);
    let mut main = Device::open_export(&root).unwrap();
    main.adopt(Some("Main")).unwrap();
    let first = sync::pull(&store, &mut main, &PullOptions::default(), &Ctx::silent()).unwrap().snapshot.unwrap();
    let audio: Vec<String> = common::library_files(&root)
        .into_iter()
        .map(|(k, _)| k)
        .filter(|k| paths::classify(k) == Some(FileClass::Audio))
        .collect();
    // rekordbox exports a track more; an older file rots, and another goes.
    // (Its files are new, as the export just wrote them; aging them would make the new export.pdb
    // look like the old one rotted.)
    let pulled = common::drive_files(&root);
    common::export_new_track(&root, "Later", "Export");
    // What the export rewrote of what was pulled: against that snapshot, it differs.
    let rewritten: Vec<String> = common::changed_files(&pulled, &common::drive_files(&root))
        .into_iter()
        .filter(|k| pulled.contains_key(k) && paths::classify(k).is_some_and(|c| c.is_library()))
        .map(|k| k.to_lowercase())
        .collect();
    // Its bytes differ from the snapshot's, its library doesn't: never damage.
    if on_onelibrary {
        let again = OneLibrary::open(&main).unwrap().to_encrypted().unwrap();
        onelibrary::write(&main, &again).unwrap();
    }
    let rotted = tc.draw(gs::sampled_from(audio.clone()));
    common::rot(&paths::to_fs(&root, &rotted));
    let gone = audio.iter().find(|k| **k != rotted).filter(|_| tc.draw(gs::booleans())).cloned();
    if let Some(g) = &gone {
        fs::remove_file(paths::to_fs(&root, g)).unwrap();
    }
    // At times one that can't be read at all.
    let locked =
        audio.iter().find(|k| **k != rotted && Some(*k) != gone.as_ref()).filter(|_| tc.draw(gs::booleans())).cloned();
    if let Some(l) = &locked {
        fs::set_permissions(paths::to_fs(&root, l), fs::Permissions::from_mode(0o000)).unwrap();
    }
    let named = tc.draw(gs::booleans());
    let opts = ScrubOptions { snapshot: named.then_some(first.id) };
    let main = Device::open_export(&root).unwrap();
    let r = scrub::scrub(&main, Some(&store), &[], &opts, &Ctx::silent()).unwrap();
    if let Some(l) = &locked {
        fs::set_permissions(paths::to_fs(&root, l), fs::Permissions::from_mode(0o644)).unwrap();
    }
    let paths_of = |v: &[FileProblem]| v.iter().map(|p| p.path.to_lowercase()).collect::<Vec<_>>();
    let mut want = vec![rotted.to_lowercase()];
    if named {
        want.extend(rewritten.iter().cloned());
    }
    want.sort();
    let mut damaged = paths_of(&r.damaged);
    damaged.sort();
    assert_eq!(damaged, want, "{r:#?}");
    // Named, the snapshot says what the file held; otherwise the hash the pull cached does.
    let reference = if named { ReferenceKind::Snapshot } else { ReferenceKind::Cache };
    assert!(r.damaged.iter().all(|p| p.reference == Some(reference)), "{r:#?}");
    assert_eq!(paths_of(&r.missing), gone.iter().map(|g| g.to_lowercase()).collect::<Vec<_>>());
    if gone.is_some() {
        // A file that's gone has no cached hash to go by: the snapshots say what it held.
        let reference = if named { ReferenceKind::Snapshot } else { ReferenceKind::History };
        assert_eq!(r.missing[0].reference, Some(reference));
        assert!(r.missing[0].expected.is_some());
    }
    assert_eq!(paths_of(&r.unreadable), locked.iter().map(|l| l.to_lowercase()).collect::<Vec<_>>());
    if locked.is_some() {
        assert!(r.unreadable[0].error.is_some());
    }
    // The new export's files are in no snapshot: taken as they are, not as damage.
    assert!(r.unverified > 0, "{r:#?}");
    assert_eq!(r.verified + r.unverified + r.damaged.len() + r.unreadable.len(), r.files, "{r:#?}");
    let again = scrub::scrub(&main, Some(&store), &[], &opts, &Ctx::silent()).unwrap();
    assert_eq!(paths_of(&again.damaged), paths_of(&r.damaged), "the damage isn't taken for the new normal");
}

/// However often `exportLibrary.db` is rewritten holding the same library (a name, a colour, a
/// checkpoint), a scrub takes it as the state saved, and finds exactly the audio that rotted.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn rewrites_of_the_same_library_are_not_damage(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let (root, _) = drawn::drive(&tc, dir.path(), "Main");
    common::add_onelibrary(&root);
    common::age(&root);
    let mut main = Device::open_export(&root).unwrap();
    main.adopt(Some("Main")).unwrap();
    let saved = sync::pull(&store, &mut main, &PullOptions::default(), &Ctx::silent()).unwrap().snapshot.unwrap();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        if tc.draw(gs::booleans()) {
            let name = tc.draw(gs::sampled_from(vec!["MAIN A".to_string(), "BOOTH".to_string()]));
            let colour = tc.draw(gs::integers::<i64>().min_value(0).max_value(8));
            player::set(&mut main, Some(&name), Some(colour)).unwrap();
        } else {
            let again = OneLibrary::open(&main).unwrap().to_encrypted().unwrap();
            onelibrary::write(&main, &again).unwrap();
        }
    }
    let audio: Vec<String> = common::library_files(&root)
        .into_iter()
        .map(|(k, _)| k)
        .filter(|k| paths::classify(k) == Some(FileClass::Audio))
        .collect();
    let rotted = tc.draw(gs::booleans()).then(|| tc.draw(gs::sampled_from(audio)));
    if let Some(key) = &rotted {
        common::rot(&paths::to_fs(&root, key));
    }

    let r = scrub::scrub(&main, Some(&store), &[], &ScrubOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(r.snapshot, Some(saved.id));
    let damaged: Vec<&str> = r.damaged.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(damaged, rotted.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(r.missing.is_empty(), "{:?}", r.missing);
}

#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn repairs_that_cant_be_made_are_skipped_or_said(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (root, _) = drawn::drive(&tc, dir.path(), "Main");
    common::age(&root);
    let mut main = Device::open_export(&root).unwrap();
    main.adopt(Some("Main")).unwrap();
    let mut mirror = Device::open(common::blank(dir.path(), "Mirror")).unwrap();
    sync::sync_drive_to_drive(&mut main, &mut mirror, &SyncOptions::default(), &Ctx::silent()).unwrap();
    let audio: Vec<String> = common::library_files(&root)
        .into_iter()
        .map(|(k, _)| k)
        .filter(|k| paths::classify(k) == Some(FileClass::Audio))
        .collect();
    let siblings = vec![Device::open(mirror.root()).unwrap()];
    // The first scrub fills the hash cache, which then says what each file held.
    assert!(scrub::scrub(&main, None, &siblings, &ScrubOptions::default(), &Ctx::silent()).unwrap().is_clean());
    // A file rotted on both drives, and one rotted here only.
    let both = tc.draw(gs::sampled_from(audio.clone()));
    let here = audio.iter().find(|k| **k != both).cloned().unwrap();
    common::rot(&paths::to_fs(&root, &both));
    common::rot(&paths::to_fs(mirror.root(), &both));
    common::rot(&paths::to_fs(&root, &here));
    let r = scrub::scrub(&main, None, &siblings, &ScrubOptions::default(), &Ctx::silent()).unwrap();
    let mut problems: Vec<FileProblem> = r.problems().cloned().collect();
    // ...and three no repair can come from: nothing known-good, a file that isn't the library's,
    // and a drive that isn't here.
    let template = problems.iter().find(|p| p.path.eq_ignore_ascii_case(&here)).unwrap().clone();
    problems.push(FileProblem { path: "Contents/unknown.mp3".into(), expected: None, ..template.clone() });
    problems.push(FileProblem { path: "notes.txt".into(), class: FileClass::Other, ..template.clone() });
    // Where the repair would come from is a drive that isn't here, or isn't known at all.
    let elsewhere = RepairSource::Drive { name: "Away".into(), root: dir.path().join("Away") };
    let repair = tc.draw(gs::booleans()).then_some(elsewhere);
    problems.push(FileProblem { path: audio[0].clone(), repair, ..template.clone() });
    // A damaged file whose time isn't known takes the time of the copy it comes from.
    let timeless = tc.draw(gs::booleans());
    if timeless {
        problems.iter_mut().find(|p| p.path.eq_ignore_ascii_case(&here)).unwrap().mtime = None;
    }

    let rep = scrub::repair(&mut main, None, &siblings, &problems, &Ctx::silent()).unwrap();
    let skipped = |why: &str| rep.skipped.iter().filter(|s| s.reason == why).count();
    assert_eq!(skipped("no known-good copy"), 1, "{rep:?}");
    assert_eq!(skipped("not a library file"), 1, "{rep:?}");
    assert_eq!(skipped("no known-good copy is available now"), 1, "{rep:?}");
    assert!(rep.repaired.iter().any(|p| p.eq_ignore_ascii_case(&here)), "{rep:?}");
    if timeless {
        // To the second: the drive's index keeps whole seconds.
        let mtime = |root: &std::path::Path| {
            let modified = fs::metadata(paths::to_fs(root, &here)).unwrap().modified().unwrap();
            modified.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
        };
        assert_eq!(mtime(&root), mtime(mirror.root()));
    }
    let failed: Vec<&str> =
        rep.failed.iter().filter(|f| f.path.eq_ignore_ascii_case(&both)).map(|f| f.error.as_str()).collect();
    assert_eq!(failed, ["the copy it would come from is damaged too"], "{rep:?}");
    // Repairing audio leaves the drive's fingerprint (sizes and times) as it was; analysis and
    // artwork are fingerprinted by content.
    for p in &problems {
        let by_content = p.class.is_library() && p.class != FileClass::Audio;
        assert_eq!(scrub::changes_fingerprint(p), by_content, "{p:?}");
    }
}

/// What stops a scrub or a repair before it writes: a cancel as it starts reading or once it
/// reads the first file, a snapshot named that the store doesn't have, a sibling drive whose
/// folders can't be listed, and another write holding the drive. The drive is left as it was.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(case = 0u8)]
#[hegel::explicit_test_case(case = 1u8)]
#[hegel::explicit_test_case(case = 2u8)]
#[hegel::explicit_test_case(case = 3u8)]
#[hegel::explicit_test_case(case = 4u8)]
fn prop_a_scrub_or_repair_that_cant_go_ahead_stops_before_writing(tc: TestCase) {
    use locallibrary::progress::Phase;
    use locallibrary::{CancelToken, Error, Event, Progress};

    /// Cancels at the first event `at` matches.
    struct CancelAt(CancelToken, fn(&Event) -> bool);
    impl Progress for CancelAt {
        fn on_event(&self, e: &Event) {
            if (self.1)(e) {
                self.0.cancel();
            }
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::age(&root);
    let mut main = Device::open_export(&root).unwrap();
    main.adopt(Some("Main")).unwrap();
    sync::pull(&store, &mut main, &PullOptions::default(), &Ctx::silent()).unwrap();
    let mut mirror = Device::open(common::blank(dir.path(), "Mirror")).unwrap();
    sync::sync_drive_to_drive(&mut main, &mut mirror, &SyncOptions::default(), &Ctx::silent()).unwrap();
    let audio = common::library_files(&root)
        .into_iter()
        .map(|(k, _)| k)
        .find(|k| paths::classify(k) == Some(FileClass::Audio))
        .unwrap();
    common::rot(&paths::to_fs(&root, &audio));
    let main = Device::open_export(&root).unwrap();
    let found = scrub::scrub(&main, Some(&store), &[], &ScrubOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(found.damaged.len(), 1, "{found:#?}");
    let before = common::drive_files(&root);

    let case = tc.draw(gs::integers::<u8>().max_value(4));
    let (scrubbed, repaired) = match case {
        0 | 1 => {
            let at: fn(&Event) -> bool = match case {
                0 => |e| matches!(e, Event::PhaseStart { phase: Phase::Verify, .. }),
                _ => |e| matches!(e, Event::Item { phase: Phase::Verify, .. }),
            };
            let cancel = CancelAt(CancelToken::new(), at);
            let ctx = Ctx::new(&cancel, cancel.0.clone());
            (scrub::scrub(&main, Some(&store), &[], &ScrubOptions::default(), &ctx).map(|_| ()), None)
        }
        2 => {
            let opts = ScrubOptions { snapshot: Some(locallibrary::hash::Hash::of(b"no such snapshot")) };
            (scrub::scrub(&main, Some(&store), &[], &opts, &Ctx::silent()).map(|_| ()), None)
        }
        3 => {
            let contents = mirror.root().join("Contents");
            fs::set_permissions(&contents, fs::Permissions::from_mode(0o000)).unwrap();
            let siblings = vec![Device::open(mirror.root()).unwrap()];
            let s = scrub::scrub(&main, None, &siblings, &ScrubOptions::default(), &Ctx::silent()).map(|_| ());
            let mut d = Device::open_export(&root).unwrap();
            let r = scrub::repair(&mut d, None, &siblings, &found.damaged, &Ctx::silent()).map(|_| ());
            fs::set_permissions(&contents, fs::Permissions::from_mode(0o755)).unwrap();
            (s, Some(r))
        }
        _ => {
            let mut d = Device::open_export(&root).unwrap();
            let _other_write = Device::open(&root).unwrap().lock().unwrap();
            (Ok(()), Some(scrub::repair(&mut d, Some(&store), &[], &found.damaged, &Ctx::silent()).map(|_| ())))
        }
    };
    match case {
        0 | 1 => assert!(matches!(scrubbed, Err(Error::Cancelled)), "{scrubbed:?}"),
        2 => assert!(matches!(scrubbed, Err(Error::NotFound(_))), "{scrubbed:?}"),
        3 => {
            assert!(matches!(scrubbed, Err(Error::Io { .. })), "{scrubbed:?}");
            assert!(matches!(repaired, Some(Err(Error::Io { .. }))), "{repaired:?}");
        }
        _ => assert!(matches!(repaired, Some(Err(Error::Locked(_)))), "{repaired:?}"),
    }
    assert_eq!(common::drive_files(&root), before, "the drive is left as it was");
}

/// A repair whose good copy, on a sibling drive, can't be read by the time it's made fails for
/// that file, saying why, and leaves it damaged.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_repair_from_a_copy_it_cant_read_says_why(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (root, _) = drawn::drive(&tc, dir.path(), "Main");
    common::age(&root);
    let mut main = Device::open_export(&root).unwrap();
    main.adopt(Some("Main")).unwrap();
    let mut mirror = Device::open(common::blank(dir.path(), "Mirror")).unwrap();
    sync::sync_drive_to_drive(&mut main, &mut mirror, &SyncOptions::default(), &Ctx::silent()).unwrap();
    let siblings = vec![Device::open(mirror.root()).unwrap()];
    assert!(scrub::scrub(&main, None, &siblings, &ScrubOptions::default(), &Ctx::silent()).unwrap().is_clean());
    let audio: Vec<String> = common::library_files(&root)
        .into_iter()
        .map(|(k, _)| k)
        .filter(|k| paths::classify(k) == Some(FileClass::Audio))
        .collect();
    let rotted = tc.draw(gs::sampled_from(audio));
    common::rot(&paths::to_fs(&root, &rotted));
    let r = scrub::scrub(&main, None, &siblings, &ScrubOptions::default(), &Ctx::silent()).unwrap();
    let copy = paths::to_fs(mirror.root(), &rotted);
    fs::set_permissions(&copy, fs::Permissions::from_mode(0o000)).unwrap();
    let rep = scrub::repair(&mut main, None, &siblings, &r.damaged, &Ctx::silent()).unwrap();
    fs::set_permissions(&copy, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(rep.repaired.is_empty(), "{rep:?}");
    let [failed] = rep.failed.as_slice() else { panic!("{rep:?}") };
    assert!(failed.path.eq_ignore_ascii_case(&rotted), "{rep:?}");
    assert!(failed.error.contains("ermission denied"), "{rep:?}");
    let again = scrub::scrub(&main, None, &siblings, &ScrubOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(again.damaged.len(), 1, "still damaged");
}

/// A OneLibrary drive (named since, at times) that lost a library file after its pull matches no
/// snapshot, but the one it last held still says what belongs: the file is missing by that
/// snapshot, and audio that rotted is damaged by the hash cached for it or, with the cache
/// forgotten, by that snapshot. Repairing it all from the store makes the drive the snapshot's
/// again, clean, its name kept.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn a_drive_that_lost_a_file_is_checked_against_the_snapshot_it_held(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    common::age(&root);
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut main = Device::open(&root).unwrap();
    let saved = sync::pull(&store, &mut main, &PullOptions::default(), &Ctx::silent()).unwrap().snapshot.unwrap();
    let name = tc.draw(gs::optional(gs::sampled_from(vec!["OTHER", "BOOTH"])));
    if let Some(name) = name {
        player::set(&mut main, Some(name), None).unwrap();
    }
    let files: Vec<String> = common::library_files(&root)
        .into_iter()
        .map(|(k, _)| k)
        .filter(|k| matches!(paths::classify(k), Some(FileClass::Audio | FileClass::Analysis)))
        .collect();
    let gone = tc.draw(gs::sampled_from(files.clone()));
    let audio: Vec<String> =
        files.into_iter().filter(|k| *k != gone && paths::classify(k) == Some(FileClass::Audio)).collect();
    let rotted = tc.draw(gs::optional(gs::sampled_from(audio)));
    let forgotten = tc.draw(gs::booleans());
    fs::remove_file(paths::to_fs(&root, &gone)).unwrap();
    if let Some(key) = &rotted {
        common::rot(&paths::to_fs(&root, key));
    }
    if forgotten {
        fs::remove_file(main.state_dir().join("index.json")).unwrap();
    }

    let main = Device::open(&root).unwrap();
    let r = scrub::scrub(&main, Some(&store), &[], &ScrubOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(r.snapshot, None, "{r:#?}");
    let missing: Vec<(String, Option<ReferenceKind>)> =
        r.missing.iter().map(|m| (m.path.to_lowercase(), m.reference)).collect();
    assert_eq!(missing, [(gone.to_lowercase(), Some(ReferenceKind::History))], "{r:#?}");
    let by = if forgotten { ReferenceKind::History } else { ReferenceKind::Cache };
    let damaged: Vec<(String, Option<ReferenceKind>)> =
        r.damaged.iter().map(|p| (p.path.to_lowercase(), p.reference)).collect();
    let want: Vec<(String, Option<ReferenceKind>)> = rotted.iter().map(|k| (k.to_lowercase(), Some(by))).collect();
    assert_eq!(damaged, want, "{r:#?}");
    assert!(r.problems().all(|p| p.repair == Some(RepairSource::Store)), "{r:#?}");

    let problems: Vec<FileProblem> = r.problems().cloned().collect();
    let mut main = main;
    let done = scrub::repair(&mut main, Some(&store), &[], &problems, &Ctx::silent()).unwrap();
    assert_eq!(done.repaired.len(), problems.len(), "{done:?}");
    let again = scrub::scrub(&main, Some(&store), &[], &ScrubOptions::default(), &Ctx::silent()).unwrap();
    assert!(again.is_clean(), "{again:#?}");
    assert_eq!(again.snapshot, Some(saved.id));
    let kept = player::read(&main).onelibrary.and_then(|o| o.name);
    if let Some(name) = name {
        assert_eq!(kept.as_deref(), Some(name));
    }
}

/// A named OneLibrary drive, pulled, whose `exportLibrary.db` rotted after a scrub with no store
/// cached its hash: that scrub finds the database damaged, a repair rewrites it from the store's
/// copy with the drive's name, and it's clean again.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_onelibrary_repaired_from_the_store_keeps_its_name(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let mut main = Device::open(&root).unwrap();
    main.adopt(Some("Main")).unwrap();
    let name = tc.draw(gs::sampled_from(vec!["MAIN A", "BOOTH", "OTHER"]));
    player::set(&mut main, Some(name), None).unwrap();
    common::age(&root);
    let mut main = Device::open(&root).unwrap();
    sync::pull(&store, &mut main, &PullOptions::default(), &Ctx::silent()).unwrap();
    let main = Device::open(&root).unwrap();
    assert!(scrub::scrub(&main, None, &[], &ScrubOptions::default(), &Ctx::silent()).unwrap().is_clean());
    common::rot(&paths::to_fs(&root, paths::LIBRARY_PLUS_DB));

    let r = scrub::scrub(&main, None, &[], &ScrubOptions::default(), &Ctx::silent()).unwrap();
    let damaged: Vec<&str> = r.damaged.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(damaged, [paths::LIBRARY_PLUS_DB], "{r:#?}");
    let problems: Vec<FileProblem> =
        r.problems().map(|p| FileProblem { repair: Some(RepairSource::Store), ..p.clone() }).collect();
    let mut main = main;
    let done = scrub::repair(&mut main, Some(&store), &[], &problems, &Ctx::silent()).unwrap();
    assert!(done.failed.is_empty() && done.repaired.len() == 1, "{done:?}");
    let main = Device::open(&root).unwrap();
    assert_eq!(player::read(&main).onelibrary.and_then(|o| o.name).as_deref(), Some(name));
    let again = scrub::scrub(&main, Some(&store), &[], &ScrubOptions::default(), &Ctx::silent()).unwrap();
    assert!(again.is_clean(), "{again:#?}");
}
