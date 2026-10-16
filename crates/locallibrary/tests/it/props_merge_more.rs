//! Merges the main merge property leaves out: into a target with OneLibrary from a source with it
//! too (artwork, history, hot cue bank lists), where the target's two databases end up alike; from
//! a source whose tracks are missing audio or analysis or point outside the library, where exactly
//! the sound tracks come; and the merges refused when planning.

use crate::common::{self, Warnings, media};
use crate::drawn;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::error::Reason;
use locallibrary::merge::{self, MatchMode, MergeOptions, PlaylistMode};
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::{Library, Playlist, backup, paths, pdb};
use rekordbox_pdb::{NewTrack, PdbEditor};

/// Track files, and playlists by path with their entries' files, comparable across databases.
fn shape(lib: &Library) -> (BTreeSet<String>, Vec<(String, Vec<String>)>) {
    let key = |p: &str| paths::key(p).to_lowercase();
    let files = lib.tracks.iter().map(|t| key(&t.file_path)).collect();
    let mut lists: Vec<(String, Vec<String>)> = lib
        .playlists
        .iter()
        .filter(|p| !p.is_folder)
        .map(|p| {
            (lib.playlist_path(p.id), p.track_ids.iter().map(|id| key(&lib.track(*id).unwrap().file_path)).collect())
        })
        .collect();
    lists.sort();
    (files, lists)
}

/// A cue on the first track of `root`'s OneLibrary, in a hot cue bank list (inside a folder, or
/// one that's missing).
fn bank_list(root: &Path, in_folder: bool) {
    let mut one = OneLibrary::open_root(root).unwrap();
    let first = one.library().unwrap().tracks[0].id;
    one.connection()
        .execute("INSERT INTO cue (cue_id, content_id, kind, inUsec) VALUES (900, ?1, 0, 2000000)", [first])
        .unwrap();
    let parent = match in_folder {
        true => one.create_hot_cue_bank_list("Gigs", 0, true).unwrap(),
        false => 77,
    };
    let list = one.create_hot_cue_bank_list("Bank", parent, false).unwrap();
    one.add_to_hot_cue_bank_list(list, 900).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
}

#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn merging_with_onelibrary_on_both_sides_keeps_the_targets_databases_alike(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = locallibrary::device::dunce_canonicalize(dir.path());
    let main = common::bigger_drive(&root, "Main");
    let only_plus = tc.draw(gs::booleans());
    match only_plus {
        true => common::onelibrary_only(&main),
        false => common::add_onelibrary(&main),
    }
    let guest = common::demo_drive(&root, "Guest");
    for n in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        drawn::track(&tc, &guest, 10 + n);
    }
    common::add_history(&guest, "HISTORY 001", &[1]);
    let art = tc.draw(gs::booleans());
    if art {
        media::set_art(&guest, &[1, 2]);
    }
    common::add_onelibrary(&guest);
    let bank = tc.draw(gs::optional(gs::booleans()));
    if let Some(in_folder) = bank {
        bank_list(&guest, in_folder);
    }
    let opts = MergeOptions {
        playlists: tc.draw(
            gs::sampled_from(vec![PlaylistMode::Folder, PlaylistMode::ByName, PlaylistMode::None]).print_as_debug(),
        ),
        folder_name: None,
        only_playlists: Vec::new(),
        match_mode: tc.draw(gs::sampled_from(vec![MatchMode::Metadata, MatchMode::Path]).print_as_debug()),
        history: tc.draw(gs::booleans()),
    };

    let before = common::drive_files(&main);
    let src = Device::open_export(&guest).unwrap();
    let mut dst = Device::open_export(&main).unwrap();
    let plus_before = common::plus_library(&main);
    let warnings = Warnings::default();
    let plan = merge::plan(&src, &dst, &opts, &warnings.ctx()).unwrap();
    let report = merge::apply(&plan, &src, &mut dst, &opts, &warnings.ctx()).unwrap();
    tc.note(&format!("{:?} {:?}", plan.warnings, warnings.said()));
    assert_eq!(report.tracks_added, plan.new_tracks.len());

    let plus = common::plus_library(&main);
    assert_eq!(plus.tracks.len(), plus_before.tracks.len() + report.tracks_added, "OneLibrary gets every new track");
    if !only_plus {
        assert_eq!(
            shape(&plus),
            shape(&pdb::read_library(&main.join(paths::EXPORT_PDB)).unwrap()),
            "both databases alike"
        );
    }
    // Artwork comes with the tracks that have it, in OneLibrary too.
    if art && report.tracks_added > 0 {
        assert!(report.artwork_added > 0);
        for t in plus.tracks.iter().filter(|t| t.artwork_path.is_some()) {
            assert!(paths::to_fs(&main, &paths::key(t.artwork_path.as_deref().unwrap())).is_file());
        }
    }
    // A bank list comes when its cue's track does, with the merge folder above it.
    let lists = OneLibrary::open_root(&main).unwrap().hot_cue_bank_lists().unwrap();
    let banks: Vec<&Playlist> = lists.iter().filter(|l| l.name == "Bank").collect();
    assert_eq!(banks.len(), report.hot_cue_bank_lists_added);
    let first_is_new = plan.new_tracks.iter().any(|t| t.source_id == src.library().unwrap().tracks[0].id);
    if bank.is_some() && first_is_new && opts.playlists != PlaylistMode::None {
        assert_eq!((banks.len(), banks[0].track_ids.len()), (1, 1), "{lists:?}");
    }
    let history: BTreeSet<&str> = plus.history.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(history.contains("HISTORY 001"), opts.history, "history comes when asked for");
    common::sqlcipher_reads(&main);

    backup::restore(&mut dst, report.backup.as_deref(), false).unwrap();
    let back = common::drive_files(&main);
    for (key, bytes) in &before {
        assert_eq!(back.get(key), Some(bytes), "{key} isn't restored");
    }
}

/// What can be wrong with a source track.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Defect {
    None,
    NoAudio,
    NoAnalysis,
}

#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn exactly_the_source_tracks_with_audio_inside_the_library_are_merged(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = locallibrary::device::dunce_canonicalize(dir.path());
    // The twelve-track export's analysis paths are outside PIONEER/ (so none is found), and drawn
    // tracks' inside it.
    let guest = common::bigger_drive(&root, "Guest");
    for n in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        drawn::track(&tc, &guest, n);
    }
    let main = common::demo_drive(&root, "Main");
    let lib = pdb::read_library(&guest.join(paths::EXPORT_PDB)).unwrap();
    let defects: Vec<Defect> = lib
        .tracks
        .iter()
        .map(|_| {
            tc.draw(
                gs::sampled_from(vec![Defect::None, Defect::None, Defect::NoAudio, Defect::NoAnalysis])
                    .print_as_debug(),
            )
        })
        .collect();
    for (t, d) in lib.tracks.iter().zip(&defects) {
        match d {
            Defect::NoAudio => fs::remove_file(paths::to_fs(&guest, &paths::key(&t.file_path))).unwrap(),
            Defect::NoAnalysis => {
                for ext in ["DAT", "EXT"] {
                    let _ = fs::remove_file(paths::to_fs(
                        &guest,
                        &paths::key(&t.analyze_path.replace(".DAT", &format!(".{ext}"))),
                    ));
                }
            }
            _ => {}
        }
    }
    // Rows pointing outside the library, at the drive's own database files or above its root.
    let outside: Vec<String> =
        drawn::some(&tc, &["/PIONEER/rekordbox/export.pdb".to_string(), "/../escape.mp3".to_string()]);
    let pdb_path = guest.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    for (n, f) in outside.iter().enumerate() {
        ed.add_track(&NewTrack { file_size: 9, ..NewTrack::new(format!("Outside {n}"), f.clone()) }).unwrap();
    }
    ed.save(&pdb_path).unwrap();

    let src = Device::open_export(&guest).unwrap();
    let mut dst = Device::open_export(&main).unwrap();
    let opts = MergeOptions { match_mode: MatchMode::Path, ..Default::default() };
    let plan = merge::plan(&src, &dst, &opts, &Ctx::silent()).unwrap();
    let src_lib = src.library().unwrap();
    let planned: BTreeSet<String> =
        plan.new_tracks.iter().map(|t| src_lib.track(t.source_id).unwrap().file_path.clone()).collect();
    let sound: BTreeSet<String> = lib
        .tracks
        .iter()
        .zip(&defects)
        .filter(|(_, d)| **d != Defect::NoAudio)
        .map(|(t, _)| t.file_path.clone())
        .collect();
    assert_eq!(planned, sound);
    let says = |what: &str| plan.warnings.iter().filter(|w| w.contains(what)).count();
    assert_eq!(says("is missing on the source; skipped"), defects.iter().filter(|d| **d == Defect::NoAudio).count());
    assert_eq!(says("unsafe file path"), outside.len());
    let analysed = |t: &locallibrary::Track| {
        let key = paths::key(&t.analyze_path);
        key.starts_with("PIONEER/USBANLZ/")
            && ["DAT", "EXT"].iter().any(|ext| paths::to_fs(&guest, &key.replace(".DAT", &format!(".{ext}"))).is_file())
    };
    let unanalysed = lib.tracks.iter().filter(|t| sound.contains(&t.file_path) && !analysed(t)).count();
    let note = plan.warnings.iter().find(|w| w.contains("have no analysis files on the source"));
    assert_eq!(
        note.map(|w| w.starts_with(&format!("{unanalysed} track(s)"))),
        (unanalysed > 0).then_some(true),
        "{:?}",
        plan.warnings
    );
    if unanalysed > 3 {
        assert!(note.unwrap().contains(&format!("and {} more", unanalysed - 3)));
    }

    let report = merge::apply(&plan, &src, &mut dst, &opts, &Ctx::silent()).unwrap();
    assert_eq!(report.tracks_added, sound.len());
    let after = dst.library().unwrap();
    for t in after.tracks.iter().skip(2) {
        assert!(paths::to_fs(&main, &paths::key(&t.file_path)).is_file(), "{} has its audio", t.file_path);
    }
}

/// Merges refused when planning, with the target left alone: into the source itself, into a drive
/// with no export (pointing at sync), a playlist or history list name the target's export.pdb
/// can't hold, a merge folder name it can't hold, or a playlist the source doesn't have.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn a_merge_the_target_cant_take_is_refused_when_planning(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = locallibrary::device::dunce_canonicalize(dir.path());
    let long = |n: usize| "x".repeat(4000 + n);
    // Each refusal on drives of its own. For a name, the most characters past 4,000 that still
    // fit: a playlist's row is 20 bytes and the name (4,024 ASCII characters fit), a history
    // list's 4 (4,040).
    for kind in 0..6 {
        let main = common::bigger_drive(&root, &format!("Main {kind}"));
        let guest = common::demo_drive(&root, &format!("Guest {kind}"));
        let extra = tc.draw(gs::integers::<usize>().max_value(60));
        let (opts, expect, fits): (MergeOptions, &str, Option<usize>) = match kind {
            0 => (MergeOptions::default(), "same drive", None),
            1 => {
                fs::remove_file(main.join(paths::EXPORT_PDB)).unwrap();
                (MergeOptions::default(), "no rekordbox export", None)
            }
            2 => {
                onelibrary_lists(&guest, &[&long(extra)], &[]);
                (MergeOptions::default(), "too long", Some(24))
            }
            3 => {
                onelibrary_lists(&guest, &[], &[&long(extra)]);
                (MergeOptions { history: true, ..Default::default() }, "too long", Some(40))
            }
            4 => (MergeOptions { folder_name: Some(long(extra)), ..Default::default() }, "too long", Some(24)),
            _ => (MergeOptions { only_playlists: vec!["Nope".into()], ..Default::default() }, "not found", None),
        };
        let before = common::Before::take(&main);
        let dst = Device::open(&main).unwrap();
        let src = match kind {
            0 => Device::open_export(&main).unwrap(),
            _ => Device::open_export(&guest).unwrap(),
        };
        match merge::plan(&src, &dst, &opts, &Ctx::silent()) {
            Ok(_) => assert!(fits.is_some_and(|n| extra <= n), "{opts:?} was planned"),
            Err(err) => {
                assert!(fits.is_none_or(|n| extra > n), "{opts:?} fits, but: {err}");
                assert!(err.to_string().contains(expect), "{err}");
                if kind == 1 {
                    let detail = err.detail();
                    assert_eq!(detail.reason, Some(Reason::NoExport));
                    let methods: Vec<Option<&str>> = detail.actions.iter().map(|a| a.method.as_deref()).collect();
                    assert_eq!(methods, [Some("sync.plan")]);
                }
            }
        }
        before.untouched(&main).unwrap();
    }
}

/// Make `root` a OneLibrary-only drive (which takes names of any length) whose playlists and
/// history lists are `playlists` and `history`, each holding its first track.
fn onelibrary_lists(root: &Path, playlists: &[&str], history: &[&str]) {
    let mut lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    let list = |(i, name): (usize, &&str)| Playlist {
        id: u32::try_from(i).unwrap() + 1,
        parent_id: None,
        name: name.to_string(),
        is_folder: false,
        sort_order: u32::try_from(i).unwrap(),
        track_ids: vec![lib.tracks[0].id],
    };
    lib.playlists = playlists.iter().enumerate().map(list).collect();
    lib.history = history.iter().enumerate().map(list).collect();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), common::onelibrary_for(&lib).to_encrypted().unwrap()).unwrap();
    for key in [paths::EXPORT_PDB, paths::EXPORT_EXT_PDB] {
        fs::remove_file(root.join(key)).unwrap();
    }
}
