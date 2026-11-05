//! A backup that doesn't record what its change left (an earlier version took it, or the change
//! didn't finish) isn't put back over the drive unless forced, and says why.

use crate::common;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::backup;
use locallibrary::device::Device;
use locallibrary::edit::Edit;

/// The drive at `root` with a playlist made on it: the backup that took, by id.
fn edited(root: &std::path::Path, name: &str) -> String {
    common::apply(root, &[Edit::CreatePlaylist { path: name.into(), folder: false }]);
    let d = Device::open_export(root).unwrap();
    backup::list(&d).unwrap()[0].id.clone()
}

#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_backup_that_doesnt_say_what_its_change_left_is_put_back_only_when_forced(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let name = tc.draw(gs::sampled_from(vec!["Warm Up", "Peak", "Ünï ♪"]));
    let id = edited(&root, name);
    let d = Device::open_export(&root).unwrap();
    assert_eq!(backup::choose(&d, None, false).unwrap().id, id, "the drive holds its change");
    // As an earlier version wrote it: no record of what the change left.
    let info = backup::dir(&d, &id).join("backup.json");
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&info).unwrap()).unwrap();
    json.as_object_mut().unwrap().remove("after");
    fs::write(&info, serde_json::to_vec(&json).unwrap()).unwrap();

    let by_id = tc.draw(gs::booleans());
    let asked = by_id.then_some(id.as_str());
    let e = backup::choose(&d, asked, false).unwrap_err();
    assert!(e.to_string().contains("doesn't say what its change left"), "{e}");
    assert_eq!(backup::choose(&d, asked, true).unwrap().id, id);
}

/// Undoing a change to the player name puts back the name the drive showed before it, and the
/// drive remembers that one again: from what the backup carries, or, for a backup that carries
/// nothing (an earlier version took it, or the drive's journal was a newer build's then), from
/// the database put back. Either way no recorded edit comes off or goes back on the journal.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(before = "FIRST", newer_journal = false, earlier_version = false)]
#[hegel::explicit_test_case(before = "FIRST", newer_journal = false, earlier_version = true)]
#[hegel::explicit_test_case(before = "FIRST", newer_journal = true, earlier_version = false)]
#[hegel::explicit_test_case(before = "FIRST", newer_journal = true, earlier_version = true)]
fn prop_undoing_a_player_name_brings_back_the_one_shown_before(tc: TestCase) {
    use locallibrary::edit::journal;
    use locallibrary::player::{self, DbIdentity};

    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    let mut d = Device::open_export(&root).unwrap();
    d.adopt(Some("Main")).unwrap();
    let before = tc.draw(gs::sampled_from(vec!["FIRST", "Booth B", "Ünï ♪"]));
    player::set(&mut d, Some(before), Some(1)).unwrap();
    // A journal this build can't read while the change is made: the backup can't carry it.
    let newer_journal = tc.draw(gs::booleans());
    let edits = d.state_dir().join("edits.json");
    if newer_journal {
        fs::write(&edits, br#"{"version": 99, "entries": []}"#).unwrap();
    }
    player::set(&mut d, Some("SECOND"), Some(2)).unwrap();
    if newer_journal {
        fs::remove_file(&edits).unwrap();
    }
    let b = backup::list(&d).unwrap().remove(0);
    // As an earlier version took it: carrying nothing.
    let earlier_version = tc.draw(gs::booleans());
    if earlier_version && !newer_journal {
        fs::remove_file(backup::dir(&d, &b.id).join("carried.json")).unwrap();
    }
    assert_eq!(
        backup::dir(&d, &b.id).join("carried.json").is_file(),
        !newer_journal && !earlier_version,
        "a newer build's journal isn't carried"
    );
    assert!(journal::taken_off(&d, &b).unwrap().is_empty());
    assert!(journal::put_back(&d, &b).unwrap().is_empty());

    let undone = backup::restore(&mut d, None, false).unwrap();
    assert_eq!(undone.id, b.id);
    let shown = DbIdentity { name: Some(before.into()), colour: Some(1) };
    assert_eq!(player::db_identity(&root), Some(shown.clone()));
    let remembered = Device::open(&root).unwrap().state().and_then(|s| s.player.clone());
    assert_eq!(remembered, Some(shown), "remembered as shown, so a refresh doesn't set SECOND again");
}

/// Undo stops before it puts anything back while another write holds the drive, when the
/// backups folder can't be read (nor can a change make a backup then), when the database in the
/// backup can't be read to check its audio, or, forced, when a file the backup lists is gone.
#[cfg(unix)]
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(case = 0u8)]
#[hegel::explicit_test_case(case = 1u8)]
#[hegel::explicit_test_case(case = 2u8)]
#[hegel::explicit_test_case(case = 3u8)]
fn prop_undo_stops_when_it_cant_read_what_it_puts_back(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;

    use locallibrary::Error;
    use locallibrary::paths;
    use locallibrary::player;

    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    let mut d = Device::open_export(&root).unwrap();
    d.adopt(Some("Main")).unwrap();
    player::set(&mut d, Some("AFTER"), Some(4)).unwrap();
    let b = backup::list(&d).unwrap().remove(0);
    let kept = backup::dir(&d, &b.id);
    let backups = kept.parent().unwrap().to_path_buf();
    let before = common::drive_files(&root);

    let case = tc.draw(gs::integers::<u8>().max_value(3));
    let r = match case {
        0 => {
            let _other_write = Device::open(&root).unwrap().lock().unwrap();
            backup::restore(&mut d, None, false)
        }
        1 => {
            fs::set_permissions(&backups, fs::Permissions::from_mode(0o000)).unwrap();
            let changed = player::set(&mut d, Some("AGAIN"), Some(5));
            let r = backup::restore(&mut d, None, false);
            fs::set_permissions(&backups, fs::Permissions::from_mode(0o755)).unwrap();
            assert!(matches!(changed, Err(Error::Io { .. })), "no backup, no change: {changed:?}");
            r
        }
        2 => {
            fs::write(paths::to_fs(&kept, paths::EXPORT_PDB), b"not a database").unwrap();
            backup::restore(&mut d, None, false)
        }
        _ => {
            fs::remove_file(paths::to_fs(&kept, paths::LIBRARY_PLUS_DB)).unwrap();
            backup::restore(&mut d, None, true)
        }
    };
    match case {
        0 => assert!(matches!(r, Err(Error::Locked(_))), "{r:?}"),
        2 => assert!(matches!(r, Err(Error::Parse { .. })), "{r:?}"),
        _ => assert!(matches!(r, Err(Error::Io { .. })), "{r:?}"),
    }
    assert_eq!(common::drive_files(&root), before, "nothing put back");
}
