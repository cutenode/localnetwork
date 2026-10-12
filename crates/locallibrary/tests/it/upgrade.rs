//! Upgrading a lossy track to a lossless file of its recording, found by its sound: the track
//! keeps its row (playlists, history), is pointed at the copy in both databases and its analysis
//! files, and its cues and beat grid move by the offset between the two files' audio.
//!
//! No MP3 encoder is at hand, so the "MP3" is a WAV named `.mp3` (symphonia goes by the content)
//! whose audio starts 26 ms later, as an MP3 encoder's delay would make it.

use crate::common::{self, add_track as add};

use std::fs;
use std::path::Path;

use locallibrary::anlz::{self, CueFile, CuePoint};
use locallibrary::api::{self, UpgradeRequest};
use locallibrary::cue_edit::{CueEdit, CuePosition, SetCue};
use locallibrary::device::Device;
use locallibrary::edit::{Edit, journal};
use locallibrary::progress::Ctx;
use locallibrary::upgrade::UpgradeOptions;
use locallibrary::{Error, Library, Store, cue_edit, paths, pdb, testing};
use loclib_mixid::audio::Resampler;
use loclib_mixid::fingerprint::RATE;
use loclib_mixid::test_util::{synth, wav_of};
use rekordbox_pdb::NewTrack;

pub(crate) const MP3: &str = "/Contents/Sound/Song.mp3";
const OTHER: &str = "/Contents/Sound/Other.mp3";
pub(crate) const DELAY_MS: i32 = 26;

/// `secs` of test signal `seed` at `rate`, after `delay_ms` of silence.
pub(crate) fn signal(seed: u32, secs: f64, rate: u32, delay_ms: i32) -> Vec<f32> {
    let mut pcm = vec![0.0f32; (rate as i32 * delay_ms / 1000) as usize];
    let mut resampler = Resampler::new(RATE, rate);
    resampler.push(&synth(secs, seed), &mut |s| pcm.extend_from_slice(s));
    resampler.finish(&mut |s| pcm.extend_from_slice(s));
    pcm
}

/// [`signal`] as a stereo WAV of `bits` bits.
pub(crate) fn recording(seed: u32, secs: f64, rate: u32, bits: u16, delay_ms: i32) -> Vec<u8> {
    let scale = 12_000.0 * f32::from(1u16 << (bits - 16));
    wav_of(rate, bits, signal(seed, secs, rate, delay_ms).iter().map(|s| (s * scale) as i32))
}

/// A blue detail waveform (`PWV3`, 150 columns a second) of `pcm` at 44.1 kHz as rekordbox draws
/// it when its timeline starts `lead` samples before the decoded audio.
pub(crate) fn waveform(pcm: &[f32], lead: usize) -> Vec<u8> {
    let columns: Vec<f32> = (0..pcm.len() / 294)
        .map(|i| {
            let (from, to) = ((i * 294).saturating_sub(lead), ((i + 1) * 294).saturating_sub(lead));
            pcm[from..to].iter().map(|v| v.abs()).sum::<f32>() / 294.0
        })
        .collect();
    let loudest = columns.iter().copied().fold(0.0, f32::max);
    let mut s = b"PWV3".to_vec();
    s.extend(0x18u32.to_be_bytes());
    s.extend((0x18 + columns.len() as u32).to_be_bytes());
    s.extend(1u32.to_be_bytes());
    s.extend((columns.len() as u32).to_be_bytes());
    s.extend(0x0096_0000u32.to_be_bytes());
    s.extend(columns.iter().map(|c| (c / loudest * 31.0).round() as u8));
    s
}

/// `section` added to the analysis file at `path`.
pub(crate) fn append(path: &Path, section: &[u8]) {
    let mut bytes = fs::read(path).unwrap();
    bytes.extend(section);
    let len = bytes.len() as u32;
    bytes[8..12].copy_from_slice(&len.to_be_bytes());
    fs::write(path, bytes).unwrap();
}

/// A PQTZ beat grid of beats 1-4 at 500 ms apart from 1 s.
fn grid() -> Vec<u8> {
    let mut s = b"PQTZ".to_vec();
    s.extend(24u32.to_be_bytes());
    s.extend((24u32 + 4 * 8).to_be_bytes());
    s.extend(0u32.to_be_bytes());
    s.extend(0x80000u32.to_be_bytes());
    s.extend(4u32.to_be_bytes());
    for n in 0..4u16 {
        s.extend((n + 1).to_be_bytes());
        s.extend(12000u16.to_be_bytes());
        s.extend((1000 + u32::from(n) * 500).to_be_bytes());
    }
    s
}

/// The bigger fixture plus the "MP3" (with cues and a beat grid, in a playlist and a history
/// list) and another song's, both databases; and a folder holding the song as a 48 kHz 24-bit
/// WAV whose audio starts at 0. Returns the drive, the folder and the song's id.
pub(crate) fn setup(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf, u32) {
    let root = common::bigger_drive(dir, "Main");
    let track = |path: &str, n: u32| NewTrack {
        artist: Some("Someone".into()),
        analyze_path: format!("/PIONEER/USBANLZ/P0CC/0000000{n}/ANLZ0000.DAT"),
        duration: 40,
        bitrate: 320,
        sample_rate: 44_100,
        ..NewTrack::new(path.rsplit('/').next().unwrap(), path)
    };
    let song = add(&root, track(MP3, 1), &recording(7919, 40.0, 44_100, 16, DELAY_MS));
    add(&root, track(OTHER, 2), &recording(3 * 7919, 40.0, 44_100, 16, 0));
    let dat = paths::to_fs(&root, &paths::key(&track(MP3, 1).analyze_path));
    let points = [
        CuePoint { hot_cue: Some(0), start_ms: 10_000, comment: "Drop".into(), ..Default::default() },
        CuePoint { start_ms: 20_000, ..Default::default() },
        CuePoint { hot_cue: Some(1), start_ms: 30_000, end_ms: Some(32_000), ..Default::default() },
    ];
    for (path, file) in [(dat.clone(), CueFile::Dat), (dat.with_extension("EXT"), CueFile::Ext)] {
        fs::write(&path, anlz::write_cue_points(&fs::read(&path).unwrap(), file, &points).unwrap()).unwrap();
    }
    append(&dat, &grid());
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    let set = ed.create_playlist("Set", 0, false).unwrap();
    ed.set_playlist_entries(set, &[1, song]).unwrap();
    ed.save(&pdb_path).unwrap();
    common::add_history(&root, "HISTORY 001", &[song]);
    common::add_onelibrary(&root);
    // One more cue, set as `cues set` does: OneLibrary's cue table gets rows for all four.
    let mut d = Device::open_export(&root).unwrap();
    let memory = CueEdit::Set(SetCue {
        hot_cue: None,
        at: CuePosition::Ms(25_000),
        snap: false,
        loop_beats: None,
        name: None,
        color: None,
    });
    let plan = cue_edit::plan(&d, &[song], &memory, &Ctx::silent()).unwrap();
    cue_edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let lossless = dir.join("Lossless");
    fs::create_dir_all(lossless.join("Album")).unwrap();
    fs::write(lossless.join("Album/Song.wav"), recording(7919, 40.0, 48_000, 24, 0)).unwrap();
    (root, lossless, song)
}

fn cues(root: &Path, id: u32) -> Vec<(u32, Option<u32>)> {
    let listed = cue_edit::list(&Device::open_export(root).unwrap(), &[id]).unwrap();
    let mut cues: Vec<(u32, Option<u32>)> = listed[0].cues.iter().map(|c| (c.start_ms, c.end_ms)).collect();
    cues.sort();
    cues
}

fn beats(root: &Path, lib: &Library, id: u32) -> Vec<u32> {
    let dat = fs::read(paths::to_fs(root, &paths::key(&lib.track(id).unwrap().analyze_path))).unwrap();
    anlz::beat_grid(&dat).unwrap().iter().map(|b| b.time_ms).collect()
}

pub(crate) fn store(dir: &Path) -> Store {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    store
}

#[test]
fn a_lossy_track_is_pointed_at_its_lossless_file_with_cues_and_grid_moved() {
    let dir = tempfile::tempdir().unwrap();
    let (root, lossless, song) = setup(dir.path());
    let _store = store(dir.path());
    let req = UpgradeRequest {
        drive: common::path(&root),
        options: UpgradeOptions { folders: vec![lossless], ..Default::default() },
    };

    let plan = api::upgrade_plan(&req, &Ctx::silent()).unwrap();
    let [u] = plan.upgrades.as_slice() else { panic!("{:?} {:?}", plan.upgrades, plan.not_upgraded) };
    assert_eq!((u.track.id, u.to.as_str(), u.shift_ms), (song, "Contents/Sound/Song.wav", -DELAY_MS));
    assert_eq!(plan.unmatched, 1, "the other song has no lossless file");
    // What rekordbox's next export writes again: the old row, analysis and cues.
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let dat = paths::key(&lib.track(song).unwrap().analyze_path);
    let exported: Vec<(String, Vec<u8>)> =
        [paths::EXPORT_PDB.to_string(), paths::LIBRARY_PLUS_DB.into(), dat.clone(), dat.replace(".DAT", ".EXT")]
            .into_iter()
            .map(|k| (k.clone(), fs::read(paths::to_fs(&root, &k)).unwrap()))
            .collect();
    let report = plan.apply(None, &Ctx::silent()).unwrap();
    assert!(report.backup.is_some());

    common::sqlcipher_reads(&root);
    let size = fs::metadata(root.join("Contents/Sound/Song.wav")).unwrap().len();
    assert!(root.join(&MP3[1..]).is_file(), "the old file stays unless asked");
    for lib in [pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap(), common::plus_library(&root)] {
        let t = lib.track(song).unwrap();
        assert_eq!(t.file_path, "/Contents/Sound/Song.wav");
        assert_eq!((t.file_size, t.sample_rate, t.sample_depth, t.bitrate), (size, 48_000, 24, 2304));
        assert!(lib.playlists.iter().any(|p| p.name == "Set" && p.track_ids.contains(&song)));
        assert!(lib.history.iter().any(|h| h.track_ids.contains(&song)), "history keeps it");
    }
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let moved = [(9_974, None), (19_974, None), (24_974, None), (29_974, Some(31_974))];
    assert_eq!(cues(&root, song), moved);
    assert_eq!(beats(&root, &lib, song), [974, 1474, 1974, 2474]);
    let mut rows: Vec<(u32, Option<u32>)> = Device::open_export(&root)
        .unwrap()
        .library_plus()
        .unwrap()
        .cue_rows(song)
        .unwrap()
        .iter()
        .map(|r| ((r.in_usec / 1000) as u32, r.out_usec.map(|o| (o / 1000) as u32)))
        .collect();
    rows.sort();
    assert_eq!(rows, moved, "OneLibrary's cue rows move too");
    let dat = fs::read(paths::to_fs(&root, &paths::key(&lib.track(song).unwrap().analyze_path))).unwrap();
    assert_eq!(anlz::read_path(&dat).unwrap().as_deref(), Some("/Contents/Sound/Song.wav"));

    // Recorded, so a replay after rekordbox exports the old row again does it again.
    let d = Device::open_export(&root).unwrap();
    let recorded = journal::edits(&d).unwrap();
    assert!(
        matches!(recorded.last(), Some(Edit::SetPath { path, file: Some(_), shift_ms: -26, .. }) if path == "/Contents/Sound/Song.wav"),
        "{recorded:?}"
    );
    for (key, bytes) in &exported {
        fs::write(paths::to_fs(&root, key), bytes).unwrap();
    }
    let (_, replayed) = common::replay(&root);
    assert_eq!(replayed.track(song).unwrap().file_path, "/Contents/Sound/Song.wav");
    assert_eq!(cues(&root, song), moved, "moved once, from where the export put them");
}

/// Asked to, the old files go (into the backup first); a track already upgraded isn't again.
#[test]
fn the_old_file_goes_only_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let (root, lossless, _) = setup(dir.path());
    let _store = store(dir.path());
    let options = UpgradeOptions { folders: vec![lossless.clone()], remove_old: true, ..Default::default() };
    let req = UpgradeRequest { drive: common::path(&root), options };
    let plan = api::upgrade_plan(&req, &Ctx::silent()).unwrap();
    assert_eq!(plan.removed, [&MP3[1..]]);
    plan.apply(None, &Ctx::silent()).unwrap();
    assert!(!root.join(&MP3[1..]).exists());
    let again = api::upgrade_plan(&req, &Ctx::silent()).unwrap();
    assert!(again.noop && again.upgrades.is_empty());

    let missing = UpgradeOptions { folders: vec![dir.path().join("nowhere")], ..Default::default() };
    let err = api::upgrade_plan(&UpgradeRequest { drive: common::path(&root), options: missing }, &Ctx::silent());
    assert!(matches!(err.unwrap_err().inner(), Error::NotFound(_)));
    let none = UpgradeOptions::default();
    let err = api::upgrade_plan(&UpgradeRequest { drive: common::path(&root), options: none }, &Ctx::silent());
    assert!(err.unwrap_err().to_string().contains("folders"));
}

/// A file of another recording as long as the track, or one already at the copy's path, is no
/// upgrade.
#[test]
fn only_the_same_recording_is_an_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let (root, lossless, song) = setup(dir.path());
    let _store = store(dir.path());
    fs::write(lossless.join("Album/Song.wav"), recording(5, 40.0, 48_000, 24, 0)).unwrap();
    let req = UpgradeRequest {
        drive: common::path(&root),
        options: UpgradeOptions { folders: vec![lossless.clone()], ..Default::default() },
    };
    let plan = api::upgrade_plan(&req, &Ctx::silent()).unwrap();
    assert!(plan.upgrades.is_empty() && plan.unmatched == 2, "{:?}", plan.upgrades);

    // Another file in the way: the copy takes the next free name.
    fs::write(lossless.join("Album/Song.wav"), recording(7919, 40.0, 48_000, 24, 0)).unwrap();
    fs::write(root.join("Contents/Sound/Song.wav"), b"in the way").unwrap();
    let plan = api::upgrade_plan(&req, &Ctx::silent()).unwrap();
    let [u] = plan.upgrades.as_slice() else { panic!("{:?}", plan.not_upgraded) };
    assert_eq!((u.track.id, u.to.as_str(), u.on_drive), (song, "Contents/Sound/Song (2).wav", false));

    // An upgrade undone leaves its copy: upgrading again uses it rather than refusing.
    fs::remove_file(root.join("Contents/Sound/Song.wav")).unwrap();
    let report = api::upgrade_plan(&req, &Ctx::silent()).unwrap().apply(None, &Ctx::silent()).unwrap();
    let mut d = Device::open_export(&root).unwrap();
    locallibrary::backup::restore(&mut d, report.backup.as_deref(), false).unwrap();
    assert!(root.join("Contents/Sound/Song.wav").is_file(), "undo leaves the copy");
    let plan = api::upgrade_plan(&req, &Ctx::silent()).unwrap();
    let [u] = plan.upgrades.as_slice() else { panic!("{:?}", plan.not_upgraded) };
    assert_eq!((u.to.as_str(), u.on_drive, plan.copy_bytes), ("Contents/Sound/Song.wav", true, 0));

    // The same copy as another track's file: that's never shared.
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    ed.add_track(&NewTrack::new("Song (WAV)", "/Contents/Sound/Song.wav")).unwrap();
    ed.save(&pdb_path).unwrap();
    let plan = api::upgrade_plan(&req, &Ctx::silent()).unwrap();
    let [u] = plan.upgrades.as_slice() else { panic!("{:?}", plan.not_upgraded) };
    assert_eq!((u.to.as_str(), u.on_drive), ("Contents/Sound/Song (2).wav", false));
}

/// Tracks that can't be upgraded are listed with why, and the others still are: two rows of one
/// file, a lossless file the drive's players can't play, a name FAT can't hold.
#[test]
fn each_track_that_cant_be_upgraded_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let (root, lossless, song) = setup(dir.path());
    let _store = store(dir.path());
    // The lossless file renamed with a colon and in NFD: the copy's name is made safe.
    let nfd = "Song: Live\u{0301}.wav";
    fs::rename(lossless.join("Album/Song.wav"), lossless.join("Album").join(nfd)).unwrap();
    let options = |players: &[&str]| UpgradeOptions {
        folders: vec![lossless.clone()],
        players: players.iter().map(|p| p.to_string()).collect(),
        ..Default::default()
    };
    let plan =
        |o| api::upgrade_plan(&UpgradeRequest { drive: common::path(&root), options: o }, &Ctx::silent()).unwrap();
    let p = plan(options(&[]));
    let [u] = p.upgrades.as_slice() else { panic!("{:?}", p.not_upgraded) };
    assert_eq!(u.to, "Contents/Sound/Song_ Livé.wav");
    // The CDJ-2000NXS plays WAV at 48 kHz at most.
    fs::write(lossless.join("Album").join(nfd), recording(7919, 40.0, 96_000, 24, 0)).unwrap();
    let p = plan(options(&["CDJ-2000NXS"]));
    let [n] = p.not_upgraded.as_slice() else { panic!("{:?}", p.upgrades) };
    assert_eq!((n.track.id, n.why.as_str()), (song, "the CDJ-2000NXS doesn't play it (WAV at 96000 Hz, 24 bit)"));

    // An analysis file that can't be rewritten: its cues can't move with the database's.
    fs::write(lossless.join("Album").join(nfd), recording(7919, 40.0, 48_000, 24, 0)).unwrap();
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let ext = paths::to_fs(&root, &paths::key(&lib.track(song).unwrap().analyze_path)).with_extension("EXT");
    let analysis = fs::read(&ext).unwrap();
    fs::write(&ext, b"PMAI not an analysis file").unwrap();
    let p = plan(options(&[]));
    let [n] = p.not_upgraded.as_slice() else { panic!("{:?}", p.upgrades) };
    assert!(n.why.starts_with("its analysis can't be rewritten"), "{}", n.why);
    fs::write(&ext, analysis).unwrap();

    // A second row for the MP3's file: neither is touched.
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    ed.add_track(&NewTrack::new("Song again", MP3)).unwrap();
    ed.save(&pdb_path).unwrap();
    let p = plan(options(&[]));
    assert!(p.upgrades.is_empty() && p.not_upgraded.iter().all(|n| n.why.contains("another row")), "{p:?}");
}

/// rekordbox times some MP3s from a frame (1,152 samples) before their first decoded sample:
/// which, the track's waveform tells, and their cues and grid move that much more.
#[test]
fn an_mp3_rekordbox_times_from_a_frame_early_moves_a_frame_more() {
    for (lead, shift) in [(1152, -DELAY_MS - 26), (0, -DELAY_MS)] {
        let dir = tempfile::tempdir().unwrap();
        let (root, lossless, song) = setup(dir.path());
        let _store = store(dir.path());
        let lib = Device::open_export(&root).unwrap().library().unwrap();
        let dat = paths::to_fs(&root, &paths::key(&lib.track(song).unwrap().analyze_path));
        append(&dat.with_extension("EXT"), &waveform(&signal(7919, 40.0, 44_100, DELAY_MS), lead));
        let options = UpgradeOptions { folders: vec![lossless], ..Default::default() };
        let plan = api::upgrade_plan(&UpgradeRequest { drive: common::path(&root), options }, &Ctx::silent()).unwrap();
        assert_eq!(plan.upgrades[0].shift_ms, shift, "rekordbox's timeline {lead} samples early");
    }
}

/// A real rekordbox analysis pair shifted both ways, as rekordcrate (an independent parser) reads
/// it: every beat moves (the first, at 25 ms, stops at the start rather than going), so do the
/// first and last beat the `.EXT`'s extended grid (`PQT2`) repeats, the detail waveform keeps its
/// length, and every section still frames.
#[test]
fn shifted_real_analysis_reads_back() {
    use binrw::BinRead;
    use rekordcrate::anlz::{ANLZ, Content};

    let d = common::fixtures().join("rekordcrate/demo_tracks/PIONEER/USBANLZ/P016/0000875E");
    let (dat, ext) = (fs::read(d.join("ANLZ0000.DAT")).unwrap(), fs::read(d.join("ANLZ0000.EXT")).unwrap());
    let grid = |bytes: &[u8]| -> Vec<u32> {
        let file = ANLZ::read(&mut std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(file.header.total_size as usize, bytes.len());
        let beats = file.sections.iter().find_map(|s| match &s.content {
            Content::BeatGrid(g) => Some(g.beats.iter().map(|b| b.time).collect()),
            _ => None,
        });
        beats.unwrap()
    };
    // PQT2's first and last beat times.
    let pqt2 = |bytes: &[u8]| {
        let at = bytes.windows(4).position(|w| w == b"PQT2").unwrap();
        let time = |o: usize| u32::from_be_bytes(bytes[at + o..at + o + 4].try_into().unwrap());
        (time(0x1c), time(0x24))
    };
    let columns = |bytes: &[u8]| anlz::detail(bytes).unwrap().unwrap().columns.len();
    let before = grid(&dat);
    assert_eq!((before[0], pqt2(&ext)), (25, (25, *before.last().unwrap())));
    for ms in [-30i64, 40] {
        let (dat2, ext2) = (anlz::shift_times(&dat, ms as i32).unwrap(), anlz::shift_times(&ext, ms as i32).unwrap());
        let moved: Vec<u32> = before.iter().map(|&t| (i64::from(t) + ms).max(0) as u32).collect();
        assert_eq!(grid(&dat2), moved, "{ms} ms");
        assert_eq!(pqt2(&ext2), (moved[0], *moved.last().unwrap()));
        let file = ANLZ::read(&mut std::io::Cursor::new(&ext2)).unwrap();
        assert_eq!(file.header.total_size as usize, ext2.len());
        assert_eq!(columns(&ext2), columns(&ext));
    }
}
