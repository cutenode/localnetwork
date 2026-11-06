//! The name and colour players show: set on a drive (adopted or not) they read back as set,
//! trimmed, and an adopted drive remembers them; undo bookkeeping that finds them unchanged keeps
//! what's remembered; a OneLibrary with no property row shows neither; and a contact that's
//! empty or holds control characters is refused before anything is written.

use crate::common;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::onelibrary::OneLibrary;
use locallibrary::paths;
use locallibrary::player::{self, DbIdentity};
use locallibrary::progress::Ctx;

#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_players_name_and_colour_read_back_as_set(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    let mut d = Device::open_export(&root).unwrap();
    let adopted = tc.draw(gs::booleans());
    if adopted {
        d.adopt(Some("Main")).unwrap();
    }
    let name = tc.draw(gs::optional(gs::sampled_from(vec!["MAIN", " Booth B ", "Ünï ♪"])));
    let colour = match name {
        None => Some(tc.draw(gs::integers::<i64>().min_value(0).max_value(8))),
        Some(_) => tc.draw(gs::optional(gs::integers::<i64>().min_value(0).max_value(8))),
    };
    let now = player::set(&mut d, name, colour).unwrap();
    let shown = now.onelibrary.clone().unwrap();
    assert_eq!(shown.name.as_deref(), name.map(str::trim));
    assert_eq!(shown.colour, Some(colour.unwrap_or(0)));
    assert_eq!(player::db_identity(&root), Some(shown.clone()));
    let remembered = d.state().and_then(|s| s.player.clone());
    // Remembered once the drive is adopted, as any drive written to is.
    assert_eq!(remembered, d.is_adopted().then(|| shown.clone()));

    // Undo bookkeeping that finds the drive showing what it showed keeps what's remembered.
    player::after_undo(&mut d, Some(shown.clone())).unwrap();
    assert_eq!(Device::open(&root).unwrap().state().and_then(|s| s.player.clone()), remembered);

    // Put back after a copy replaced the database: only what differs is written.
    let other = DbIdentity { name: Some("OTHER".into()), colour: Some(5) };
    assert!(!player::preserve(&d, None).unwrap());
    assert!(!player::preserve(&d, Some(&shown)).unwrap(), "already shown");
    assert!(player::preserve(&d, Some(&other)).unwrap());
    assert_eq!(player::db_identity(&root), Some(other));

    // A database with no property row shows no name and no colour.
    let one = OneLibrary::open_root(&root).unwrap();
    one.connection().execute("DELETE FROM property", []).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    assert_eq!(player::db_identity(&root), Some(DbIdentity::default()));
}

/// rekordbox's own record of a drive is the first of its record files (by name) that holds one;
/// files that aren't records, or are empty, are passed over. A drive without OneLibrary can't be
/// given a name or colour.
#[hegel::test(test_cases = 20)]
fn rekordboxs_record_is_the_first_file_that_holds_one(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::blank(dir.path(), "USB");
    let backup = root.join("PIONEER/DeviceLibBackup");
    fs::create_dir_all(&backup).unwrap();
    let kinds = ["garbage", "empty", "record", "other name"];
    let mut want = None;
    for (n, kind) in tc.draw(gs::vecs(gs::sampled_from(kinds.to_vec())).max_size(4)).into_iter().enumerate() {
        let name = match kind {
            "other name" => format!("notes_{n}.json"),
            _ => format!("rbDevLibBaInfo_{n}.json"),
        };
        let body = match kind {
            "garbage" => "not json".to_string(),
            "empty" => r#"{"info": []}"#.to_string(),
            _ => format!(r#"{{"info": [{{"device_id": "id{n}", "device_name": "USB {n}"}}]}}"#),
        };
        fs::write(backup.join(name), body).unwrap();
        if kind == "record" && want.is_none() {
            want = Some(format!("id{n}"));
        }
    }
    let d = Device::open(&root).unwrap();
    assert_eq!(player::read(&d).rekordbox.map(|r| r.device_id), want);
    assert_eq!(player::check(&d, Some("MAIN"), None).unwrap_err().code(), "invalid");
}

#[hegel::test(test_cases = 20)]
fn a_contact_with_nothing_to_contact_is_refused(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::blank(dir.path(), "USB");
    let mut d = Device::open(&root).unwrap();
    let contact = tc.draw(gs::sampled_from(vec!["", "   ", "sam@example.com\nrm", "call\u{7}me", "\t"]));
    let playlist = tc.draw(gs::booleans());
    let e = player::set_contact(&mut d, contact, playlist, &Ctx::silent()).unwrap_err();
    assert_eq!(e.code(), "invalid");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0, "nothing written");
}

/// Undo that changed what a remembering drive shows makes it remember what it shows now, or
/// nothing once it shows neither a name nor a colour; a drive that remembers nothing is left so.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_undo_that_changed_the_shown_name_remembers_the_new_one(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    let mut d = Device::open_export(&root).unwrap();
    let shown = player::set(&mut d, Some("MAIN"), Some(3)).unwrap().onelibrary.unwrap();
    // What the undone database shows: never "MAIN", so always a change.
    let undone = DbIdentity {
        name: tc.draw(gs::optional(gs::sampled_from(vec!["OTHER".to_string(), "B2".to_string()]))),
        colour: Some(tc.draw(gs::integers::<i64>().min_value(0).max_value(8))),
    };
    assert!(player::preserve(&d, Some(&undone)).unwrap());
    player::after_undo(&mut d, Some(shown)).unwrap();
    let has_any = undone.name.is_some() || undone.colour != Some(0);
    let remembered = Device::open(&root).unwrap().state().and_then(|s| s.player.clone());
    assert_eq!(remembered, has_any.then_some(undone.clone()));
    // Remembering nothing now, another undo leaves it so.
    if !has_any {
        player::after_undo(&mut d, None).unwrap();
        assert_eq!(Device::open(&root).unwrap().state().and_then(|s| s.player.clone()), None);
    }
}

/// Outside a run marked on the drive, a stash that replaces the database returns what the drive
/// shows (nothing shown: no name, no colour) and keeps nothing; inside one, what it stashed first
/// stays, whatever the drive shows later.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_stash_is_kept_only_in_a_marked_run(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let plus = tc.draw(gs::booleans());
    if plus {
        common::add_onelibrary(&root);
    }
    let mut d = Device::open_export(&root).unwrap();
    let shown = player::db_identity(&root).unwrap_or_default();
    assert_eq!(player::stash(&mut d, false).unwrap(), None);
    assert_eq!(player::stash(&mut d, true).unwrap(), Some(shown.clone()));
    assert!(d.state().and_then(|s| s.pending.as_ref()).is_none(), "nothing marked");
    d.begin(locallibrary::device::LineageOp::Sync, "sync from B", None).unwrap();
    assert_eq!(player::stash(&mut d, true).unwrap(), Some(shown.clone()));
    if plus {
        player::preserve(&d, Some(&DbIdentity { name: Some("LATER".into()), colour: Some(1) })).unwrap();
    }
    let replaces = tc.draw(gs::booleans());
    assert_eq!(player::stash(&mut d, replaces).unwrap(), Some(shown.clone()));
    let marked = Device::open(&root).unwrap().state().and_then(|s| s.pending.clone()).and_then(|p| p.player);
    assert_eq!(marked, Some(shown));
}

/// A OneLibrary property row whose name isn't text or whose colour isn't a number shows neither,
/// and can't be set over.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_property_row_of_the_wrong_type_shows_no_identity(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    let wrong = tc.draw(gs::sampled_from(vec![
        "UPDATE property SET deviceName = X'00ff'",
        "UPDATE property SET backGroundColorType = 'red'",
        "UPDATE property SET backGroundColorType = 2.5",
    ]));
    let one = OneLibrary::open_root(&root).unwrap();
    one.connection().execute(wrong, []).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let mut d = Device::open_export(&root).unwrap();
    assert_eq!(player::db_identity(&root), None, "{wrong}");
    assert_eq!(player::read(&d).onelibrary, None);
    let before = fs::read(root.join(paths::LIBRARY_PLUS_DB)).unwrap();
    assert!(player::set(&mut d, Some("MAIN"), None).is_err(), "{wrong}");
    assert_eq!(fs::read(root.join(paths::LIBRARY_PLUS_DB)).unwrap(), before);
}

/// A record file that can't be read (a folder by that name) is passed over for the next one.
#[hegel::test(test_cases = 4)]
fn prop_a_record_that_cant_be_read_is_passed_over(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::blank(dir.path(), "USB");
    let backup = root.join("PIONEER/DeviceLibBackup");
    let folders = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    for n in 0..folders {
        fs::create_dir_all(backup.join(format!("rbDevLibBaInfo_{n}.json"))).unwrap();
    }
    let record = tc.draw(gs::booleans());
    if record {
        fs::write(backup.join("rbDevLibBaInfo_9.json"), r#"{"info": [{"device_id": "id9"}]}"#).unwrap();
    }
    let got = player::rekordbox_device(&root).map(|r| r.device_id);
    assert_eq!(got, record.then(|| "id9".to_string()));
}

/// A drive another process is writing takes neither a name and colour nor a contact, and is
/// left as it was.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_locked_drive_takes_no_name_or_contact(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    let mut d = Device::open_export(&root).unwrap();
    let held = d.lock().unwrap();
    let before = common::drive_files(&root);
    let e = match tc.draw(gs::booleans()) {
        true => player::set(&mut d, Some("MAIN"), Some(2)).unwrap_err(),
        false => player::set_contact(&mut d, "sam@example.com", tc.draw(gs::booleans()), &Ctx::silent()).unwrap_err(),
    };
    assert_eq!(e.code(), "locked", "{e}");
    // What can't be set is refused before the lock is wanted.
    assert_eq!(player::set(&mut d, Some(""), None).unwrap_err().code(), "invalid");
    assert!(common::changed_files(&before, &common::drive_files(&root)).is_empty());
    drop(held);
}
