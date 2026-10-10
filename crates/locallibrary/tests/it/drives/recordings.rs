//! Set recordings collected off a drive with their tracklists: a history list the player kept,
//! written next to each copy as the tracks played; and collected through the service layer.

use crate::common::{self, path};

use std::path::Path;

use locallibrary::api::{self, DriveRef};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::recordings::{self, CollectOptions, Tracklist};
use locallibrary::{Error, paths};
use rekordbox_pdb::PdbEditor;

/// The demo export with two history lists (the newer playing track 2 then 1) and two
/// recordings on it.
fn drive(dir: &Path) -> Device {
    let root = common::demo_drive(dir, "AZ");
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    for (name, tracks) in [("HISTORY 001", [1, 2]), ("HISTORY 002", [2, 1])] {
        let h = ed.create_history_playlist(name).unwrap();
        for t in tracks {
            ed.add_to_history(h, t).unwrap();
        }
    }
    ed.save(&pdb).unwrap();
    std::fs::create_dir_all(root.join("ALPHATHETA REC")).unwrap();
    // REC002 ended an hour after REC001.
    let now = std::time::SystemTime::now();
    for (name, secs, ended) in
        [("REC001.WAV", 1.0, now - std::time::Duration::from_secs(3600)), ("REC002.WAV", 2.0, now)]
    {
        let path = root.join("ALPHATHETA REC").join(name);
        std::fs::write(&path, common::media::wav_of(44_100, 16, 2, secs)).unwrap();
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(ended).unwrap();
    }
    Device::open(&root).unwrap()
}

#[test]
fn the_newest_recording_gets_the_newest_history_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = drive(dir.path());
    let to = dir.path().join("Sets");
    let opts = CollectOptions { to: to.clone(), only: vec![], tracklist: Tracklist::Latest, delete: false };
    let r = recordings::collect(&mut d, &opts, &Ctx::silent()).unwrap();
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    let lists: Vec<(&str, Option<String>)> = r
        .collected
        .iter()
        .map(|c| (c.recording.as_str(), c.tracklist.as_ref().map(|t| std::fs::read_to_string(t).unwrap())))
        .collect();
    let lib = d.library().unwrap();
    let name = |id| lib.track(id).unwrap().display_name();
    let expected = format!("HISTORY 002\n\n01. {}\n02. {}\n", name(2), name(1));
    assert_eq!(lists, [("ALPHATHETA REC/REC001.WAV", None), ("ALPHATHETA REC/REC002.WAV", Some(expected))]);

    // Copied already: each says so, and nothing is overwritten.
    let again = recordings::collect(&mut d, &opts, &Ctx::silent()).unwrap();
    assert!(again.collected.is_empty() && again.failed.len() == 2);
    assert!(again.failed[0].error.contains("already exists"), "{:?}", again.failed[0]);
}

#[test]
fn a_named_history_list_goes_with_each_recording_chosen() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = drive(dir.path());
    let opts = |only: &str, tracklist| CollectOptions {
        to: dir.path().join("Sets"),
        only: vec![only.into()],
        tracklist,
        delete: false,
    };
    let named = Tracklist::Named("HISTORY 001".into());
    let r = recordings::collect(&mut d, &opts("/alphatheta rec/rec001.wav", named), &Ctx::silent()).unwrap();
    assert_eq!(r.collected.len(), 1);
    assert_eq!(r.collected[0].recording, "ALPHATHETA REC/REC001.WAV");
    assert!(std::fs::read_to_string(r.collected[0].tracklist.as_ref().unwrap()).unwrap().starts_with("HISTORY 001\n"));
    let err = recordings::collect(&mut d, &opts("REC009.WAV", Tracklist::None), &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("recording \"REC009.WAV\""), "{err}");
    let unknown = Tracklist::Named("HISTORY 009".into());
    let err = recordings::collect(&mut d, &opts("ALPHATHETA REC/REC002.WAV", unknown), &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("history list \"HISTORY 009\""), "{err}");
}

/// Recordings players made are listed, and collected as planned: the ones asked for (an unknown
/// one is refused), copied off the drive and, with `delete`, removed from it.
#[test]
fn recordings_are_collected_as_planned() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    std::fs::create_dir(root.join("PIONEER REC")).unwrap();
    for name in ["set one.wav", "set two.wav"] {
        std::fs::write(root.join("PIONEER REC").join(name), b"not much of a recording").unwrap();
    }
    let listed: Vec<String> =
        api::recordings_list(&DriveRef { drive: path(&root) }).unwrap().into_iter().map(|r| r.path).collect();
    assert_eq!(listed.len(), 2, "{listed:?}");
    let out = dir.path().join("out");
    let collect = |only: &[&str], delete| api::CollectRecordings {
        drive: path(&root),
        options: CollectOptions {
            to: out.clone(),
            only: only.iter().map(|s| s.to_string()).collect(),
            tracklist: Tracklist::None,
            delete,
        },
    };

    let err = api::recordings_collect_plan(&collect(&["/PIONEER REC/encore.wav"], false)).unwrap_err();
    assert!(matches!(&err, Error::NotFound(m) if m.contains("encore")), "{err}");
    let plan = api::recordings_collect_plan(&collect(&["/pioneer rec/SET TWO.wav"], true)).unwrap();
    assert_eq!(plan.recordings.len(), 1);
    assert!(plan.destructive);
    assert_eq!(plan.summary.len(), 2, "{:?}", plan.summary);

    let report = api::recordings_collect(&collect(&["/PIONEER REC/set two.wav"], true), &Ctx::silent()).unwrap();
    assert_eq!(report.collected.len(), 1, "{:?}", report.failed);
    assert!(report.collected[0].copied_to.starts_with(out.join("Main")));
    assert!(!root.join("PIONEER REC/set two.wav").exists());
    assert!(root.join("PIONEER REC/set one.wav").exists());
    assert!(api::recordings_collect_plan(&collect(&[], false)).unwrap().recordings.len() == 1);
}
