//! Editing through the service layer, from outside lists and rules: history lists deleted once
//! the store keeps them, rules for some drives, and a My Tag filter on a drive without My Tags.
//! (Importing a tracklist is the tracklist plugin's: the CLI's props_cli_tracklist.rs.)

use crate::common::{self, path};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::Error;
use locallibrary::api;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;

/// The demo export with history lists, saved in a store: deleting one the drive lacks isn't
/// found; once the store keeps them, the ones named go. Rules for a drive by its name apply to
/// it alone and are removed by path. A My Tag filter on a drive without My Tags matches no track,
/// and negated, every one.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn edits_kept_or_refused_as_their_requests_say(tc: TestCase) {
    let which = tc.draw(gs::sampled_from(vec!["history 001", "HISTORY 002", " History 001 "]));
    let negated = tc.draw(gs::booleans());

    let dir = tempfile::tempdir().unwrap();
    let ctx = Ctx::silent();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let volumes = common::isolate(dir.path(), Some(store.root()));
    let root = common::demo_drive(&volumes, "Main");
    common::adopt(Some(&store), &root, "Main");
    let ids: Vec<u32> = common::library(&root).tracks.iter().map(|t| t.id).collect();
    for name in ["HISTORY 001", "HISTORY 002"] {
        common::add_history(&root, name, &ids[..1]);
    }
    let delete = |names: Vec<String>| api::DeleteHistoryRequest { drive: path(&root), names, force: false };
    let e =
        api::delete_history_plan(Some(&store), &delete(vec!["HISTORY 999".into()]), &ctx).map(|p| p.noop).unwrap_err();
    assert!(matches!(&e, Error::NotFound(m) if m == "no such history list on the drive"), "{e}");
    api::pull(&store, &api::PullRequest { drive: path(&root), ..Default::default() }, &ctx).unwrap();
    let plan = api::delete_history_plan(Some(&store), &delete(vec![which.into()]), &ctx).unwrap();
    plan.apply(Some(&store), &ctx).unwrap();
    let left: Vec<String> =
        common::library(&root).history.into_iter().filter(|h| !h.is_folder).map(|h| h.name).collect();
    let gone = which.trim().to_uppercase();
    assert_eq!(left, ["HISTORY 001", "HISTORY 002"].into_iter().filter(|n| *n != gone).collect::<Vec<_>>());

    let rule = api::SetSmartPlaylist {
        path: "Everything".into(),
        filter: "bpm:>=1".into(),
        sort: None,
        limit: None,
        drives: vec!["main".into()],
    };
    let rules = api::smart_set(&store, &rule).unwrap();
    assert_eq!(rules.playlists.len(), 1);
    api::smart_apply(&store, &api::DriveRef { drive: path(&root) }, &ctx).unwrap();
    assert!(common::library(&root).playlists.iter().any(|p| p.name == "Everything"));
    let left = api::smart_remove(&store, &api::RemoveSmartPlaylist { path: "/everything/".into() }).unwrap();
    assert!(left.playlists.is_empty());

    let filter = if negated { "-tag:Peak" } else { "tag:Peak" };
    let page = api::tracks_query(&api::TracksQuery {
        library: path(&root).into(),
        filter: Some(filter.into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(page.total, if negated { ids.len() } else { 0 });
}
