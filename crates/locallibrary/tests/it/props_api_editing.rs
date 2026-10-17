//! Properties of the service layer's edits (`api::editing`, `api::tags`, `api::cues`): edits
//! made through it agree with the same edits made through `edit` directly, undo and replay give
//! back the library they should, refused requests change nothing, and queries, imports, history
//! deletions and smart playlists answer what the library and their models say.

use crate::common::{self, Before, library, path};

use indexmap::IndexMap;
use std::fs;
use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
use locallibrary::api::{
    self, ClearEdits, CueOp, DeleteHistoryRequest, DriveRef, EditCues, EditRequest, ImportRequest, ListCues,
    PullRequest, RemoveSmartPlaylist, RestoreBackup, SetSmartPlaylist, TagAction, TagsEdit, TracksQuery, UndoRequest,
};
use locallibrary::cue_edit::{self, CueEdit, CuePosition, CueRef, HotCue, SetCue};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions, TrackFields};
use locallibrary::error::Reason;
use locallibrary::import;
use locallibrary::progress::Ctx;
use locallibrary::query::{self, Filter, QueryContext, TrackSelector};
use locallibrary::{Library, Store, Track, diff, paths};

/// The twelve-track export with a playlist and two history lists, in a folder of its own.
fn fresh(dir: &Path, name: &str) -> PathBuf {
    let root = common::bigger_drive(dir, name);
    common::add_history(&root, "HISTORY 001", &[1, 2]);
    common::add_history(&root, "History 002", &[3, 1]);
    root
}

fn ids(lib: &Library) -> Vec<u32> {
    lib.tracks.iter().map(|t| t.id).collect()
}

/// Playlists by path (folder or not) with their entries' files.
type Playlists = Vec<(String, bool, Vec<String>)>;
/// Each track's title, artist, rating and comment by its file.
type Details = IndexMap<String, (String, String, u8, String)>;

/// Playlists by path with their entries' files, and each track's details by file: what an edit
/// changes, comparable across exports whose row ids differ.
fn shape(lib: &Library) -> (Playlists, Details) {
    let file = |id: &u32| lib.track(*id).map(|t| t.file_path.clone()).unwrap_or_default();
    let mut playlists: Vec<(String, bool, Vec<String>)> = lib
        .playlists
        .iter()
        .map(|p| (lib.playlist_path(p.id), p.is_folder, p.track_ids.iter().map(file).collect()))
        .collect();
    playlists.sort();
    let tracks = lib
        .tracks
        .iter()
        .map(|t| (t.file_path.clone(), (t.title.clone(), t.artist.clone(), t.rating, t.comment.clone())))
        .collect();
    (playlists, tracks)
}

fn some(tc: &TestCase, ids: &[u32]) -> TrackSelector {
    TrackSelector {
        ids: tc.draw(gs::vecs(gs::sampled_from(ids.to_vec())).min_size(1).max_size(3)),
        ..Default::default()
    }
}

/// An edit of `lib`'s playlists or tracks, naming playlists it has or might have.
#[hegel::test_helper]
fn an_edit(tc: &TestCase, lib: &Library) -> Edit {
    let ids = ids(lib);
    let mut names: Vec<String> =
        lib.playlists.iter().filter(|p| !p.is_folder).map(|p| lib.playlist_path(p.id)).collect();
    names.extend(["Warmup".to_string(), "Sets/Peak".to_string()]);
    let playlist = tc.draw(gs::sampled_from(names));
    match tc.draw(gs::integers::<u8>().max_value(8)) {
        0 => Edit::CreatePlaylist {
            path: tc.draw(gs::sampled_from(vec!["Warmup", "Sets/Peak", "Sets/Late"])).into(),
            folder: false,
        },
        1 => Edit::AddTracks {
            playlist,
            tracks: some(tc, &ids),
            position: tc.draw(gs::optional(gs::integers::<usize>().max_value(4))),
            sort: None,
            allow_repeats: tc.draw(gs::booleans()),
        },
        2 => Edit::RemoveTracks { playlist, tracks: Some(some(tc, &ids)), positions: Vec::new() },
        3 => Edit::RenamePlaylist { playlist, name: tc.draw(gs::sampled_from(vec!["Renamed", "Warmup 2"])).into() },
        4 => Edit::DeletePlaylist { playlist, with_tracks: false },
        5 => Edit::EditTracks {
            tracks: some(tc, &ids),
            fields: TrackFields {
                rating: tc.draw(gs::optional(gs::integers::<u8>().max_value(5))),
                comment: tc.draw(gs::optional(gs::sampled_from(vec!["", "warm", "peak ♪"]).map(String::from))),
                ..Default::default()
            },
        },
        6 => Edit::SortPlaylist {
            playlist,
            by: tc.draw(gs::sampled_from(vec!["bpm", "-rating", "title", "key,bpm"])).into(),
        },
        7 => Edit::MoveTracks {
            playlist,
            positions: vec![tc.draw(gs::integers::<usize>().max_value(3))],
            to: tc.draw(gs::integers::<usize>().max_value(3)),
        },
        _ => Edit::DeleteTracks { tracks: some(tc, &ids), keep_files: tc.draw(gs::booleans()) },
    }
}

/// Batches of edits planned and applied through the service layer leave a drive as the same
/// edits made with `edit` on a copy of it; a refused batch is refused alike and changes nothing.
/// Undo takes the last batch back, a replay after rekordbox exports over the drive makes the
/// batches still recorded again, and clearing the journal forgets them.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn edits_agree_with_edit_and_undo_and_replay(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let a = fresh(dir.path(), "A");
    let b = dir.path().join("B");
    common::copy_dir(&a, &b);
    let original = fs::read(a.join(paths::EXPORT_PDB)).unwrap();
    let spec = path(&a);
    let mut libraries = vec![library(&a)];
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(2)) {
        let lib = library(&a);
        let edits: Vec<Edit> =
            (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3))).map(|_| an_edit(&tc, &lib)).collect();
        tc.note(&format!("{edits:?}"));
        let req = EditRequest { drive: spec.clone(), edits: edits.clone(), options: EditOptions::default() };
        let before = Before::take(&a);
        let planned = api::edit_plan(&req, &Ctx::silent());
        let direct = edit::plan(&Device::open_export(&b).unwrap(), &edits, &EditOptions::default(), &Ctx::silent());
        let (planned, direct) = match (planned, direct) {
            (Ok(p), Ok(d)) => (p, d),
            (Err(e), Err(d)) => {
                assert_eq!(e.code(), d.code(), "{e} / {d}");
                before.untouched(&a).unwrap();
                continue;
            }
            (p, d) => panic!("through the api: {:?}; directly: {:?}", p.err(), d.err()),
        };
        assert_eq!((&*planned.operation, planned.noop), ("edit", direct.is_noop()));
        assert_eq!(planned.summary[..direct.changes.len()], direct.changes[..]);
        assert_eq!(planned.destructive, direct.destructive());
        // The plan shown, or planned again and applied in one go.
        let report = match tc.draw(gs::booleans()) {
            true => planned.apply(None, &Ctx::silent()).unwrap(),
            false => api::edit_apply(&req, &Ctx::silent()).unwrap(),
        };
        edit::apply(&direct, &mut Device::open_export(&b).unwrap(), &Ctx::silent()).unwrap();
        assert_eq!(library(&a), library(&b), "the drive edited through the api and directly");
        assert_eq!(report.backup.is_some(), !direct.is_noop());
        if !direct.is_noop() {
            libraries.push(library(&a));
        }
    }
    let r = DriveRef { drive: spec.clone() };
    let journal = api::edits_list(&r).unwrap();
    assert_eq!(journal.len(), libraries.len() - 1, "one entry per batch that changed something");

    // Undo: the library before the last batch, with its entry off the journal.
    if libraries.len() > 1 {
        // What restoring its backup would change: from the library now to the one before it.
        let preview = api::restore_preview(&RestoreBackup { drive: spec.clone(), backup: None, force: false }).unwrap();
        let label = format!("backup {}", preview.backup.id);
        let now = library(&a);
        let expected = diff::between(&now, &libraries[libraries.len() - 2], (&preview.diff.left, &label), None);
        assert_eq!(serde_json::to_value(&preview.diff).unwrap(), serde_json::to_value(&expected).unwrap());
        let unknown = RestoreBackup { drive: spec.clone(), backup: Some("nothing".into()), force: false };
        assert_eq!(api::restore_preview(&unknown).unwrap_err().code(), "not_found");
        let undo = UndoRequest { drive: Some(spec.clone()), ..Default::default() };
        assert_eq!(api::undo_plan(&undo).unwrap().plan.journal.len(), 1);
        api::undo_apply(&undo, &Ctx::silent()).unwrap();
        libraries.pop();
        assert_eq!(library(&a), *libraries.last().unwrap());
        assert_eq!(api::edits_list(&r).unwrap().len(), libraries.len() - 1);
    }

    // rekordbox exports the library as it was over the drive: a replay makes the edits again.
    let edited = shape(libraries.last().unwrap());
    fs::write(a.join(paths::EXPORT_PDB), &original).unwrap();
    let replay = api::edits_replay_plan(&r, &Ctx::silent()).unwrap();
    assert_eq!(replay.operation, "edits.replay");
    api::edits_replay_apply(&r, &Ctx::silent()).unwrap();
    assert_eq!(shape(&library(&a)), edited, "replayed");
    assert_eq!(api::edits_list(&r).unwrap().len(), libraries.len() - 1, "a replay isn't recorded");

    // Forgetting them: only those before a time, then the rest.
    let clear = ClearEdits { drive: spec.clone(), before: Some(chrono::DateTime::<chrono::Utc>::MIN_UTC) };
    let plan = api::edits_clear_plan(&clear).unwrap();
    assert!(plan.noop && plan.plan.entries.is_empty());
    let all = ClearEdits { before: None, ..clear };
    let plan = api::edits_clear_plan(&all).unwrap();
    assert_eq!((plan.noop, plan.plan.entries.len()), (libraries.len() == 1, libraries.len() - 1));
    assert_eq!(api::edits_clear(&all).unwrap().removed, libraries.len() - 1);
    assert!(api::edits_list(&r).unwrap().is_empty());
}

const FILTERS: &[&str] = &[
    "",
    "bpm:120-130",
    "rating:>=1",
    "-genre:house",
    "key:8A~",
    "artist:a",
    "time:>3:00",
    "is:unfiled",
    "has:artwork",
    "plays:>0",
    "a",
    "-a",
    "title:\"the\"",
    "year:>2000",
    "is:played",
];
const SORTS: &[&str] = &["bpm", "-rating", "key,bpm", "title", "-time"];

/// A query of a drive's tracks answers the tracks its filter selects (only the playlist's
/// entries, in its order, with one), ordered by its sort and cut to its page; cached and uncached
/// agree, and an artist filter selects the tracks whose artist holds the text. A filter naming
/// no field, a sort by nothing and a playlist the drive lacks are refused.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn queries_agree_with_their_filter(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let root = fresh(dir.path(), "Main");
    let lib = library(&root);
    let cache = api::LibraryCache::new(1);
    let playlists: Vec<String> =
        lib.playlists.iter().filter(|p| !p.is_folder).map(|p| lib.playlist_path(p.id)).collect();
    for _ in 0..8 {
        let filter = tc.draw(gs::sampled_from(FILTERS.to_vec()));
        let sort = tc.draw(gs::optional(gs::sampled_from(SORTS.to_vec())));
        let playlist = tc.draw(gs::optional(gs::sampled_from(playlists.clone())));
        let offset = tc.draw(gs::integers::<usize>().max_value(lib.tracks.len()));
        let limit = tc.draw(gs::optional(gs::integers::<usize>().max_value(5)));
        let req = TracksQuery {
            library: path(&root).into(),
            filter: (!filter.is_empty()).then(|| filter.into()),
            playlist: playlist.clone(),
            sort: sort.map(String::from),
            offset,
            limit,
        };
        let page = api::tracks_query_cached(&cache, &req).unwrap();
        assert_eq!(
            serde_json::to_value(&page).unwrap(),
            serde_json::to_value(api::tracks_query(&req).unwrap()).unwrap()
        );
        let f = if filter.is_empty() { Filter::default() } else { Filter::parse(filter).unwrap() };
        let ctx = QueryContext::new(&lib);
        let mut expected: Vec<&Track> = match &playlist {
            Some(p) => lib
                .tracks_by_id(&lib.find_playlist(p).unwrap().track_ids)
                .into_iter()
                .filter(|t| f.matches(t, &ctx))
                .collect(),
            None => f.select(&ctx),
        };
        if let Some(s) = sort {
            query::sort_tracks(&mut expected, &query::parse_sort(s).unwrap());
        }
        assert_eq!(page.total, expected.len());
        assert_eq!(page.total_duration_secs, expected.iter().map(|t| u64::from(t.duration_secs)).sum::<u64>());
        let cut: Vec<u32> = expected.iter().skip(offset).take(limit.unwrap_or(usize::MAX)).map(|t| t.id).collect();
        assert_eq!(page.tracks.iter().map(|t| t.id).collect::<Vec<_>>(), cut, "{req:?}");
        if filter == "artist:a" && playlist.is_none() {
            let holds: Vec<u32> =
                lib.tracks.iter().filter(|t| t.artist.to_lowercase().contains('a')).map(|t| t.id).collect();
            let mut got: Vec<u32> = expected.iter().map(|t| t.id).collect();
            got.sort_unstable();
            assert_eq!(got, holds);
        }
    }
    let refused = |req: TracksQuery| api::tracks_query_cached(&cache, &req).unwrap_err().code();
    let base = TracksQuery { library: path(&root).into(), ..Default::default() };
    assert_eq!(refused(TracksQuery { filter: Some("gnere:house".into()), ..base.clone() }), "invalid");
    assert_eq!(refused(TracksQuery { sort: Some("nonsense".into()), ..base.clone() }), "invalid");
    assert_eq!(refused(TracksQuery { playlist: Some("No such list".into()), ..base }), "not_found");
}

/// Deleting history lists takes off exactly the lists named (every one when none are), and
/// only once the store keeps them or when forced: otherwise it's a conflict naming the lists
/// not saved, the drive left as it was. Lists the drive doesn't have are not found.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn history_is_deleted_only_once_kept(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let root = fresh(dir.path(), "Main");
    let on_drive = ["HISTORY 001", "History 002"];
    let names = tc
        .draw(gs::vecs(gs::sampled_from(vec!["HISTORY 001", " history 002", "HISTORY 009"])).max_size(2).unique(true));
    let force = tc.draw(gs::booleans());
    let req = DeleteHistoryRequest { drive: path(&root), names: names.iter().map(|n| n.to_string()).collect(), force };
    let wanted: Vec<&str> = on_drive
        .iter()
        .copied()
        .filter(|l| names.is_empty() || names.iter().any(|n| n.trim().eq_ignore_ascii_case(l)))
        .collect();
    let deleted = |root: &Path| {
        let left: Vec<String> = library(root).history.iter().filter(|h| !h.is_folder).map(|h| h.name.clone()).collect();
        let expected: Vec<&str> = on_drive.iter().copied().filter(|l| !wanted.contains(l)).collect();
        assert_eq!(left, expected);
    };
    // Without the store: refused unless forced.
    let before = Before::take(&root);
    match api::delete_history_apply(None, &req, &Ctx::silent()) {
        Err(e) if wanted.is_empty() => {
            assert_eq!(e.code(), "not_found", "{e}");
            return before.untouched(&root).unwrap();
        }
        Err(e) if !force => {
            assert_eq!((e.code(), e.reason()), ("conflict", Some(Reason::NotSaved)), "{e}");
            assert_eq!(e.detail().fields["lists"], serde_json::json!(wanted));
            before.untouched(&root).unwrap();
        }
        Err(e) => panic!("{req:?} refused: {e}"),
        Ok(_) => {
            assert!(force, "{req:?} made unforced");
            return deleted(&root);
        }
    }
    // Once the store keeps them, they go.
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    api::pull(&store, &PullRequest { drive: path(&root), ..Default::default() }, &Ctx::silent()).unwrap();
    let plan = api::delete_history_plan(Some(&store), &req, &Ctx::silent()).unwrap();
    assert!(plan.destructive && plan.operation == "history.delete");
    api::delete_history_apply(Some(&store), &req, &Ctx::silent()).unwrap();
    deleted(&root);
}

/// Importing a tracklist finds what `import` finds in the drive's library, and fills the
/// playlist with the tracks found, in the list's order (after its entries, appending); a choice
/// for an entry the list doesn't have, or of a track the drive lacks, is refused.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn imports_fill_the_playlist_with_what_is_found(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let root = fresh(dir.path(), "Main");
    let lib = library(&root);
    let mut names: Vec<String> = lib.tracks.iter().map(Track::display_name).collect();
    names.extend(["Nobody - Nothing".to_string(), "Unknown Artist - Untitled".to_string()]);
    let lines = tc.draw(gs::vecs(gs::sampled_from(names)).min_size(1).max_size(6));
    let file = dir.path().join("set.txt");
    fs::write(&file, lines.join("\n")).unwrap();
    let into = tc.draw(gs::sampled_from(vec![
        "Imported".to_string(),
        lib.playlist_path(lib.playlists.iter().find(|p| !p.is_folder).unwrap().id),
    ]));
    let append = tc.draw(gs::booleans());
    let req = ImportRequest {
        drive: path(&root),
        file: file.clone(),
        into: into.clone(),
        playlist: None,
        append,
        choices: IndexMap::new(),
    };
    let bytes = fs::read(&file).unwrap();
    let entries = import::parse(&bytes, Some("set.txt")).unwrap();
    let expected = import::match_entries(&lib, &entries);
    let planned = api::import_plan(&req, &Ctx::silent()).unwrap();
    assert_eq!(serde_json::to_value(&planned.plan.report.entries).unwrap(), serde_json::to_value(&expected).unwrap());
    let found = planned.plan.report.track_ids();
    assert_eq!(planned.plan.plan.is_none(), found.is_empty());
    let existing: Vec<u32> =
        lib.find_playlist(&into).filter(|_| append).map(|p| p.track_ids.clone()).unwrap_or_default();
    match api::import_apply(&req, &Ctx::silent()) {
        Ok(_) => {
            let after = library(&root);
            let entries = &after.find_playlist(&into).unwrap().track_ids;
            assert_eq!(*entries, [existing, found].concat());
        }
        Err(e) => assert!(found.is_empty() && e.code() == "not_found", "{e}"),
    }
    let n = planned.plan.report.entries.len();
    let choose = |i: usize, track: u32| ImportRequest { choices: [(i, track)].into_iter().collect(), ..req.clone() };
    assert_eq!(api::import_plan(&choose(n, lib.tracks[0].id), &Ctx::silent()).unwrap_err().code(), "invalid");
    assert_eq!(api::import_plan(&choose(0, 9999), &Ctx::silent()).unwrap_err().code(), "not_found");
}

/// Smart playlists kept in the store follow a model of rules by path: set adds or replaces,
/// remove takes one off (an unknown path is not found), and an invalid filter is refused with
/// the rules as they were. Applied to a drive, each rule's playlist holds what a query with its
/// filter, sort and limit answers.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn smart_playlists_follow_their_rules(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = fresh(dir.path(), "Main");
    let mut model: IndexMap<String, (String, Option<String>, Option<usize>)> = IndexMap::new();
    let paths = vec!["Smart/Fast", "Smart/Rated", "Picks"];
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5)) {
        let p = tc.draw(gs::sampled_from(paths.clone()));
        if tc.draw(gs::weighted_booleans(0.7)) {
            let filter = tc.draw(gs::sampled_from(vec!["bpm:>120", "rating:>=1", "a", "gnere:house"]));
            let sort = tc.draw(gs::optional(gs::sampled_from(SORTS.to_vec()))).map(String::from);
            let limit = tc.draw(gs::optional(gs::integers::<usize>().min_value(1).max_value(4)));
            let req = SetSmartPlaylist {
                path: format!("/{p}/"),
                filter: filter.into(),
                sort: sort.clone(),
                limit,
                drives: Vec::new(),
            };
            match api::smart_set(&store, &req) {
                Ok(_) => {
                    model.insert(p.to_string(), (filter.into(), sort, limit));
                }
                Err(e) => assert!(filter == "gnere:house" && e.code() == "invalid", "{e}"),
            }
        } else {
            match api::smart_remove(&store, &RemoveSmartPlaylist { path: p.into() }) {
                Ok(_) => assert!(model.shift_remove(p).is_some(), "{p} removed though never set"),
                Err(e) => assert!(!model.contains_key(p) && e.code() == "not_found", "{e}"),
            }
        }
        let rules = api::smart_list(&store).unwrap();
        let listed: Vec<(String, String)> =
            rules.playlists.iter().map(|r| (r.path.clone(), r.filter.clone())).collect();
        let expected: Vec<(String, String)> = model.iter().map(|(p, (f, _, _))| (p.clone(), f.clone())).collect();
        assert_eq!(listed, expected);
    }
    let unknown = SetSmartPlaylist {
        path: "Elsewhere".into(),
        filter: "a".into(),
        sort: None,
        limit: None,
        drives: vec!["Nowhere".into()],
    };
    assert_eq!(api::smart_set(&store, &unknown).unwrap_err().code(), "not_found");
    let r = DriveRef { drive: path(&root) };
    if model.is_empty() {
        assert_eq!(api::smart_apply_plan(&store, &r, &Ctx::silent()).unwrap_err().code(), "not_found");
        return;
    }
    api::smart_apply(&store, &r, &Ctx::silent()).unwrap();
    let lib = library(&root);
    for (p, (filter, sort, limit)) in &model {
        let q = TracksQuery {
            library: path(&root).into(),
            filter: Some(filter.clone()),
            sort: sort.clone(),
            limit: *limit,
            ..Default::default()
        };
        let wanted: Vec<u32> = api::tracks_query(&q).unwrap().tracks.iter().map(|t| t.id).collect();
        assert_eq!(lib.find_playlist(p).map(|l| l.track_ids.clone()), Some(wanted), "{p}");
    }
}

/// My Tag edits through the service layer follow a model of tags and the tracks carrying them:
/// adding gives the tracks the tag, removing takes it off, renaming keeps its tracks, deleting
/// takes it off every track; what's listed is the model, cached or not, and a plan that changes
/// nothing says so.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn tag_edits_follow_a_model(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let root = common::demo_drive(dir.path(), "Main");
    let lib = library(&root);
    let ids = ids(&lib);
    let mut model: IndexMap<String, Vec<u32>> = IndexMap::new();
    // rekordbox keeps four categories: a tag in another is refused.
    let tags = vec!["Genre/Deep", "Genre/Tech", "Situation/Warm", "Mood/Warm"];
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5)) {
        let tag = tc.draw(gs::sampled_from(tags.clone())).to_string();
        let tracks = some(&tc, &ids);
        let action = match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 | 1 => TagAction::Add { tag: tag.clone(), tracks: tracks.clone() },
            2 => TagAction::Remove { tag: tag.clone(), tracks: tracks.clone() },
            _ => TagAction::Delete { tag: tag.clone() },
        };
        tc.note(&format!("{action:?}"));
        let req = TagsEdit { drive: path(&root), action: action.clone() };
        let chosen: Vec<u32> = tracks.ids.clone();
        let carried = model.get(&tag).cloned();
        let result = api::tags_plan(&req).and_then(|p| {
            let noop = p.noop;
            p.apply(None, &Ctx::silent()).map(|_| noop)
        });
        match (&action, result) {
            (TagAction::Add { .. }, Ok(noop)) => {
                let on = model.entry(tag.clone()).or_default();
                let new: Vec<u32> = chosen.iter().copied().filter(|t| !on.contains(t)).collect();
                assert_eq!(noop, new.is_empty() && carried.is_some(), "{action:?}");
                on.extend(new);
                on.sort_unstable();
                on.dedup();
            }
            (TagAction::Remove { .. }, Ok(noop)) => {
                let on = model.get_mut(&tag).expect("removed a tag the model lacks");
                assert_eq!(noop, !on.iter().any(|t| chosen.contains(t)));
                on.retain(|t| !chosen.contains(t));
            }
            (TagAction::Delete { .. }, Ok(_)) => {
                assert!(model.shift_remove(&tag).is_some(), "deleted {tag}, which the model lacks")
            }
            (TagAction::Add { .. }, Err(e)) => {
                assert!(tag.starts_with("Mood/") && e.code() == "invalid", "{action:?}: {e}")
            }
            (_, Err(e)) => assert!(carried.is_none() && e.code() == "not_found", "{action:?}: {e}"),
            (TagAction::Rename { .. }, Ok(_)) => unreachable!(),
        }
        let by_track = api::tags_by_track(&DriveRef { drive: path(&root) }).unwrap();
        for t in &lib.tracks {
            let mut have: Vec<String> =
                model.iter().filter(|(_, on)| on.contains(&t.id)).map(|(n, _)| n.clone()).collect();
            have.sort();
            let mut got = by_track.get(&paths::key(&t.file_path).to_lowercase()).cloned().unwrap_or_default();
            got.sort();
            assert_eq!(got, have, "#{}'s tags", t.id);
        }
    }
    // Renaming a tag the drive has (to a new name in its category) keeps its tracks.
    if let Some((tag, on)) = model.iter().find(|(_, on)| !on.is_empty()).map(|(t, o)| (t.clone(), o.clone())) {
        let req = TagsEdit { drive: path(&root), action: TagAction::Rename { tag: tag.clone(), to: "Renamed".into() } };
        api::tags_apply(&req, &Ctx::silent()).unwrap();
        let renamed = format!("{}/Renamed", tag.split_once('/').unwrap().0);
        let by_track = api::tags_by_track(&DriveRef { drive: path(&root) }).unwrap();
        for id in on {
            let key = paths::key(&lib.track(id).unwrap().file_path).to_lowercase();
            assert!(by_track[&key].contains(&renamed), "#{id} lost its tag: {:?}", by_track[&key]);
        }
    }
}

/// Cue edits through the service layer leave the analysis files as the same edits made with
/// `cue_edit` on a copy of the drive, list what was set, and an edit refused is refused alike.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn cue_edits_agree_with_cue_edit(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let a = common::demo_drive(dir.path(), "A");
    let b = dir.path().join("B");
    common::copy_dir(&a, &b);
    let ids = ids(&library(&a));
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let tracks = some(&tc, &ids);
        let slot = tc.draw(gs::optional(gs::integers::<u8>().max_value(7))).map(HotCue);
        let at = match tc.draw(gs::booleans()) {
            true => CuePosition::Beat(tc.draw(gs::integers::<u32>().min_value(1).max_value(64))),
            false => CuePosition::Ms(tc.draw(gs::integers::<u32>().max_value(400_000))),
        };
        let set = SetCue { hot_cue: slot, at, snap: false, loop_beats: None, name: Some("In".into()), color: None };
        let (op, direct) = match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 | 1 => (CueOp::Set(set.clone()), CueEdit::Set(set)),
            2 => (CueOp::Delete { cue: CueRef::All }, CueEdit::Delete(CueRef::All)),
            _ => {
                let from = tc.draw(gs::sampled_from(ids.clone()));
                (CueOp::Copy { from: TrackSelector { ids: vec![from], ..Default::default() } }, CueEdit::Copy { from })
            }
        };
        let req = EditCues { drive: path(&a), tracks: tracks.clone(), edit: op };
        // As the service layer selects them: each track once.
        let d = Device::open(&b).unwrap();
        let selected = tracks.select_on(&d, &d.library().unwrap()).unwrap();
        let theirs = cue_edit::plan(&d, &selected, &direct, &Ctx::silent());
        match (api::cues_plan(&req, &Ctx::silent()), theirs) {
            (Ok(mine), Ok(theirs)) => {
                assert_eq!(mine.noop, theirs.is_noop());
                assert_eq!(mine.destructive, theirs.tracks.iter().any(|t| t.after.len() < t.before.len()));
                mine.apply(None, &Ctx::silent()).unwrap();
                cue_edit::apply(&theirs, &mut Device::open(&b).unwrap(), &Ctx::silent()).unwrap();
            }
            (Err(e), Err(d)) => assert_eq!(e.code(), d.code(), "{e} / {d}"),
            (m, t) => panic!("through the api: {:?}; directly: {:?}", m.err(), t.err()),
        }
        let analysis = |root: &Path| -> Vec<(String, Vec<u8>)> {
            common::library_files(root).into_iter().filter(|(k, _)| k.starts_with("PIONEER/USBANLZ/")).collect()
        };
        assert_eq!(analysis(&a), analysis(&b));
    }
    let listed = api::cues_list(&ListCues {
        library: path(&a).into(),
        tracks: TrackSelector { ids: ids.clone(), ..Default::default() },
    })
    .unwrap();
    let direct = cue_edit::list(&Device::open(&b).unwrap(), &ids).unwrap();
    assert_eq!(serde_json::to_value(&listed).unwrap(), serde_json::to_value(&direct).unwrap());
    // Copying from several tracks at once is refused.
    let req = EditCues {
        drive: path(&a),
        tracks: TrackSelector { ids: vec![ids[0]], ..Default::default() },
        edit: CueOp::Copy { from: TrackSelector { ids: ids.clone(), ..Default::default() } },
    };
    assert_eq!(api::cues_apply(&req, &Ctx::silent()).unwrap_err().code(), "invalid");
}

/// Replacing duplicates replaces each Sync Manager copy (`<name>-1.<ext>`, the same audio) with
/// the other copy of its recording: one of each pair stays, playlist entries of the one that
/// goes move to it, and its file goes.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn dedupe_keeps_one_of_each_copy(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let root = fresh(dir.path(), "Main");
    let lib = library(&root);
    let originals: Vec<u32> = tc.draw(gs::vecs(gs::sampled_from(ids(&lib))).min_size(1).max_size(2).unique(true));
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut pairs = Vec::new();
    for id in &originals {
        let orig = lib.track(*id).unwrap();
        let (stem, ext) = orig.file_path.rsplit_once('.').unwrap();
        let copy_path = format!("{stem}-1.{ext}");
        let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
        let copy = ed
            .add_track(&rekordbox_pdb::NewTrack {
                artist: Some(orig.artist.clone()),
                duration: orig.duration_secs as u16,
                file_size: orig.file_size as u32,
                ..rekordbox_pdb::NewTrack::new(orig.title.clone(), copy_path.clone())
            })
            .unwrap();
        ed.save(&pdb_path).unwrap();
        fs::copy(paths::to_fs(&root, &paths::key(&orig.file_path)), paths::to_fs(&root, &paths::key(&copy_path)))
            .unwrap();
        pairs.push((*id, copy, orig.file_path.clone(), copy_path));
    }
    let entries: Vec<u32> = pairs.iter().map(|p| p.1).collect();
    let set = Edit::SetTracks {
        playlist: "Set".into(),
        tracks: TrackSelector { ids: entries, ..Default::default() },
        sort: None,
        limit: None,
        create: true,
    };
    common::apply(&root, &[set]);
    let req = api::DedupeRequest { drive: path(&root), ..Default::default() };
    let plan = api::dedupe_plan(&req, &Ctx::silent()).unwrap();
    let removed: Vec<u32> = plan.plan.tracks_removed.iter().map(|t| t.id).collect();
    assert_eq!(removed.len(), pairs.len(), "{:?}", plan.summary);
    assert!(removed.iter().all(|r| pairs.iter().any(|(a, b, ..)| r == a || r == b)), "{removed:?} of {pairs:?}");
    api::dedupe_apply(&req, &Ctx::silent()).unwrap();
    let after = library(&root);
    let set = after.find_playlist("Set").unwrap();
    for (a, b, a_path, b_path) in &pairs {
        let kept: Vec<&Track> = [a, b].iter().filter_map(|id| after.track(**id)).collect();
        assert_eq!(kept.len(), 1, "one of #{a} and #{b} stays");
        assert!(set.track_ids.contains(&kept[0].id), "Set's entry moved to the one kept");
        let (gone, kept_file) = if kept[0].id == *a { (b_path, a_path) } else { (a_path, b_path) };
        assert!(!paths::to_fs(&root, &paths::key(gone)).exists(), "{gone} left");
        assert!(paths::to_fs(&root, &paths::key(kept_file)).exists(), "{kept_file} deleted");
    }
}

/// Tracks whose audio went missing get it back when exactly one file of its name is found: one
/// moved elsewhere on the drive is pointed at where it is, one only on this computer is copied
/// back to its path; after that there's nothing left to relocate.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn relocating_finds_each_moved_file(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let root = common::bigger_drive(dir.path(), "Main");
    let music = dir.path().join("Music");
    fs::create_dir_all(&music).unwrap();
    let (mut repointed, mut copied) = (Vec::new(), Vec::new());
    for i in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let secs = tc.draw(gs::integers::<u16>().min_value(1).max_value(2));
        let device_path = format!("/Contents/Tones/Tone {i}.wav");
        let track = rekordbox_pdb::NewTrack {
            duration: secs,
            ..rekordbox_pdb::NewTrack::new(format!("Tone {i}"), device_path.clone())
        };
        let id = common::add_track(&root, track, &common::media::wav_of(8_000, 16, 1, f64::from(secs)));
        let at = paths::to_fs(&root, &paths::key(&device_path));
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => {}
            1 => {
                let to = root.join(format!("Contents/Moved/Tone {i}.wav"));
                fs::create_dir_all(to.parent().unwrap()).unwrap();
                fs::rename(&at, &to).unwrap();
                repointed.push(id);
            }
            _ => {
                fs::rename(&at, music.join(format!("Tone {i}.wav"))).unwrap();
                copied.push(id);
            }
        }
    }
    let req = api::RelocateRequest {
        drive: path(&root),
        options: locallibrary::relocate::RelocateOptions { folders: vec![music.clone()] },
    };
    let plan = api::relocate_plan(&req, &Ctx::silent()).unwrap();
    let ids = |r: &[locallibrary::relocate::Relocation]| -> Vec<u32> { r.iter().map(|r| r.track.id).collect() };
    assert_eq!((ids(&plan.plan.repointed), ids(&plan.plan.copied)), (repointed.clone(), copied.clone()));
    assert_eq!(plan.noop, repointed.is_empty() && copied.is_empty());
    api::relocate_apply(&req, &Ctx::silent()).unwrap();
    let lib = library(&root);
    for t in lib.tracks.iter().filter(|t| repointed.contains(&t.id) || copied.contains(&t.id)) {
        assert!(paths::to_fs(&root, &paths::key(&t.file_path)).is_file(), "#{} still lacks its file", t.id);
        assert_eq!(t.file_path.contains("/Moved/"), repointed.contains(&t.id));
    }
    assert!(api::relocate_plan(&req, &Ctx::silent()).unwrap().noop, "relocated twice");
}

/// Packing a drive for a set puts exactly the chosen playlists and their tracks on the target,
/// registers it in the store with its write, and packing again (as it was last packed) has
/// nothing to copy.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn packing_puts_the_chosen_playlists_on_the_target(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let club = fresh(dir.path(), "Club");
    let pick = |ids: Vec<u32>| TrackSelector { ids, ..Default::default() };
    common::apply(
        &club,
        &[
            Edit::CreatePlaylist { path: "Sets/Late".into(), folder: false },
            Edit::AddTracks {
                playlist: "Sets/Late".into(),
                tracks: pick(vec![2, 4]),
                position: None,
                sort: None,
                allow_repeats: false,
            },
            Edit::CreatePlaylist { path: "Early".into(), folder: false },
            Edit::AddTracks {
                playlist: "Early".into(),
                tracks: pick(vec![1, 4]),
                position: None,
                sort: None,
                allow_repeats: false,
            },
        ],
    );
    let lib = library(&club);
    let names: Vec<String> = lib.playlists.iter().filter(|p| !p.is_folder).map(|p| lib.playlist_path(p.id)).collect();
    let chosen = tc.draw(gs::vecs(gs::sampled_from(names)).min_size(1).max_size(2).unique(true));
    let stick = common::blank(dir.path(), "Stick");
    let req = api::PackRequest {
        source: path(&club),
        target: path(&stick),
        options: locallibrary::pack::PackOptions { playlists: chosen.clone(), ..Default::default() },
    };
    let plan = api::pack_plan(&req, &Ctx::silent()).unwrap();
    let mut tracks: Vec<u32> = chosen.iter().flat_map(|p| lib.find_playlist(p).unwrap().track_ids.clone()).collect();
    tracks.sort_unstable();
    tracks.dedup();
    assert_eq!(plan.plan.tracks, tracks.len());
    assert!(
        plan.summary[0].starts_with(&format!("pack {} from Club onto Stick", plan.plan.playlists.join(", "))),
        "{:?}",
        plan.summary
    );
    api::pack_apply(Some(&store), &req, &Ctx::silent()).unwrap();
    let packed = library(&stick);
    let files = |l: &Library, ids: &[u32]| -> Vec<String> {
        let mut f: Vec<String> =
            l.tracks.iter().filter(|t| ids.is_empty() || ids.contains(&t.id)).map(|t| t.file_path.clone()).collect();
        f.sort();
        f
    };
    assert_eq!(files(&packed, &[]), files(&lib, &tracks));
    let mut on_stick: Vec<String> =
        packed.playlists.iter().filter(|p| !p.is_folder).map(|p| packed.playlist_path(p.id)).collect();
    let mut want = chosen.clone();
    on_stick.sort();
    want.sort();
    assert_eq!(on_stick, want);
    let writes: Vec<u32> = store.drives().unwrap().into_values().map(|d| d.log.writes).collect();
    assert!(writes.contains(&1), "the stick is registered with its write: {writes:?}");
    let again = api::PackRequest { options: Default::default(), ..req };
    assert!(api::pack_plan(&again, &Ctx::silent()).unwrap().plan.copies.is_empty(), "packed twice");
}
