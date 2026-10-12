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
