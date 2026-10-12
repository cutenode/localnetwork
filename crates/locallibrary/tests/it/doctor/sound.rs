//! The same recording in another file or encoding, found by its sound: a 48 kHz, 24-bit copy
//! that starts 26 ms later (as an MP3's encoder delay would) is a duplicate, an edit with 16 s cut
//! out isn't, and replacing a copy carries its cues onto a keeper that has none, moved by the
//! offset between them.

use crate::common::{self, add_track as add};

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use locallibrary::anlz::{self, CueFile, CuePoint};
use locallibrary::api::{self, DedupeRequest};
use locallibrary::device::Device;
use locallibrary::doctor::{self, DoctorOptions, DuplicateReason, Section};
use locallibrary::edit::{Edit, journal};
use locallibrary::progress::{CancelToken, Ctx, Event, Phase, Progress};
use locallibrary::{Error, Store, backup, cue_edit, paths, testing};
use loclib_mixid::audio::Resampler;
use loclib_mixid::fingerprint::RATE;
use loclib_mixid::test_util::{synth, wav_of};
use rekordbox_pdb::NewTrack;

pub(crate) const ORIGINAL: &str = "/Contents/Sound/Song.wav";
pub(crate) const HI_RES: &str = "/Contents/Sound/Song (48 kHz).wav";
const EDIT: &str = "/Contents/Sound/Song (edit).wav";
const OTHER: &str = "/Contents/Sound/Other.wav";
/// How much later the hi-res copy's audio starts.
pub(crate) const DELAY_MS: u32 = 26;

/// `secs` of test signal `seed` as a stereo WAV at `rate` Hz and `bits` bits, after `delay_ms` of
/// silence, with the stretch `cut` (seconds) taken out.
pub(crate) fn recording(seed: u32, secs: f64, rate: u32, bits: u16, delay_ms: u32, cut: Option<(f64, f64)>) -> Vec<u8> {
    let mut x = synth(secs, seed);
    if let Some((from, to)) = cut {
        x.drain((from * f64::from(RATE)) as usize..(to * f64::from(RATE)) as usize);
    }
    let mut pcm = vec![0.0f32; (rate * delay_ms / 1000) as usize];
    let mut resampler = Resampler::new(RATE, rate);
    resampler.push(&x, &mut |s| pcm.extend_from_slice(s));
    resampler.finish(&mut |s| pcm.extend_from_slice(s));
    let scale = 12_000.0 * f32::from(1u16 << (bits - 16));
    wav_of(rate, bits, pcm.iter().map(|s| (s * scale) as i32))
}

pub(crate) fn track(title: &str, path: &str, n: u32, secs: u16, rate: u32, bits: u16) -> NewTrack {
    NewTrack {
        artist: Some("Someone".into()),
        analyze_path: format!("/PIONEER/USBANLZ/P0BB/0000000{n}/ANLZ0000.DAT"),
        duration: secs,
        bitrate: rate * u32::from(bits) * 2 / 1000,
        sample_rate: rate,
        sample_depth: bits,
        ..NewTrack::new(title, path)
    }
}

/// The bigger fixture plus a song as a 44.1 kHz WAV, the same song as a 48 kHz 24-bit WAV that
/// starts 26 ms later, an edit of it, and another song as long as it. Returns their ids.
fn drive_with_copies(dir: &Path) -> (std::path::PathBuf, [u32; 4]) {
    let root = common::bigger_drive(dir, "Main");
    let ids = [
        add(&root, track("Song", ORIGINAL, 1, 40, 44_100, 16), &recording(7919, 40.0, 44_100, 16, 0, None)),
        add(
            &root,
            track("Song (hi-res)", HI_RES, 2, 40, 48_000, 24),
            &recording(7919, 40.0, 48_000, 24, DELAY_MS, None),
        ),
        add(
            &root,
            track("Song (edit)", EDIT, 3, 24, 44_100, 16),
            &recording(7919, 40.0, 44_100, 16, 0, Some((12.0, 28.0))),
        ),
        add(&root, track("Other", OTHER, 4, 40, 44_100, 16), &recording(3 * 7919, 40.0, 44_100, 16, 0, None)),
    ];
    (root, ids)
}

/// A store for this thread's drive lookups, which keeps the fingerprints.
pub(crate) fn store(dir: &Path) -> Store {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    store
}

/// Counts the tracks decoded for their fingerprints and the pairs compared.
#[derive(Default)]
struct Counted {
    decoded: AtomicUsize,
    compared: AtomicUsize,
}

impl Progress for Counted {
    fn on_event(&self, e: &Event) {
        match e {
            Event::Item { phase: Phase::Fingerprint, .. } => self.decoded.fetch_add(1, Ordering::Relaxed),
            Event::Item { phase: Phase::Match, .. } => self.compared.fetch_add(1, Ordering::Relaxed),
            _ => 0,
        };
    }
}

#[test]
fn the_same_recording_in_another_encoding_is_a_duplicate_by_sound() {
    let dir = tempfile::tempdir().unwrap();
    let (root, [original, hi_res, edit, other]) = drive_with_copies(dir.path());
    let _store = store(dir.path());
    let d = Device::open_export(&root).unwrap();
    let opts = DoctorOptions { by_sound: true, only: vec![Section::Duplicates], ..Default::default() };

    let counted = Counted::default();
    let r = doctor::run(&d, &opts, &Ctx::new(&counted, CancelToken::new())).unwrap();
    let groups = r.duplicates.unwrap();
    let group = groups.iter().find(|g| g.reasons.contains(&DuplicateReason::SameRecording)).expect("found by sound");
    assert_eq!(group.reasons, [DuplicateReason::SameRecording]);
    let ids: Vec<u32> = group.tracks.iter().map(|t| t.track.id).collect();
    assert_eq!(ids, [hi_res, original], "{groups:#?}");
    // Both lossless: the higher sample rate wins, and says so.
    assert_eq!(group.keep, hi_res);
    assert!(group.tracks.iter().all(|t| t.lossless));
    assert_eq!(group.keep_reason, "48000 Hz over 44100");
    // The edit shares its audio but not its length; the other song is as long but sounds different.
    assert!(!groups.iter().flat_map(|g| &g.tracks).any(|t| t.track.id == edit || t.track.id == other));
    // Each of the four songs (the fixture's placeholders don't decode) once; pairs of about the
    // same length only: the song, its copy and the other song.
    assert!(counted.decoded.load(Ordering::Relaxed) >= 4);
    assert_eq!(counted.compared.load(Ordering::Relaxed), 3);

    // Fingerprinted once: the store keeps the prints.
    let again = Counted::default();
    let r = doctor::run(&d, &opts, &Ctx::new(&again, CancelToken::new())).unwrap();
    assert_eq!(again.decoded.load(Ordering::Relaxed), 0);
    assert_eq!(r.duplicates.unwrap().len(), groups.len());

    // Without asking for it, nothing is decoded and the copy isn't found.
    let plain = doctor::run(&d, &DoctorOptions { by_sound: false, ..opts.clone() }, &Ctx::silent()).unwrap();
    assert!(!plain.duplicates.unwrap().iter().any(|g| g.tracks.iter().any(|t| t.track.id == hi_res)));

    let cancel = CancelToken::new();
    cancel.cancel();
    let stopped = doctor::run(&d, &opts, &Ctx::new(&Counted::default(), cancel));
    assert!(stopped.is_err_and(|e| matches!(e.inner(), Error::Cancelled)));
    testing::store_root_on_this_thread(None);
}

/// The cues `id` has in its analysis files: (hot cue slot, start, end).
pub(crate) fn cues(root: &Path, id: u32) -> Vec<(Option<String>, u32, Option<u32>)> {
    let listed = cue_edit::list(&Device::open_export(root).unwrap(), &[id]).unwrap();
    listed[0].cues.iter().map(|c| (c.hot_cue.clone(), c.start_ms, c.end_ms)).collect()
}

/// Give `id` a hot cue, a memory cue and a looping hot cue, in both analysis files.
fn set_cues(root: &Path, id: u32) {
    set_points(
        root,
        id,
        &[
            CuePoint {
                hot_cue: Some(0),
                start_ms: 10_000,
                comment: "Drop".into(),
                color_code: 0x2a,
                ..Default::default()
            },
            CuePoint { start_ms: 20_000, ..Default::default() },
            CuePoint { hot_cue: Some(1), start_ms: 30_000, end_ms: Some(32_000), ..Default::default() },
        ],
    );
}

/// Give `id` these cues, in both analysis files.
pub(crate) fn set_points(root: &Path, id: u32, points: &[CuePoint]) {
    let lib = Device::open_export(root).unwrap().library().unwrap();
    let dat = paths::key(&lib.track(id).unwrap().analyze_path);
    for (key, file) in [(dat.clone(), CueFile::Dat), (dat.replace(".DAT", ".EXT"), CueFile::Ext)] {
        let path = paths::to_fs(root, &key);
        fs::write(&path, anlz::write_cue_points(&fs::read(&path).unwrap(), file, points).unwrap()).unwrap();
    }
}

#[test]
fn replacing_a_copy_carries_its_cues_onto_a_keeper_without_any() {
    let dir = tempfile::tempdir().unwrap();
    let (root, [original, hi_res, _, _]) = drive_with_copies(dir.path());
    common::add_onelibrary(&root);
    let _store = store(dir.path());
    set_cues(&root, original);
    let before = cues(&root, original);
    assert!(cues(&root, hi_res).is_empty());

    let req = DedupeRequest { drive: common::path(&root), by_sound: true, ..Default::default() };
    let plan = api::dedupe_plan(&req, &Ctx::silent()).unwrap();
    assert_eq!(plan.tracks_removed.iter().map(|t| t.id).collect::<Vec<_>>(), [original]);
    assert!(
        plan.changes.iter().any(|c| c.starts_with("carry the cues of /Contents/Sound/Song.wav")),
        "{:?}",
        plan.changes
    );
    let report = plan.apply(None, &Ctx::silent()).unwrap();

    // Each cue moved to where the same moment is in the copy, to the millisecond: the offset is
    // measured from the audio, not only the fingerprints.
    let after = cues(&root, hi_res);
    assert_eq!(after.len(), before.len());
    for ((slot, start, end), (was_slot, was_start, was_end)) in after.iter().zip(&before) {
        let moved = |at: u32, was: u32| i64::from(at) - i64::from(was) == i64::from(DELAY_MS);
        assert_eq!(slot, was_slot);
        assert!(moved(*start, *was_start), "{after:?} from {before:?}");
        assert_eq!(end.is_some(), was_end.is_some());
        assert!(end.zip(*was_end).is_none_or(|(e, w)| moved(e, w)), "{after:?} from {before:?}");
    }
    let d = Device::open_export(&root).unwrap();
    assert_eq!(cue_edit::list(&d, &[hi_res]).unwrap()[0].onelibrary_cues, Some(before.len()));
    assert!(d.library().unwrap().track(original).is_none());
    assert!(!paths::to_fs(&root, &paths::key(ORIGINAL)).exists());

    // Recorded so a replay after rekordbox's next export carries them again, before replacing.
    let recorded: Vec<Edit> = journal::list(&d).unwrap().into_iter().flat_map(|e| e.edits).collect();
    assert!(matches!(recorded.as_slice(), [Edit::Cues { .. }, Edit::ReplaceTrack { .. }]), "{recorded:?}");

    // One backup for all of it: undo puts the copy and the keeper's empty cues back.
    let mut d = Device::open_export(&root).unwrap();
    backup::restore(&mut d, report.backup.as_deref(), false).unwrap();
    assert!(cues(&root, hi_res).is_empty());
    assert_eq!(cues(&root, original), before);
    testing::store_root_on_this_thread(None);
}

/// Tracks that only share artist, title and length may be different recordings: the copy with
/// cues that can't be carried stays, cues and all, and the plan says why.
#[test]
fn a_copy_whose_cues_cant_be_carried_stays() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let _store = store(dir.path());
    let original = add(&root, track("Song", ORIGINAL, 1, 40, 44_100, 16), &recording(7919, 40.0, 44_100, 16, 0, None));
    let lookalike = add(&root, track("Song", HI_RES, 2, 40, 48_000, 24), &recording(5, 40.0, 48_000, 24, 0, None));
    set_cues(&root, original);

    let req = DedupeRequest { drive: common::path(&root), metadata: true, ..Default::default() };
    let plan = api::dedupe_plan(&req, &Ctx::silent()).unwrap();
    assert!(plan.tracks_removed.is_empty(), "{:?}", plan.tracks_removed);
    let why =
        format!("{ORIGINAL} stays: its cues couldn't be carried onto {HI_RES}: they don't sound like one recording");
    assert!(plan.warnings.contains(&why), "{:?}", plan.warnings);
    plan.apply(None, &Ctx::silent()).unwrap();
    assert!(cues(&root, lookalike).is_empty());
    assert_eq!(cues(&root, original).len(), 3);
    testing::store_root_on_this_thread(None);
}

/// A keeper with cues of its own keeps them: a copy whose cues it has too (the same file in
/// another folder) is replaced, one with cues it lacks stays. Copies of one file, name and time
/// are found by sound too.
#[test]
fn a_copy_with_cues_the_keeper_lacks_stays() {
    let dir = tempfile::tempdir().unwrap();
    let (root, [original, hi_res, _, _]) = drive_with_copies(dir.path());
    let _store = store(dir.path());
    let twin = "/Contents/Copy/Song (48 kHz).wav";
    let copy = add(
        &root,
        track("Song (twin)", twin, 5, 40, 48_000, 24),
        &fs::read(paths::to_fs(&root, &paths::key(HI_RES))).unwrap(),
    );
    let mtime = filetime::FileTime::from_unix_time(1_700_000_000, 0);
    for p in [HI_RES, twin] {
        filetime::set_file_mtime(paths::to_fs(&root, &paths::key(p)), mtime).unwrap();
    }
    let own = [CuePoint { start_ms: 5_000, ..Default::default() }];
    set_points(&root, hi_res, &own);
    set_points(&root, copy, &own);
    set_cues(&root, original);

    let req = DedupeRequest { drive: common::path(&root), by_sound: true, ..Default::default() };
    let plan = api::dedupe_plan(&req, &Ctx::silent()).unwrap();
    assert_eq!(plan.tracks_removed.iter().map(|t| t.id).collect::<Vec<_>>(), [copy], "{:?}", plan.warnings);
    let why = format!(
        "{ORIGINAL} stays: its cues couldn't be carried onto {HI_RES}: it has cues of its own, and not all of these"
    );
    assert!(plan.warnings.contains(&why), "{:?}", plan.warnings);
    testing::store_root_on_this_thread(None);
}

/// One file in two folders, with one name and time, is two tracks' audio: compared by sound like
/// any other pair.
#[test]
fn the_same_file_in_two_folders_is_one_recording() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let _store = store(dir.path());
    let audio = recording(7919, 40.0, 44_100, 16, 0, None);
    let a = add(&root, track("Song", "/Contents/A/Song.wav", 1, 40, 44_100, 16), &audio);
    let b = add(&root, track("Song (again)", "/Contents/B/Song.wav", 2, 40, 44_100, 16), &audio);
    let mtime = filetime::FileTime::from_unix_time(1_700_000_000, 0);
    for p in ["Contents/A/Song.wav", "Contents/B/Song.wav"] {
        filetime::set_file_mtime(paths::to_fs(&root, p), mtime).unwrap();
    }
    let d = Device::open_export(&root).unwrap();
    let opts = DoctorOptions { by_sound: true, only: vec![Section::Duplicates], ..Default::default() };
    let groups = doctor::run(&d, &opts, &Ctx::silent()).unwrap().duplicates.unwrap();
    let ids: Vec<Vec<u32>> = groups.iter().map(|g| g.tracks.iter().map(|t| t.track.id).collect()).collect();
    assert_eq!(ids, [vec![a, b]], "{groups:#?}");
    testing::store_root_on_this_thread(None);
}
