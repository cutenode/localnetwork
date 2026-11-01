//! [`with_history`]: a library's history lists as playlists, for software that keeps no history.

use hegel::TestCase;
use hegel::generators as gs;
use loclib_interop::{HISTORY_FOLDER, with_history};
use loclib_model::Library;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::common;

/// History lists join a library (all of it, or part, as an export of some playlists has it) as
/// playlists: its own playlists stay as they were, a new top-level folder `History` goes after
/// them, and below it each list and folder keeps its name, kind, order and tracks, in the folder
/// it was in. Every node's id is distinct, and the tracks are the library's and those the lists
/// play, in the order of the library they were cut from. With no history, nothing changes.
#[hegel::test(test_cases = 200)]
fn history_lists_join_as_playlists(tc: TestCase) {
    let all = common::library(&tc, 8);
    let history = common::playlists(&tc, &all.tracks);
    let tracks = all.tracks.iter().filter(|_| tc.draw(gs::booleans())).cloned().collect();
    let lib = Library { tracks, playlists: all.playlists.clone(), history: history.clone(), history_playlists: 1 };
    let out = with_history(lib.clone(), &history, &all.tracks);
    if history.is_empty() {
        assert_eq!(out, lib);
        return;
    }

    let (own, added) = out.playlists.split_at(lib.playlists.len());
    assert_eq!(own, lib.playlists);
    let (folder, lists) = added.split_first().unwrap();
    assert_eq!((folder.name.as_str(), folder.is_folder, folder.parent_id), (HISTORY_FOLDER, true, None));
    let top = lib.playlists.iter().filter(|p| p.parent_id.is_none());
    assert!(top.clone().all(|p| p.sort_order < folder.sort_order), "after the others");
    assert_eq!(lists.len(), history.len());
    let ids: FxHashMap<u32, u32> = history.iter().zip(lists).map(|(h, p)| (h.id, p.id)).collect();
    for (h, p) in history.iter().zip(lists) {
        assert_eq!(
            (&p.name, p.is_folder, p.sort_order, &p.track_ids),
            (&h.name, h.is_folder, h.sort_order, &h.track_ids)
        );
        assert_eq!(p.parent_id, Some(h.parent_id.map_or(folder.id, |parent| ids[&parent])));
    }
    let distinct: FxHashSet<u32> = out.playlists.iter().map(|p| p.id).collect();
    assert_eq!(distinct.len(), out.playlists.len());

    let played: FxHashSet<u32> =
        lib.tracks.iter().map(|t| t.id).chain(history.iter().flat_map(|h| h.track_ids.iter().copied())).collect();
    let want: Vec<u32> = all.tracks.iter().map(|t| t.id).filter(|id| played.contains(id)).collect();
    assert_eq!(out.tracks.iter().map(|t| t.id).collect::<Vec<_>>(), want);
    assert!(out.history.is_empty() && out.history_playlists == 0);
}
