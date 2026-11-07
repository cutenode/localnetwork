//! Merges the other merge properties leave out: drives that change between planning and
//! applying, a merge cancelled as it writes, a target whose export.pdb can't be read, and hot cue
//! bank lists side by side.

use crate::common::{self, Warnings};
use crate::drawn;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::merge::{self, MergeOptions, PlaylistMode};
use locallibrary::onelibrary::OneLibrary;
use locallibrary::paths;
use locallibrary::progress::{CancelToken, Ctx, Event, Phase};

/// How a source changes after its merge was planned.
#[derive(Clone, Copy, Debug)]
enum Change {
    /// Its export.pdb can no longer be read.
    Unreadable,
    /// A planned track's audio file is gone.
    AudioGone,
    /// Its export.pdb no longer has a planned track.
    TrackGone,
    /// Nothing changed, but the merge is cancelled as it starts on the databases.
    Cancelled,
    /// The target's databases changed instead: the plan is stale.
    TargetChanged,
}

/// A merge whose source changed after it was planned (its database unreadable, a planned track's
/// audio or row gone), whose target's databases changed, or that's cancelled as it edits the
/// databases, fails, and its target (one with export.pdb, and one with OneLibrary only) is left
/// as it was.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_merge_that_cannot_finish_leaves_the_target_unchanged(tc: TestCase) {
    let changes = [Change::Unreadable, Change::AudioGone, Change::TrackGone, Change::Cancelled, Change::TargetChanged];
    let change = tc.draw(gs::sampled_from(changes.to_vec()));
    let dir = tempfile::tempdir().unwrap();
    let guest = common::demo_drive(dir.path(), "Guest");
    let pdb = guest.join(paths::EXPORT_PDB);
    let without_drawn = fs::read(&pdb).unwrap();
    let drawn_id = drawn::track(&tc, &guest, 10);
    let src = Device::open_export(&guest).unwrap();
    let audio = paths::to_fs(&guest, &paths::key(&src.library().unwrap().track(drawn_id).unwrap().file_path));
    let mut targets = Vec::new();
    for (name, db) in [("Main", paths::EXPORT_PDB), ("Plus", paths::LIBRARY_PLUS_DB)] {
        let main = common::bigger_drive(dir.path(), name);
        if db == paths::LIBRARY_PLUS_DB {
            common::onelibrary_only(&main);
        }
        let dst = Device::open_export(&main).unwrap();
        let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
        assert!(plan.new_tracks.iter().any(|t| t.source_id == drawn_id), "the drawn track is planned");
        targets.push((main.join(db), dst, plan));
    }
    match change {
        Change::Unreadable => fs::write(&pdb, b"not a database").unwrap(),
        Change::AudioGone => fs::remove_file(&audio).unwrap(),
        Change::TrackGone => fs::write(&pdb, &without_drawn).unwrap(),
        Change::Cancelled => {}
        Change::TargetChanged => {
            for (db, _, _) in &targets {
                let mut bytes = fs::read(db).unwrap();
                bytes.push(0);
                fs::write(db, bytes).unwrap();
            }
        }
    }
    for (_, mut dst, plan) in targets {
        let cancel = CancelToken::new();
        let progress = {
            let (cancel, cancels) = (cancel.clone(), matches!(change, Change::Cancelled));
            move |e: &Event| {
                if cancels && matches!(e, Event::PhaseStart { phase: Phase::Write, .. }) {
                    cancel.cancel();
                }
            }
        };
        let before = common::Before::take(dst.root());
        let ctx = Ctx::new(&progress, cancel);
        let err = merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &ctx).unwrap_err();
        tc.note(&format!("{change:?} into {}: {err}", dst.root().display()));
        let wanted = match change {
            Change::AudioGone | Change::TrackGone => matches!(err.inner(), locallibrary::Error::NotFound(_)),
            Change::Cancelled => matches!(err.inner(), locallibrary::Error::Cancelled),
            Change::TargetChanged => matches!(err.inner(), locallibrary::Error::StalePlan(_)),
            Change::Unreadable => true,
        };
        assert!(wanted, "{err}");
        before.untouched(dst.root()).unwrap();
    }
}

/// A merge into a drive whose export.pdb can't be read is refused when planning, whatever the
/// unreadable bytes.
#[hegel::test(test_cases = 10)]
fn prop_a_merge_into_an_unreadable_export_is_refused(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let guest = common::demo_drive(dir.path(), "Guest");
    let main = common::blank(dir.path(), "Main");
    let bytes = tc.draw(gs::binary().max_size(64));
    let pdb = main.join(paths::EXPORT_PDB);
    fs::create_dir_all(pdb.parent().unwrap()).unwrap();
    fs::write(&pdb, &bytes).unwrap();
    let (src, dst) = (Device::open_export(&guest).unwrap(), Device::open(&main).unwrap());
    assert!(merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).is_err());
}

/// A source's hot cue bank lists side by side, each with a cue of a track the merge brings, come
/// into the target's merge folder in the order the source shows them (by position, then id),
/// whatever positions they were given.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_hot_cue_bank_lists_side_by_side_keep_their_order(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let guest = common::demo_drive(dir.path(), "Guest");
    common::add_onelibrary(&guest);
    let n = tc.draw(gs::integers::<usize>().min_value(2).max_value(4));
    let positions: Vec<i64> = (0..n).map(|_| tc.draw(gs::integers::<i64>().min_value(0).max_value(3))).collect();
    let mut one = OneLibrary::open_root(&guest).unwrap();
    let first = one.library().unwrap().tracks[0].id;
    let mut lists = Vec::new();
    for (i, position) in positions.iter().enumerate() {
        let cue = 900 + i as i64;
        one.connection()
            .execute("INSERT INTO cue (cue_id, content_id, kind, inUsec) VALUES (?1, ?2, 0, 2000000)", (cue, first))
            .unwrap();
        let list = one.create_hot_cue_bank_list(&format!("Bank {i}"), 0, false).unwrap();
        one.add_to_hot_cue_bank_list(list, cue).unwrap();
        one.connection()
            .execute("UPDATE hotCueBankList SET sequenceNo = ?1 WHERE hotCueBankList_id = ?2", (position, list))
            .unwrap();
        lists.push((*position, list, format!("Bank {i}")));
    }
    fs::write(guest.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    lists.sort();
    let wanted: Vec<String> = lists.into_iter().map(|(_, _, name)| name).collect();

    let main = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&main);
    let (src, mut dst) = (Device::open_export(&guest).unwrap(), Device::open_export(&main).unwrap());
    let opts =
        MergeOptions { playlists: PlaylistMode::Folder, folder_name: Some("From Guest".into()), ..Default::default() };
    let warnings = Warnings::default();
    let plan = merge::plan(&src, &dst, &opts, &warnings.ctx()).unwrap();
    let report = merge::apply(&plan, &src, &mut dst, &opts, &warnings.ctx()).unwrap();
    assert_eq!(report.hot_cue_bank_lists_added, n, "{:?}", warnings.said());

    let lists = OneLibrary::open_root(&main).unwrap().hot_cue_bank_lists().unwrap();
    let folder = lists.iter().find(|l| l.is_folder && l.name == "From Guest").expect("the merge folder");
    let mut inside: Vec<_> = lists.iter().filter(|l| l.parent_id == Some(folder.id)).collect();
    inside.sort_by_key(|l| (l.sort_order, l.id));
    assert_eq!(inside.iter().map(|l| l.name.clone()).collect::<Vec<_>>(), wanted, "{lists:?}");
}
