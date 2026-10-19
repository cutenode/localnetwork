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
use locallibrary::paths::{self, FileClass};
use locallibrary::progress::Ctx;
use locallibrary::scrub::{self, FileProblem, ReferenceKind, RepairSource, ScrubOptions};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, SyncOptions};

#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn a_scrub_after_the_drive_changed_checks_what_the_store_knows(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let (root, _) = drawn::drive(&tc, dir.path(), "Main");
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
