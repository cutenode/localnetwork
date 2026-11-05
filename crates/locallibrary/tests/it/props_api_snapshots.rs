//! Properties of the store's snapshots through the service layer (`api::snapshots`): each
//! snapshot's library read back whole or a page at a time, and pruning, dry or applied.

use crate::common::{self, path};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, PruneOptions, PullRequest, SnapshotLibraryRequest};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;

/// A drive pulled `n` times, a track exported before each pull but the first: each snapshot's
/// library is the drive's as it was then, whole when no page is asked for, else the tracks from
/// the offset on, at most the limit of them, saying how many there are in all. Pruning to the
/// last `keep`, dry, says the older ones go and removes none; applied, it removes just those.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_snapshots_read_back_in_pages_and_prune_as_the_dry_run_says(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let ctx = Ctx::silent();
    let a = common::demo_drive(dir.path(), "A");
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let mut snaps = Vec::new();
    for i in 0..n {
        if i > 0 {
            common::export_new_track(&a, "New", &format!("Track {i}"));
        }
        let req = PullRequest { drive: path(&a), message: Some(format!("pull {i}")), ..Default::default() };
        snaps.push((api::pull(&store, &req, &ctx).unwrap().snapshot.unwrap(), common::library(&a)));
    }
    let ids = |tracks: &[locallibrary::model::Track]| tracks.iter().map(|t| t.id).collect::<Vec<_>>();
    for (snap, lib) in &snaps {
        let most = lib.tracks.len() + 1;
        let offset = tc.draw(gs::optional(gs::integers::<usize>().max_value(most)));
        let limit = tc.draw(gs::optional(gs::integers::<usize>().max_value(most)));
        let req = SnapshotLibraryRequest { snapshot: snap.short_id.clone(), offset, limit };
        let read = api::snapshot_library(&store, &req).unwrap();
        let skip = offset.unwrap_or(0);
        let want: Vec<u32> = ids(&lib.tracks).into_iter().skip(skip).take(limit.unwrap_or(usize::MAX)).collect();
        assert_eq!(ids(&read.library.tracks), want);
        match read.page {
            None => assert!(offset.is_none() && limit.is_none()),
            Some(p) => assert_eq!((p.offset, p.count, p.total), (skip, want.len(), lib.tracks.len())),
        }
    }

    let keep = tc.draw(gs::integers::<usize>().min_value(1).max_value(n));
    // Newest first, as a prune lists them.
    let gone: Vec<_> = snaps[..n - keep].iter().rev().map(|(s, _)| s.id).collect();
    let opts = PruneOptions { keep_last: Some(keep), dry_run: true, ..Default::default() };
    let dry = api::store_prune(&store, &opts, &ctx).unwrap();
    assert!(dry.dry_run);
    assert_eq!(dry.libraries[0].removed.iter().map(|s| s.id).collect::<Vec<_>>(), gone);
    assert!(snaps.iter().all(|(s, _)| store.load_snapshot(&s.id).is_ok()), "a dry run removes nothing");
    let report = api::store_prune(&store, &PruneOptions { dry_run: false, ..opts }, &ctx).unwrap();
    assert_eq!((report.removed_snapshots, report.freed_bytes), (gone.len(), dry.freed_bytes));
    for (s, _) in &snaps {
        assert_eq!(store.load_snapshot(&s.id).is_err(), gone.contains(&s.id));
    }
}
