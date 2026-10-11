//! Finding missing tracks' audio: on the drive where a file was moved, in folders given, and only
//! when exactly one file fits a track.

use crate::common::{self, media};

use std::fs;
use std::path::{Path, PathBuf};

use locallibrary::api::{self, RelocateRequest};
use locallibrary::device::Device;
use locallibrary::doctor::{self, DoctorOptions, Problem, Section};
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::relocate::RelocateOptions;
use locallibrary::{anlz, backup, paths, pdb};
use rekordbox_pdb::NewTrack;

/// A tone track whose audio is `secs` long at `rate` Hz (mono, 16-bit), and its file's size.
fn tone(title: &str, path: &str, secs: u16) -> NewTrack {
    NewTrack {
        duration: secs,
        analyze_path: format!("/PIONEER/USBANLZ/P0CC/{title}/ANLZ0000.DAT"),
        ..NewTrack::new(title, path)
    }
}

fn at(root: &Path, device_path: &str) -> PathBuf {
    paths::to_fs(root, &paths::key(device_path))
}

fn put(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// The tracks still missing their audio on the drive at `root`.
fn missing(root: &Path) -> Vec<u32> {
    let opts = DoctorOptions { only: vec![Section::Missing], ..Default::default() };
    let r = doctor::run(&Device::open_export(root).unwrap(), &opts, &Ctx::silent()).unwrap();
    r.missing.unwrap().iter().filter(|b| b.problem == Problem::MissingAudio).map(|b| b.track.id).collect()
}

#[test]
fn missing_files_are_found_on_the_drive_and_in_folders_when_one_fits() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let music = dir.path().join("Music");
    // Two seconds as 8 kHz audio and one second as 16 kHz: the same size, not the same length.
    let (two, one) = (media::wav_of(8_000, 16, 1, 2.0), media::wav_of(16_000, 16, 1, 1.0));
    assert_eq!(two.len(), one.len());
    let moved = common::add_track(&root, tone("Moved", "/Contents/Tones/Moved.wav", 2), &two);
    let copied = common::add_track(&root, tone("Copied", "/Contents/Tones/Copied.wav", 2), &two);
    let sized = common::add_track(&root, tone("Sized", "/Contents/Tones/Sized.wav", 2), &two);
    let timed = common::add_track(&root, tone("Timed", "/Contents/Tones/Timed.wav", 2), &two);
    let twins = common::add_track(&root, tone("Twins", "/Contents/Tones/Twins.wav", 2), &two);
    let gone = common::add_track(&root, tone("Gone", "/Contents/Tones/Gone.wav", 2), &two);
    common::add_onelibrary(&root);
    for t in ["Moved", "Copied", "Sized", "Timed", "Twins", "Gone"] {
        fs::remove_file(at(&root, &format!("/Contents/Tones/{t}.wav"))).unwrap();
    }
    // Moved on the drive, its name's letter case changed on the way.
    put(&at(&root, "/Contents/Elsewhere/MOVED.wav"), &two);
    put(&music.join("a/Copied.wav"), &two);
    // Of two, one has the size the database recorded.
    put(&music.join("a/Sized.wav"), &two);
    put(&music.join("b/Sized.wav"), &[two.as_slice(), b"tagged"].concat());
    // Of two of one size, one has the track's length.
    put(&music.join("a/Timed.wav"), &two);
    put(&music.join("b/Timed.wav"), &one);
    // Two the same: neither is picked.
    put(&music.join("a/Twins.wav"), &two);
    put(&music.join("b/Twins.wav"), &two);
    // Never mistaken for audio, nor looked into: notes, OS clutter, a backup on the drive.
    put(&music.join("a/Gone.txt"), b"notes");
    put(&music.join("._Gone.wav"), &two);
    put(&root.join(".locallibrary/backups/1/Contents/Tones/Gone.wav"), &two);
    assert!([moved, copied, sized, timed, twins, gone].iter().all(|id| missing(&root).contains(id)));

    // The drive itself too: its own files are looked through once, as the drive's.
    let folders = vec![music.clone(), root.clone()];
    let req = RelocateRequest { drive: common::path(&root), options: RelocateOptions { folders } };
    let plan = api::relocate_plan(&req, &Ctx::silent()).unwrap();
    let ids = |list: &[locallibrary::relocate::Relocation]| list.iter().map(|r| r.track.id).collect::<Vec<_>>();
    assert_eq!(ids(&plan.repointed), [moved]);
    assert_eq!(plan.repointed[0].file.path, "Contents/Elsewhere/MOVED.wav");
    let mut copies = ids(&plan.copied);
    copies.sort();
    assert_eq!(copies, [copied, sized, timed]);
    let source = |id| PathBuf::from(&plan.copied.iter().find(|r| r.track.id == id).unwrap().file.path);
    assert_eq!(source(sized), music.join("a/Sized.wav"));
    assert_eq!(source(timed), music.join("a/Timed.wav"));
    assert_eq!(plan.ambiguous.len(), 1);
    assert_eq!(plan.ambiguous[0].track.id, twins);
    assert_eq!(plan.ambiguous[0].candidates.len(), 2);
    assert!(plan.not_found.iter().any(|t| t.id == gone));
    assert!(!plan.noop);

    let report = plan.apply(None, &Ctx::silent()).unwrap();
    let still = missing(&root);
    assert!(still.contains(&twins) && still.contains(&gone), "{still:?}");
    assert!(![moved, copied, sized, timed].iter().any(|id| still.contains(id)), "{still:?}");
    // Pointed at where it is, in both databases and its analysis files.
    let lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    let t = lib.track(moved).unwrap();
    assert_eq!(t.file_path, "/Contents/Elsewhere/MOVED.wav");
    let plus = OneLibrary::open_root(&root).unwrap().library().unwrap();
    assert!(plus.tracks.iter().any(|p| p.file_path == "/Contents/Elsewhere/MOVED.wav"));
    for ext in ["DAT", "EXT"] {
        let bytes = fs::read(at(&root, &t.analyze_path.replace("DAT", ext))).unwrap();
        assert_eq!(anlz::read_path(&bytes).unwrap().as_deref(), Some("/Contents/Elsewhere/MOVED.wav"));
    }
    // Copied to the path the track has, which the databases still say.
    assert_eq!(fs::read(at(&root, "/Contents/Tones/Timed.wav")).unwrap(), two);
    assert_eq!(lib.track(timed).unwrap().file_path, "/Contents/Tones/Timed.wav");
    assert!(music.join("b/Timed.wav").is_file(), "copied, not moved");

    // Undo points the moved track back (forced: the tracks it points at a missing file were
    // missing before too).
    let mut d = Device::open_export(&root).unwrap();
    backup::restore(&mut d, report.backup.as_deref(), true).unwrap();
    let lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    assert_eq!(lib.track(moved).unwrap().file_path, "/Contents/Tones/Moved.wav");
}

/// One file two tracks would each take is neither's; a folder that isn't there is an error;
/// with nothing missing there's nothing to do.
#[test]
fn a_file_two_tracks_could_be_is_neithers() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let audio = media::wav();
    let a = common::add_track(&root, NewTrack::new("Intro", "/Contents/One/Intro.wav"), &audio);
    let b = common::add_track(&root, NewTrack::new("Intro", "/Contents/Two/Intro.wav"), &audio);
    let req = |folders| RelocateRequest { drive: common::path(&root), options: RelocateOptions { folders } };
    assert!(api::relocate_plan(&req(Vec::new()), &Ctx::silent()).unwrap().noop);

    for t in ["One", "Two"] {
        fs::remove_file(at(&root, &format!("/Contents/{t}/Intro.wav"))).unwrap();
    }
    let music = dir.path().join("Music");
    put(&music.join("Intro.wav"), &audio);
    let plan = api::relocate_plan(&req(vec![music.clone()]), &Ctx::silent()).unwrap();
    assert!(plan.noop && plan.copied.is_empty());
    let mut torn: Vec<u32> = plan.ambiguous.iter().map(|x| x.track.id).collect();
    torn.sort();
    assert_eq!(torn, [a, b]);
    assert!(plan.warnings.iter().any(|w| w.contains("could be the file of several tracks")), "{:?}", plan.warnings);

    let err = api::relocate_plan(&req(vec![dir.path().join("Nowhere")]), &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("Nowhere"), "{err}");
}

/// A lone file of a track's name that isn't as long as the track doesn't fit it, and is listed;
/// one for a track kept outside `Contents/` isn't copied there.
#[test]
fn what_relocating_cant_vouch_for_is_said() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let two = media::wav_of(8_000, 16, 1, 2.0);
    let long = common::add_track(&root, tone("Long", "/Contents/Tones/Long.wav", 9), &two);
    common::add_track(&root, tone("Outside", "/Music/Outside.wav", 2), &two);
    fs::remove_file(at(&root, "/Contents/Tones/Long.wav")).unwrap();
    fs::remove_file(at(&root, "/Music/Outside.wav")).unwrap();
    let music = dir.path().join("Music");
    put(&music.join("Long.wav"), &two);
    put(&music.join("Outside.wav"), &two);
    let req = RelocateRequest { drive: common::path(&root), options: RelocateOptions { folders: vec![music] } };
    let plan = api::relocate_plan(&req, &Ctx::silent()).unwrap();
    assert!(plan.copied.is_empty() && plan.noop, "{:?}", plan.copied);
    assert_eq!(plan.unfit.iter().map(|a| (a.track.id, a.candidates.len())).collect::<Vec<_>>(), [(long, 1)]);
    let said = |what: &str| plan.warnings.iter().any(|w| w.contains(what));
    assert!(said("Outside's file belongs outside Contents/"), "{:?}", plan.warnings);
}

/// Pointing a track at another file is an edit like the others: the file must be audio on the
/// drive that no other track has.
#[test]
fn set_path_refuses_what_isnt_a_free_audio_file_on_the_drive() {
    use locallibrary::edit::{self, Edit, EditOptions};
    use locallibrary::query::TrackSelector;
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let point = |path: &str| {
        let track = TrackSelector::paths([lib.tracks[0].file_path.clone()]);
        edit::plan(
            &d,
            &[Edit::SetPath { track, path: path.into(), file: None, shift_ms: 0 }],
            &EditOptions::default(),
            &Ctx::silent(),
        )
    };
    for bad in ["/Contents/nothing.wav", "/PIONEER/rekordbox/export.pdb", "/../outside.wav"] {
        assert!(point(bad).is_err(), "{bad}");
    }
    assert!(point(&lib.tracks[1].file_path).unwrap_err().to_string().contains("is the audio file of"));
    put(&at(&root, "/Contents/New/x.wav"), &media::wav());
    let plan = point("/Contents/New/x.wav").unwrap();
    assert!(!plan.is_noop() && plan.changes[0].contains("/Contents/New/x.wav"), "{:?}", plan.changes);
    assert!(plan.warnings.iter().all(|w| !w.contains("next sync")), "{:?}", plan.warnings);
}
