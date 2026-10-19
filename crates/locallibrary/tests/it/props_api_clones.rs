//! A write to one clone of a library and the library's other mounted clones: which are copied
//! onto, and why the rest are left alone; and the drives a library named beside them means.

use crate::common::{self, path};
use crate::props_api_drives::saved_copy;

use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api;
use locallibrary::device::dunce_canonicalize;
use locallibrary::edit::{Edit, EditOptions};
use locallibrary::paths;
use locallibrary::progress::Ctx;

/// A clone of Main as it is now at `root`, adopted under its folder's name and registered.
fn clone_of(store: &locallibrary::store::Store, main: &Path, root: &Path) -> PathBuf {
    std::fs::create_dir(root).unwrap();
    api::sync_apply(Some(store), &common::sync_pair(main, root), &Ctx::silent()).unwrap();
    common::adopt(Some(store), root, root.file_name().unwrap().to_str().unwrap());
    dunce_canonicalize(root)
}

/// Main, saved and exported to since, with clones of its library mounted, each drawn: Club, played
/// at a set since it was a clone of the state before; Copy, holding Main's library; Backup, newer
/// than Main; and Spare, holding no export. A playlist made on Main is planned to be copied onto
/// the clones that hold no more than Main, said in its summary; Backup is left alone as the newer,
/// Club as back from a set (a sync of the two keeps the players' changes), each with a warning;
/// Spare, which a sync or provision fills, isn't a clone to copy onto. Applied, the clones copied
/// onto hold Main's library and the others are as they were.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(club = true, copy = true, backup = true, spare = true)]
fn a_write_reaches_the_clones_that_are_no_newer(tc: TestCase) {
    let club = tc.draw(gs::booleans());
    let copy = tc.draw(gs::booleans());
    let backup = tc.draw(gs::booleans());
    let spare = tc.draw(gs::booleans());

    let dir = tempfile::tempdir().unwrap();
    let ctx = Ctx::silent();
    let (store, main) = saved_copy(dir.path());
    let volumes = main.parent().unwrap().to_path_buf();
    let club = club.then(|| clone_of(&store, &main, &volumes.join("Club")));
    common::export_new_track(&main, "Artist", "At home");
    api::pull(&store, &api::PullRequest { drive: path(&main), ..Default::default() }, &ctx).unwrap();
    if let Some(c) = &club {
        let ids: Vec<u32> = common::library(c).tracks.iter().map(|t| t.id).collect();
        common::add_history(c, "HISTORY 001", &ids[..1]);
    }
    let copy = copy.then(|| clone_of(&store, &main, &volumes.join("Copy")));
    let backup = backup.then(|| clone_of(&store, &main, &volumes.join("Backup")));
    if let Some(b) = &backup {
        common::export_new_track(b, "Artist", "At the club");
    }
    let spare = spare.then(|| clone_of(&store, &main, &volumes.join("Spare")));
    if let Some(s) = &spare {
        std::fs::remove_file(s.join(paths::EXPORT_PDB)).unwrap();
    }
    let before: Vec<(PathBuf, locallibrary::Library)> =
        [&club, &backup].into_iter().flatten().map(|r| (r.clone(), common::library(r))).collect();

    let req = api::EditRequest {
        drive: path(&main),
        edits: vec![Edit::CreatePlaylist { path: "Friday".into(), folder: false }],
        options: EditOptions::default(),
    };
    let plan = api::edit_plan(&req, &ctx).unwrap();
    let mut clones: Vec<(PathBuf, Option<String>)> =
        plan.clones.iter().map(|c| (c.root.clone(), c.left_alone.clone())).collect();
    clones.sort();
    let mut want: Vec<(PathBuf, Option<String>)> = Vec::new();
    if let Some(b) = &backup {
        want.push((b.clone(), Some("Backup is newer than Main: it isn't copied over".into())));
    }
    if let Some(c) = &club {
        let why = "not copying Main onto Club: one of them came back from a set (sync the two first, which keeps the \
                   players' changes in the store)";
        want.push((c.clone(), Some(why.into())));
    }
    if let Some(c) = &copy {
        want.push((c.clone(), None));
    }
    want.sort();
    assert_eq!(clones, want);
    for (_, why) in &want {
        if let Some(w) = why {
            assert!(plan.warnings.contains(w), "{w:?} in {:?}", plan.warnings);
        }
    }
    let copied_onto = plan.summary.iter().any(|l| l == "then copied onto Copy");
    assert_eq!(copied_onto, copy.is_some(), "{:?}", plan.summary);

    plan.apply(Some(&store), &ctx).unwrap();
    let friday = |r: &Path| common::library(r).playlists.iter().any(|p| p.name == "Friday");
    assert!(friday(&main));
    if let Some(c) = &copy {
        assert_eq!(common::library(c), common::library(&main), "copied onto");
    }
    for (r, lib) in before {
        assert_eq!(common::library(&r), lib, "{} left as it was", r.display());
    }
    if let Some(s) = &spare {
        assert!(!s.join(paths::EXPORT_PDB).exists(), "nothing written to Spare");
    }
}

/// A library (House: Main and its clone Copy, mounted) and its drives named by path, in any
/// order and any number of times: the drives meant are each of the library's clones once for
/// the library, where neither was named by path before it; and each drive named by path as
/// often as it's named, unless the library named before it brought it in already. Nothing is
/// left out (neither is an archive or retired drive).
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(specs = vec![0usize, 2, 1])]
fn a_library_and_its_drives_named_together_mean_each_drive_once(tc: TestCase) {
    let specs: Vec<usize> = tc.draw(gs::vecs(gs::integers::<usize>().max_value(2)).min_size(1).max_size(5));
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = saved_copy(dir.path());
    let copy = clone_of(&store, &main, &main.with_file_name("Copy"));
    api::library_rename(&store, &api::RenameLibrary { library: "Main".into(), name: "House".into() }).unwrap();
    let group = api::library_clones("House").unwrap().expect("a library").drives;
    let mut clones = group.clone();
    clones.sort();
    let mut both = vec![main.clone(), copy.clone()];
    both.sort();
    assert_eq!(clones, both, "the library's mounted clones");

    // The model: what each spec brings in, by root.
    let (mut named, mut cloned, mut want): (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) = Default::default();
    let mut asked = Vec::new();
    for s in &specs {
        match s {
            0 => {
                asked.push("House".to_string());
                for root in &group {
                    if !named.contains(root) && !cloned.contains(root) {
                        want.push(root.clone());
                        cloned.push(root.clone());
                    }
                }
            }
            _ => {
                let root = if *s == 1 { &main } else { &copy };
                asked.push(path(root));
                if !cloned.contains(root) {
                    named.push(root.clone());
                    want.push(root.clone());
                }
            }
        }
    }
    let (drives, left_out) = api::expand_libraries(&asked).unwrap();
    let got: Vec<PathBuf> = drives.iter().map(|d| dunce_canonicalize(Path::new(d))).collect();
    assert_eq!(got, want, "{asked:?}");
    assert!(left_out.is_empty());
}
