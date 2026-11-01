//! Syncing two drives, and several, against what compare says of them and a model of what changed
//! on each since they last held the same library: which way a sync goes, when it's refused and
//! why, and a drive back from a set keeping its players' changes in the store first.

use crate::common::{self, path};
use crate::props_api_drives::{demo_export, saved_copy, saved_demo};
use crate::props_api_jobs::own_identity;

use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{HealthCheck, PrintableGenerator, TestCase};
use locallibrary::api::{self, Direction, SyncRequest};
use locallibrary::compare::{Confidence, Relation};
use locallibrary::device::Device;
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
use locallibrary::{Error, paths};

/// What happened to a drive since the two last held the same library.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Change {
    None,
    /// A player recorded a history list: a change players make.
    History,
    /// rekordbox exported a track to it.
    Export,
    /// rekordbox exported it with a track fewer, a few minutes later.
    Remove,
}

impl Change {
    fn own(self) -> bool {
        matches!(self, Change::Export | Change::Remove)
    }
}

fn make(root: &Path, change: Change) {
    match change {
        Change::None => {}
        Change::History => {
            let ids: Vec<u32> = common::library(root).tracks.iter().map(|t| t.id).collect();
            common::add_history(root, "HISTORY 001", &ids);
        }
        Change::Export => {
            common::export_new_track(root, "Artist", &format!("Fresh on {}", root.display()));
        }
        Change::Remove => {
            let pdb = root.join(paths::EXPORT_PDB);
            let last = common::library(root).tracks.iter().map(|t| t.id).max().unwrap();
            let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
            ed.remove_tracks(&[last]).unwrap();
            ed.save(&pdb).unwrap();
            let later = std::time::SystemTime::now() + std::time::Duration::from_secs(180);
            std::fs::File::options().write(true).open(&pdb).unwrap().set_modified(later).unwrap();
        }
    }
}

fn changes() -> impl PrintableGenerator<Change> {
    gs::sampled_from(vec![Change::None, Change::History, Change::Export, Change::Remove])
}

/// Two drives hold the same library, as compare tells libraries apart.
fn same(a: &Path, b: &Path) -> bool {
    let pair = api::ComparePair { left: path(a), right: path(b) };
    api::compare(&pair, &Ctx::silent()).unwrap().relation == Relation::Identical
}

fn labels(e: &Error) -> Vec<String> {
    e.detail().actions.into_iter().map(|a| a.label).collect()
}

/// Main (saved in the store) and Club (a clone of it the store knows, or a copy of the export
/// made outside locallibrary, which shares no history with it), each changed or not, and maybe a
/// blank drive. Synced as a pair, a sync goes the way compare says and refuses what it can't
/// order: a guess unless it's taken, drives that both changed unless one only came back from a
/// set (then it's overwritten, after its players' changes are saved in the store, which it
/// needs), and what compare can't tell, saying why. Pinned to a direction, keeping the players'
/// changes is only for a drive back from a set. Synced as several (refused for blank drives alone,
/// none of them the newest), the newest goes onto every other, a drive back from a set is sent to
/// be synced as a pair first, and a target that went or changed since the plan fails alone.
///
/// The explicit cases are the situations each branch needs: a guess, drives back from a set
/// synced with the store and without, pinned and among several, what compare can't tell, and a
/// target that went or changed since the plan.
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(linked = false, main_change = Change::None, club_change = Change::History, spare = false, allow_low_confidence = false, with_store = false, as_pair = false, pinned = false, direction = Direction::LeftToRight, meddle = 0u8)]
#[hegel::explicit_test_case(linked = true, main_change = Change::Export, club_change = Change::History, spare = false, allow_low_confidence = false, with_store = true, as_pair = true, pinned = false, direction = Direction::LeftToRight, meddle = 0u8)]
#[hegel::explicit_test_case(linked = true, main_change = Change::History, club_change = Change::Remove, spare = false, allow_low_confidence = false, with_store = false, as_pair = true, pinned = false, direction = Direction::LeftToRight, meddle = 0u8)]
#[hegel::explicit_test_case(linked = true, main_change = Change::Export, club_change = Change::History, spare = true, allow_low_confidence = false, with_store = true, as_pair = false, pinned = true, direction = Direction::LeftToRight, meddle = 0u8)]
#[hegel::explicit_test_case(linked = false, main_change = Change::None, club_change = Change::Remove, spare = false, allow_low_confidence = true, with_store = false, as_pair = false, pinned = true, direction = Direction::RightToLeft, meddle = 0u8)]
#[hegel::explicit_test_case(linked = false, main_change = Change::None, club_change = Change::History, spare = true, allow_low_confidence = true, with_store = true, as_pair = false, pinned = false, direction = Direction::LeftToRight, meddle = 1u8)]
#[hegel::explicit_test_case(linked = true, main_change = Change::Export, club_change = Change::None, spare = true, allow_low_confidence = false, with_store = false, as_pair = false, pinned = false, direction = Direction::LeftToRight, meddle = 2u8)]
fn syncs_go_the_way_compare_and_the_changes_say(tc: TestCase) {
    let linked = tc.draw(gs::booleans());
    let main_change = tc.draw(changes());
    let club_change = tc.draw(changes());
    let spare = tc.draw(gs::booleans());
    let allow_low_confidence = tc.draw(gs::booleans());
    let with_store = tc.draw(gs::booleans());
    // Applied as a pair, or left for a sync of several.
    let as_pair = tc.draw(gs::booleans());
    let pinned = tc.draw(gs::booleans());
    let direction = tc.draw(gs::sampled_from(vec![Direction::LeftToRight, Direction::RightToLeft]));
    // The blank drive goes (1), or gets a stray audio file the plan didn't see (2), before a sync
    // of several is applied.
    let meddle = tc.draw(gs::integers::<u8>().max_value(2));

    let dir = tempfile::tempdir().unwrap();
    let ctx = Ctx::silent();
    let (store, main) = saved_copy(dir.path());
    let volumes = main.parent().unwrap().to_path_buf();
    let club = volumes.join("Club");
    match linked {
        true => {
            saved_demo().1.write(&club);
            own_identity(&store, &mut Device::open(&club).unwrap(), "Club");
        }
        false => {
            demo_export().write(&club);
        }
    }
    let club = Device::open(&club).unwrap().root().to_path_buf();
    make(&main, main_change);
    make(&club, club_change);
    // The drive back from a set, as the model has it: a clone whose only change is one players
    // make, while the other's are its own.
    let back: Option<&Path> = match (main_change, club_change) {
        _ if !linked => None,
        (Change::History, c) if c.own() => Some(&main),
        (c, Change::History) if c.own() => Some(&club),
        _ => None,
    };
    let compared = api::compare(&api::ComparePair { left: path(&main), right: path(&club) }, &ctx).unwrap();
    if back.is_some() {
        assert_eq!(compared.relation, Relation::Diverged, "both changed since the snapshot they share");
    }
    let low = compared.confidence == Confidence::Low;

    let spare = spare.then(|| common::blank(&volumes, "Spare"));
    if let Some(s) = &spare {
        let e = api::sync_plan(&common::sync_pair(s, s), &ctx).unwrap_err();
        assert!(
            api::sync_many_plan(&common::sync_many(&[s]), &ctx).is_err_and(|e| e.to_string().contains("at least two"))
        );
        let twice = common::sync_many(&[&main, &main]);
        assert!(api::sync_many_plan(&twice, &ctx).is_err_and(|e| e.to_string().contains("listed twice")));
        assert!(e.to_string().contains("same drive"), "{e}");
        let empty = common::blank(&volumes, "Empty");
        let e = api::sync_plan(&common::sync_pair(s, &empty), &ctx).unwrap_err();
        assert!(e.to_string().contains("neither drive holds a rekordbox export"), "{e}");
        let e = api::sync_many_plan(&common::sync_many(&[s, &empty]), &ctx).map(|p| p.noop).unwrap_err();
        assert!(matches!(&e, Error::Invalid(m) if m == "none of the drives holds a rekordbox export"), "{e}");
        std::fs::remove_dir(empty).unwrap();
        // Another library altogether: copying either way, or merging either way.
        let other = common::bigger_drive(&volumes, "Other");
        let e = api::sync_plan(&common::sync_pair(&main, &other), &ctx).unwrap_err();
        assert_eq!(e.reason(), Some(Reason::Unrelated), "{e}");
        assert_eq!(labels(&e).len(), 4);
        std::fs::remove_dir_all(other).unwrap();
    }

    // As a pair, the way compare says.
    let req = SyncRequest { allow_low_confidence, ..common::sync_pair(&main, &club) };
    let planned = api::sync_plan(&req, &ctx);
    let source: Option<&Path> = match compared.relation {
        Relation::Identical => None,
        Relation::LeftNewer | Relation::RightNewer if low && !allow_low_confidence => {
            let e = planned.as_ref().unwrap_err();
            assert_eq!(e.reason(), Some(Reason::Uncertain), "{e}");
            assert_eq!(labels(e), ["Copy Main over Club", "Copy Club over Main", "Take the guess"]);
            assert_eq!(e.detail().actions[2].params["allow_low_confidence"], true);
            None
        }
        Relation::LeftNewer => Some(&main),
        Relation::RightNewer => Some(&club),
        Relation::Diverged if back.is_some() => Some(if back == Some(&main) { &club } else { &main }),
        Relation::Diverged | Relation::Undetermined => {
            let e = planned.as_ref().unwrap_err();
            match compared.relation {
                Relation::Undetermined => {
                    assert_eq!(e.reason(), Some(Reason::Uncertain), "{e}");
                    assert!(e.to_string().contains(compared.undetermined().unwrap()), "{e} says why");
                }
                _ => assert_eq!(e.reason(), Some(Reason::Diverged), "{e}"),
            }
            assert_eq!(labels(e).len(), 4, "copying either way, or merging either way");
            None
        }
        Relation::Unrelated => panic!("copies of one export are related"),
    };
    if compared.relation == Relation::Identical {
        let planned = planned.as_ref().unwrap();
        assert!(planned.noop, "{:?}", planned.summary);
        let done = planned.apply(Some(&store), &ctx).unwrap();
        assert!(done.report.is_none() && done.target.is_none());
    }
    if let Some(source) = source.filter(|_| as_pair) {
        let planned = planned.unwrap();
        let target = if source == main { &club } else { &main };
        assert_eq!((planned.source.as_deref(), planned.target.as_deref()), (Some(source), Some(target.as_path())));
        let came_back = planned.after_set.as_ref().map(|g| g.back_from_set.drive.clone());
        let name = |p: &Path| Device::open(p).unwrap().name();
        assert_eq!(came_back, back.map(name));
        if let Some(b) = back {
            assert!(
                planned.summary[0].starts_with(&format!("{} came back from a set", name(b))),
                "{:?}",
                planned.summary
            );
        }
        let done = planned.apply(with_store.then_some(&store), &ctx);
        match (back, with_store) {
            (Some(_), false) => {
                let e = done.unwrap_err();
                assert_eq!(e.reason(), Some(Reason::NotSaved), "{e}");
                assert_eq!(e.detail().actions[0].method.as_deref(), Some("pull"));
                assert!(!same(source, target), "nothing copied");
            }
            (Some(_), true) => {
                let done = done.unwrap();
                assert_eq!(done.saved.len(), 2, "the state it came back with, then the result");
                let history =
                    api::history(&store, &api::HistoryRequest { library: "Main".into(), limit: None }).unwrap();
                assert_eq!(history[0].id, done.saved[1].id, "the result is the library's latest");
                assert!(history.iter().any(|s| s.id == done.saved[0].id));
                assert!(same(source, target));
            }
            (None, _) => {
                assert!(done.unwrap().saved.is_empty());
                assert!(same(source, target));
            }
        }
        return;
    }

    // Pinned to a direction, keeping the players' changes needs a drive back from a set to write
    // over.
    if pinned {
        let (source, target) = match direction {
            Direction::LeftToRight => (&main, &club),
            _ => (&club, &main),
        };
        // Without keeping them, it goes the way it's told, deleting what only the target has.
        let told = api::sync_plan(&SyncRequest { direction, ..req.clone() }, &ctx).unwrap();
        assert_eq!((told.source.as_ref(), told.target.as_ref()), (Some(source), Some(target)));
        let deletes = told.plan.plan.as_ref().is_some_and(|p| !p.deletes.is_empty());
        assert_eq!(told.destructive, deletes);
        assert_eq!(told.summary.iter().any(|l| l.starts_with("delete ")), deletes, "{:?}", told.summary);
        let pinned = SyncRequest { direction, keep_player_changes: true, ..req.clone() };
        match api::sync_plan(&pinned, &ctx) {
            Ok(p) => {
                assert_eq!(back, Some(target.as_path()));
                assert!(p.after_set.is_some());
            }
            Err(e) => {
                assert!(matches!(e, Error::StalePlan(_)), "{e}");
                assert_ne!(back, Some(target.as_path()));
            }
        }
    }

    // As several: Main against Club, as compare has them, with the blank drive always a target.
    let mut drives = vec![main.clone(), club.clone()];
    drives.extend(spare.clone());
    let refs: Vec<&Path> = drives.iter().map(PathBuf::as_path).collect();
    let many = api::SyncManyRequest { allow_low_confidence, ..common::sync_many(&refs) };
    let planned = api::sync_many_plan(&many, &ctx);
    let newest = match compared.relation {
        Relation::LeftNewer | Relation::RightNewer if low && !allow_low_confidence => {
            let e = planned.unwrap_err();
            assert_eq!(e.reason(), Some(Reason::Uncertain), "{e}");
            assert_eq!(labels(&e).last().map(String::as_str), Some("Take the guess"));
            return;
        }
        Relation::Identical | Relation::LeftNewer => &main,
        Relation::RightNewer => &club,
        _ => {
            let e = planned.unwrap_err();
            match back {
                Some(b) => {
                    assert_eq!(e.reason(), Some(Reason::BackFromSet), "{e}");
                    let b = Device::open(b).unwrap().name();
                    assert!(e.to_string().starts_with(&format!("{b} came back from a set")), "{e}");
                    assert_eq!(labels(&e), ["Sync Main and Club first"]);
                }
                None => assert!(matches!(e.reason(), Some(Reason::Diverged | Reason::Uncertain)), "{e}"),
            }
            return;
        }
    };
    let planned = planned.unwrap();
    assert_eq!(&planned.source, newest);
    let meddle = if spare.is_some() { meddle } else { 0 };
    if let Some(s) = &spare {
        match meddle {
            1 => std::fs::remove_dir_all(s).unwrap(),
            2 => {
                std::fs::create_dir_all(s.join("Contents/Stray")).unwrap();
                std::fs::write(s.join("Contents/Stray/stray.mp3"), "left by someone").unwrap();
            }
            _ => {}
        }
    }
    // Through the plan, a drive gone since stops the whole sync before anything is written; one
    // whose files changed fails alone, as does a gone one when the sync is confirmed as planned.
    let store = with_store.then_some(&store);
    let results = match planned.apply(store, &ctx) {
        Err(e) => {
            assert!(meddle == 1 && matches!(e, Error::StalePlan(_)), "{e}");
            api::sync_many_apply(store, &many, &planned.plan, &ctx).unwrap()
        }
        Ok(results) => {
            assert_ne!(meddle, 1, "applied though a drive went");
            results
        }
    };
    for r in &results {
        let failed = spare.as_ref().is_some_and(|s| s == &r.target) && meddle != 0;
        assert_eq!(r.error.is_some(), failed, "{r:?}");
        if !failed {
            assert!(same(newest, &r.target), "{} holds the newest", r.name);
        }
    }
    let other = if newest == &main { &club } else { &main };
    assert!(same(newest, other));
}
