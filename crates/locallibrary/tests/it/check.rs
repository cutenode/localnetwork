mod drive_states;

use crate::common;

use std::fs;
use std::path::Path;

use locallibrary::api::{self, CheckRequest, ClearPlayers, SetPlayers};
use locallibrary::check::{CheckOptions, CheckReport, Level, PlayersFrom};
use locallibrary::device::{Device, LineageOp};
use locallibrary::doctor::{DoctorOptions, Problem, Section};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Error, paths};
use rekordbox_pdb::{NewTrack, PdbEditor};

fn level(r: &CheckReport, drive: &str, check: &str) -> Level {
    r.items
        .iter()
        .filter(|i| i.drive == drive && i.check == check)
        .map(|i| i.level)
        .max()
        .unwrap_or_else(|| panic!("no {check} item for {drive}: {r:#?}"))
}

#[test]
fn ready_drives_pass_and_problems_are_named() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    common::add_history(&a, "HISTORY 001", &[1, 2]);
    let b = dir.path().join("B");
    common::copy_dir(&a, &b);
    let mut da = Device::open_export(&a).unwrap();
    sync::pull(&store, &mut da, &PullOptions::default(), &Ctx::silent()).unwrap();
    let check = |players: &[&str]| {
        let req = CheckRequest {
            drives: vec![a.display().to_string(), b.display().to_string()],
            options: CheckOptions { players: players.iter().map(|p| p.to_string()).collect(), ..Default::default() },
        };
        api::check(Some(&store), &req, &Ctx::silent()).unwrap()
    };

    let r = check(&["CDJ-2000NXS2"]);
    assert_eq!(r.drives, ["A", "B"]);
    assert_eq!(r.problems, 0, "{r:#?}");
    assert!(r.ok());
    assert_eq!(level(&r, "B", "same_library"), Level::Info);
    // B is a copy of what the store holds for A: its history is safe too.
    assert_eq!(level(&r, "A", "history"), Level::Info);
    assert_eq!(level(&r, "B", "history"), Level::Info);
    assert_eq!(level(&r, "A", "interrupted"), Level::Info);
    assert_eq!(level(&r, "A", "databases"), Level::Info);
    assert_eq!(level(&r, "A", "players"), Level::Info);
    // The placeholder audio is smaller than export.pdb recorded, but doesn't open as audio, so
    // nothing says it was cut short.
    assert_eq!(level(&r, "A", "files"), Level::Info);
    let first = r.items.iter().position(|i| i.drive == "B").unwrap();
    assert!(r.items[..first].iter().all(|i| i.drive == "A"), "grouped by drive");

    // Now: B gets a new export, loses an audio file and has an interrupted sync; A's history
    // grows past the store, and a CDJ-3000X needs OneLibrary.
    common::export_new_track(&b, "New", "Track");
    let lib = locallibrary::pdb::read_library(&b.join(paths::EXPORT_PDB)).unwrap();
    fs::remove_file(paths::to_fs(&b, &paths::key(&lib.track(2).unwrap().file_path))).unwrap();
    Device::open(&b).unwrap().begin(LineageOp::Sync, "sync from A", None).unwrap();
    common::add_history(&a, "HISTORY 002", &[3]);

    let r = check(&["CDJ-3000X"]);
    assert!(!r.ok());
    assert_eq!(level(&r, "B", "same_library"), Level::Problem);
    assert_eq!(level(&r, "B", "files"), Level::Problem);
    assert_eq!(level(&r, "B", "interrupted"), Level::Problem);
    assert_eq!(level(&r, "A", "history"), Level::Warning);
    // B changed since the pull, but its only history list is in the store.
    assert_eq!(level(&r, "B", "history"), Level::Info);
    assert_eq!(level(&r, "A", "players"), Level::Problem);
    let msg = &r.items.iter().find(|i| i.drive == "B" && i.check == "files").unwrap().message;
    assert!(msg.contains("1 track(s) have no audio file"), "{msg}");
}

#[test]
fn drifting_databases_and_bad_requests() {
    let dir = tempfile::tempdir().unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&a);
    common::export_new_track(&a, "Only", "In export.pdb");
    let req = |drives: Vec<String>| CheckRequest { drives, options: CheckOptions::default() };
    let r = api::check(None, &req(vec![a.display().to_string()]), &Ctx::silent()).unwrap();
    assert_eq!(level(&r, "A", "databases"), Level::Problem);
    // No history lists, so nothing to keep.
    assert_eq!(level(&r, "A", "history"), Level::Info);

    let blank = dir.path().join("Blank");
    fs::create_dir(&blank).unwrap();
    let r = api::check(None, &req(vec![blank.display().to_string()]), &Ctx::silent()).unwrap();
    assert_eq!(level(&r, "Blank", "export"), Level::Problem);

    let twice = req(vec![a.display().to_string(), a.display().to_string()]);
    assert!(matches!(api::check(None, &twice, &Ctx::silent()), Err(Error::Invalid(_))));
    assert!(matches!(api::check(None, &req(vec![]), &Ctx::silent()), Err(Error::Invalid(_))));
}

/// Export one more track to `root`'s export.pdb, with its audio file.
fn add_track(root: &Path, file: &str, rate: u32, depth: u16, bytes: &[u8]) -> u32 {
    export_track(root, file, rate, depth, 1, bytes.len() as u32, bytes)
}

/// The same, with the length and size the database records.
fn export_track(root: &Path, file: &str, rate: u32, depth: u16, secs: u16, size: u32, bytes: &[u8]) -> u32 {
    let device_path = format!("/Contents/Formats/{file}");
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    let track = NewTrack {
        sample_rate: rate,
        sample_depth: depth,
        duration: secs,
        file_size: size,
        ..NewTrack::new(file, device_path.clone())
    };
    let id = ed.add_track(&track).unwrap();
    ed.save(&pdb_path).unwrap();
    let audio = paths::to_fs(root, &paths::key(&device_path));
    fs::create_dir_all(audio.parent().unwrap()).unwrap();
    fs::write(audio, bytes).unwrap();
    id
}

#[test]
fn track_formats_against_players() {
    let dir = tempfile::tempdir().unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    for i in 0..3 {
        add_track(&a, &format!("hi{i}.flac"), 96_000, 24, b"fLaC placeholder");
    }
    // A row that doesn't say: the file's header does.
    add_track(&a, "unknown.wav", 0, 0, &common::media::wav_of(88_200, 24, 2, 0.1));
    add_track(&a, "float.wav", 44_100, 32, b"RIFF placeholder");
    let check = |players: &[&str]| {
        let req = CheckRequest {
            drives: vec![a.display().to_string()],
            options: CheckOptions { players: players.iter().map(|p| p.to_string()).collect(), ..Default::default() },
        };
        api::check(None, &req, &Ctx::silent()).unwrap()
    };

    let r = check(&["CDJ-2000NXS"]);
    assert_eq!(level(&r, "A", "formats"), Level::Problem);
    let messages: Vec<&str> = r.unloadable.iter().map(|u| u.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "3 FLAC files won't load on the CDJ-2000NXS: it doesn't play FLAC",
            "1 WAV file at 88.2 kHz won't load on the CDJ-2000NXS: it plays WAV at 44.1 and 48 kHz",
            "1 WAV file at 32-bit won't load on the CDJ-2000NXS: it plays WAV at 16 and 24-bit",
        ],
    );
    let flac = &r.unloadable[0];
    assert_eq!((flac.drive.as_str(), flac.player.as_str(), flac.format.as_str()), ("A", "CDJ-2000NXS", "FLAC"));
    assert_eq!(flac.tracks.len(), 3);
    assert!(flac.tracks.iter().all(|t| t.file_path.ends_with(".flac")));
    assert!(r.unloadable.iter().all(|u| u.transcodable), "lossless: packing them as copies the player plays fixes it");
    // Every group is also an item, so the checklist and the exit code count it.
    for u in &r.unloadable {
        assert!(r.items.iter().any(|i| i.check == "formats" && i.message == u.message));
    }
    assert!(!r.ok());

    // The NXS2 plays FLAC and 88.2 kHz WAV; 32-bit WAV it doesn't.
    let r = check(&["CDJ-2000NXS2"]);
    assert_eq!(r.unloadable.len(), 1, "{:#?}", r.unloadable);
    assert_eq!(r.unloadable[0].bit_depth, Some(32));

    // What scripts read.
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["unloadable"][0]["reason"], "bit_depth");
    assert_eq!(json["players"][0]["from"], "given");
}

#[test]
fn remembered_players_and_drives_with_one_database() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    let mut da = Device::open_export(&a).unwrap();
    sync::pull(&store, &mut da, &PullOptions::default(), &Ctx::silent()).unwrap();
    let req = CheckRequest { drives: vec![a.display().to_string()], options: CheckOptions::default() };

    // No models anywhere: a drive with only export.pdb is a warning, not "ready" without a word.
    let r = api::check(Some(&store), &req, &Ctx::silent()).unwrap();
    assert_eq!(level(&r, "A", "players"), Level::Warning);
    let msg = &r.items.iter().find(|i| i.check == "players").unwrap().message;
    assert!(msg.contains("CDJ-3000X"), "{msg}");
    assert_eq!(r.players[0].from, PlayersFrom::None);

    // Remembered for every library...
    api::players_set(&store, &SetPlayers { models: vec!["cdj-2000nxs2".into()], library: None }).unwrap();
    let r = api::check(Some(&store), &req, &Ctx::silent()).unwrap();
    assert_eq!(r.players[0].from, PlayersFrom::Default);
    assert_eq!(r.players[0].models, ["CDJ-2000NXS2"]);
    assert_eq!(level(&r, "A", "players"), Level::Info);
    assert_eq!(level(&r, "A", "formats"), Level::Info);

    // ...and for the drive's own library, which wins.
    let library = store.libraries().unwrap().remove(0);
    let set = SetPlayers { models: vec!["CDJ-3000X".into(), "CDJ-3000".into()], library: Some(library.name.clone()) };
    let shown = api::players_set(&store, &set).unwrap();
    assert_eq!(shown.libraries[0].name.as_deref(), Some(library.name.as_str()));
    let r = api::check(Some(&store), &req, &Ctx::silent()).unwrap();
    assert_eq!(r.players[0].from, PlayersFrom::Library);
    assert_eq!(r.players[0].library.as_deref(), Some(library.name.as_str()));
    assert_eq!(level(&r, "A", "players"), Level::Problem, "the CDJ-3000X needs OneLibrary");

    // --players wins over what's remembered.
    let given = CheckRequest {
        options: CheckOptions { players: vec!["CDJ-3000".into()], ..Default::default() },
        ..req.clone()
    };
    assert_eq!(api::check(Some(&store), &given, &Ctx::silent()).unwrap().players[0].from, PlayersFrom::Given);

    let unknown = SetPlayers { models: vec!["CDJ-9999".into()], library: None };
    assert!(matches!(api::players_set(&store, &unknown), Err(Error::Invalid(_))));
    let clear = ClearPlayers { library: Some(library.name.clone()) };
    let plan = api::players_clear_plan(&store, &clear).unwrap();
    assert_eq!((plan.from.as_slice(), plan.to.is_empty()), (&["CDJ-3000X".to_string(), "CDJ-3000".into()][..], true));
    // Remembered differently since the plan: it's stale.
    api::players_set(&store, &SetPlayers { models: vec!["CDJ-3000".into()], library: None }).unwrap();
    assert!(matches!(plan.apply(Some(&store), &Ctx::silent()), Err(Error::StalePlan(_))));
    let cleared = api::players_clear(&store, &clear).unwrap();
    assert!(cleared.libraries.is_empty());
    assert_eq!(cleared.default, ["CDJ-3000"]);
    assert!(api::players_clear_plan(&store, &clear).unwrap().noop);
}

#[test]
fn audio_cut_short_by_an_interrupted_copy() {
    let dir = tempfile::tempdir().unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    let whole = common::media::wav_of(8_000, 16, 2, 30.0);
    let size = whole.len() as u32;
    export_track(&a, "whole.wav", 8_000, 16, 30, size, &whole);
    export_track(&a, "cut.wav", 8_000, 16, 30, size, &whole[..whole.len() / 2]);
    let req = |audio| CheckRequest {
        drives: vec![a.display().to_string()],
        options: CheckOptions { players: vec!["CDJ-3000".into()], audio },
    };

    // Without --audio only files smaller than recorded are decoded, and only the cut one is named.
    let r = api::check(None, &req(false), &Ctx::silent()).unwrap();
    assert!(r.items.iter().all(|i| i.check != "audio") && r.audio.is_empty());
    let files: Vec<&str> = r.items.iter().filter(|i| i.check == "files").map(|i| i.message.as_str()).collect();
    assert_eq!(files, ["1 track(s) stop short of their length (an interrupted copy?): cut.wav"]);
    assert_eq!(level(&r, "A", "files"), Level::Problem);

    let r = api::check(None, &req(true), &Ctx::silent()).unwrap();
    assert_eq!(level(&r, "A", "audio"), Level::Problem);
    let cut = r.audio.iter().find(|g| g.kind == "truncated").unwrap();
    assert_eq!(cut.files.len(), 1);
    assert!(cut.files[0].track.file_path.ends_with("cut.wav"));
    assert!(cut.files[0].problem.to_string().starts_with("stops at 0:15 of 0:30"), "{}", cut.files[0].problem);
    assert!(r.items.iter().any(|i| i.check == "audio" && i.message == cut.message));
    // The fixture's stand-in audio files aren't audio at all.
    let unreadable = r.audio.iter().find(|g| g.kind == "unreadable").unwrap();
    assert!(unreadable.files.iter().all(|f| !f.track.file_path.contains("Formats")));
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["audio"][0]["files"][0]["problem"]["kind"], "truncated");

    let options = DoctorOptions { audio: true, only: vec![Section::Audio], ..Default::default() };
    let report = api::doctor(&api::DoctorRequest { drive: a.display().to_string(), options }, &Ctx::silent()).unwrap();
    let audio = report.audio.unwrap();
    assert_eq!(audio.checked, 14);
    assert!(audio.problems.iter().any(|f| f.track.file_path.ends_with("cut.wav")));
    assert!(!audio.problems.iter().any(|f| f.track.file_path.ends_with("whole.wav")));
    // Doctor leaves it out unless asked, but its missing section names the cut file.
    let options = DoctorOptions { only: vec![Section::Missing], ..Default::default() };
    let report = api::doctor(&api::DoctorRequest { drive: a.display().to_string(), options }, &Ctx::silent()).unwrap();
    assert!(report.audio.is_none());
    let cut: Vec<_> = report.missing.unwrap().into_iter().filter(|m| m.problem == Problem::Truncated).collect();
    assert_eq!(cut.len(), 1, "{cut:#?}");
    assert_eq!(cut[0].path, "Contents/Formats/cut.wav");
    assert_eq!(cut[0].detail.as_deref(), Some("stops at 0:15 of 0:30"));
}

/// MP3 and FLAC copies cut short are found as the WAV one is, whether the file's header still
/// promises its whole length (an MP3 with an Info frame, FLAC) or its length comes from its size
/// (a constant bit rate MP3 with no header); an M4A cut short is found too, as a file that won't
/// open. The M4A is a committed fixture (`tests/fixtures/audio/`): nothing here encodes AAC.
#[test]
fn compressed_audio_cut_short_by_an_interrupted_copy() {
    use loclib_mixid::test_util::{MP3_FRAME_SAMPLES, mp3};
    use loclib_transcode::{Container, Dither, Recipe};

    let dir = tempfile::tempdir().unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    // 20 s at 44.1 kHz.
    let frames = 20 * 44_100 / MP3_FRAME_SAMPLES + 1;
    let wav = dir.path().join("src.wav");
    fs::write(&wav, loclib_mixid::test_util::wav(22_050, 16, 20.0)).unwrap();
    let flac = dir.path().join("src.flac");
    let recipe = Recipe { container: Container::Flac, sample_rate: 22_050, bit_depth: 16, dither: Dither::None };
    loclib_transcode::transcode(&wav, &recipe, &flac, &Ctx::silent()).unwrap();
    let m4a = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio/silence.m4a")).unwrap();
    // Half its audio: the index is first, and the frames after it.
    let mdat = m4a.windows(4).position(|w| w == b"mdat").unwrap() + 4;
    let files = [
        ("tagged.mp3", 44_100, mp3(frames, Some((1105, 1000))), None),
        ("untagged.mp3", 44_100, mp3(frames, None), None),
        ("tone.flac", 22_050, fs::read(&flac).unwrap(), None),
        ("silence.m4a", 8_000, m4a.clone(), Some(mdat + (m4a.len() - mdat) / 2)),
    ];
    for (name, rate, whole, cut_at) in &files {
        let size = whole.len() as u32;
        export_track(&a, &format!("whole-{name}"), *rate, 16, 20, size, whole);
        let cut = &whole[..cut_at.unwrap_or(whole.len() / 2)];
        export_track(&a, &format!("cut-{name}"), *rate, 16, 20, size, cut);
    }
    let req = CheckRequest {
        drives: vec![a.display().to_string()],
        options: CheckOptions { players: vec!["CDJ-3000".into()], audio: true },
    };
    let r = api::check(None, &req, &Ctx::silent()).unwrap();
    assert_eq!(level(&r, "A", "audio"), Level::Problem);
    // What's wrong with each of these files, by name: the whole ones have nothing wrong.
    let mut found: Vec<(&str, &str)> = r
        .audio
        .iter()
        .flat_map(|g| &g.files)
        .filter_map(|f| Some((f.track.file_path.strip_prefix("/Contents/Formats/")?, f.problem.kind())))
        .collect();
    found.sort_unstable();
    let truncated = ["cut-tagged.mp3", "cut-tone.flac", "cut-untagged.mp3"].map(|f| (f, "truncated"));
    // An M4A cut short in its audio doesn't open at all: its last box runs past the end.
    assert_eq!(found, [&[("cut-silence.m4a", "unreadable")][..], &truncated].concat(), "{:#?}", r.audio);
    let m4a = r.audio.iter().flat_map(|g| &g.files).find(|f| f.track.file_path.ends_with("cut-silence.m4a"));
    assert!(m4a.unwrap().problem.to_string().ends_with(": end of stream"), "{m4a:?}");
}

/// rekordbox keeps the size a file had on import, and tag editors rewrite tags afterwards: a file
/// of another size that plays to its end isn't reported.
#[test]
fn retagged_audio_isnt_cut_short() {
    let dir = tempfile::tempdir().unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    let whole = common::media::wav_of(8_000, 16, 2, 30.0);
    let size = whole.len() as u32;
    export_track(&a, "grown.wav", 8_000, 16, 30, size - 1024, &whole);
    export_track(&a, "shrunk.wav", 8_000, 16, 30, size + 4096, &whole);
    let req = CheckRequest { drives: vec![a.display().to_string()], options: CheckOptions::default() };
    let r = api::check(None, &req, &Ctx::silent()).unwrap();
    assert_eq!(level(&r, "A", "files"), Level::Info, "{r:#?}");
    let options = DoctorOptions { only: vec![Section::Missing], ..Default::default() };
    let report = api::doctor(&api::DoctorRequest { drive: a.display().to_string(), options }, &Ctx::silent()).unwrap();
    assert_eq!(report.missing.unwrap().len(), 0);
}
