//! Exports for other DJ software of a library with what each format has to work around: two
//! track rows for one file, a FLAC file, a colour and an album, two playlists of one name, and
//! a hot cue past H.

use crate::common;

use std::path::{Path, PathBuf};

use locallibrary::anlz::{self, CueFile, CuePoint};
use locallibrary::device::Device;
use locallibrary::export::PathStyle;
use locallibrary::interop::{self, InteropOptions, Software};
use locallibrary::paths;
use locallibrary::progress::Ctx;
use rekordbox_pdb::{NewTrack, PdbEditor};

const FLAC: &str = "/Contents/Other/lossless.flac";

/// The demo export, plus a second row for track 1's file, a red FLAC track 3 of an album in
/// two playlists both called "Set", and hot cue I on track 1.
fn drive(dir: &Path) -> PathBuf {
    let root = common::demo_drive(dir, "Main");
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    let one = ed.database().unwrap().tracks.iter().find(|t| t.id == 1).unwrap().file_path().to_string();
    ed.add_track(&NewTrack::new("Again", one)).unwrap();
    let flac = ed
        .add_track(&NewTrack {
            album: Some("Album".into()),
            track_number: 3,
            color_id: 2,
            file_size: 4,
            ..NewTrack::new("Lossless", FLAC)
        })
        .unwrap();
    for _ in 0..2 {
        let set = ed.create_playlist("Set", 0, false).unwrap();
        ed.add_to_playlist(set, flac, None).unwrap();
    }
    ed.save(&pdb).unwrap();
    std::fs::create_dir_all(paths::to_fs(&root, "Contents/Other")).unwrap();
    std::fs::write(paths::to_fs(&root, &paths::key(FLAC)), b"fLaC").unwrap();

    let d = Device::open(&root).unwrap();
    let dat = d.path(&paths::key(&d.library().unwrap().track(1).unwrap().analyze_path));
    let cues = [CuePoint { hot_cue: Some(8), start_ms: 1_000, ..Default::default() }];
    for (path, kind) in [(dat.with_extension("EXT"), CueFile::Ext), (dat, CueFile::Dat)] {
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, anlz::write_cue_points(&bytes, kind, &cues).unwrap()).unwrap();
    }
    root
}

fn warned(plan: &interop::InteropPlan, what: &str) -> bool {
    plan.warnings.iter().any(|w| w.contains(what))
}

#[test]
fn traktor_gets_one_entry_a_file_with_its_colour_and_album() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(drive(dir.path())).unwrap();
    let out = dir.path().join("traktor");
    let plan = interop::apply(&d, Software::Traktor, &out, &InteropOptions::default(), &Ctx::silent()).unwrap();
    assert!(warned(&plan, "1 hot cue(s) past H left out"), "{:?}", plan.warnings);
    assert!(warned(&plan, "1 track row(s) share a file with another"), "{:?}", plan.warnings);
    let nml = std::fs::read_to_string(&plan.files[0].path).unwrap();
    assert_eq!(nml.matches("<ENTRY ").count(), 3);
    let flac = nml.split("<ENTRY ").find(|e| e.contains("lossless.flac")).unwrap();
    assert!(flac.contains(r#"<ALBUM TRACK="3" TITLE="Album">"#), "{flac}");
    assert!(flac.contains(r#" COLOR="1""#), "red is Traktor's first colour: {flac}");
}

#[test]
fn serato_copies_each_file_once_and_tags_only_what_it_can() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(drive(dir.path())).unwrap();
    // MP3 audio Serato's tag can go into, for the demo tracks' placeholders.
    for t in d.library().unwrap().tracks.iter().filter(|t| t.file_path.ends_with(".mp3")) {
        std::fs::write(d.path(&paths::key(&t.file_path)), common::media::mp3()).unwrap();
    }
    let opts = InteropOptions { write_tags: true, ..Default::default() };
    let plan = interop::plan(&d, Software::Serato, &dir.path().join("serato"), &opts, &Ctx::silent()).unwrap();
    let copies: Vec<&str> = plan.files.iter().map(|f| f.what.as_str()).filter(|w| w.starts_with("copy of")).collect();
    assert_eq!(copies.len(), 3, "{:#?}", plan.files);
    assert_eq!(copies.iter().filter(|w| w.ends_with("with Serato's cue tag")).count(), 2, "not the FLAC file");
    assert!(warned(&plan, "1 track(s) aren't MP3, AIFF or WAV"), "{:?}", plan.warnings);
}

#[test]
fn engine_names_same_named_playlists_apart_and_takes_relative_paths_only() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(drive(dir.path())).unwrap();
    let out = dir.path().join("engine");
    let opts = InteropOptions { skip_analysis: true, ..Default::default() };
    let plan = interop::apply(&d, Software::Engine, &out, &opts, &Ctx::silent()).unwrap();
    assert!(warned(&plan, "1 track row(s) share a file with another and are one Engine track"));
    assert!(plan.notes.iter().any(|n| n.starts_with("--no-analysis changes nothing")), "{:?}", plan.notes);
    let db = rusqlite::Connection::open(out.join("Engine Library/Database2/m.db")).unwrap();
    let mut titles: Vec<String> = db
        .prepare("SELECT title FROM Playlist")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    titles.sort();
    assert_eq!(titles, ["Set", "Set (2)"]);
    let tracks: i64 = db.query_row("SELECT count(*) FROM Track", [], |r| r.get(0)).unwrap();
    assert_eq!(tracks, 3);

    let absolute = InteropOptions { paths: PathStyle::Absolute, ..Default::default() };
    let err = interop::plan(&d, Software::Engine, &out, &absolute, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("Engine Library paths are relative"), "{err}");
}

/// Options a format has no use for are refused, saying which format.
#[test]
fn options_a_format_cant_use_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    let out = dir.path().join("out");
    let refused = |software, opts: InteropOptions| {
        interop::plan(&d, software, &out, &opts, &Ctx::silent()).unwrap_err().to_string()
    };
    let tags = InteropOptions { write_tags: true, ..Default::default() };
    assert_eq!(refused(Software::Engine, tags), "Engine DJ keeps cue points in its own files: tags aren't written");
    let drive = InteropOptions { paths: PathStyle::Drive, ..Default::default() };
    assert_eq!(refused(Software::Serato, drive), "Serato locates tracks by their full path");
}
