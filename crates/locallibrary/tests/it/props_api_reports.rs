//! Properties of the service layer's reports and outputs (`api::check`, `api::diff`,
//! `api::plays`, `api::export`, `api::interop`, `api::doctor`, `api::mix`, `api::sidecar`):
//! each agrees with the library it reports on, a model of what it remembers, or the direct call
//! it wraps; outputs write what their plans say and replace nothing unasked; and the sidecar's
//! messages read back as they were written.

use crate::common::{self, Before, path};

use indexmap::IndexMap;
use rustc_hash::FxHashSet;
use std::fs;

use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
use locallibrary::api::{
    self, CheckRequest, ClearPlayers, DiffRequest, DoctorRequest, DriveRef, ExportPlaylist, HistoryArchiveRequest,
    InteropRequest, PlayStatsRequest, PullRequest, SetPlayers,
};
use locallibrary::check::CheckOptions;
use locallibrary::device::Device;
use locallibrary::doctor::{self, DoctorOptions};
use locallibrary::error::Reason;
use locallibrary::export::{Format, PathStyle};
use locallibrary::interop::{InteropOptions, Software};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::{Library, diff, plays};
use uuid::Uuid;

// ---------------------------------------------------------------------------------------------
// Player models
// ---------------------------------------------------------------------------------------------

const MODELS: &[&str] = &["CDJ-3000X", "cdj-2000nxs2", " XDJ-AZ ", "CDJ-3000", "CDJ-9999", ""];

/// The models the store remembers follow a model of a default and one list per library: set
/// replaces a list with the models named (each once, as the table spells them), clear forgets
/// it, a plan says when it changes nothing, and naming no model, an unknown one or a library the
/// store lacks is refused with the models as they were.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn remembered_players_follow_a_model(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let libraries: Vec<(Uuid, String)> =
        ["Main", "Club"].iter().map(|n| (store.create_library(n).unwrap().id, n.to_string())).collect();
    let (mut default, mut by_library): (Vec<String>, IndexMap<Uuid, Vec<String>>) = (Vec::new(), IndexMap::new());
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(8)) {
        let library = tc.draw(gs::optional(gs::sampled_from(vec!["Main", "club", "Nowhere"])));
        let id = library.and_then(|l| libraries.iter().find(|(_, n)| n.eq_ignore_ascii_case(l)).map(|(id, _)| *id));
        let known = library.is_none() || id.is_some();
        let before = api::players_show(&store).unwrap();
        let result = if tc.draw(gs::weighted_booleans(0.7)) {
            let names: Vec<String> = tc
                .draw(gs::vecs(gs::sampled_from(MODELS.to_vec())).max_size(3))
                .iter()
                .map(|m| m.to_string())
                .collect();
            let canonical: Option<Vec<String>> = names
                .iter()
                .map(|n| n.trim())
                .filter(|n| !n.is_empty())
                .map(|n| {
                    locallibrary::equipment::MODELS
                        .iter()
                        .find(|m| m.name.eq_ignore_ascii_case(n))
                        .map(|m| m.name.to_string())
                })
                .collect::<Option<Vec<String>>>()
                .map(|mut v| {
                    let mut seen = FxHashSet::default();
                    v.retain(|m| seen.insert(m.clone()));
                    v
                });
            let req = SetPlayers { models: names.clone(), library: library.map(String::from) };
            let planned = api::players_set_plan(&store, &req);
            match (&canonical, known) {
                (Some(models), true) if !models.is_empty() => {
                    let p = planned.unwrap();
                    let from = match id {
                        Some(id) => by_library.get(&id).cloned().unwrap_or_default(),
                        None => default.clone(),
                    };
                    assert_eq!((&p.plan.from, &p.plan.to, p.noop), (&from, models, from == *models));
                    assert_eq!(p.plan.library, id);
                    match id {
                        Some(id) => drop(by_library.insert(id, models.clone())),
                        None => default = models.clone(),
                    }
                    api::players_set(&store, &req).map(drop).map_err(drop)
                }
                (Some(models), _) if models.is_empty() => {
                    assert_eq!(planned.unwrap_err().code(), "invalid", "no models");
                    Err(())
                }
                (None, _) => {
                    assert!(planned.is_err(), "{names:?} remembered");
                    Err(())
                }
                (_, false) => {
                    let e = planned.unwrap_err();
                    assert_eq!((e.code(), e.reason()), ("not_found", Some(Reason::UnknownLibrary)), "{e}");
                    Err(())
                }
                _ => unreachable!(),
            }
        } else {
            let req = ClearPlayers { library: library.map(String::from) };
            let planned = api::players_clear_plan(&store, &req);
            if !known {
                assert_eq!(planned.unwrap_err().reason(), Some(Reason::UnknownLibrary));
                Err(())
            } else {
                let from = match id {
                    Some(id) => by_library.shift_remove(&id).unwrap_or_default(),
                    None => std::mem::take(&mut default),
                };
                let p = planned.unwrap();
                assert_eq!((&p.plan.from, p.plan.to.is_empty(), p.noop), (&from, true, from.is_empty()));
                api::players_clear(&store, &req).map(drop).map_err(drop)
            }
        };
        let shown = api::players_show(&store).unwrap();
        if result.is_err() {
            assert_eq!(
                serde_json::to_value(&shown).unwrap(),
                serde_json::to_value(&before).unwrap(),
                "refused, but changed"
            );
        }
        assert_eq!(shown.default, default);
        let listed: Vec<(Uuid, Option<String>, Vec<String>)> =
            shown.libraries.iter().map(|l| (l.id, l.name.clone(), l.models.clone())).collect();
        let expected: Vec<(Uuid, Option<String>, Vec<String>)> = by_library
            .iter()
            .map(|(id, m)| (*id, libraries.iter().find(|(i, _)| i == id).map(|(_, n)| n.clone()), m.clone()))
            .collect();
        assert_eq!(listed, expected);
    }
    assert_eq!(api::player_models().len(), locallibrary::equipment::MODELS.len());
}

// ---------------------------------------------------------------------------------------------
// A drive and its snapshots
// ---------------------------------------------------------------------------------------------

/// Play statistics as a model has them: history lists, tracks played, the ids of those never
/// played, and the most played (id, play count, lists it's in), most first.
struct StatsModel {
    lists: usize,
    played: usize,
    never: Vec<u32>,
    most: Vec<(u32, u16, usize)>,
}

/// What a drive's history lists played: for each track, its play count and the lists it's in.
fn stats_model(lib: &Library, limit: usize) -> StatsModel {
    let lists: Vec<_> = lib.history.iter().filter(|h| !h.is_folder).collect();
    let mut played = Vec::new();
    let mut never = Vec::new();
    for t in &lib.tracks {
        let in_lists = lists.iter().filter(|l| l.track_ids.contains(&t.id)).count();
        if t.play_count == 0 && in_lists == 0 {
            never.push(t.id);
        } else {
            played.push((t.id, t.play_count, in_lists));
        }
    }
    played.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(a.0.cmp(&b.0)));
    let n = played.len();
    played.truncate(limit);
    StatsModel { lists: lists.len(), played: n, never, most: played }
}

/// A drive pulled into the store, then given history lists, a new export and playlist edits at
/// random: a diff with its snapshot, by id or by its library's name, is `diff`'s between their
/// libraries and adds exactly the tracks and playlists the drive gained; play statistics count
/// what a model of play counts and history lists does, and what was played since the snapshot
/// (by id, or by a date after it) is the history lists recorded since; the history archive is
/// `plays`' for the library however it's named.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn diffs_and_play_statistics_agree_with_the_libraries(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_history(&root, "HISTORY 001", &[1, 2]);
    api::pull(&store, &PullRequest { drive: path(&root), ..Default::default() }, &Ctx::silent()).unwrap();
    let head = store.find_library("Main").unwrap().unwrap().head.unwrap();
    let before = Device::open(&root).unwrap().library().unwrap();
    let ids: Vec<u32> = before.tracks.iter().map(|t| t.id).collect();
    let mut recorded = Vec::new();
    for n in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        let name = format!("HISTORY {:03}", n + 2);
        common::add_history(&root, &name, &tc.draw(gs::vecs(gs::sampled_from(ids.clone())).min_size(1).max_size(3)));
        recorded.push(name);
    }
    if tc.draw(gs::booleans()) {
        common::export_new_track(&root, "Artist", "Fresh");
    }
    if tc.draw(gs::booleans()) {
        let tracks = locallibrary::query::TrackSelector { ids: vec![ids[0]], ..Default::default() };
        common::apply(
            &root,
            &[
                locallibrary::edit::Edit::CreatePlaylist { path: "Sets/Late".into(), folder: false },
                locallibrary::edit::Edit::AddTracks {
                    playlist: "Sets/Late".into(),
                    tracks,
                    position: None,
                    sort: None,
                    allow_repeats: false,
                },
            ],
        );
    }
    let after = Device::open(&root).unwrap().library().unwrap();

    // The diff, by snapshot id and by library name (Main isn't mounted: its latest snapshot).
    let short = head.to_string()[..8].to_string();
    for left in [format!("snapshot:{short}"), "Main".to_string()] {
        let d =
            api::diff(Some(&store), &DiffRequest { left: left.clone(), right: path(&root), playlist: None }).unwrap();
        // The snapshot holds the library as it was pulled.
        let direct = diff::between(&before, &after, (&d.left, &d.right), None);
        assert_eq!(serde_json::to_value(&d).unwrap(), serde_json::to_value(&direct).unwrap(), "{left}");
        let files = |l: &Library| -> FxHashSet<String> { l.tracks.iter().map(|t| t.file_path.clone()).collect() };
        let added: FxHashSet<String> = d.added_tracks.iter().map(|t| t.file_path.clone()).collect();
        assert_eq!(added, files(&after).difference(&files(&before)).cloned().collect());
        assert!(d.removed_tracks.is_empty());
        let playlists = |l: &Library| -> FxHashSet<String> {
            l.playlists
                .iter()
                .map(|p| if p.is_folder { format!("{}/", l.playlist_path(p.id)) } else { l.playlist_path(p.id) })
                .collect()
        };
        let added: FxHashSet<String> = d.added_playlists.iter().cloned().collect();
        assert_eq!(added, playlists(&after).difference(&playlists(&before)).cloned().collect());
    }
    let same = api::diff(None, &DiffRequest { left: path(&root), right: path(&root), playlist: None }).unwrap();
    assert!(same.is_empty(), "a drive differs from itself");
    let none = DiffRequest { left: path(&root), right: path(&root), playlist: Some("Nowhere".into()) };
    assert_eq!(api::diff(None, &none).unwrap_err().code(), "not_found");
    assert_eq!(
        api::diff(None, &DiffRequest { left: format!("snapshot:{short}"), right: path(&root), playlist: None })
            .unwrap_err()
            .code(),
        "invalid"
    );

    // Play statistics, now and since the snapshot: by its id, or a date after it (one before
    // every snapshot has none).
    let limit = tc.draw(gs::optional(gs::integers::<usize>().max_value(4)));
    let StatsModel { lists, played, never, most } = stats_model(&after, limit.unwrap_or(20));
    let sinces =
        [None, Some(short.clone()), Some(format!("snapshot:{short}")), Some("2999".into()), Some("1999-12".into())];
    for since in sinces {
        let req = PlayStatsRequest { source: path(&root), since: since.clone(), limit };
        if since.is_some() {
            assert_eq!(api::play_stats(None, &req).unwrap_err().reason(), Some(Reason::NoStore));
        }
        let stats = match api::play_stats(Some(&store), &req) {
            Err(e) if since.as_deref() == Some("1999-12") => {
                assert!(e.code() == "not_found" && e.to_string().contains("the oldest is"), "{e}");
                continue;
            }
            r => r.unwrap(),
        };
        assert_eq!((stats.tracks, stats.history_lists, stats.played), (after.tracks.len(), lists, played));
        assert_eq!(stats.never_played.iter().map(|t| t.id).collect::<Vec<_>>(), never);
        assert_eq!(
            stats.most_played.iter().map(|p| (p.track.id, p.play_count, p.history_lists)).collect::<Vec<_>>(),
            most
        );
        assert_eq!(stats.since.is_some(), since.is_some());
        if let Some(s) = &stats.since {
            assert_eq!((s.snapshot, &s.new_histories), (head, &recorded));
            let in_new: FxHashSet<u32> = after
                .history
                .iter()
                .filter(|h| recorded.contains(&h.name))
                .flat_map(|h| h.track_ids.iter().copied())
                .collect();
            let got: FxHashSet<u32> = s.tracks.iter().map(|t| t.track.id).collect();
            assert_eq!(got, in_new);
        }
    }
    // The snapshot since a date after it: itself, with nothing recorded since.
    let req = PlayStatsRequest { source: format!("snapshot:{short}"), since: Some("2999-01-01".into()), limit };
    let since = api::play_stats(Some(&store), &req).unwrap().since.unwrap();
    assert!(since.snapshot == head && since.new_histories.is_empty());
    let other = common::demo_drive(dir.path(), "Other");
    let req = PlayStatsRequest { source: path(&other), since: Some("2999".into()), limit: None };
    assert!(api::play_stats(Some(&store), &req).unwrap_err().to_string().contains("isn't linked"));

    // The archive, by library name, by the drive, and of every library.
    let lib = store.find_library("Main").unwrap().unwrap();
    let archive = serde_json::to_value(plays::archive(&store, Some(lib.id), &Ctx::silent()).unwrap()).unwrap();
    let by_name = api::history_archive(
        &store,
        &HistoryArchiveRequest { library: Some("main".into()), drive: None },
        &Ctx::silent(),
    )
    .unwrap();
    let by_drive = api::history_archive(
        &store,
        &HistoryArchiveRequest { library: None, drive: Some(path(&root)) },
        &Ctx::silent(),
    )
    .unwrap();
    let every = api::history_archive(&store, &HistoryArchiveRequest::default(), &Ctx::silent()).unwrap();
    assert_eq!(serde_json::to_value(&by_name).unwrap(), archive);
    assert_eq!(serde_json::to_value(&by_drive).unwrap(), archive);
    assert_eq!(serde_json::to_value(&every).unwrap(), archive, "one library");
    assert_eq!(by_name.lists.iter().map(|l| l.list.name.as_str()).collect::<Vec<_>>(), ["HISTORY 001"]);
    let nowhere = HistoryArchiveRequest { library: Some("Nowhere".into()), drive: None };
    assert_eq!(api::history_archive(&store, &nowhere, &Ctx::silent()).unwrap_err().code(), "not_found");
    let history = api::drive_history(&DriveRef { drive: path(&root) }).unwrap();
    let names: Vec<&str> = history.lists.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["HISTORY 001"].into_iter().chain(recorded.iter().map(String::as_str)).collect::<Vec<_>>());

    // rekordbox imports the drive's history, deleting it: the store's archive still has it.
    let pdb = root.join(locallibrary::paths::EXPORT_PDB);
    let all: Vec<u32> = after.history.iter().map(|h| h.id).collect();
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
    ed.remove_history_playlists(&all).unwrap();
    ed.save(&pdb).unwrap();
    let out = dir.path().join("HISTORY 001.m3u8");
    let req = ExportPlaylist {
        drive: path(&root),
        playlist: None,
        history: Some("HISTORY 001".into()),
        archived: true,
        format: None,
        output: out.clone(),
        paths: PathStyle::default(),
        split: false,
        replace: false,
    };
    assert_eq!(api::export_playlist(None, &req, &Ctx::silent()).unwrap_err().code(), "invalid");
    let unarchived = ExportPlaylist { archived: false, ..req.clone() };
    assert_eq!(api::export_playlist(Some(&store), &unarchived, &Ctx::silent()).unwrap_err().code(), "not_found");
    let report = api::export_playlist(Some(&store), &req, &Ctx::silent()).unwrap();
    assert_eq!((report.files.len(), report.files[0].tracks), (1, 2));
    assert!(out.is_file());
}

// ---------------------------------------------------------------------------------------------
// Outputs
// ---------------------------------------------------------------------------------------------

/// Exporting a playlist, a folder (whole or split), a history list or the whole library writes
/// one file per selection holding as many tracks as it has, in the format asked for or its
/// file's; files already there are refused unless replacing, and left as they were. Exports for
/// other DJ software write nothing when planned and what the plan says when applied, refused
/// over files already there unless replacing. A doctor report is `doctor`'s, and cleaning
/// orphans deletes exactly the files planted where nothing uses them.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn outputs_write_what_they_say(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_history(&root, "HISTORY 001", &[1, 2, 3]);
    let tracks = locallibrary::query::TrackSelector { ids: vec![2, 4], ..Default::default() };
    common::apply(
        &root,
        &[
            locallibrary::edit::Edit::CreatePlaylist { path: "Sets/Late".into(), folder: false },
            locallibrary::edit::Edit::AddTracks {
                playlist: "Sets/Late".into(),
                tracks: tracks.clone(),
                position: None,
                sort: None,
                allow_repeats: false,
            },
            locallibrary::edit::Edit::CreatePlaylist { path: "Sets/Early".into(), folder: false },
            locallibrary::edit::Edit::AddTracks {
                playlist: "Sets/Early".into(),
                tracks,
                position: None,
                sort: None,
                allow_repeats: false,
            },
        ],
    );
    let lib = Device::open(&root).unwrap().library().unwrap();
    let out = dir.path().join("out");
    fs::create_dir(&out).unwrap();

    // Playlists, history lists, the library.
    let first = lib.playlists.iter().find(|p| !p.is_folder && p.parent_id.is_none()).unwrap();
    let late = lib.find_playlist("Sets/Late").unwrap();
    // Each selection in turn, in a format asked for or taken from its file name.
    for choice in 0..5u8 {
        let (playlist, history, split, counts): (Option<String>, Option<String>, bool, Vec<usize>) = match choice {
            0 => (None, None, false, vec![lib.tracks.len()]),
            1 => (Some(lib.playlist_path(first.id)), None, false, vec![first.track_ids.len()]),
            // A folder: one file per playlist, or one with each of its tracks once.
            2 => (Some("Sets".into()), None, true, vec![late.track_ids.len(), late.track_ids.len()]),
            3 => (Some("Sets".into()), None, false, vec![late.track_ids.len()]),
            _ => (None, Some("history 001".into()), false, vec![3]),
        };
        let format =
            tc.draw(gs::optional(gs::sampled_from(vec![Format::M3u8, Format::Csv, Format::Txt]).print_as_debug()));
        let ext = tc.draw(gs::sampled_from(vec!["m3u8", "csv", "txt", "list"]));
        let output = if split { out.join(format!("split {choice}")) } else { out.join(format!("set {choice}.{ext}")) };
        let req = ExportPlaylist {
            drive: path(&root),
            playlist,
            history,
            archived: false,
            format,
            output: output.clone(),
            paths: tc.draw(
                gs::sampled_from(vec![PathStyle::Relative, PathStyle::Drive, PathStyle::Absolute]).print_as_debug(),
            ),
            split,
            replace: false,
        };
        let report = api::export_playlist(None, &req, &Ctx::silent()).unwrap();
        let expected_format = format.unwrap_or(match (split, ext) {
            (false, "csv") => Format::Csv,
            (false, "txt") => Format::Txt,
            _ => Format::M3u8,
        });
        assert_eq!(report.format, expected_format);
        let mut tracks: Vec<usize> = report.files.iter().map(|f| f.tracks).collect();
        let mut want = counts.clone();
        tracks.sort_unstable();
        want.sort_unstable();
        assert_eq!(tracks, want, "{req:?}");
        for f in &report.files {
            assert!(f.path.is_file() && (split || f.path == output), "{:?}", f.path);
        }
        let written: Vec<Vec<u8>> = report.files.iter().map(|f| fs::read(&f.path).unwrap()).collect();
        let again = api::export_playlist(None, &req, &Ctx::silent()).unwrap_err();
        assert_eq!(again.code(), "conflict", "{again}");
        assert_eq!(
            report.files.iter().map(|f| fs::read(&f.path).unwrap()).collect::<Vec<_>>(),
            written,
            "left as it was"
        );
        assert!(api::export_playlist(None, &ExportPlaylist { replace: true, ..req.clone() }, &Ctx::silent()).is_ok());
        let missing =
            ExportPlaylist { history: Some("HISTORY 404".into()), playlist: None, split: false, ..req.clone() };
        assert_eq!(api::export_playlist(None, &missing, &Ctx::silent()).unwrap_err().code(), "not_found");
    }

    // For other DJ software.
    let software =
        tc.draw(gs::sampled_from(vec![Software::Traktor, Software::Serato, Software::Engine]).print_as_debug());
    let target = out.join("dj");
    let req =
        InteropRequest { drive: path(&root), software, output: target.clone(), options: InteropOptions::default() };
    let plan = api::interop_plan(&req, &Ctx::silent()).unwrap();
    assert!(!target.exists(), "planning wrote");
    assert!(!plan.destructive && plan.plan.tracks == lib.tracks.len());
    let done = api::interop_apply(&req, &Ctx::silent()).unwrap();
    for f in &plan.plan.files {
        assert!(f.path.is_file(), "{:?} planned but not written", f.path);
    }
    assert_eq!(done.files.len(), plan.plan.files.len());
    let replan = api::interop_plan(&req, &Ctx::silent()).unwrap();
    assert!(replan.destructive && replan.plan.files.iter().all(|f| f.exists));
    let replace = InteropRequest { options: InteropOptions { replace: true, ..Default::default() }, ..req.clone() };
    assert!(api::interop_apply(&replace, &Ctx::silent()).is_ok());

    // Doctor, and orphans planted where nothing uses them.
    let report =
        api::doctor(&DoctorRequest { drive: path(&root), options: DoctorOptions::default() }, &Ctx::silent()).unwrap();
    let direct = doctor::run(&Device::open(&root).unwrap(), &DoctorOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(serde_json::to_value(&report).unwrap(), serde_json::to_value(&direct).unwrap());
    let planted: Vec<String> =
        (0..tc.draw(gs::integers::<usize>().max_value(2))).map(|i| format!("Contents/Stray/{i}.mp3")).collect();
    for p in &planted {
        let f = root.join(p);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(&f, b"ID3 stray").unwrap();
    }
    let r = DriveRef { drive: path(&root) };
    let plan = api::doctor_orphans_plan(&r, &Ctx::silent()).unwrap();
    let mut found: Vec<String> = plan.plan.files.iter().map(|f| f.path.clone()).collect();
    found.sort();
    assert_eq!(found, planted);
    assert_eq!((plan.noop, plan.destructive), (planted.is_empty(), !planted.is_empty()));
    let before = Before::take(&root);
    let cleaned = api::doctor_orphans_apply(&r, &Ctx::silent());
    if planted.is_empty() {
        before.untouched(&root).unwrap();
    }
    cleaned.unwrap();
    for p in &planted {
        assert!(!root.join(p).exists(), "{p} left");
    }
}

/// Checking drives before a set is `check`'s report for the same drives; no drives, or a drive
/// named twice, is refused.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn checks_agree_with_check(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let a = common::demo_drive(dir.path(), "A");
    let b = common::demo_drive(dir.path(), "B");
    let players: Vec<String> = tc
        .draw(gs::vecs(gs::sampled_from(vec!["CDJ-3000", "CDJ-2000NXS2"])).max_size(1))
        .iter()
        .map(|p| p.to_string())
        .collect();
    let options = CheckOptions { players, audio: false };
    let drives = if tc.draw(gs::booleans()) { vec![a.clone(), b.clone()] } else { vec![a.clone()] };
    let req = CheckRequest { drives: drives.iter().map(|d| path(d)).collect(), options: options.clone() };
    let report = api::check(None, &req, &Ctx::silent()).unwrap();
    let devices: Vec<Device> = drives.iter().map(|d| Device::open(d).unwrap()).collect();
    let direct = locallibrary::check::check(&devices, None, &options, &Ctx::silent()).unwrap();
    let strip = |v: serde_json::Value| v.to_string();
    assert_eq!(strip(serde_json::to_value(&report).unwrap()), strip(serde_json::to_value(&direct).unwrap()));
    let twice = CheckRequest { drives: vec![path(&a), path(&a)], options: options.clone() };
    assert!(api::check(None, &twice, &Ctx::silent()).unwrap_err().to_string().contains("listed twice"));
    assert_eq!(
        api::check(None, &CheckRequest { drives: Vec::new(), options }, &Ctx::silent()).unwrap_err().code(),
        "invalid"
    );
}

/// A recording identified against a drive (whose placeholder audio plays none of it) is kept
/// when asked, once per save, listed newest first, read back with its id, and written as a page
/// whose size is the page's; as a data document it's written once unless replacing. Fetching a
/// report that isn't kept is refused.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn mix_reports_are_kept_once_per_save(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    common::isolate(dir.path(), None);
    let main = common::demo_drive(&dir.path().join("volumes"), "Main");
    let ctx = Ctx::silent();
    let index = api::mix_index(&store, &api::IndexMixLibrary { drive: None }, &ctx).unwrap();
    assert_eq!((index.tracks, index.indexed, index.failed.len()), (2, 0, 2), "placeholder audio doesn't decode");
    let cache = api::LibraryCache::new(1);
    let mut saved = Vec::new();
    for n in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(2)) {
        let mix = dir.path().join(format!("Set {n}.wav"));
        let channels = tc.draw(gs::integers::<u16>().min_value(1).max_value(2));
        fs::write(&mix, common::media::wav_of(44_100, 16, channels, 0.5)).unwrap();
        let identify = api::IdentifyMix { mix: mix.clone(), drive: Some(path(&main)), tempo_range: None };
        // The first is kept, so there's always one to read back.
        let save = n == 0 || tc.draw(gs::booleans());
        let req = api::IdentifyMixRequest { identify: identify.clone(), extras: tc.draw(gs::booleans()), save };
        let found = api::mix_identify_cached(&cache, &store, &req, &ctx).unwrap();
        assert!(found.report.plays.is_empty() && found.extras.is_empty());
        assert_eq!(found.audio.as_deref(), Some(mix.as_path()));
        assert_eq!(found.saved.is_some(), save);
        saved.extend(found.saved);
        let out = dir.path().join(format!("set {n}.json"));
        let export = |replace| api::ExportMix { identify: identify.clone(), output: Some(out.clone()), replace };
        let doc = api::mix_export_cached(&cache, &store, &export(false), &ctx).unwrap();
        let written: serde_json::Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
        assert_eq!(written["format"], doc.format);
        assert!(api::mix_export_cached(&cache, &store, &export(false), &ctx).is_err(), "replaced unasked");
        assert!(api::mix_export_cached(&cache, &store, &export(true), &ctx).is_ok());
    }
    let kept = api::mix_reports_list(store.root()).unwrap();
    let mut listed: Vec<String> = kept.iter().map(|r| r.id.clone()).collect();
    listed.sort();
    saved.sort();
    assert_eq!(listed, saved);
    for id in &saved {
        let got = api::mix_report_get(store.root(), &api::ReportRef { id: id.clone(), extras: true }).unwrap();
        assert_eq!(got.saved.as_deref(), Some(id.as_str()));
        let page =
            api::mix_report_html(&store, &api::ReportHtml { id: id.clone(), inline: tc.draw(gs::booleans()) }).unwrap();
        assert_eq!(fs::metadata(&page.path).unwrap().len(), page.bytes);
        assert!(page.html.is_none_or(|h| h.len() as u64 == page.bytes));
    }
    assert!(api::mix_report_get(store.root(), &api::ReportRef { id: "nothing".into(), extras: false }).is_err());
    assert!(api::mix_drive(Some(&path(&main))).is_ok() && api::mix_drive(None).is_ok());
}

// ---------------------------------------------------------------------------------------------
// The sidecar's messages
// ---------------------------------------------------------------------------------------------

/// A JSON id as a client might send one.
fn id_value(tc: &TestCase) -> serde_json::Value {
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => serde_json::Value::Null,
        1 => serde_json::json!(tc.draw(gs::integers::<i64>())),
        2 => serde_json::json!(tc.draw(gs::text().max_size(8))),
        _ => serde_json::json!(tc.draw(gs::floats::<f64>().min_value(-1e9).max_value(1e9))),
    }
}

fn round_trip<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> serde_json::Value {
    let json = serde_json::to_value(value).unwrap();
    let back: T = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&back).unwrap(), json);
    json
}

/// The sidecar's requests and results read back as they were written, ids passed through as
/// they came, and old names (`sync_set`) and missing fields read as documented.
#[hegel::test(test_cases = 200)]
fn sidecar_messages_round_trip(tc: TestCase) {
    let id = id_value(&tc);
    let text = || tc.draw(gs::text().max_size(10));
    let flag = || tc.draw(gs::booleans());
    let ms = || tc.draw(gs::optional(gs::integers::<u64>().max_value(100_000)));
    let request_id = match &id {
        serde_json::Value::String(s) => Some(api::RequestId::String(s.clone())),
        serde_json::Value::Number(n) => Some(api::RequestId::Number(n.clone())),
        _ => None,
    };
    if let Some(r) = &request_id {
        assert_eq!(round_trip(r), id);
    }
    assert_eq!(round_trip(&api::CancelRequest { id: id.clone() })["id"], id);
    round_trip(&api::Cancelled { cancelled: flag() });
    round_trip(&api::ApplyPlan { plan_id: text() });
    round_trip(&api::WriteOptions { overwrite_archive: flag() });
    round_trip(&api::Initialized {
        protocol: tc.draw(gs::integers::<u32>()),
        version: text(),
        capabilities: vec![text()],
        methods: vec![text(), text()],
    });
    round_trip(&api::VersionInfo { version: text(), methods: vec![text()] });
    let started =
        chrono::DateTime::from_timestamp(tc.draw(gs::integers::<i64>().min_value(0).max_value(4_000_000_000)), 0)
            .unwrap();
    let job = round_trip(&api::JobInfo { id: id.clone(), method: text(), started, queued: flag() });
    assert_eq!(job.get("queued").is_some(), job["queued"] == true, "queued only when it is");
    round_trip(&api::Subscribe { kinds: vec![text()], interval_ms: ms() });
    round_trip(&api::Subscribed { kinds: vec![text()], drives: Vec::new() });
    round_trip(&api::Unsubscribed { unsubscribed: flag() });
    let watch = api::StartWatch { pull: flag(), sync_group: flag(), settle_ms: ms(), interval_ms: ms() };
    round_trip(&watch);
    let old: api::StartWatch = serde_json::from_value(serde_json::json!({ "sync_set": watch.sync_group })).unwrap();
    assert_eq!((old.sync_group, old.pull, old.settle_ms), (watch.sync_group, false, None));
    round_trip(&api::WatchStarted { watching: flag() });
    round_trip(&api::StopWatch { cancel: flag() });
    round_trip(&api::WatchStopped { stopped: flag() });
    round_trip(&api::Shutdown { cancel: flag() });
    round_trip(&api::GcRequest { dry_run: flag() });
    round_trip(&api::DescribeRequest { method: tc.draw(gs::optional(gs::text().max_size(6))) });
    let data = api::ErrorData { kind: text(), reason: None, fields: serde_json::Map::new(), actions: Vec::new() };
    round_trip(&data);
    let _: api::NoParams = serde_json::from_value(serde_json::json!({ "anything": id })).unwrap();
    let finished = api::JobEvent::Finished {
        id: id.clone(),
        method: text(),
        ok: flag(),
        kind: tc.draw(gs::optional(gs::text().max_size(6))),
    };
    let json = serde_json::to_value(&finished).unwrap();
    assert_eq!((json["event"].as_str(), &json["id"]), (Some("job.finished"), &id));
    let api::JobEvent::Finished { kind, .. } = &finished else { unreachable!() };
    assert_eq!(json.get("kind").is_some(), kind.is_some());
    let started_event =
        serde_json::to_value(api::JobEvent::Started { id: id.clone(), method: text(), started }).unwrap();
    assert_eq!(started_event["event"], "job.started");
    let step = api::ProgressEvent {
        event: api::Progressed::Step(api::StepStarted::default()),
        scope: Default::default(),
        items: None,
    };
    let step = serde_json::to_value(api::ProgressParams { id: id.clone(), event: step }).unwrap();
    assert_eq!((&step["id"], &step["event"]["event"]), (&id, &serde_json::json!("step")));
}

/// Keys a stored key and its annotation can be, as Camelot codes and GiantSteps names.
const KEY_NAMES: &[(&str, &str)] = &[("8A", "A minor"), ("8B", "C major"), ("9A", "E minor"), ("11B", "A major")];

/// MIREX's credit for `stored` against the annotated `truth`: 1 for the key, 0.5 a fifth away in
/// its mode, 0.3 its relative, 0.2 its parallel, else (and for no key) nothing.
fn credit(truth: &str, stored: &str) -> f64 {
    let (Some(t), Some(s)) =
        (locallibrary::harmony::Camelot::parse(truth), locallibrary::harmony::Camelot::parse(stored))
    else {
        return 0.0;
    };
    let steps = (i32::from(s.number) - i32::from(t.number)).rem_euclid(12);
    match () {
        _ if s == t => 1.0,
        _ if s.major == t.major && (steps == 1 || steps == 11) => 0.5,
        _ if s == t.relative() => 0.3,
        _ if s == t.parallel() => 0.2,
        _ => 0.0,
    }
}

/// Scoring rekordbox's stored keys against annotations gives each track MIREX's credit, the
/// score their mean, over the annotated tracks on the drive; annotations of tracks the drive
/// lacks are counted unmatched.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn stored_keys_are_scored_as_mirex_scores_them(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    common::isolate(dir.path(), None);
    let root = common::blank(dir.path(), "GIANTSTEPS");
    fs::create_dir_all(root.join("PIONEER/rekordbox")).unwrap();
    fs::copy(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../vendor/rekordbox-pdb/tests/data/bigger-export.pdb"),
        root.join(locallibrary::paths::EXPORT_PDB),
    )
    .unwrap();
    let keys = dir.path().join("keys");
    fs::create_dir_all(&keys).unwrap();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let mut expected = Vec::new();
    for i in 0..n {
        let id = format!("{}", 1001 + i);
        let stored = tc.draw(gs::sampled_from(KEY_NAMES.iter().map(|k| k.0).chain([""]).collect::<Vec<_>>()));
        let (truth, name) = tc.draw(gs::sampled_from(KEY_NAMES.to_vec()));
        let path = format!("/Contents/UnknownArtist/UnknownAlbum/{id}.LOFI.mp3");
        let track = rekordbox_pdb::NewTrack {
            key: Some(stored.into()),
            ..rekordbox_pdb::NewTrack::new(id.clone(), path.as_str())
        };
        common::add_track(&root, track, id.as_bytes());
        fs::write(keys.join(format!("{id}.LOFI.key")), name).unwrap();
        expected.push(credit(truth, stored));
    }
    let strays = tc.draw(gs::integers::<usize>().max_value(2));
    for i in 0..strays {
        fs::write(keys.join(format!("{}.LOFI.key", 9000 + i)), "D major").unwrap();
    }
    let options = locallibrary::sound::bench::BenchOptions { keys: Some(keys), tempos: None, audio: None };
    let req = api::AnalysisBenchRequest { drive: Some(path(&root)), options };
    let r = api::analysis_bench(&req, &Ctx::silent()).unwrap();
    let rb = &r.keys[locallibrary::sound::bench::REKORDBOX];
    assert_eq!((rb.tracks, r.unmatched), (n, strays));
    let mean = expected.iter().sum::<f64>() / n as f64;
    assert!((rb.score - mean).abs() < 1e-9, "{} for {expected:?}", rb.score);
}
