mod api;
mod destructive;
mod details;
mod drifted_databases;
mod journal_replay;
mod mixed;
mod refusals;
mod set_path;

use crate::common;

use std::fs;
use std::path::Path;

use crate::common::{apply, replay};
use locallibrary::device::{Device, LineageOp};
use locallibrary::edit::{self, Edit, EditOptions, TrackFields};
use locallibrary::model::TrackColor;
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::{Library, backup, paths, pdb};

fn pdb_library(root: &Path) -> Library {
    pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap()
}

fn ids(sel: &[u32]) -> TrackSelector {
    TrackSelector { ids: sel.to_vec(), ..Default::default() }
}

/// Track file paths of a playlist, in order.
fn files(lib: &Library, playlist: &str) -> Vec<String> {
    let p = lib.find_playlist(playlist).unwrap_or_else(|| panic!("no playlist {playlist}"));
    p.track_ids.iter().map(|id| lib.track(*id).unwrap().file_path.clone()).collect()
}

/// A drive with both databases holding the bigger fixture's twelve tracks.
fn both_databases(dir: &Path) -> std::path::PathBuf {
    let root = common::bigger_drive(dir, "Main");
    common::add_onelibrary(&root);
    root
}

#[test]
fn playlist_edits_land_in_both_databases_and_can_be_undone() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    let before = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    let before_lib = pdb_library(&root);
    let first: Vec<u32> = before_lib.tracks.iter().take(4).map(|t| t.id).collect();

    let plan = apply(
        &root,
        &[
            Edit::CreatePlaylist { path: "Shows/2026/Friday".into(), folder: false },
            Edit::AddTracks {
                playlist: "Shows/2026/Friday".into(),
                tracks: ids(&first),
                position: None,
                sort: Some("-title".into()),
                allow_repeats: false,
            },
            Edit::CreatePlaylist { path: "Shows/Warmup".into(), folder: false },
            Edit::MovePlaylist { playlist: "Shows/Warmup".into(), into: Some("Shows".into()), position: Some(1) },
            Edit::RenamePlaylist { playlist: "Shows/2026/Friday".into(), name: "Friday, 10 October".into() },
            Edit::MoveTracks { playlist: "Shows/2026/Friday, 10 October".into(), positions: vec![4], to: 1 },
        ],
    );
    assert_eq!(plan.databases, ["export.pdb", "exportLibrary.db"]);
    assert_eq!(plan.changes.len(), 6);

    common::sqlcipher_reads(&root);
    for lib in [pdb_library(&root), common::plus_library(&root)] {
        let shows = lib.find_playlist("Shows").unwrap();
        assert!(shows.is_folder);
        let kids: Vec<&str> = lib.children(Some(shows.id)).iter().map(|p| p.name.as_str()).collect();
        assert_eq!(kids, ["Warmup", "2026"]);
        let mut want: Vec<&locallibrary::Track> = first.iter().map(|id| before_lib.track(*id).unwrap()).collect();
        want.sort_by_key(|t| std::cmp::Reverse(locallibrary::model::normalize(&t.title)));
        let mut want: Vec<String> = want.iter().map(|t| t.file_path.clone()).collect();
        let last = want.pop().unwrap();
        want.insert(0, last);
        assert_eq!(files(&lib, "Shows/2026/Friday, 10 October"), want);
    }
    let pdb_bytes = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    let tree_rows = common::rekordcrate_rows(&pdb_bytes, rekordcrate::pdb::PageType::PlaylistTree);
    assert_eq!(tree_rows, pdb_library(&root).playlists.len());

    let d = Device::open_export(&root).unwrap();
    assert_eq!(d.state().unwrap().lineage.last().unwrap().op, LineageOp::Edit);
    assert_eq!(edit::journal::list(&d).unwrap().len(), 1);
    let mut d = Device::open_export(&root).unwrap();
    backup::restore(&mut d, None, false).unwrap();
    assert_eq!(fs::read(root.join(paths::EXPORT_PDB)).unwrap(), before);
    assert!(common::plus_library(&root).find_playlist("Shows").is_none());
}

#[test]
fn deleting_a_folder_removes_everything_below_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    apply(
        &root,
        &[
            Edit::CreatePlaylist { path: "Old/Sets/A".into(), folder: false },
            Edit::AddTracks {
                playlist: "Old/Sets/A".into(),
                tracks: TrackSelector { filter: Some("bpm:>0".into()), ..Default::default() },
                position: None,
                sort: None,
                allow_repeats: false,
            },
            Edit::CreatePlaylist { path: "Old/B".into(), folder: false },
        ],
    );
    let tracks = pdb_library(&root).tracks.len();
    apply(&root, &[Edit::DeletePlaylist { playlist: "Old".into(), with_tracks: false }]);
    for lib in [pdb_library(&root), common::plus_library(&root)] {
        assert!(lib.playlists.iter().all(|p| !["Old", "Sets", "A", "B"].contains(&p.name.as_str())));
        assert_eq!(lib.tracks.len(), tracks);
    }
    let bytes = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    let entries = common::rekordcrate_rows(&bytes, rekordcrate::pdb::PageType::PlaylistEntries);
    assert_eq!(entries, pdb_library(&root).playlists.iter().map(|p| p.track_ids.len()).sum::<usize>());
}

#[test]
fn track_details_change_in_both_databases() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    let lib = pdb_library(&root);
    let t = lib.tracks[2].clone();
    let old_genre = t.genre.clone();
    apply(
        &root,
        &[Edit::EditTracks {
            tracks: ids(&[t.id]),
            fields: TrackFields {
                title: Some("A Brand New Title That Is Rather Long Indeed".into()),
                artist: Some("Somebody Else".into()),
                genre: Some("Hard Techno".into()),
                key: Some("8A".into()),
                comment: Some("peak time".into()),
                rating: Some(5),
                color: Some("aqua".into()),
                year: Some(1999),
                ..Default::default()
            },
        }],
    );
    common::sqlcipher_reads(&root);
    for lib in [pdb_library(&root), common::plus_library(&root)] {
        let n = lib.tracks.iter().find(|x| x.file_path == t.file_path).unwrap();
        assert_eq!(n.title, "A Brand New Title That Is Rather Long Indeed");
        assert_eq!(n.artist, "Somebody Else");
        assert_eq!((n.genre.as_str(), n.key.as_str(), n.comment.as_str()), ("Hard Techno", "8A", "peak time"));
        assert_eq!((n.rating, n.color, n.year), (5, Some(TrackColor::Aqua), 1999));
        assert_eq!(
            (n.bpm, n.duration_secs, n.analyze_path.as_str()),
            (t.bpm, t.duration_secs, t.analyze_path.as_str())
        );
        if !old_genre.is_empty() && !lib.tracks.iter().any(|x| x.genre == old_genre) {
            let db = rekordbox_pdb::Database::from_file(root.join(paths::EXPORT_PDB)).unwrap();
            assert!(db.genres.iter().all(|g| g.name != old_genre), "an unused genre is removed");
        }
    }
    let bytes = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    assert_eq!(common::rekordcrate_rows(&bytes, rekordcrate::pdb::PageType::Tracks), lib.tracks.len());
}

#[test]
fn deleting_tracks_removes_files_nobody_else_uses() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    let lib = pdb_library(&root);
    let t = lib.tracks[0].clone();
    let audio = paths::to_fs(&root, &paths::key(&t.file_path));
    assert!(audio.exists());
    let plan = apply(&root, &[Edit::DeleteTracks { tracks: ids(&[t.id]), keep_files: false }]);
    assert_eq!(plan.tracks_removed.len(), 1);
    assert!(plan.files.iter().any(|f| f.eq_ignore_ascii_case(&paths::key(&t.file_path))));
    assert!(!audio.exists());
    common::sqlcipher_reads(&root);
    for lib in [pdb_library(&root), common::plus_library(&root)] {
        assert_eq!(lib.tracks.len(), 11);
        assert!(lib.playlists.iter().all(|p| p.track_ids.iter().all(|id| lib.track(*id).is_some())));
    }
    // Undo brings the file back from the backup.
    let mut d = Device::open_export(&root).unwrap();
    backup::restore(&mut d, None, false).unwrap();
    assert!(audio.exists());
    assert_eq!(pdb_library(&root).tracks.len(), 12);
}

#[test]
fn replacing_a_duplicate_moves_its_entries_to_the_keeper() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    let lib = pdb_library(&root);
    let (dup, keep) = (lib.tracks[1].clone(), lib.tracks[5].clone());
    apply(
        &root,
        &[
            Edit::CreatePlaylist { path: "Set".into(), folder: false },
            Edit::AddTracks {
                playlist: "Set".into(),
                tracks: ids(&[lib.tracks[0].id, dup.id, lib.tracks[2].id]),
                position: None,
                sort: None,
                allow_repeats: false,
            },
        ],
    );
    apply(
        &root,
        &[Edit::ReplaceTrack {
            duplicate: TrackSelector::paths([dup.file_path.clone()]),
            keep: TrackSelector::paths([keep.file_path.clone()]),
            keep_duplicate: false,
        }],
    );
    common::sqlcipher_reads(&root);
    for lib in [pdb_library(&root), common::plus_library(&root)] {
        let set = files(&lib, "Set");
        assert_eq!(set.len(), 3);
        assert_eq!(set[1], keep.file_path);
        assert!(lib.tracks.iter().all(|t| t.file_path != dup.file_path));
    }
}

#[test]
fn history_lists_can_be_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_history(&root, "HISTORY 001", &[1, 2]);
    common::add_history(&root, "HISTORY 002", &[3]);
    common::add_onelibrary(&root);
    apply(&root, &[Edit::DeleteHistory { names: vec!["history 001".into()], all: false }]);
    for lib in [pdb_library(&root), common::plus_library(&root)] {
        assert_eq!(lib.history.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(), ["HISTORY 002"]);
    }
    apply(&root, &[Edit::DeleteHistory { names: vec![], all: true }]);
    assert!(pdb_library(&root).history.is_empty());
    let db = rekordbox_pdb::Database::from_file(root.join(paths::EXPORT_PDB)).unwrap();
    assert!(db.history_entries.is_empty());
}

#[test]
fn recorded_edits_replay_after_a_new_export() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    let pdb_before = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    let plus_before = fs::read(root.join(paths::LIBRARY_PLUS_DB)).unwrap();
    let lib = pdb_library(&root);
    let rated = lib.tracks[3].file_path.clone();
    apply(
        &root,
        &[
            Edit::EditTracks {
                tracks: ids(&[lib.tracks[3].id]),
                fields: TrackFields { rating: Some(5), ..Default::default() },
            },
            Edit::SetTracks {
                playlist: "Rules/Five stars".into(),
                tracks: TrackSelector { filter: Some("rating:5".into()), ..Default::default() },
                sort: None,
                limit: None,
                create: true,
            },
            Edit::CreatePlaylist { path: "Scratch".into(), folder: false },
        ],
    );
    apply(&root, &[Edit::DeletePlaylist { playlist: "Scratch".into(), with_tracks: false }]);
    // rekordbox exports again: the edits are gone from the databases.
    fs::write(root.join(paths::EXPORT_PDB), &pdb_before).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), &plus_before).unwrap();
    assert!(pdb_library(&root).find_playlist("Rules").is_none());

    let (edits, _) = replay(&root);
    assert_eq!(edits.len(), 4);
    for lib in [pdb_library(&root), common::plus_library(&root)] {
        assert!(lib.find_playlist("Scratch").is_none());
        assert_eq!(lib.tracks.iter().find(|t| t.file_path == rated).unwrap().rating, 5);
        // The tracks the rule picked, after the rating made before it.
        assert!(files(&lib, "Rules/Five stars").contains(&rated));
    }
    let d = Device::open_export(&root).unwrap();
    assert_eq!(edit::journal::list(&d).unwrap().len(), 2, "a replay isn't recorded again");
}

/// A replay with no export since changes nothing (the drive has every recorded edit still), so
/// its report counts no changes and names no database written.
#[test]
fn a_replay_of_edits_the_drive_still_has_writes_nothing() {
    use locallibrary::cue_edit::{self, CueEdit, CuePosition, HotCue, SetCue};
    use locallibrary::tags;
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    let first = d.library().unwrap().tracks[0].id;
    let tag = tags::Edit::Add { tag: "Genre/Deep".into(), tracks: vec![first] };
    tags::apply(&tags::plan(&d, &tag).unwrap(), &mut d, &Ctx::silent()).unwrap();
    let set = CueEdit::Set(SetCue {
        hot_cue: Some(HotCue(2)),
        at: CuePosition::Beat(17),
        snap: false,
        loop_beats: None,
        name: None,
        color: None,
    });
    cue_edit::apply(&cue_edit::plan(&d, &[first], &set, &Ctx::silent()).unwrap(), &mut d, &Ctx::silent()).unwrap();
    let before = common::drive_files(&root);

    let edits = edit::journal::edits(&d).unwrap();
    assert_eq!(edits.len(), 2);
    let plan = edit::plan(&d, &edits, &EditOptions { lenient: true, no_journal: true }, &Ctx::silent()).unwrap();
    // The My Tag edit is made as the replay is applied, in exportExt.pdb, which the plan names.
    assert!(plan.databases.iter().any(|db| db == "exportExt.pdb"), "{:?}", plan.databases);
    let report = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert_eq!((report.changes, report.databases.len()), (0, 0), "{report:?}");
    let after = common::drive_files(&root);
    let databases = |files: &common::Files| {
        files
            .iter()
            .filter(|(k, _)| k.starts_with("PIONEER/rekordbox/"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(databases(&after), databases(&before), "nothing written");
}

/// `tags` and `cues` edits are recorded too, and a replay after a new export makes them again
/// with the playlist edits, under one backup that undoes the whole replay.
#[test]
fn a_replay_redoes_my_tag_and_cue_edits_under_one_backup() {
    use locallibrary::cue_edit::{self, CueEdit, CuePosition, HotCue, SetCue};
    use locallibrary::tags;
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let mut d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let (first, second) = (lib.tracks[0].id, lib.tracks[1].id);
    let mut exported: Vec<(String, Vec<u8>)> = Vec::new();
    for key in [paths::EXPORT_PDB, paths::EXPORT_EXT_PDB, paths::LIBRARY_PLUS_DB].map(String::from).into_iter().chain(
        lib.tracks.iter().flat_map(|t| {
            let dat = paths::key(&t.analyze_path);
            [dat.replace(".DAT", ".EXT"), dat]
        }),
    ) {
        exported.push((key.clone(), fs::read(paths::to_fs(&root, &key)).unwrap()));
    }

    let tag = tags::Edit::Add { tag: "Genre/Deep".into(), tracks: vec![first, second] };
    tags::apply(&tags::plan(&d, &tag).unwrap(), &mut d, &Ctx::silent()).unwrap();
    let set = CueEdit::Set(SetCue {
        hot_cue: Some(HotCue(2)),
        at: CuePosition::Beat(17),
        snap: false,
        loop_beats: Some(4.0),
        name: Some("Drop".into()),
        color: Some("red".into()),
    });
    cue_edit::apply(&cue_edit::plan(&d, &[first], &set, &Ctx::silent()).unwrap(), &mut d, &Ctx::silent()).unwrap();
    apply(
        &root,
        &[Edit::SetEntries {
            playlist: "Shows/Set".into(),
            paths: vec![lib.tracks[1].file_path.clone()],
            create: true,
        }],
    );
    let journal = edit::journal::list(&d).unwrap();
    assert_eq!(journal.len(), 3);
    assert_eq!(journal[0].changes, ["add My Tag Genre/Deep to 2 track(s)"]);
    assert_eq!(journal[1].changes, ["set hot cue C on 1 track(s)"]);
    let state = |d: &Device| {
        let tagged: Vec<usize> =
            tags::read(d).unwrap().categories.iter().flat_map(|c| &c.tags).map(|t| t.tracks.len()).collect();
        let cues = cue_edit::list(d, &[first]).unwrap().remove(0);
        (
            tagged,
            format!("{:?} {:?}", cues.cues, cues.onelibrary_cues),
            d.library().unwrap().find_playlist("Shows/Set").map(|p| p.track_ids.len()),
        )
    };
    let edited = state(&d);
    common::sqlcipher_reads(&root);
    assert_eq!(edited.2, Some(1));

    // rekordbox exports again.
    for (key, bytes) in &exported {
        fs::write(paths::to_fs(&root, key), bytes).unwrap();
    }
    let undone = common::drive_files(&root);
    let backups = backup::list(&d).unwrap().len();
    let (edits, _) = replay(&root);
    assert_eq!(edits.len(), 3);
    let d = Device::open_export(&root).unwrap();
    assert_eq!(state(&d), edited);
    assert_eq!(backup::list(&d).unwrap().len(), backups + 1, "one backup for the replay");
    assert_eq!(edit::journal::list(&d).unwrap().len(), 3, "a replay isn't recorded again");

    let mut d = Device::open_export(&root).unwrap();
    backup::restore(&mut d, None, false).unwrap();
    assert_eq!(common::changed_files(&undone, &common::drive_files(&root)), Vec::<String>::new());
}

/// Regression: restoring a backup (undo) left the edits it undid in the journal, so `edits
/// replay` made them again. A restore takes them off: the edit the backup was made for, and
/// every one recorded since.
#[test]
fn undoing_edits_takes_them_off_the_journal() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    for name in ["One", "Two", "Three"] {
        apply(&root, &[Edit::CreatePlaylist { path: name.into(), folder: false }]);
    }
    let mut d = Device::open_export(&root).unwrap();
    let journal = edit::journal::list(&d).unwrap();
    let backups: Vec<&str> = journal.iter().map(|e| e.backup.as_deref().unwrap()).collect();
    assert_eq!(backups.len(), 3);
    backup::restore(&mut d, Some(backups[1]), false).unwrap();
    let journal = edit::journal::list(&d).unwrap();
    assert_eq!(journal.iter().map(|e| e.changes.clone()).collect::<Vec<_>>(), [["create playlist One"]]);
    let (_, lib) = replay(&root);
    assert!(lib.find_playlist("One").is_some());
    assert!(lib.find_playlist("Two").is_none() && lib.find_playlist("Three").is_none());
}

/// The journal says which format it's in: an older one without a version still reads (and is
/// written back with one), and one a newer build wrote is refused before anything is edited,
/// and left as it is.
#[test]
fn the_journal_has_a_version() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let d = Device::open_export(&root).unwrap();
    let file = d.state_dir().join("edits.json");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let old = r#"[{"at":"2026-10-01T12:00:00Z","edits":[{"op":"create_playlist","path":"Old"}],"changes":["create playlist Old"]}]"#;
    fs::write(&file, old).unwrap();
    assert_eq!(edit::journal::edits(&d).unwrap(), [Edit::CreatePlaylist { path: "Old".into(), folder: false }]);
    apply(&root, &[Edit::CreatePlaylist { path: "New".into(), folder: false }]);
    let stored: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    // The lowest version its edits need (see `edit::journal::VERSION`).
    assert_eq!(stored["version"], 2);
    assert_eq!(stored["entries"].as_array().unwrap().len(), 2);

    let newer = r#"{"version": 99, "entries": [{"at": "2027-01-01T00:00:00Z", "edits": [{"op": "something_new"}]}]}"#;
    fs::write(&file, newer).unwrap();
    let err = edit::journal::list(&d).unwrap_err().to_string();
    assert!(err.contains("written by a newer locallibrary"), "{err}");
    let files = common::drive_files(&root);
    let edits = [Edit::CreatePlaylist { path: "Newer".into(), folder: false }];
    let err = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap_err().to_string();
    assert!(err.contains("written by a newer locallibrary"), "{err}");
    assert!(common::changed_files(&files, &common::drive_files(&root)).is_empty());
    assert_eq!(fs::read_to_string(&file).unwrap(), newer);
}

#[test]
fn a_playlist_missing_from_onelibrary_is_skipped_there_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    // Only export.pdb gets this playlist (the databases have drifted apart).
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    ed.create_playlist("Only in pdb", 0, false).unwrap();
    ed.save(&pdb_path).unwrap();
    let mut d = Device::open_export(&root).unwrap();
    let edits = vec![Edit::AddTracks {
        playlist: "Only in pdb".into(),
        tracks: ids(&[1, 2]),
        position: None,
        sort: None,
        allow_repeats: false,
    }];
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    assert!(
        plan.warnings.iter().any(|w| w.contains("exportLibrary.db") && w.contains("Only in pdb")),
        "{:?}",
        plan.warnings
    );
    let report = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("skipped")));
    assert_eq!(pdb_library(&root).find_playlist("Only in pdb").unwrap().track_ids.len(), 2);
}

/// Both databases with two top-level playlists named "Twin" (track 0 in the first made, track 1
/// in the second, which is shown first) and two folders named "Shows", only the second holding a
/// "Set" (track 3). export.pdb stores the twins in another order than players show them.
fn twins_drive(dir: &Path) -> (std::path::PathBuf, Vec<u32>) {
    let root = common::bigger_drive(dir, "Main");
    let t: Vec<u32> = pdb_library(&root).tracks.iter().map(|t| t.id).collect();
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    let (first, second) =
        (ed.create_playlist("Twin", 0, false).unwrap(), ed.create_playlist("Twin", 0, false).unwrap());
    ed.set_playlist_entries(first, &[t[0]]).unwrap();
    ed.set_playlist_entries(second, &[t[1]]).unwrap();
    ed.set_sort_order(first, 101).unwrap();
    ed.set_sort_order(second, 100).unwrap();
    ed.create_playlist("Shows", 0, true).unwrap();
    let shows = ed.create_playlist("Shows", 0, true).unwrap();
    let set = ed.create_playlist("Set", shows, false).unwrap();
    ed.set_playlist_entries(set, &[t[3]]).unwrap();
    ed.save(&pdb_path).unwrap();
    common::add_onelibrary(&root);
    (root, t)
}

/// Regression: plan read a path's entries from the first of its same-named playlists as stored,
/// apply wrote them into the first shown, and a path the first shown of two same-named folders
/// lacked failed to apply.
#[test]
fn edits_change_the_first_shown_of_same_named_playlists() {
    let dir = tempfile::tempdir().unwrap();
    let (root, t) = twins_drive(dir.path());
    let add = |playlist: &str, track: u32| Edit::AddTracks {
        playlist: playlist.into(),
        tracks: ids(&[track]),
        position: None,
        sort: None,
        allow_repeats: false,
    };
    let plan = apply(&root, &[add("Twin", t[2]), add("Shows/Set", t[4])]);
    let changed: Vec<(&str, usize, usize)> =
        plan.playlists_changed.iter().map(|c| (c.path.as_str(), c.before, c.after)).collect();
    assert_eq!(changed, [("Shows/Set", 1, 2), ("Twin", 1, 2)]);
    let before = pdb_library(&root);
    let file = |i: usize| before.track(t[i]).unwrap().file_path.clone();
    for lib in [pdb_library(&root), common::plus_library(&root)] {
        let entries = |p: &locallibrary::model::Playlist| -> Vec<String> {
            p.track_ids.iter().map(|id| lib.track(*id).unwrap().file_path.clone()).collect()
        };
        let twins: Vec<Vec<String>> =
            lib.children(None).into_iter().filter(|p| p.name == "Twin").map(entries).collect();
        assert_eq!(twins, [vec![file(1), file(2)], vec![file(0)]]);
        assert_eq!(files(&lib, "Shows/Set"), [file(3), file(4)]);
    }
}

/// Regression: the plan counted every changed playlist of a path with the numbers of the first
/// of them.
#[test]
fn the_plan_counts_each_same_named_playlist_on_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let (root, t) = twins_drive(dir.path());
    let d = Device::open_export(&root).unwrap();
    // Track 0 is in the twin shown second (and in "aab", which holds every track).
    let edits = [Edit::DeleteTracks { tracks: ids(&[t[0]]), keep_files: true }];
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    let twin = plan.playlists_changed.iter().find(|c| c.path == "Twin").unwrap();
    assert_eq!((twin.before, twin.after, twin.added, twin.removed), (1, 0, 0, 1));
}

/// Regression: for a playlist only the second of two same-named folders holds, the plan said it
/// existed already but listed it as created, and apply made another in the first folder, which
/// its path then named.
#[test]
fn a_playlist_in_the_second_of_same_named_folders_is_not_made_again() {
    let dir = tempfile::tempdir().unwrap();
    let (root, t) = twins_drive(dir.path());
    let file = pdb_library(&root).track(t[4]).unwrap().file_path.clone();
    let plan = apply(
        &root,
        &[
            Edit::SetEntries { playlist: "Shows/Set".into(), paths: vec![file.clone()], create: true },
            Edit::CreatePlaylist { path: "Shows/Set".into(), folder: false },
        ],
    );
    assert_eq!(plan.changes[1], "playlist Shows/Set exists already");
    assert!(plan.playlists_created.is_empty(), "{:?}", plan.playlists_created);
    for lib in [pdb_library(&root), common::plus_library(&root)] {
        let shows = lib.children(None).into_iter().filter(|p| p.name == "Shows");
        assert_eq!(shows.map(|g| lib.children(Some(g.id)).len()).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(files(&lib, "Shows/Set"), [file.as_str()]);
    }
}

/// Regression: renaming a playlist to the name it has counted as a change, so the player asked
/// to write it. Now it changes nothing (spaces around the name go); another case of the name is
/// a rename, and the name a twin beside it has is taken.
#[test]
fn renaming_a_playlist_to_its_own_name_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (root, _) = twins_drive(dir.path());
    let d = Device::open_export(&root).unwrap();
    let plan = |playlist: &str, name: &str| {
        let rename = Edit::RenamePlaylist { playlist: playlist.into(), name: name.into() };
        edit::plan(&d, &[rename], &EditOptions::default(), &Ctx::silent())
    };
    let same = plan("Shows/Set", " Set ").unwrap();
    assert!(same.is_noop(), "{:?}", same.changes);
    assert_eq!(same.changes, ["Shows/Set is called that already"]);
    assert!(same.recorded.is_empty());
    assert!(!plan("Shows/Set", "set").unwrap().is_noop());
    assert!(plan("Twin", "Twin").is_err());
}

/// Regression: edits couldn't reach a playlist shown after a folder of its name: they found the
/// folder, and said it was one. Both databases get the edits, each on the node of the kind meant.
#[test]
fn a_playlist_shown_after_a_folder_of_its_name_is_edited_in_both_databases() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let t: Vec<u32> = pdb_library(&root).tracks.iter().map(|t| t.id).collect();
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    let folder = ed.create_playlist("Set", 0, true).unwrap();
    ed.create_playlist("Inside", folder, false).unwrap();
    let list = ed.create_playlist("Set", 0, false).unwrap();
    ed.set_playlist_entries(list, &[t[0]]).unwrap();
    ed.save(&pdb_path).unwrap();
    common::add_onelibrary(&root);
    apply(
        &root,
        &[
            Edit::AddTracks {
                playlist: "Set".into(),
                tracks: ids(&[t[1]]),
                position: None,
                sort: None,
                allow_repeats: false,
            },
            Edit::RenamePlaylist { playlist: "Set/".into(), name: "Crates".into() },
            Edit::SortPlaylist { playlist: "Set".into(), by: "-title".into() },
        ],
    );
    let before = pdb_library(&root);
    let mut want: Vec<&locallibrary::Track> = [t[0], t[1]].iter().map(|id| before.track(*id).unwrap()).collect();
    want.sort_by_key(|t| std::cmp::Reverse(locallibrary::model::normalize(&t.title)));
    let want: Vec<String> = want.iter().map(|t| t.file_path.clone()).collect();
    for lib in [pdb_library(&root), common::plus_library(&root)] {
        assert_eq!(files(&lib, "Set"), want);
        assert!(lib.find_playlist("Crates/Inside").is_some());
    }
}

/// Regression: a smart playlist rule's fill found the playlist to fill ignoring case, so a rule
/// for "Rules/FoO" filled "Rules/Foo". A fill that makes its playlist matches it exactly.
#[test]
fn a_rule_fills_the_playlist_of_exactly_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    let t: Vec<u32> = pdb_library(&root).tracks.iter().map(|t| t.id).collect();
    let rule = |path: &str| locallibrary::smart::SmartPlaylist {
        path: path.into(),
        filter: "rating:5".into(),
        sort: None,
        limit: None,
        drives: vec![],
    };
    apply(
        &root,
        &[
            Edit::SetEntries { playlist: "Rules/Foo".into(), paths: vec![], create: true },
            Edit::EditTracks { tracks: ids(&[t[2]]), fields: TrackFields { rating: Some(5), ..Default::default() } },
            rule("Rules/FoO").edit(),
        ],
    );
    let file = pdb_library(&root).track(t[2]).unwrap().file_path.clone();
    for lib in [pdb_library(&root), common::plus_library(&root)] {
        let rules = lib.find_playlist("Rules").unwrap();
        let kids: Vec<(&str, usize)> =
            lib.children(Some(rules.id)).iter().map(|p| (p.name.as_str(), p.track_ids.len())).collect();
        assert_eq!(kids, [("Foo", 0), ("FoO", 1)]);
        assert_eq!(files(&lib, "Rules/FoO"), [file.as_str()]);
    }
}

#[test]
fn onelibrary_only_drives_are_edited_too() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::onelibrary_only(&root);
    apply(
        &root,
        &[
            Edit::CreatePlaylist { path: "New".into(), folder: false },
            Edit::AddTracks {
                playlist: "New".into(),
                tracks: TrackSelector { filter: Some("bpm:>0".into()), ..Default::default() },
                position: None,
                sort: Some("bpm".into()),
                allow_repeats: false,
            },
            Edit::SortPlaylist { playlist: "New".into(), by: "-bpm".into() },
        ],
    );
    common::sqlcipher_reads(&root);
    let lib = common::plus_library(&root);
    let bpms: Vec<f64> =
        lib.find_playlist("New").unwrap().track_ids.iter().map(|id| lib.track(*id).unwrap().bpm).collect();
    assert!(!bpms.is_empty());
    assert!(bpms.windows(2).all(|w| w[0] >= w[1]), "{bpms:?}");
}

#[test]
fn a_stale_plan_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    let plan = edit::plan(
        &d,
        &[Edit::CreatePlaylist { path: "X".into(), folder: false }],
        &EditOptions::default(),
        &Ctx::silent(),
    )
    .unwrap();
    common::export_new_track(&root, "Artist", "Late addition");
    assert!(matches!(edit::apply(&plan, &mut d, &Ctx::silent()), Err(locallibrary::Error::StalePlan(_))));
}

#[test]
fn bulk_edits_on_a_large_library_stay_quick() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::large_drive(dir.path(), "Big", false);
    let lib = pdb_library(&root);
    let start = std::time::Instant::now();
    let mut d = Device::open_export(&root).unwrap();
    let edits = vec![
        Edit::SetTracks {
            playlist: "Everything".into(),
            tracks: TrackSelector { filter: Some("bpm:>0".into()), ..Default::default() },
            sort: Some("key,bpm".into()),
            limit: None,
            create: true,
        },
        Edit::EditTracks {
            tracks: TrackSelector { filter: Some("bpm:>=128".into()), ..Default::default() },
            fields: TrackFields { color: Some("red".into()), ..Default::default() },
        },
    ];
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert!(start.elapsed() < std::time::Duration::from_secs(60), "{:?}", start.elapsed());
    let after = pdb_library(&root);
    assert_eq!(
        after.find_playlist("Everything").unwrap().track_ids.len(),
        lib.tracks.iter().filter(|t| t.bpm > 0.0).count()
    );
    assert!(after.tracks.iter().filter(|t| t.bpm >= 128.0).all(|t| t.color == Some(TrackColor::Red)));
}

#[test]
fn removing_a_guest_crate_takes_its_tracks_and_files() {
    let dir = tempfile::tempdir().unwrap();
    let guest_root = common::demo_drive(dir.path(), "Guest");
    // The guest's set: a playlist holding both of the demo export's tracks.
    let guest_pdb = guest_root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&guest_pdb).unwrap();
    let set = ed.create_playlist("Saturday", 0, false).unwrap();
    for t in pdb_library(&guest_root).tracks {
        ed.add_to_playlist(set, t.id, None).unwrap();
    }
    ed.save(&guest_pdb).unwrap();
    let guest = locallibrary::Device::open_export(&guest_root).unwrap();
    let root = both_databases(dir.path());
    let mut main = Device::open_export(&root).unwrap();
    let opts = locallibrary::merge::MergeOptions { folder_name: Some("Sam".into()), ..Default::default() };
    let plan = locallibrary::merge::plan(&guest, &main, &opts, &Ctx::silent()).unwrap();
    locallibrary::merge::apply(&plan, &guest, &mut main, &opts, &Ctx::silent()).unwrap();
    let merged = pdb_library(&root);
    assert_eq!(merged.tracks.len(), 14);
    let guest_files: Vec<String> = merged
        .tracks
        .iter()
        .filter(|t| {
            !pdb_library(&common::bigger_drive(dir.path(), "Ref")).tracks.iter().any(|o| o.file_path == t.file_path)
        })
        .map(|t| t.file_path.clone())
        .collect();
    assert_eq!(guest_files.len(), 2);

    let plan = apply(&root, &[Edit::DeletePlaylist { playlist: "Sam".into(), with_tracks: true }]);
    assert_eq!(plan.tracks_removed.len(), 2);
    for lib in [pdb_library(&root), common::plus_library(&root)] {
        assert_eq!(lib.tracks.len(), 12);
        assert!(lib.find_playlist("Sam").is_none());
    }
    for f in guest_files {
        assert!(!paths::to_fs(&root, &paths::key(&f)).exists(), "{f} deleted");
    }
}

#[test]
fn importing_a_spotify_csv_makes_a_playlist_in_both_databases() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    let lib = pdb_library(&root);
    let (a, b) = (&lib.tracks[5], &lib.tracks[7]);
    let csv = format!(
        "\"Track Name\",\"Artist Name(s)\",\"Duration (ms)\"\n\"{}\",\"{}\",{}\n\"{}\",\"{}\",{}\n\"Not on the drive\",\"Nobody\",1000\n",
        a.title,
        a.artist,
        a.duration_secs * 1000,
        b.title,
        b.artist,
        b.duration_secs * 1000
    );
    let file = dir.path().join("spotify.csv");
    fs::write(&file, csv).unwrap();
    let req = locallibrary::api::ImportRequest {
        drive: root.display().to_string(),
        file,
        into: "From Spotify".into(),
        playlist: None,
        append: false,
        choices: Default::default(),
    };
    let preview = locallibrary::api::import_plan(&req, &Ctx::silent()).unwrap();
    assert_eq!((preview.report.summary.found, preview.report.summary.missing), (2, 1));
    locallibrary::api::import_apply(&req, &Ctx::silent()).unwrap();
    for l in [pdb_library(&root), common::plus_library(&root)] {
        assert_eq!(files(&l, "From Spotify"), [a.file_path.clone(), b.file_path.clone()]);
    }
}

/// Regression: an imported list lost the tracks it played twice, `--into` filled a playlist of
/// that name in another folder, and a track picked by hand was reported as found by its path.
#[test]
fn importing_keeps_repeats_and_fills_exactly_its_path() {
    use locallibrary::import::{MatchKind, Status};
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let lib = pdb_library(&root);
    let (a, b) = (&lib.tracks[5], &lib.tracks[7]);
    apply(
        &root,
        &[Edit::SetEntries { playlist: "Shows/Friday".into(), paths: vec![b.file_path.clone()], create: true }],
    );
    let m3u = format!(
        "#EXTM3U\n/Users/me/Music{a}\n/Users/me/Music{b}\n/Users/me/Music{a}\n#EXTINF:5000,{title}\n/nowhere/x.mp3\n",
        a = a.file_path,
        b = b.file_path,
        title = b.title
    );
    let file = dir.path().join("set.m3u8");
    fs::write(&file, m3u).unwrap();
    let req = |append, choices: &[(usize, u32)]| locallibrary::api::ImportRequest {
        drive: root.display().to_string(),
        file: file.clone(),
        into: "Friday".into(),
        playlist: None,
        append,
        choices: choices.iter().copied().collect(),
    };
    let preview = locallibrary::api::import_plan(&req(false, &[]), &Ctx::silent()).unwrap();
    assert_eq!(preview.report.entries[3].status, Status::Ambiguous { candidates: vec![b.id] }, "its length is off");
    let preview = locallibrary::api::import_plan(&req(false, &[(3, b.id)]), &Ctx::silent()).unwrap();
    assert_eq!(preview.report.entries[3].status, Status::Found { track_id: b.id, how: MatchKind::Chosen });
    locallibrary::api::import_apply(&req(false, &[(3, b.id)]), &Ctx::silent()).unwrap();
    let (fa, fb) = (a.file_path.as_str(), b.file_path.as_str());
    let lib = pdb_library(&root);
    assert_eq!(files(&lib, "Shows/Friday"), [fb], "the nested namesake is left alone");
    let top = lib.children(None).into_iter().find(|p| p.name == "Friday").unwrap();
    assert_eq!(lib.playlist_path(top.id), "Friday");
    assert_eq!(files(&lib, "Friday"), [fa, fb, fa, fb]);
    locallibrary::api::import_apply(&req(true, &[]), &Ctx::silent()).unwrap();
    assert_eq!(files(&pdb_library(&root), "Friday"), [fa, fb, fa, fb, fa, fb, fa]);
}

/// Regression: a history list was selected from export.pdb's lists only, so one a player that
/// reads only OneLibrary recorded there couldn't be.
#[test]
fn history_lists_only_onelibrary_has_are_selected() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    let plus = common::plus_library(&root);
    let mut one = locallibrary::onelibrary::OneLibrary::open_root(&root).unwrap();
    let list = one.create_history("HISTORY 003", 0, false).unwrap();
    for t in [&plus.tracks[3], &plus.tracks[1]] {
        one.add_to_history(list, t.id).unwrap();
    }
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let played = TrackSelector { history: Some("HISTORY 003".into()), ..Default::default() };
    apply(
        &root,
        &[
            Edit::CreatePlaylist { path: "Played".into(), folder: false },
            Edit::AddTracks {
                playlist: "Played".into(),
                tracks: played,
                position: None,
                sort: None,
                allow_repeats: false,
            },
        ],
    );
    let want = [plus.tracks[3].file_path.as_str(), plus.tracks[1].file_path.as_str()];
    assert_eq!(files(&pdb_library(&root), "Played"), want);
}

/// Regression: importing a rekordbox XML playlist took the first whose path matched ignoring
/// case, so asking for "SET" next to "Set" imported "Set".
#[test]
fn importing_an_xml_playlist_takes_the_one_of_exactly_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let lib = pdb_library(&root);
    let (a, b) = (&lib.tracks[5], &lib.tracks[7]);
    let location = |t: &locallibrary::Track| format!("file://localhost/Volumes/USB{}", t.file_path.replace(' ', "%20"));
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <COLLECTION Entries="2">
    <TRACK TrackID="1" Name="a" Location="{}"/>
    <TRACK TrackID="2" Name="b" Location="{}"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="2">
      <NODE Name="Set" Type="1" KeyType="0" Entries="1"><TRACK Key="1"/></NODE>
      <NODE Name="SET" Type="1" KeyType="0" Entries="1"><TRACK Key="2"/></NODE>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>
"#,
        location(a),
        location(b)
    );
    let file = dir.path().join("rekordbox.xml");
    fs::write(&file, xml).unwrap();
    let req = |playlist: &str| locallibrary::api::ImportRequest {
        drive: root.display().to_string(),
        file: file.clone(),
        into: "Imported".into(),
        playlist: Some(playlist.into()),
        append: false,
        choices: Default::default(),
    };
    locallibrary::api::import_apply(&req("SET"), &Ctx::silent()).unwrap();
    assert_eq!(files(&pdb_library(&root), "Imported"), [b.file_path.as_str()]);
    assert!(locallibrary::api::import_plan(&req("set"), &Ctx::silent()).is_err(), "two match ignoring case");
}

#[test]
fn my_tags_follow_removed_and_replaced_tracks() {
    use locallibrary::tags;
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Demo");
    common::export_new_track(&root, "Artist", "Extra");
    let lib = pdb_library(&root);
    let ids: Vec<u32> = lib.tracks.iter().map(|t| t.id).collect();
    assert_eq!(ids.len(), 3);
    let mut d = Device::open_export(&root).unwrap();
    let plan = tags::plan(&d, &tags::Edit::Add { tag: "Genre/Deep".into(), tracks: vec![ids[0], ids[1]] }).unwrap();
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let links = || {
        let ext = rekordbox_pdb::ExtDatabase::from_file(root.join(paths::EXPORT_EXT_PDB)).unwrap();
        let mut l: Vec<u32> = ext.tag_tracks.iter().map(|l| l.track_id).collect();
        l.sort_unstable();
        l
    };
    assert_eq!(links(), [ids[0], ids[1]]);

    // Replacing track 1 with track 3 moves its tag; deleting track 2 takes its link away.
    apply(
        &root,
        &[
            Edit::ReplaceTrack { duplicate: ids_sel(ids[0]), keep: ids_sel(ids[2]), keep_duplicate: false },
            Edit::DeleteTracks { tracks: ids_sel(ids[1]), keep_files: true },
        ],
    );
    assert_eq!(links(), [ids[2]]);
    let by_track = tags::by_track(&Device::open_export(&root).unwrap()).unwrap();
    let key = paths::key(&lib.track(ids[2]).unwrap().file_path).to_lowercase();
    assert_eq!(by_track[&key], ["Genre/Deep"]);

    // `tag:` filters see them.
    let page = locallibrary::api::tracks_query(&locallibrary::api::TracksQuery {
        library: root.display().to_string().into(),
        filter: Some("tag:deep".into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(page.tracks.iter().map(|t| t.id).collect::<Vec<_>>(), [ids[2]]);
}

fn ids_sel(id: u32) -> TrackSelector {
    TrackSelector { ids: vec![id], ..Default::default() }
}

#[test]
fn dedupe_replaces_sync_manager_copies() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    let lib = pdb_library(&root);
    let orig = lib.tracks[4].clone();
    // rekordbox's Sync Manager exported it again as "<name>-1.<ext>".
    let (stem, ext) = orig.file_path.rsplit_once('.').unwrap();
    let copy_path = format!("{stem}-1.{ext}");
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    let copy = ed
        .add_track(&rekordbox_pdb::NewTrack {
            artist: Some(orig.artist.clone()),
            duration: orig.duration_secs as u16,
            file_size: orig.file_size as u32,
            ..rekordbox_pdb::NewTrack::new(orig.title.clone(), copy_path.clone())
        })
        .unwrap();
    let p = ed.create_playlist("Set", 0, false).unwrap();
    ed.add_to_playlist(p, copy, None).unwrap();
    ed.save(&pdb_path).unwrap();
    fs::copy(paths::to_fs(&root, &paths::key(&orig.file_path)), paths::to_fs(&root, &paths::key(&copy_path))).unwrap();

    let req = locallibrary::api::DedupeRequest { drive: root.display().to_string(), ..Default::default() };
    let plan = locallibrary::api::dedupe_plan(&req, &Ctx::silent()).unwrap();
    assert_eq!(plan.tracks_removed.len(), 1, "{:?}", plan.changes);
    locallibrary::api::dedupe_apply(&req, &Ctx::silent()).unwrap();
    let after = pdb_library(&root);
    assert_eq!(files(&after, "Set"), std::slice::from_ref(&orig.file_path));
    assert!(!paths::to_fs(&root, &paths::key(&copy_path)).exists());
    assert!(paths::to_fs(&root, &paths::key(&orig.file_path)).exists());
}

/// Regression: without `metadata`, a track that only shared artist, title and length with a Sync
/// Manager copy's group (another edit, say) was replaced too, its file deleted. And with no
/// duplicates to replace, dedupe failed rather than having nothing to do.
#[test]
fn dedupe_replaces_another_edit_only_with_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    let lib = pdb_library(&root);
    let orig = lib.tracks[4].clone();
    let req = |metadata| locallibrary::api::DedupeRequest {
        drive: root.display().to_string(),
        metadata,
        ..Default::default()
    };
    let none = locallibrary::api::dedupe_plan(&req(false), &Ctx::silent()).unwrap();
    assert!(none.noop && none.warnings.iter().any(|w| w.contains("metadata")), "{:?}", none.warnings);
    let (stem, ext) = orig.file_path.rsplit_once('.').unwrap();
    let copy_path = format!("{stem}-1.{ext}");
    let edit_path = format!("/Contents/Other/{} (edit).{ext}", orig.title);
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    for (path, size) in [(&copy_path, orig.file_size), (&edit_path, orig.file_size + 1)] {
        ed.add_track(&rekordbox_pdb::NewTrack {
            artist: Some(orig.artist.clone()),
            duration: orig.duration_secs as u16,
            file_size: size as u32,
            ..rekordbox_pdb::NewTrack::new(orig.title.clone(), path.clone())
        })
        .unwrap();
        let to = paths::to_fs(&root, &paths::key(path));
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(paths::to_fs(&root, &paths::key(&orig.file_path)), to).unwrap();
    }
    ed.save(&pdb_path).unwrap();

    let removed = |metadata| -> Vec<String> {
        let plan = locallibrary::api::dedupe_plan(&req(metadata), &Ctx::silent()).unwrap();
        plan.tracks_removed.iter().map(|t| t.file_path.clone()).collect()
    };
    assert_eq!(removed(false), std::slice::from_ref(&copy_path));
    let mut both = removed(true);
    both.sort();
    let mut want = vec![copy_path, edit_path];
    want.sort();
    assert_eq!(both, want);
}

#[test]
fn track_details_say_where_a_track_is_filed_and_what_is_wrong_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let lib = pdb_library(&root);
    let orig = lib.tracks.iter().find(|t| !lib.playlists.iter().all(|p| !p.track_ids.contains(&t.id))).unwrap().clone();
    // A Sync Manager copy of it, in a playlist of its own.
    let (stem, ext) = orig.file_path.rsplit_once('.').unwrap();
    let copy_path = format!("{stem}-1.{ext}");
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    let copy = ed
        .add_track(&rekordbox_pdb::NewTrack {
            artist: Some(orig.artist.clone()),
            duration: orig.duration_secs as u16,
            ..rekordbox_pdb::NewTrack::new(orig.title.clone(), copy_path.clone())
        })
        .unwrap();
    let p = ed.create_playlist("Set", 0, false).unwrap();
    ed.add_to_playlist(p, orig.id, None).unwrap();
    ed.save(&pdb_path).unwrap();
    common::add_history(&root, "HISTORY 001", &[orig.id]);
    fs::remove_file(paths::to_fs(&root, &paths::key(&orig.file_path))).unwrap();

    let lib = pdb_library(&root);
    let mut playlists: Vec<String> = lib
        .playlists
        .iter()
        .filter(|p| !p.is_folder && p.track_ids.contains(&orig.id))
        .map(|p| lib.playlist_path(p.id))
        .collect();
    playlists.sort();
    assert!(playlists.len() >= 2, "{playlists:?}");
    let req = |tracks| locallibrary::api::TrackDetailsRequest { library: root.display().to_string().into(), tracks };
    let got = locallibrary::api::track_details(&req(ids(&[orig.id, copy]))).unwrap();
    assert_eq!(got.iter().map(|d| d.track.id).collect::<Vec<_>>(), [orig.id, copy], "in the order asked for");
    let d = &got[0];
    assert!(!d.audio_present);
    assert_eq!(d.playlists, playlists);
    assert_eq!(d.history, ["HISTORY 001"]);
    assert_eq!(d.copies.iter().map(|c| (c.id, c.file_path.as_str())).collect::<Vec<_>>(), [(copy, copy_path.as_str())]);
    assert!(d.analysis.analysed);
    assert_eq!(d.analysis.beats, 0, "the synthetic analysis file has no beat grid");
    let c = &got[1];
    assert!(c.playlists.is_empty() && c.history.is_empty());
    assert_eq!(c.copies.iter().map(|c| c.id).collect::<Vec<_>>(), [orig.id]);
    assert!(!c.analysis.analysed, "the copy has no analysis files");

    let err = locallibrary::api::track_details(&req(ids(&[99_999]))).unwrap_err();
    assert!(matches!(err, locallibrary::Error::NotFound(_)), "{err}");

    // Real analysis files: rekordbox's beat grid and phrases.
    let demo = common::demo_drive(dir.path(), "Demo");
    let req = locallibrary::api::TrackDetailsRequest {
        library: demo.display().to_string().into(),
        tracks: TrackSelector { query: Some("demo track 1".into()), ..Default::default() },
    };
    let a = &locallibrary::api::track_details(&req).unwrap()[0].analysis;
    assert_eq!(
        (a.beats, a.bars, a.tempo_range),
        (368, 92, Some(locallibrary::model::BpmRange { low: 128.0, high: 128.0 }))
    );
    assert!(a.waveform && a.mood.is_some());
    assert_eq!(a.phrases.first().map(|p| (p.kind.as_str(), p.bars)), Some(("Chorus", 9)));
}

/// Removing and moving entries, and deleting a playlist with its tracks, are recorded as what
/// they changed: a replay on a later export that added entries keeps those, and deletes only the
/// tracks deleted at the time.
#[test]
fn a_replay_keeps_what_a_later_export_added() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let lib = pdb_library(&root);
    // Its own playlists go, so the tracks are in none.
    let top: Vec<Edit> = lib
        .children(None)
        .iter()
        .map(|p| Edit::DeletePlaylist { playlist: p.name.clone(), with_tracks: false })
        .collect();
    apply(&root, &top);
    let t: Vec<u32> = lib.tracks.iter().map(|t| t.id).take(7).collect();
    let set = |playlist: &str, tracks: &[u32]| Edit::SetTracks {
        playlist: playlist.into(),
        tracks: ids(tracks),
        sort: None,
        limit: None,
        create: true,
    };
    apply(&root, &[set("Gig", &t[..3]), set("Guest", &t[4..6])]);
    let mut d = Device::open_export(&root).unwrap();
    edit::journal::clear(&d, None).unwrap();
    let exported = common::drive_files(&root);

    apply(&root, &[Edit::RemoveTracks { playlist: "Gig".into(), tracks: Some(ids(&t[..1])), positions: vec![] }]);
    apply(&root, &[Edit::MoveTracks { playlist: "Gig".into(), positions: vec![2], to: 1 }]);
    apply(&root, &[Edit::DeletePlaylist { playlist: "Guest".into(), with_tracks: true }]);
    let kinds: Vec<String> = edit::journal::edits(&d)
        .unwrap()
        .iter()
        .map(|e| format!("{e:?}").split([' ', '{']).next().unwrap().to_string())
        .collect();
    assert_eq!(kinds, ["RemoveTracks", "RemoveTracks", "AddTracks", "DeletePlaylist", "DeleteTracks"]);

    // rekordbox exports again: the edits are gone, and its playlists gained a track each.
    for (key, bytes) in &exported {
        let p = paths::to_fs(&root, key);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
    let later = [
        Edit::AddTracks {
            playlist: "Gig".into(),
            tracks: ids(&t[3..4]),
            position: None,
            sort: None,
            allow_repeats: false,
        },
        Edit::AddTracks {
            playlist: "Guest".into(),
            tracks: ids(&t[6..7]),
            position: None,
            sort: None,
            allow_repeats: false,
        },
    ];
    let plan = edit::plan(&d, &later, &EditOptions { lenient: false, no_journal: true }, &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();

    let (_, after) = replay(&root);
    let path = |id: u32| lib.track(id).unwrap().file_path.clone();
    assert_eq!(files(&after, "Gig"), [path(t[2]), path(t[1]), path(t[3])]);
    assert!(after.find_playlist("Guest").is_none());
    let left: Vec<&str> = after.tracks.iter().map(|x| x.file_path.as_str()).collect();
    assert!(!left.contains(&path(t[4]).as_str()) && !left.contains(&path(t[5]).as_str()));
    assert!(left.contains(&path(t[6]).as_str()), "a track added to Guest since stays");
    assert!(paths::to_fs(&root, &paths::key(&path(t[6]))).is_file());
}

/// A replay is planned from the journal: one cleared since the plan is a stale plan.
#[test]
fn a_replay_plan_is_stale_once_the_journal_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let exported = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    apply(&root, &[Edit::CreatePlaylist { path: "First".into(), folder: false }]);
    fs::write(root.join(paths::EXPORT_PDB), &exported).unwrap();
    let drive = locallibrary::api::DriveRef { drive: root.display().to_string() };
    let plan = locallibrary::api::edits_replay_plan(&drive, &Ctx::silent()).unwrap();
    assert!(!plan.noop);
    locallibrary::api::edits_clear(&locallibrary::api::ClearEdits { drive: drive.drive.clone(), before: None })
        .unwrap();
    let err = plan.apply(None, &Ctx::silent()).unwrap_err();
    assert!(matches!(err, locallibrary::Error::StalePlan(_)), "{err}");
}

/// A replay stopped while it orders a playlist for mixing stops: the order isn't left out as an
/// edit that no longer applies. (The first track's analysis file is a pipe, so planning waits
/// there, past the check made before each edit, until the test has stopped it.)
#[cfg(unix)]
#[test]
fn stopping_a_replay_while_it_orders_a_set_stops_it() {
    use std::sync::mpsc;
    use std::time::Duration;
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let set = lib.playlists.iter().find(|p| p.track_ids.len() > 1).expect("a playlist");
    let first = lib.track(set.track_ids[0]).unwrap();
    let pipe = paths::to_fs(&root, &paths::key(&first.analyze_path)).with_extension("EXT");
    fs::remove_file(&pipe).unwrap();
    assert!(std::process::Command::new("mkfifo").arg(&pipe).status().unwrap().success());
    let order = Edit::PlanSet { playlist: lib.playlist_path(set.id), start: None, into: None };
    let cancel = locallibrary::CancelToken::new();
    let (done, planned) = mpsc::channel();
    let stop = cancel.clone();
    std::thread::spawn(move || {
        let ctx = Ctx { cancel, ..Ctx::silent() };
        let options = EditOptions { lenient: true, no_journal: true };
        let _ = done.send(edit::plan(&device, &[order], &options, &ctx).map(|_| ()));
    });
    // Opening the pipe to write waits until planning opens it to read.
    let (opened, writer) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = opened.send(fs::OpenOptions::new().write(true).open(&pipe).unwrap());
    });
    let writer = writer.recv_timeout(Duration::from_secs(10)).expect("planning read the analysis");
    stop.cancel();
    drop(writer);
    let r = planned.recv_timeout(Duration::from_secs(10)).expect("planning finished");
    assert!(matches!(r, Err(locallibrary::Error::Cancelled)), "{r:?}");
}

/// Regression: an edit could nest playlists past `model::MAX_DEPTH`, which reading the drive back
/// takes for a loop in a damaged file, moving the deepest rows to the top level. Such an edit is
/// refused when it's planned; so is merging a tree that deep inside a folder.
#[test]
fn playlists_nest_no_deeper_than_a_drive_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let path = |n: usize| (1..=n).map(|i| format!("F{i}")).collect::<Vec<_>>().join("/");
    let plan = |edits: &[Edit]| {
        let d = Device::open_export(&root).unwrap();
        edit::plan(&d, edits, &EditOptions::default(), &Ctx::silent()).map(|_| ()).unwrap_err().to_string()
    };
    let err = plan(&[Edit::CreatePlaylist { path: path(65), folder: false }]);
    assert!(err.contains("65 levels deep"), "{err}");

    apply(&root, &[Edit::CreatePlaylist { path: path(64), folder: false }]);
    apply(&root, &[Edit::CreatePlaylist { path: "Sets/Fri".into(), folder: false }]);
    let lib = Device::open(&root).unwrap().library().unwrap();
    assert_eq!(lib.playlist_path(lib.find_playlist(&path(64)).unwrap().id), path(64), "read back where it was made");
    // A folder moved takes what's in it down with it.
    let err = plan(&[Edit::MovePlaylist { playlist: "Sets".into(), into: Some(path(63)), position: None }]);
    assert!(err.contains("65 levels deep"), "{err}");
    apply(&root, &[Edit::MovePlaylist { playlist: "Sets".into(), into: Some(path(62)), position: None }]);

    let src = Device::open_export(&root).unwrap();
    let dst = Device::open_export(common::demo_drive(dir.path(), "Other")).unwrap();
    let opts = locallibrary::merge::MergeOptions::default();
    let err = locallibrary::merge::plan(&src, &dst, &opts, &Ctx::silent()).map(|_| ()).unwrap_err().to_string();
    assert!(err.contains("merge by name instead"), "{err}");
}

/// Setting what tracks already hold changes nothing, so replaying a field edit that's still in
/// place is a no-op rather than counting every track it names as changed.
#[test]
fn a_field_edit_already_in_place_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_databases(dir.path());
    let first = pdb_library(&root).tracks[0].id;
    let edit =
        Edit::EditTracks { tracks: ids(&[first]), fields: TrackFields { rating: Some(5), ..Default::default() } };
    assert_eq!(apply(&root, std::slice::from_ref(&edit)).tracks_changed, 1);
    let d = Device::open_export(&root).unwrap();
    let again = edit::plan(&d, &[edit], &EditOptions { lenient: true, no_journal: true }, &Ctx::silent()).unwrap();
    assert_eq!(again.tracks_changed, 0);
    assert!(again.is_noop());
}
