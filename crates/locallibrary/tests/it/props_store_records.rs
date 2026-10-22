//! The store's records against a model: libraries renamed (names unique ignoring case), labels
//! put on and taken off snapshots (a label names one snapshot), chains of snapshots walked from
//! the head, ancestry, pruning that keeps heads, labels and the history through what it removes,
//! and the profiles kept beside them.

use indexmap::IndexMap;
use std::collections::BTreeSet;

use chrono::{TimeZone, Utc};
use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::equipment::Remembered;
use locallibrary::model::LibrarySummary;
use locallibrary::paths::FileClass;
use locallibrary::smart::{SmartPlaylist, SmartPlaylists};
use locallibrary::store::{Snapshot, Store};
use locallibrary::tree::FileEntry;
use locallibrary::{Error, Hash};

const NAMES: &[&str] = &["Main", "main", "Club", " Club ", "Ünï", ""];
const LABELS: &[&str] = &["before NYE", "BEFORE nye", "Tokyo", " ", "keep"];

#[hegel::test(test_cases = 30, suppress_health_check = [HealthCheck::TooSlow])]
fn the_stores_records_match_the_model(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let lib = store.create_library("Main").unwrap();
    let other = store.create_library("Other").unwrap();
    let object = store.import_bytes(b"audio").unwrap();
    // A chain of snapshots (each the child of the one before, some of an earlier one).
    let mut chain: Vec<Hash> = Vec::new();
    let mut parents: IndexMap<Hash, Vec<Hash>> = IndexMap::new();
    for n in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
        let parent = match (chain.last(), tc.draw(gs::weighted_booleans(0.2))) {
            (Some(_), true) => vec![tc.draw(gs::sampled_from(&chain))],
            (Some(last), false) => vec![*last],
            (None, _) => vec![],
        };
        let snap = Snapshot {
            id: Hash::of(b"set on save"),
            library_id: lib.id,
            parents: parent.clone(),
            created: Utc.timestamp_opt(1_790_000_000 + 3600 * n as i64, 0).unwrap(),
            message: None,
            source: None,
            fingerprint: Hash::of(&[n as u8]),
            summary: LibrarySummary::default(),
            lineage: Vec::new(),
            files: vec![FileEntry {
                path: "Contents/a.mp3".into(),
                size: 5,
                mtime: 0,
                class: FileClass::Audio,
                hash: Some(object),
            }],
        };
        let saved = store.save_snapshot(snap).unwrap();
        parents.insert(saved.id, parent);
        chain.push(saved.id);
    }
    let head = *chain.last().unwrap();
    store.set_head(lib.id, head).unwrap();
    // The history from the head, by first parents.
    let mut want = vec![head];
    while let Some(p) = parents[want.last().unwrap()].first() {
        want.push(*p);
    }
    let history: Vec<Hash> =
        store.history(&store.library_by_id(lib.id).unwrap().unwrap(), None).unwrap().iter().map(|s| s.id).collect();
    assert_eq!(history, want);
    for a in &chain {
        for d in &chain {
            let mut reach = vec![*d];
            let mut seen = BTreeSet::new();
            while let Some(x) = reach.pop() {
                if seen.insert(x) {
                    reach.extend(parents[&x].iter().copied());
                }
            }
            assert_eq!(store.is_ancestor(a, d).unwrap(), seen.contains(a));
        }
    }

    // Library names and labels, in any order.
    let mut names: IndexMap<uuid::Uuid, String> =
        IndexMap::from([(lib.id, "Main".to_string()), (other.id, "Other".to_string())]);
    let mut labels: IndexMap<String, (String, Hash)> = IndexMap::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(8)) {
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => {
                let id = if tc.draw(gs::booleans()) { lib.id } else { other.id };
                let name = tc.draw(gs::sampled_from(NAMES));
                let taken = names.iter().any(|(o, n)| *o != id && n.eq_ignore_ascii_case(name.trim()));
                match store.rename_library(id, name) {
                    Ok(()) => {
                        assert!(!name.trim().is_empty() && !taken);
                        names[&id] = name.trim().to_string();
                    }
                    Err(e) => assert!(name.trim().is_empty() || (taken && matches!(e.inner(), Error::Conflict(_)))),
                }
            }
            1 => {
                let label = tc.draw(gs::sampled_from(LABELS));
                let on = tc.draw(gs::sampled_from(&chain));
                let key = label.trim().to_lowercase();
                match store.add_label(&on, label) {
                    Ok(l) => {
                        assert!(!key.is_empty());
                        assert!(labels.get(&key).is_none_or(|(_, s)| *s == on), "a label names one snapshot");
                        labels.entry(key).or_insert((l.name.clone(), on));
                    }
                    Err(e) => assert!(key.is_empty() || labels.get(&key).is_some_and(|(_, s)| *s != on), "{e}"),
                }
            }
            _ => {
                let label = tc.draw(gs::sampled_from(LABELS));
                let r = store.remove_label(label);
                assert_eq!(r.is_ok(), labels.shift_remove(&label.trim().to_lowercase()).is_some());
            }
        }
        let held: IndexMap<uuid::Uuid, String> =
            store.libraries().unwrap().into_iter().map(|l| (l.id, l.name)).collect();
        assert_eq!(held, names);
        let by: IndexMap<String, Hash> =
            store.labels().unwrap().into_iter().map(|l| (l.name.to_lowercase(), l.snapshot)).collect();
        assert_eq!(by, labels.iter().map(|(k, (_, s))| (k.clone(), *s)).collect::<IndexMap<_, _>>());
        for (k, (_, s)) in &labels {
            assert_eq!(store.resolve_snapshot(&k.to_uppercase()).unwrap(), *s, "labels resolve in any case");
        }
    }

    // Pruning keeps the head and labelled snapshots, and history walks through what it removed.
    let kept: BTreeSet<Hash> = labels.values().map(|(_, s)| *s).chain([head]).collect();
    let doomed: Vec<Hash> = chain.iter().copied().filter(|s| !kept.contains(s) && tc.draw(gs::booleans())).collect();
    if let Some(k) = kept.iter().next() {
        assert!(store.forget_snapshots(&[*k]).is_err(), "a head or labelled snapshot stays");
    }
    store.forget_snapshots(&doomed).unwrap();
    let history: Vec<Hash> =
        store.history(&store.library_by_id(lib.id).unwrap().unwrap(), None).unwrap().iter().map(|s| s.id).collect();
    assert_eq!(history, want.iter().copied().filter(|s| !doomed.contains(s)).collect::<Vec<_>>());
    for a in &chain {
        assert!(store.is_ancestor(a, &head).unwrap() || !want.contains(a));
    }
    let stats = store.stats().unwrap();
    assert_eq!((stats.libraries, stats.snapshots, stats.objects), (2, chain.len() - doomed.len(), 1));

    // What's kept beside the snapshots reads back as saved.
    let mut smart = SmartPlaylists::default();
    smart
        .set(SmartPlaylist {
            path: "Smart/Peak".into(),
            filter: "bpm:>=128".into(),
            sort: None,
            limit: Some(3),
            drives: vec![],
        })
        .unwrap();
    store.save_smart_playlists(&smart).unwrap();
    assert_eq!(store.smart_playlists().unwrap(), smart);
    let mut remembered = Remembered::default();
    remembered.libraries.insert(lib.id, vec!["CDJ-3000".into()]);
    store.save_player_models(&remembered).unwrap();
    assert_eq!(store.player_models().unwrap().for_library(Some(lib.id)).0, ["CDJ-3000".to_string()]);
}
