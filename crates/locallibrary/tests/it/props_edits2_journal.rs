//! The journal of edits when it isn't one this build can use, and clearing it up to a time.

use crate::common;

use std::fs;

use chrono::{DateTime, Duration, TimeZone, Utc};
use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::backup;
use locallibrary::cue_edit::{self, CueEdit, CuePosition, HotCue, SetCue};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions};
use locallibrary::progress::Ctx;

/// A journal that isn't one this build can read or add to (not JSON, entries that aren't edits,
/// a newer version, or not a list of entries) is refused by everything that reads it or adds to
/// it (listing, the edits to replay, what an undo takes off, clearing, planning an edit, making a
/// cue edit), and is left as it was.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_journal_this_build_cant_use_is_refused_and_kept(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    // An edit, so the drive has a backup an undo could take the journal's edits off with.
    common::apply(&root, &[Edit::CreatePlaylist { path: "Made".into(), folder: false }]);
    let mut d = Device::open_export(&root).unwrap();
    let info = backup::list(&d).unwrap().remove(0);
    let at = tc.draw(gs::integers::<u32>().min_value(1_000).max_value(60_000));
    let cue = cue_edit::plan(
        &d,
        &[1],
        &CueEdit::Set(SetCue {
            hot_cue: Some(HotCue(0)),
            at: CuePosition::Ms(at),
            snap: false,
            loop_beats: None,
            name: None,
            color: None,
        }),
        &Ctx::silent(),
    )
    .unwrap();
    let file = d.state_dir().join("edits.json");
    let unusable = [
        "{ not json".to_string(),
        r#"[{"at": 5, "edits": "none"}]"#.to_string(),
        format!(
            r#"{{"version": {}, "entries": []}}"#,
            edit::journal::VERSION + tc.draw(gs::integers::<u32>().min_value(1).max_value(9))
        ),
        "42".to_string(),
    ];
    for bytes in &unusable {
        fs::write(&file, bytes).unwrap();
        assert!(edit::journal::list(&d).is_err(), "{bytes}");
        assert!(edit::journal::edits(&d).is_err(), "{bytes}");
        assert!(edit::journal::taken_off(&d, &info).is_err(), "{bytes}");
        assert!(edit::journal::clear(&d, None).is_err(), "{bytes}");
        let made = Edit::CreatePlaylist { path: "Another".into(), folder: false };
        assert!(edit::plan(&d, &[made], &EditOptions::default(), &Ctx::silent()).is_err(), "{bytes}");
        assert!(cue_edit::apply(&cue, &mut d, &Ctx::silent()).is_err(), "{bytes}");
        assert_eq!(fs::read_to_string(&file).unwrap(), *bytes);
    }
}

/// Clearing the journal before a time takes off exactly the entries recorded before it, oldest
/// or not, and keeps the rest in order; clearing it all takes everything.
#[hegel::test(test_cases = 60)]
fn clearing_the_journal_before_a_time_takes_exactly_the_earlier_entries(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(dir.path()).unwrap();
    let start: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let hours: Vec<i64> = tc.draw(gs::vecs(gs::integers::<i64>().min_value(0).max_value(48)).max_size(6));
    let entries: Vec<serde_json::Value> = hours
        .iter()
        .enumerate()
        .map(|(i, h)| {
            serde_json::json!({
                "at": start + Duration::hours(*h),
                "edits": [{"op": "create_playlist", "path": format!("P{i}")}],
                "changes": [],
            })
        })
        .collect();
    let file = d.state_dir().join("edits.json");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, serde_json::to_vec(&serde_json::json!({"version": 2, "entries": entries})).unwrap()).unwrap();
    let before =
        tc.draw(gs::optional(gs::integers::<i64>().min_value(0).max_value(49))).map(|h| start + Duration::hours(h));
    let kept: Vec<String> = hours
        .iter()
        .enumerate()
        .filter(|(_, h)| before.is_some_and(|b| start + Duration::hours(**h) >= b))
        .map(|(i, _)| format!("P{i}"))
        .collect();
    assert_eq!(edit::journal::clear(&d, before).unwrap(), hours.len() - kept.len());
    let left: Vec<String> = edit::journal::edits(&d)
        .unwrap()
        .into_iter()
        .map(|e| match e {
            Edit::CreatePlaylist { path, .. } => path,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(left, kept);
}
