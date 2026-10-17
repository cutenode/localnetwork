//! What makes an edit plan destructive, so a front end says so when it asks: it loses tracks,
//! files, playlists, history lists, a playlist's entries, cues or My Tags.

use crate::common;

use locallibrary::cue_edit::{CueChange, CueRef};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions};
use locallibrary::progress::Ctx;
use locallibrary::tags::TagChange;

#[test]
fn a_plan_that_loses_something_is_destructive() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    let files: Vec<String> = d.library().unwrap().tracks.iter().map(|t| t.file_path.clone()).collect();
    let (first, second) = (files[0].clone(), files[1].clone());
    let entries = |paths: Vec<String>| Edit::SetEntries { playlist: "Two".into(), paths, create: true };
    let tag = |tracks| TagChange::Add { tag: "Genre/Deep".into(), tracks };
    let made = edit::plan(
        &d,
        &[entries(files.clone()), Edit::MyTag(tag(vec![first.clone()]))],
        &EditOptions::default(),
        &Ctx::silent(),
    );
    edit::apply(&made.unwrap(), &mut d, &Ctx::silent()).unwrap();

    let destructive =
        |edit: Edit| edit::plan(&d, &[edit], &EditOptions::default(), &Ctx::silent()).unwrap().destructive();
    assert!(!destructive(Edit::CreatePlaylist { path: "New".into(), folder: false }));
    assert!(!destructive(entries(vec![second.clone(), first.clone()])), "reordered, it keeps its entries");
    assert!(!destructive(entries(vec![first.clone(), second.clone(), first.clone()])));
    assert!(!destructive(Edit::MyTag(tag(vec![second.clone()]))));
    assert!(destructive(entries(vec![first.clone()])), "it's left with fewer entries");
    assert!(destructive(entries(Vec::new())));
    assert!(destructive(Edit::Cues { tracks: vec![first.clone()], change: CueChange::Delete(CueRef::All) }));
    assert!(destructive(Edit::MyTag(TagChange::Remove { tag: "Genre/Deep".into(), tracks: vec![first.clone()] })));
    assert!(destructive(Edit::MyTag(TagChange::Delete { tag: "Genre/Deep".into() })));
    assert!(destructive(Edit::DeletePlaylist { playlist: "Two".into(), with_tracks: false }));
}
