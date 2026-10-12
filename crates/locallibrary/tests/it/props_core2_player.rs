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
