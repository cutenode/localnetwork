//! A write cut short at either side of its swap: before it, the drive is as it was; after it,
//! the backup made first is kept and named, and restoring it puts every file back.

use crate::common;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions};
use locallibrary::progress::Ctx;
use locallibrary::tags::{self, TagChange};
use locallibrary::{backup, paths};

/// A My Tag edit of any of the drive's tracks that fails just before its swap leaves the drive
/// as it was, with no backup; one that fails just after it has the tag on those tracks, keeps
/// the one backup made before the plan and says undoing takes back what it made, and restoring
/// that backup puts every file back.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_write_cut_short_at_its_swap_can_be_taken_back(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    let files: Vec<String> = d.library().unwrap().tracks.iter().map(|t| t.file_path.clone()).collect();
    let tracks: Vec<String> = tc.draw(gs::subsequences(&files).min_size(1));
    let tag = format!(
        "{}/{}",
        tc.draw(gs::sampled_from(vec!["Genre", "Situation"])),
        tc.draw(gs::sampled_from(vec!["Deep", "Peak Time", "Ünï"]))
    );
    let edit = Edit::MyTag(TagChange::Add { tag: tag.clone(), tracks: tracks.clone() });
    let plan = edit::plan(&d, &[edit], &EditOptions::default(), &Ctx::silent()).unwrap();
    let before = common::Before::take(&root);

    let after_swap = tc.draw(gs::booleans());
    let fail =
        if after_swap { locallibrary::testing::fail_after_swap } else { locallibrary::testing::fail_before_swap };
    fail(true);
    let err = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap_err().to_string();
    fail(false);
    if !after_swap {
        before.untouched(&root).unwrap();
        return;
    }
    assert!(
        err.starts_with("the plan stopped after 0 of its edits: ") && err.ends_with("Undoing takes back what it made"),
        "{err}"
    );
    let by_track = tags::by_track(&Device::open_export(&root).unwrap()).unwrap();
    for t in &tracks {
        let has = by_track.get(&paths::file_key(t)).is_some_and(|tags| tags.contains(&tag));
        assert!(has, "{t} lacks {tag}: {by_track:?}");
    }
    let backups = backup::list(&Device::open_export(&root).unwrap()).unwrap();
    assert_eq!(backups.len(), 1, "{backups:?}");
    backup::restore(&mut Device::open_export(&root).unwrap(), None, false).unwrap();
    assert_eq!(common::changed_files(&before.files, &common::drive_files(&root)), Vec::<String>::new());
}
