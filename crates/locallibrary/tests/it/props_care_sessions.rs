//! Sessions gathered from the drives at hand: each history list once, whichever drives hold it.

use crate::common;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::paths;
use locallibrary::plays;
use locallibrary::sets::SetBook;
use locallibrary::sets::session;

/// Any history lists on any drives gather as one session each, naming every drive it's on in
/// the order the drives were given, recorded at some time before now (with nothing pulled, no
/// earlier bound); a drive whose OneLibrary can't be read gives what its export.pdb holds and a
/// warning naming the drive.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn every_history_list_is_one_session_naming_its_drives(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let lists: [(&str, &[u32]); 3] = [("HISTORY 001", &[1]), ("HISTORY 002", &[1, 2]), ("HISTORY 003", &[2])];
    let mut drives = Vec::new();
    let mut broken = Vec::new();
    for name in ["A", "B", "C"].into_iter().take(tc.draw(gs::integers::<usize>().min_value(1).max_value(3))) {
        let root = common::demo_drive(dir.path(), name);
        for (list, tracks) in lists {
            if tc.draw(gs::booleans()) {
                common::add_history(&root, list, tracks);
            }
        }
        if tc.draw(gs::booleans()) {
            common::add_onelibrary(&root);
            fs::write(root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();
            broken.push(name);
        }
        drives.push(Device::open_export(&root).unwrap());
    }
    let histories: Vec<plays::DriveHistory> = drives.iter().map(plays::DriveHistory::read).collect();
    let gathered = session::gather_from(None, &histories, &SetBook::default());

    // Each drive's own lists, as the drive reads them.
    let mut want: Vec<(String, Vec<String>)> = Vec::new();
    for d in &drives {
        for list in plays::drive_history(d).unwrap().lists {
            let id = list.fingerprint();
            match want.iter_mut().find(|(w, _)| *w == id) {
                Some((_, on)) => on.push(d.name()),
                None => want.push((id, vec![d.name()])),
            }
        }
    }
    assert_eq!(gathered.sessions.len(), want.len(), "{:?}", gathered.sessions);
    for (id, on) in &want {
        let s = gathered.sessions.iter().find(|s| s.id == *id).unwrap();
        assert_eq!(&s.drives, on, "{}", s.name);
        assert_eq!(s.window.after, None, "{}", s.name);
        assert!(s.window.before.is_some_and(|b| b <= chrono::Utc::now()), "{:?}", s.window);
    }
    let warned: Vec<&str> = gathered.warnings.iter().map(|w| w.split(": ").next().unwrap()).collect();
    assert_eq!(warned, broken, "{:?}", gathered.warnings);
    assert!(gathered.warnings.iter().all(|w| w.ends_with("only export.pdb's history was read")));
}

/// A list recorded since two drives were pulled, and on both (one of them given twice), is one
/// session naming each drive once, recorded after the earlier of the two pulls; it's found by its
/// list's name in any case.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_list_on_drives_pulled_apart_was_recorded_after_the_earlier_pull(tc: TestCase) {
    use locallibrary::progress::Ctx;
    use locallibrary::store::Store;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut drives = Vec::new();
    for name in ["A", "B"] {
        let root = common::demo_drive(dir.path(), name);
        let mut d = Device::open_export(&root).unwrap();
        d.adopt(Some(name)).unwrap();
        locallibrary::sync::pull(&store, &mut d, &Default::default(), &Ctx::silent()).unwrap();
        common::add_history(&root, "HISTORY 009", &[1]);
        drives.push(Device::open_export(&root).unwrap());
    }
    let archive = plays::archive(&store, None, &Ctx::silent()).unwrap();
    let pulls: Vec<_> = drives.iter().map(|d| archive.last_pulled(d).unwrap()).collect();
    let order = tc.draw(gs::sampled_from(vec![[0, 1, 0], [1, 0, 1], [0, 0, 1]]));
    let histories: Vec<plays::DriveHistory> = order.iter().map(|&i| plays::DriveHistory::read(&drives[i])).collect();
    let gathered = session::gather_from(Some(&archive), &histories, &SetBook::default());
    let [s] = gathered.sessions.as_slice() else { panic!("{:?}", gathered.sessions) };
    let mut named: Vec<usize> = Vec::new();
    for i in order {
        if !named.contains(&i) {
            named.push(i);
        }
    }
    assert_eq!(s.drives, named.iter().map(|&i| drives[i].name()).collect::<Vec<_>>());
    assert_eq!(s.window.after, pulls.iter().min().copied());
    let query = tc.draw(gs::sampled_from(vec!["history 009", "History 009"]));
    assert_eq!(session::find(&gathered.sessions, query).unwrap().id, s.id);
}
