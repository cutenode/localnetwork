//! A drive's own records: an operation cut off on it says what it was and what finishes it,
//! whichever it was; its lineage keeps the newest states without repeats; and rekordbox's
//! playlist sync files read back what they say.

use crate::common;

use chrono::Utc;
use hegel::generators as gs;
use hegel::{Generator, TestCase};
use locallibrary::Hash;
use locallibrary::device::{Device, DeviceState, LineageEntry, LineageOp, RekordboxSync};

const OPS: &[LineageOp] = &[
    LineageOp::Observed,
    LineageOp::Pull,
    LineageOp::Push,
    LineageOp::Sync,
    LineageOp::Merge,
    LineageOp::Artwork,
    LineageOp::Restore,
    LineageOp::Clean,
    LineageOp::Convert,
    LineageOp::Edit,
    LineageOp::Pack,
    LineageOp::Export,
    LineageOp::Repair,
    LineageOp::Other,
];

#[hegel::test(test_cases = 60)]
fn a_cut_off_operation_says_what_finishes_it(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::blank(dir.path(), "USB");
    let op = tc.draw(gs::sampled_from(OPS.to_vec()).print_as_debug());
    let description = tc.draw(gs::sampled_from(vec!["", "sync from Main", "Ünï"]));
    let backup = tc.draw(gs::optional(gs::sampled_from(vec!["20261011T120000Z"]))).map(String::from);
    let mut d = Device::open(&root).unwrap();
    d.begin(op, description, backup.clone()).unwrap();
    let d = Device::open(&root).unwrap();
    let p = d.state().unwrap().pending.clone().expect("marked as interrupted");
    let said = p.describe();
    assert!(said.contains("didn't finish (started "), "{said}");
    assert_eq!(said.contains(&format!("({description})")), !description.is_empty(), "{said}");
    let generic = matches!(op, LineageOp::Edit | LineageOp::Observed | LineageOp::Other);
    assert_eq!(said.starts_with("a locallibrary operation"), generic, "{said}");
    let remedy = p.remedy();
    let copies = matches!(op, LineageOp::Sync | LineageOp::Push | LineageOp::Pack | LineageOp::Export);
    assert_eq!(remedy.contains("undo"), !copies && backup.is_some(), "{remedy}");
    // Its library isn't read until it's finished, and the refusal says what to do.
    let refused = d.finished().unwrap_err();
    assert!(refused.to_string().contains(remedy) && refused.to_string().contains(&said), "{refused}");
    let mut d = d;
    d.finish(None).unwrap();
    assert!(Device::open(&root).unwrap().finished().is_ok());
}

#[hegel::test(test_cases = 200)]
fn the_lineage_keeps_the_newest_states_without_repeats(tc: TestCase) {
    let entry = |n: u8| LineageEntry {
        fingerprint: Hash::of(&[n]),
        at: Utc::now(),
        op: LineageOp::Sync,
        snapshot: None,
        source: None,
        tracks: 0,
    };
    let mut s = DeviceState::new("USB");
    let mut model: Vec<u8> = Vec::new();
    for n in tc.draw(gs::vecs(gs::integers::<u8>().max_value(5)).max_size(300)) {
        s.record(entry(n));
        if model.last() != Some(&n) {
            model.push(n);
        }
        let held: Vec<Hash> = s.lineage.iter().map(|e| e.fingerprint).collect();
        let tail: Vec<Hash> = model.iter().rev().take(held.len()).rev().map(|n| Hash::of(&[*n])).collect();
        assert_eq!(held, tail, "the newest, in order");
        assert!(held.len() == model.len() || held.len() == 256, "capped at 256, never short");
        assert_eq!(s.head().map(|h| h.fingerprint), model.last().map(|n| Hash::of(&[*n])));
    }
    // Taking over another drive's history ends with the state it now holds, once.
    let source: Vec<LineageEntry> = (0..tc.draw(gs::integers::<u8>().max_value(4))).map(entry).collect();
    let last = tc.draw(gs::integers::<u8>().max_value(5));
    s.inherit_with(&source, entry(last));
    assert_eq!(s.head().unwrap().fingerprint, Hash::of(&[last]));
    let repeats = s.lineage.windows(2).filter(|w| w[0].fingerprint == w[1].fingerprint).count();
    assert_eq!(repeats, 0);
}

#[hegel::test(test_cases = 200)]
fn rekordbox_sync_files_read_back(tc: TestCase) {
    let automatic = tc.draw(gs::booleans());
    let id = tc.draw(gs::integers::<i32>());
    let nodes = tc.draw(gs::integers::<usize>().max_value(5));
    let mut xml = format!(
        r#"<?xml version="1.0"?><Sync DBID="{id}" AutomaticSync="{}" Ver="1"><Playlists>"#,
        u8::from(automatic)
    );
    for n in 0..nodes {
        xml.push_str(&format!(r#"<NODE Id="{n}" Name="List {n}"/>"#));
    }
    xml.push_str("</Playlists></Sync>");
    let s = RekordboxSync::parse("onelibrary", &xml).unwrap();
    assert_eq!((s.automatic, s.collection, s.playlists, s.library), (automatic, Some(id as u32), nodes, "onelibrary"));
    assert!(RekordboxSync::parse("onelibrary", &xml.replace("<Sync ", "<Other ")).is_none());
}

/// A drive's volume name is its folder's own name, wherever the folder is; at the top of the file
/// system, where the folder has no name of its own, it's the whole path.
#[hegel::test(test_cases = 50)]
fn a_volumes_name_is_its_folders(tc: TestCase) {
    let name = tc.draw(gs::text().alphabet("abcXYZ 09-_.é♪").min_size(1).max_size(12));
    // Windows drops a folder name's trailing dots, so `0.` makes the folder `0`.
    tc.assume(!name.trim_matches('.').is_empty() && name.trim() == name && !name.ends_with('.'));
    let dir = tempfile::tempdir().unwrap();
    let mut at = dir.path().to_path_buf();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        at.push("deeper");
    }
    at.push(&name);
    std::fs::create_dir_all(&at).unwrap();
    let d = Device::open(&at).unwrap();
    assert_eq!(d.volume_name(), name);
    let top = Device::open(std::path::Path::new("/")).unwrap();
    assert_eq!(top.volume_name(), top.root().display().to_string());
}
