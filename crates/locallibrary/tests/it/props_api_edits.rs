//! Editing through the service layer, from outside lists and rules: a rekordbox XML playlist
//! imported into a playlist, a tracklist of nothing the drive has, history lists deleted once
//! the store keeps them, rules for some drives, and a My Tag filter on a drive without My Tags.

use crate::common::{self, path};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::Error;
use locallibrary::api;
use locallibrary::device::Device;
use locallibrary::import::{MatchKind, Status};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::xml::{self, XmlOptions};

/// The twelve-track export with a playlist, and the rekordbox XML of its library: importing that
/// playlist from the XML (into a new playlist, or onto the end of one, or replacing it) finds each
/// of its tracks, in order, or the track chosen for an entry, and the playlist then holds those
/// after what it kept; a playlist the XML lacks isn't found. A list of tracks the drive doesn't
/// have plans nothing.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(onto_itself = true, append = true, choice = None::<usize>)]
fn an_xml_playlist_imports_as_the_tracks_it_names(tc: TestCase) {
    let onto_itself = tc.draw(gs::booleans());
    let append = tc.draw(gs::booleans());
    let choice = tc.draw(gs::optional(gs::integers::<usize>().max_value(20)));

    let dir = tempfile::tempdir().unwrap();
    let ctx = Ctx::silent();
    common::isolate(dir.path(), None);
    let root = common::bigger_drive(dir.path(), "Main");
    let lib = common::library(&root);
    let list = lib.playlists.iter().find(|p| !p.is_folder && !p.track_ids.is_empty()).expect("a playlist");
    let name = lib.playlist_path(list.id);
    let (text, _) = xml::export(&Device::open(&root).unwrap(), &XmlOptions::default(), &ctx).unwrap();
    let file = dir.path().join("collection.xml");
    std::fs::write(&file, text).unwrap();
    let into = if onto_itself { name.clone() } else { "Imported".to_string() };
    // A choice of another track for an entry the list has.
    let chosen = choice.filter(|i| *i < list.track_ids.len()).map(|i| {
        let other = lib.tracks.iter().find(|t| t.id != list.track_ids[i]).unwrap().id;
        (i, other)
    });
    let req = api::ImportRequest {
        drive: path(&root),
        file: file.clone(),
        into: into.clone(),
        playlist: Some(format!("/{name}/")),
        append,
        choices: chosen.into_iter().collect(),
    };
    let plan = api::import_plan(&req, &ctx).unwrap();
    let mut want: Vec<u32> = list.track_ids.clone();
    if let Some((i, t)) = chosen {
        want[i] = t;
    }
    let found: Vec<u32> = plan.plan.report.entries.iter().filter_map(|e| e.status.track_id()).collect();
    assert_eq!(found, want, "each entry, as the XML names it");
    if let Some((i, t)) = chosen {
        assert!(
            matches!(plan.plan.report.entries[i].status, Status::Found { track_id, how: MatchKind::Chosen } if track_id == t)
        );
    }
    let kept: Vec<u32> = if append && onto_itself { list.track_ids.clone() } else { Vec::new() };
    plan.apply(None, &ctx).unwrap();
    let after = common::library(&root);
    assert_eq!(after.find_playlist(&into).unwrap().track_ids, [kept, want].concat());

    let missing = api::ImportRequest { playlist: Some("Nowhere".into()), ..req.clone() };
    assert!(matches!(api::import_plan(&missing, &ctx).map(|p| p.noop), Err(Error::NotFound(_))));
    let nothing = dir.path().join("nothing.txt");
    std::fs::write(&nothing, "Nobody - Nothing\nNo One - Nowhere\n").unwrap();
    let none = api::ImportRequest { file: nothing, playlist: None, choices: Default::default(), ..req };
    let plan = api::import_plan(&none, &ctx).unwrap();
    assert!(plan.noop && plan.plan.plan.is_none());
    assert_eq!(plan.summary, ["none of the list's tracks are on the drive"]);
}

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
