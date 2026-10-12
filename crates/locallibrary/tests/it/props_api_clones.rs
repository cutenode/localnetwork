//! A write to one clone of a library and the library's other mounted clones: which are copied
//! onto, and why the rest are left alone.

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
