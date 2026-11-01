//! Merging one drawn drive's library into another's, with the options drawn: playlists in a
//! folder, by name or not at all, only some of them, history lists, matching by path or by
//! details, and OneLibrary and artwork on either side. Nothing the target had is lost, every
//! source track and entry chosen arrives (or is matched to one there), new tracks come with their
//! audio byte for byte, merging again changes nothing, and undoing gives the target back.

use crate::common::{self, media};
use crate::drawn;

use indexmap::IndexMap;
use std::collections::BTreeSet;
use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::merge::{self, MatchMode, MergeOptions, PlaylistMode};
use locallibrary::progress::Ctx;
use locallibrary::{Library, backup, paths};

/// Playlists by folded path, with the (folded) titles of their entries, a multiset.
fn lists(lib: &Library) -> IndexMap<String, Vec<String>> {
    let mut out: IndexMap<String, Vec<String>> = IndexMap::new();
    for p in lib.playlists.iter().filter(|p| !p.is_folder) {
        let e = out.entry(lib.playlist_path(p.id).to_lowercase()).or_default();
        e.extend(p.track_ids.iter().map(|id| lib.track(*id).unwrap().title.to_lowercase()));
        e.sort();
    }
    out
}

fn contains_all(big: &[String], small: &[String]) -> bool {
    let mut big = big.to_vec();
    small.iter().all(|s| match big.iter().position(|b| b == s) {
        Some(i) => {
            big.remove(i);
            true
        }
        None => false,
    })
}

#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn merging_loses_nothing_and_undoes_exactly(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = locallibrary::device::dunce_canonicalize(dir.path());
    let (main, _) = drawn::drive(&tc, &root, "Main");
    if tc.draw(gs::weighted_booleans(0.3)) {
        common::add_onelibrary(&main);
    }
    // The guest: rekordbox's demo export (real analysis), some of the same tracks, a few of its own.
    let guest = common::demo_drive(&root, "Guest");
    for n in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        drawn::track(&tc, &guest, 10 + n);
    }
    common::add_history(&guest, "HISTORY 001", &[1]);
    common::apply(&guest, &[locallibrary::edit::Edit::CreatePlaylist { path: "Sets/Friday".into(), folder: false }]);
    common::apply(
        &guest,
        &[locallibrary::edit::Edit::AddTracks {
            playlist: "Sets/Friday".into(),
            tracks: locallibrary::query::TrackSelector { filter: Some("bpm:>=1".into()), ..Default::default() },
            position: None,
            sort: None,
            allow_repeats: false,
        }],
    );
    if tc.draw(gs::booleans()) {
        media::set_art(&guest, &[1]);
    }
    let src = Device::open_export(&guest).unwrap();
    let src_lib = src.library().unwrap();
    let names: Vec<String> =
        src_lib.playlists.iter().filter(|p| !p.is_folder).map(|p| src_lib.playlist_path(p.id)).collect();
    let mode = tc.draw(gs::sampled_from(vec![PlaylistMode::Folder, PlaylistMode::ByName, PlaylistMode::None]));
    let opts = MergeOptions {
        playlists: mode,
        folder_name: tc.draw(gs::optional(gs::sampled_from(vec!["From Guest", "Ünï"]))).map(Into::into),
        only_playlists: drawn::some(&tc, &names),
        match_mode: tc.draw(gs::sampled_from(vec![MatchMode::Metadata, MatchMode::Path])),
        history: tc.draw(gs::booleans()),
    };

    let before = common::drive_files(&main);
    let mut dst = Device::open_export(&main).unwrap();
    let target_before = dst.library().unwrap();
    let plan = merge::plan(&src, &dst, &opts, &Ctx::silent()).unwrap();
    let report = merge::apply(&plan, &src, &mut dst, &opts, &Ctx::silent()).unwrap();
    assert_eq!((report.tracks_added, report.tracks_matched), (plan.new_tracks.len(), plan.matched_tracks));
    let after = dst.library().unwrap();

    // Nothing the target had is lost.
    for t in &target_before.tracks {
        let now = after.tracks.iter().find(|x| x.file_path == t.file_path).expect("every track stays");
        assert_eq!((&now.title, &now.artist), (&t.title, &t.artist));
    }
    let (had, has) = (lists(&target_before), lists(&after));
    for (path, entries) in &had {
        assert!(contains_all(&has[path], entries), "{path} lost entries");
    }
    assert_eq!(after.tracks.len(), target_before.tracks.len() + report.tracks_added);
    // The chosen tracks arrive: their audio, byte for byte.
    for t in &plan.new_tracks {
        let src_track = src_lib.track(t.source_id).unwrap();
        let bytes = fs::read(paths::to_fs(&guest, &paths::key(&src_track.file_path))).unwrap();
        assert_eq!(fs::read(paths::to_fs(&main, &t.audio)).unwrap(), bytes, "{}", t.name);
        assert!(after.tracks.iter().any(|x| paths::key(&x.file_path).eq_ignore_ascii_case(&t.audio)));
    }
    // ...and the chosen playlists with them, where the mode puts them.
    let chosen: BTreeSet<String> = if opts.only_playlists.is_empty() {
        names.iter().cloned().collect()
    } else {
        opts.only_playlists.iter().cloned().collect()
    };
    let folder = opts.folder_name.clone().unwrap_or_else(|| src.name());
    for (path, entries) in lists(&src_lib).iter().filter(|(p, _)| chosen.iter().any(|c| c.to_lowercase() == **p)) {
        let at = match mode {
            // A chosen playlist whose folder wasn't chosen goes straight into the merge folder.
            PlaylistMode::Folder if !opts.only_playlists.is_empty() => {
                format!("{}/{}", folder.to_lowercase(), path.rsplit('/').next().unwrap())
            }
            PlaylistMode::Folder => format!("{}/{path}", folder.to_lowercase()),
            PlaylistMode::ByName => path.clone(),
            _ => continue,
        };
        let got = has.get(&at).unwrap_or_else(|| panic!("{at} isn't in {:?}", has.keys().collect::<Vec<_>>()));
        assert!(got.len() >= entries.len(), "{at}: {got:?} for {entries:?}");
    }
    if mode == PlaylistMode::None {
        assert_eq!(has.len(), had.len(), "no playlists made");
    }
    let history_names: BTreeSet<&str> = after.history.iter().map(|h| h.name.as_str()).collect();
    if opts.history && !target_before.history.iter().any(|h| h.name == "HISTORY 001") {
        assert!(history_names.contains("HISTORY 001"));
    }

    // Again: nothing to add.
    let again = merge::plan(&src, &Device::open_export(&main).unwrap(), &opts, &Ctx::silent()).unwrap();
    assert!(again.new_tracks.is_empty(), "{:?}", again.new_tracks);
    // Undone: every file the target had is back as it was.
    backup::restore(&mut dst, report.backup.as_deref(), false).unwrap();
    let back = common::drive_files(&main);
    for (key, bytes) in &before {
        assert_eq!(back.get(key), Some(bytes), "{key} isn't restored");
    }
}
