//! The journal of edits: read back in each format it has had (a bare list, then versioned), and
//! refused when a newer locallibrary wrote it; My Tag edits made with their own command, or as
//! edits, and a replay after a new export makes them again together with the playlist edits
//! recorded around them, under one backup.

use crate::common;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions};
use locallibrary::progress::Ctx;
use locallibrary::tags::{self, TagChange};
use locallibrary::{backup, paths};

#[hegel::test(test_cases = 40)]
fn the_journal_reads_back_in_every_format_it_has_had(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(dir.path()).unwrap();
    let file = d.state_dir().join("edits.json");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let names: Vec<String> = tc.draw(gs::vecs(gs::text().alphabet("abcXYZ /é").min_size(1).max_size(8)).max_size(3));
    let entries: Vec<serde_json::Value> = names
        .iter()
        .map(|n| {
            serde_json::json!({
                "at": "2026-10-01T12:00:00Z",
                "edits": [{"op": "create_playlist", "path": n}],
                "changes": [format!("create playlist {n}")],
            })
        })
        .collect();
    // 1 was a bare list; 2 has a version (0 or none are read as older); later ones are refused.
    let version = tc.draw(gs::optional(gs::integers::<u64>().max_value(4)));
    let stored = match (tc.draw(gs::booleans()), version) {
        (true, _) => serde_json::Value::Array(entries),
        (false, None) => serde_json::json!({ "entries": entries }),
        (false, Some(v)) => serde_json::json!({ "version": v, "entries": entries }),
    };
    let bare = stored.is_array();
    fs::write(&file, serde_json::to_vec(&stored).unwrap()).unwrap();
    match edit::journal::list(&d) {
        Ok(list) => {
            assert!(bare || version.is_none_or(|v| v <= u64::from(edit::journal::VERSION)), "version {version:?} read");
            let got: Vec<Edit> = list.into_iter().flat_map(|e| e.edits).collect();
            let want: Vec<Edit> =
                names.iter().map(|n| Edit::CreatePlaylist { path: n.clone(), folder: false }).collect();
            assert_eq!(got, want);
        }
        Err(err) => {
            assert!(!bare && version.is_some_and(|v| v > u64::from(edit::journal::VERSION)), "{err}");
            assert!(err.to_string().contains("written by a newer locallibrary"), "{err}");
        }
    }
    // Anything else isn't a journal.
    let other = tc.draw(gs::sampled_from(vec!["7", "\"edits\"", "true", "null"]));
    fs::write(&file, other).unwrap();
    assert!(edit::journal::list(&d).unwrap_err().to_string().contains("not a journal of edits"));
}

/// A tag edit for `files`, by the shape the journal records.
#[hegel::test_helper]
fn draw_tag(tc: &TestCase, files: &[String]) -> TagChange {
    let tag = tc.draw(gs::sampled_from(vec!["Genre/Deep", "Genre/Peak", "Situation/Warm"])).to_string();
    let tracks: Vec<String> = files.iter().filter(|_| tc.draw(gs::booleans())).cloned().collect();
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 | 1 if !tracks.is_empty() => TagChange::Add { tag, tracks },
        _ => TagChange::Add { tag, tracks: files[..1].to_vec() },
    }
}

#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn a_replay_makes_my_tag_and_playlist_edits_again_under_one_backup(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    if tc.draw(gs::booleans()) {
        common::add_onelibrary(&root);
    }
    let mut d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let files: Vec<String> = lib.tracks.iter().map(|t| t.file_path.clone()).collect();
    let exported: Vec<(&str, Vec<u8>)> = [paths::EXPORT_PDB, paths::EXPORT_EXT_PDB, paths::LIBRARY_PLUS_DB]
        .into_iter()
        .filter_map(|k| fs::read(root.join(k)).ok().map(|b| (k, b)))
        .collect();

    // Tag and playlist edits, in a drawn order: the replay keeps it.
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        match tc.draw(gs::booleans()) {
            true => {
                let change = draw_tag(&tc, &files);
                let TagChange::Add { tag, tracks } = &change else { unreachable!() };
                let ids: Vec<u32> =
                    tracks.iter().map(|f| lib.tracks.iter().find(|t| &t.file_path == f).unwrap().id).collect();
                let plan = tags::plan(&d, &tags::Edit::Add { tag: tag.clone(), tracks: ids }).unwrap();
                tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
            }
            false => {
                let picked: Vec<String> = files.iter().filter(|_| tc.draw(gs::booleans())).cloned().collect();
                common::apply(&root, &[Edit::SetEntries { playlist: "Shows/Set".into(), paths: picked, create: true }]);
            }
        }
    }
    let state = |d: &Device| {
        let tagged: Vec<(String, usize)> = tags::read(d)
            .unwrap()
            .categories
            .iter()
            .flat_map(|c| &c.tags)
            .map(|t| (t.path.clone(), t.tracks.len()))
            .collect();
        let lib = d.library().unwrap();
        let set = lib
            .find_playlist("Shows/Set")
            .map(|p| p.track_ids.iter().map(|id| lib.track(*id).unwrap().file_path.clone()).collect::<Vec<_>>());
        (tagged, set)
    };
    let edited = state(&d);
    let journal = edit::journal::list(&d).unwrap().len();

    // rekordbox exports again: the replay gives the drive the edits back.
    for (key, bytes) in &exported {
        fs::write(root.join(key), bytes).unwrap();
    }
    let backups = backup::list(&d).unwrap().len();
    let mut d = Device::open_export(&root).unwrap();
    let edits = edit::journal::edits(&d).unwrap();
    let plan = edit::plan(&d, &edits, &EditOptions { lenient: true, no_journal: true }, &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let d = Device::open_export(&root).unwrap();
    assert_eq!(state(&d), edited);
    assert_eq!(backup::list(&d).unwrap().len(), backups + 1, "one backup for the replay");
    assert_eq!(edit::journal::list(&d).unwrap().len(), journal, "a replay isn't recorded again");

    // Made as an edit, a My Tag edit is planned and recorded as given.
    let tag = Edit::MyTag(draw_tag(&tc, &files));
    let plan = edit::plan(&d, std::slice::from_ref(&tag), &EditOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(plan.recorded, [tag]);
}
