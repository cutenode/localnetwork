//! Pointing a track at another file (`Edit::SetPath`): one row per file, the format code from
//! the path in both databases, sizes past 4 GiB, and replays that keep journal order and back up
//! what they rewrite.

use crate::common;

use std::fs;
use std::path::Path;

use locallibrary::backup;
use locallibrary::device::Device;
use locallibrary::edit::{self, AudioFile, Edit, EditOptions};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::{anlz, paths, pdb};
use rekordbox_pdb::PdbEditor;

fn point(from: &str, to: &str, file: Option<AudioFile>, shift_ms: i32) -> Edit {
    Edit::SetPath { track: TrackSelector::paths([from.to_string()]), path: to.into(), file, shift_ms }
}

/// The bigger fixture plus an MP3 with analysis files, both databases, and a WAV beside it no
/// track uses. Returns the drive and the MP3's path.
fn drive(dir: &Path) -> (std::path::PathBuf, String) {
    let root = common::bigger_drive(dir, "Main");
    let mp3 = "/Contents/New/song.mp3";
    let track = rekordbox_pdb::NewTrack {
        analyze_path: "/PIONEER/USBANLZ/P0CD/00000001/ANLZ0000.DAT".into(),
        duration: 60,
        ..rekordbox_pdb::NewTrack::new("Song", mp3)
    };
    common::add_track(&root, track, &common::media::mp3());
    common::add_onelibrary(&root);
    fs::write(root.join("Contents/New/x.wav"), common::media::wav()).unwrap();
    (root, mp3.to_string())
}

/// Two rows for one file can't be told apart: pointing "the track" of that file is refused.
#[test]
fn a_file_with_two_rows_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (root, first) = drive(dir.path());
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    ed.add_track(&rekordbox_pdb::NewTrack::new("Again", first.clone())).unwrap();
    ed.save(&pdb_path).unwrap();
    let d = Device::open_export(&root).unwrap();
    let edits = [point(&first, "/Contents/New/x.wav", None, 0)];
    let err = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("several tracks"), "{err}");
}

/// The format code follows the path whether or not the file's details are given, and a size
/// past 4 GiB is kept whole in OneLibrary (export.pdb keeps it modulo 2^32).
#[test]
fn format_code_and_large_sizes() {
    let dir = tempfile::tempdir().unwrap();
    let (root, first) = drive(dir.path());
    let big = AudioFile { size: 5 << 30, sample_rate: 48_000, bit_depth: 24, bitrate: 2304 };
    common::apply(&root, &[point(&first, "/Contents/New/x.wav", Some(big), 0)]);
    let plus = common::plus_library(&root);
    let t = plus.tracks.iter().find(|t| t.file_path == "/Contents/New/x.wav").unwrap();
    assert_eq!(t.file_size, 5 << 30);
    let in_pdb = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    let t = in_pdb.tracks.iter().find(|t| t.file_path == "/Contents/New/x.wav").unwrap();
    assert_eq!(t.file_size, (5u64 << 30) % (1 << 32));
    assert!(rekordcrate_row(&root, "x.wav").contains("unknown6: 11,"), "WAV's code");

    // Back to an MP3 name with no details given: the code follows (1 for MP3).
    fs::write(root.join("Contents/New/y.mp3"), common::media::mp3()).unwrap();
    common::apply(&root, &[point("/Contents/New/x.wav", "/Contents/New/y.mp3", None, 0)]);
    assert!(rekordcrate_row(&root, "y.mp3").contains("unknown6: 1,"));
}

/// The row of the track whose file is `name`, as rekordcrate (an independent parser) prints it:
/// its fields are private, and the format code is its `unknown6`.
fn rekordcrate_row(root: &Path, name: &str) -> String {
    use binrw::BinRead;
    use rekordcrate::pdb::{Header, PageType};
    let bytes = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    let mut reader = std::io::Cursor::new(&bytes);
    let header = Header::read(&mut reader).unwrap();
    let table = header.tables.iter().find(|t| t.page_type == PageType::Tracks).unwrap();
    let pages = header.read_pages(&mut reader, binrw::Endian::NATIVE, (&table.first_page, &table.last_page)).unwrap();
    pages
        .iter()
        .flat_map(|p| p.row_groups.iter())
        .flat_map(|g| g.present_rows())
        .map(|r| format!("{r:?}"))
        .find(|r| r.contains(name))
        .unwrap()
}

/// A replay makes a recorded pointing and the cue edits around it in the order they were
/// recorded, backs up the analysis files the pointing rewrites (so undo puts them back), and
/// shifts the cues once however often it runs.
#[test]
fn a_replay_keeps_order_and_backs_up_what_it_rewrites() {
    use locallibrary::cue_edit::{CueChange, CuePosition, SetCue};
    let dir = tempfile::tempdir().unwrap();
    let (root, first) = drive(dir.path());
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let dat = paths::key(&lib.tracks.iter().find(|t| t.file_path == first).unwrap().analyze_path);
    let dat_file = paths::to_fs(&root, &dat);
    let points = [anlz::CuePoint { start_ms: 10_000, ..Default::default() }];
    fs::write(
        &dat_file,
        anlz::write_cue_points(&fs::read(&dat_file).unwrap(), anlz::CueFile::DatAlone, &points).unwrap(),
    )
    .unwrap();
    let set = |path: &str, ms| Edit::Cues {
        tracks: vec![path.into()],
        change: CueChange::Set(SetCue {
            hot_cue: None,
            at: CuePosition::Ms(ms),
            snap: false,
            loop_beats: None,
            name: None,
            color: None,
        }),
    };
    // As recorded: the pointing (cues 26 ms earlier), then a cue set on the new file.
    let journal = vec![point(&first, "/Contents/New/x.wav", None, -26), set("/Contents/New/x.wav", 30_000)];
    let mut d = Device::open_export(&root).unwrap();
    let opts = EditOptions { lenient: true, no_journal: true };
    let replay = |d: &mut Device| {
        let plan = edit::plan(d, &journal, &opts, &Ctx::silent()).unwrap();
        edit::apply(&plan, d, &Ctx::silent()).unwrap()
    };
    let cues = |root: &Path| -> Vec<u32> {
        let mut ms: Vec<u32> = anlz::read_cue_points(&fs::read(paths::to_fs(root, &dat)).unwrap(), None)
            .unwrap()
            .iter()
            .map(|c| c.start_ms)
            .collect();
        ms.sort();
        ms
    };
    let report = replay(&mut d);
    assert_eq!(cues(&root), [9_974, 30_000], "{:?}", report.warnings);
    // Undoing it puts the analysis file back as it was; replaying again moves the cues once.
    backup::restore(&mut d, report.backup.as_deref(), false).unwrap();
    assert_eq!(cues(&root), [10_000]);
    replay(&mut d);
    assert_eq!(cues(&root), [9_974, 30_000]);
}
