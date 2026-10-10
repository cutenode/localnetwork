//! Sessions (crate::sets::session): gathering, windows, imported logs and suggestions.

use crate::common;

use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use locallibrary::api::{self, DismissSessions, ImportSessionLog, SessionsRequest, SuggestSets};
use locallibrary::device::Device;
use locallibrary::fleet::{self, Role};
use locallibrary::mix::{MixInfo, MixReport, Play, report};
use locallibrary::offsite::{self, CopyOptions};
use locallibrary::progress::Ctx;
use locallibrary::sets::session::{self, Gathered};
use locallibrary::sets::suggest::Candidate;
use locallibrary::sets::{EntryOrigin, Set, SetBook, Window};
use locallibrary::store::{SnapshotInfo, Store};
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Library, Track, paths, pdb};
use tempfile::TempDir;

/// A store, and two drives of one library (Main and Backup) in the folder this thread's drives
/// are looked for in. Each drive has its own identity once it's pulled.
struct Desk {
    dir: TempDir,
    store: Store,
    main: PathBuf,
    backup: PathBuf,
}

impl Desk {
    fn new() -> Desk {
        let dir = tempfile::tempdir().unwrap();
        let volumes = dir.path().join("volumes");
        let main = common::bigger_drive(&volumes, "Main");
        let backup = volumes.join("Backup");
        common::copy_dir(&main, &backup);
        locallibrary::testing::volumes_on_this_thread(Some(vec![volumes]));
        let store = Store::open_or_init(dir.path().join("store")).unwrap();
        Desk { dir, store, main, backup }
    }

    /// Save a drive into the library as its newest state, whichever drive was pulled last.
    fn pull(&self, root: &Path) -> SnapshotInfo {
        let opts = PullOptions { library: Some("Club".into()), force: true, ..Default::default() };
        sync::pull(&self.store, &mut Device::open(root).unwrap(), &opts, &Ctx::silent()).unwrap().snapshot.unwrap()
    }

    fn sessions(&self, req: &SessionsRequest) -> Gathered {
        api::sessions(Some(&self.store), req, &Ctx::silent()).unwrap()
    }

    fn library(&self) -> Library {
        pdb::read_library(&self.main.join(paths::EXPORT_PDB)).unwrap()
    }

    /// A Beat Link Trigger log of `plays` (title, artist, started and stopped, in UTC), in the
    /// order they're given (the order they stopped in).
    fn log(&self, name: &str, plays: &[(&str, &str, &str, &str)]) -> PathBuf {
        let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut text = String::from("Title,Artist,Album,Player,Source,Started,Stopped,Play Time\n");
        for (title, artist, started, stopped) in plays {
            let java = |t: &str| {
                let t = DateTime::parse_from_rfc3339(t).unwrap();
                t.format("%a %b %d %H:%M:%S UTC %Y").to_string()
            };
            text += &format!("{},{},,2,Player 2 USB,{},{},05:00\n", q(title), q(artist), java(started), java(stopped));
        }
        let path = self.dir.path().join(format!("{name}.csv"));
        std::fs::write(&path, text).unwrap();
        path
    }

    fn import(&self, file: &Path, drive: Option<&Path>, name: Option<&str>) -> locallibrary::sets::ImportedSession {
        let drive = drive.map(|d| d.display().to_string());
        let req = ImportSessionLog { file: file.to_path_buf(), drive, name: name.map(String::from) };
        api::sessions_import(&self.store, &req).unwrap()
    }

    fn suggest(&self) -> Vec<Candidate> {
        api::sets_suggest(&self.store, &SuggestSets::default(), &Ctx::silent()).unwrap().candidates
    }
}

fn at(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

#[test]
fn sessions_are_dated_by_their_own_drives_pulls_and_listed_once() {
    let desk = Desk::new();
    let spare = desk.main.with_file_name("Spare");
    common::copy_dir(&desk.main, &spare);
    let first = desk.pull(&desk.main);
    common::add_history(&desk.backup, "HISTORY 001", &[5, 6, 7]);
    let backup_pull = desk.pull(&desk.backup);
    common::add_history(&desk.main, "HISTORY 001", &[1, 2, 3, 4]);
    let second = desk.pull(&desk.main);
    // Copied to a drive that was never pulled: it was still recorded after Main's first pull.
    common::add_history(&spare, "HISTORY 001", &[1, 2, 3, 4]);
    // Not pulled yet, and the same list on both drives.
    for root in [&desk.main, &desk.backup] {
        common::add_history(root, "HISTORY 002", &[8, 9, 10]);
    }

    let got = desk.sessions(&SessionsRequest::default());
    assert!(got.warnings.is_empty(), "{:?}", got.warnings);
    let listed: Vec<(&str, usize, Option<&str>)> =
        got.sessions.iter().map(|s| (s.name.as_str(), s.tracks.len(), s.first_seen_on.as_deref())).collect();
    assert_eq!(
        listed,
        [("HISTORY 002", 3, None), ("HISTORY 001", 4, Some("Main")), ("HISTORY 001", 3, Some("Backup"))]
    );
    let [unsaved, main_list, backup_list] = &got.sessions[..] else { unreachable!() };

    // Main's list: after Main's previous pull, though Backup was pulled since.
    let window = Window { after: Some(first.created), before: Some(second.created), exact: false };
    assert_eq!(main_list.window, window);
    assert_eq!(main_list.drives, ["Main", "Spare"], "on two drives and in the store, listed once");
    assert_eq!((backup_list.window.after, backup_list.window.before), (None, Some(backup_pull.created)));
    // In no snapshot: since the drives holding it were last pulled, either of them.
    let mut drives = unsaved.drives.clone();
    drives.sort();
    assert_eq!(drives, ["Backup", "Main"]);
    assert_eq!(unsaved.window.after, Some(backup_pull.created));
    assert!(unsaved.window.before.unwrap() > second.created && !unsaved.window.exact);
    assert_eq!(unsaved.source, "device_library");
    assert!(
        main_list.tracks.iter().all(|t| t.origin == EntryOrigin::Session && t.session.as_ref() == Some(&main_list.id))
    );
    assert_eq!(main_list.tracks[0].file.as_deref(), Some(desk.library().track(1).unwrap().file_path.as_str()));

    // One drive: its own lists, and its library's in the store.
    let req = SessionsRequest { drive: Some(desk.backup.display().to_string()), ..Default::default() };
    let got = desk.sessions(&req);
    assert_eq!(got.sessions.len(), 3);
    assert_eq!(
        (got.sessions[0].drives.as_slice(), got.sessions[0].window.after),
        (&["Backup".to_string()][..], Some(backup_pull.created))
    );
    assert!(got.sessions[1].drives.is_empty(), "Main's list isn't on Backup");

    // Without the store, what's on the drives: undated.
    let got = api::sessions(None, &SessionsRequest::default(), &Ctx::silent()).unwrap();
    assert_eq!(got.sessions.len(), 3);
    assert!(got.sessions.iter().all(|s| s.window.after.is_none() && s.first_seen_on.is_none()));

    // rekordbox takes Main's lists off it; the store still has the one it saved.
    rekordbox_imports_history(&desk.main);
    let got = desk.sessions(&SessionsRequest::default());
    let names: Vec<(&str, Vec<String>)> = got.sessions.iter().map(|s| (s.name.as_str(), s.drives.clone())).collect();
    assert_eq!(
        names,
        [
            ("HISTORY 002", vec!["Backup".into()]),
            ("HISTORY 001", vec!["Spare".into()]),
            ("HISTORY 001", vec!["Backup".into()])
        ]
    );
    assert_eq!(got.sessions[1].window, window);
}

/// A CDJ-3000X, XDJ-AZ or OPUS-QUAD stick has no export.pdb: its sessions are in OneLibrary.
#[test]
fn sessions_on_a_onelibrary_only_drive_are_found() {
    let desk = Desk::new();
    common::add_history(&desk.main, "HISTORY 001", &[1, 2, 3]);
    common::onelibrary_only(&desk.main);
    let got = desk.sessions(&SessionsRequest::default());
    let main: Vec<(&str, &str)> = got
        .sessions
        .iter()
        .filter(|s| s.drives.iter().any(|d| d == "Main"))
        .map(|s| (s.name.as_str(), s.source.as_str()))
        .collect();
    assert_eq!(main, [("HISTORY 001", "onelibrary")]);
}

#[test]
fn sessions_are_found_by_id_prefix_or_by_name_when_only_one_has_it() {
    let desk = Desk::new();
    common::add_history(&desk.main, "HISTORY 001", &[1, 2, 3]);
    common::add_history(&desk.backup, "HISTORY 001", &[4, 5]);
    common::add_history(&desk.backup, "HISTORY 002", &[6]);
    desk.pull(&desk.main);
    let got = desk.sessions(&SessionsRequest::default());
    let s = &got.sessions;
    let id =
        |name: &str, tracks: usize| s.iter().find(|x| x.name == name && x.tracks.len() == tracks).unwrap().id.clone();
    let (main, backup, second) = (id("HISTORY 001", 3), id("HISTORY 001", 2), id("HISTORY 002", 1));

    let found = |q: &str| session::find(s, q).map(|x| x.id.clone());
    assert_eq!(found(&main).unwrap(), main);
    assert_eq!(found(&main[..6]).unwrap(), main);
    assert_eq!(found(&main[..6].to_uppercase()).unwrap(), main);
    assert_eq!(found("history 002").unwrap(), second);
    assert_eq!(found("/HISTORY 002").unwrap(), second);
    let err = found("HISTORY 001").unwrap_err();
    assert_eq!(err.code(), "invalid");
    let main_session = s.iter().find(|x| x.id == main).unwrap();
    let text = err.to_string();
    assert!(text.contains(&format!("{} HISTORY 001 (Main, {})", &main[..8], main_session.window.label())), "{text}");
    assert!(text.contains(&format!("{} HISTORY 001 (Backup, ", &backup[..8])), "{text}");
    assert_eq!(found("HISTORY 009").unwrap_err().code(), "not_found");
    assert_eq!(found(&main[..3]).unwrap_err().code(), "not_found", "a prefix needs 4 characters");
    assert_eq!(found(" ").unwrap_err().code(), "invalid");
}

#[test]
fn logs_from_beat_link_trigger_are_sessions_with_their_times() {
    let desk = Desk::new();
    std::fs::remove_dir_all(&desk.backup).unwrap();
    let lib = desk.library();
    let t = |id: u32| lib.track(id).unwrap();
    let row = |id: u32, started, stopped| (t(id).title.as_str(), t(id).artist.as_str(), started, stopped);
    // Rows are written when a track stops: the third track played longest.
    let file = desk.log(
        "Friday",
        &[
            row(1, "2026-10-03T23:00:00Z", "2026-10-03T23:06:00Z"),
            row(2, "2026-10-03T23:05:00Z", "2026-10-03T23:11:00Z"),
            ("Unknown (no metadata found)", "", "2026-10-03T23:10:00Z", "2026-10-03T23:12:30Z"),
            row(3, "2026-10-03T23:08:00Z", "2026-10-03T23:20:00Z"),
        ],
    );
    // Main is the only rekordbox drive mounted: the log's tracks are looked up in it.
    let log = desk.import(&file, None, None);
    assert_eq!((log.name.as_str(), log.tool.as_str(), log.plays.len()), ("Friday", "beat_link_trigger", 4));
    let files: Vec<Option<&str>> = log.plays.iter().map(|p| p.entry.file.as_deref()).collect();
    assert_eq!(
        files,
        [Some(t(1).file_path.as_str()), Some(t(2).file_path.as_str()), Some(t(3).file_path.as_str()), None]
    );
    assert_eq!(log.plays[0].entry.bpm, t(1).bpm);
    assert_eq!(
        (log.plays[3].entry.title.as_str(), log.plays[3].player.as_deref()),
        ("Unknown (no metadata found)", Some("2"))
    );

    // The same log again is the same session.
    let again = desk.import(&file, None, Some("Warehouse"));
    assert_eq!(again.id, log.id);
    let book = SetBook::load(&desk.store).unwrap();
    assert_eq!(book.imported.len(), 1);
    assert_eq!(book.imported[0].name, "Warehouse");

    let got = desk.sessions(&SessionsRequest::default());
    let s = got.sessions.iter().find(|s| s.id == log.id).unwrap();
    assert_eq!(
        s.window,
        Window { after: Some(at("2026-10-03T23:00:00Z")), before: Some(at("2026-10-03T23:20:00Z")), exact: true }
    );
    let starts: Vec<Option<u32>> = s.tracks.iter().map(|e| e.start_seconds).collect();
    assert_eq!(starts, [Some(0), Some(300), Some(480), Some(600)]);
    assert!(s.tracks.iter().all(|e| e.session.as_ref() == Some(&log.id)));
    assert_eq!((s.name.as_str(), s.source.as_str(), s.drives.len()), ("Warehouse", "beat_link_trigger", 0));
    assert_eq!(session::find(&got.sessions, "warehouse").unwrap().id, log.id);

    // A drive that isn't there is refused; the log is left as it was.
    let req = ImportSessionLog { file: file.clone(), drive: Some("Nowhere".into()), name: None };
    assert_eq!(api::sessions_import(&desk.store, &req).unwrap_err().code(), "not_found");
    // With no drive mounted, it's kept without files.
    locallibrary::testing::volumes_on_this_thread(Some(vec![]));
    let unmatched = desk.import(&file, None, None);
    assert_eq!(unmatched.id, log.id);
    assert!(unmatched.plays.iter().all(|p| p.entry.file.is_none()));
    assert_eq!(SetBook::load(&desk.store).unwrap().imported.len(), 1);
}

#[test]
fn suggestions_group_a_night_and_leave_out_checks_and_decided_sessions() {
    let desk = Desk::new();
    let lib = desk.library();
    desk.pull(&desk.backup);
    // A track check, saved by Main's first pull.
    common::add_history(&desk.main, "HISTORY 001", &[1, 2]);
    desk.pull(&desk.main);
    // One night, back to back: Backup was pulled after it, Main later, with a check it saved too.
    common::add_history(&desk.backup, "HISTORY 001", &[5, 6, 7, 8]);
    desk.pull(&desk.backup);
    common::add_history(&desk.main, "HISTORY 002", &[1, 2, 3, 4]);
    common::add_history(&desk.main, "HISTORY 003", &[9, 10]);
    desk.pull(&desk.main);
    // Played since, on both: not saved yet.
    common::add_history(&desk.main, "HISTORY 004", &[11, 12, 4]);
    common::add_history(&desk.backup, "HISTORY 002", &[5, 6]);
    let backup_id = Device::open(&desk.backup).unwrap().id().unwrap();
    fleet::set_role(&desk.store, backup_id, Some(Role::Set)).unwrap();
    report::save(
        &desk.store,
        &recording("Friday at the Warehouse", &[lib.track(1).unwrap(), lib.track(2).unwrap(), lib.track(3).unwrap()]),
    )
    .unwrap();
    // Logs from other nights: a set, a check, and a set that's been written up already.
    let titles = |ids: &[u32]| ids.iter().map(|&id| lib.track(id).unwrap()).collect::<Vec<_>>();
    let log = |name: &str, day: &str, tracks: Vec<&Track>| {
        let plays: Vec<(String, String, String, String)> = tracks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                (t.title.clone(), t.artist.clone(), format!("{day}T22:0{i}:00Z"), format!("{day}T22:0{i}:50Z"))
            })
            .collect();
        let plays: Vec<(&str, &str, &str, &str)> =
            plays.iter().map(|(a, b, c, d)| (a.as_str(), b.as_str(), c.as_str(), d.as_str())).collect();
        desk.import(&desk.log(name, &plays), Some(&desk.main), None).id
    };
    let old = log("January", "2025-01-10", titles(&[10, 11, 12]));
    let check = log("February", "2025-02-01", titles(&[4, 5]));
    let written_up = log("March", "2025-03-01", titles(&[6, 7, 8]));
    let mut book = SetBook::load(&desk.store).unwrap();
    book.sets.push(set_from(&written_up));
    book.save(&desk.store).unwrap();

    let got = desk.sessions(&SessionsRequest::default());
    let id = |name: &str, drive: &str| {
        got.sessions
            .iter()
            .find(|s| s.name == name && (s.drives.iter().chain(&s.first_seen_on).any(|d| d == drive)))
            .unwrap()
            .id
            .clone()
    };
    let night = [id("HISTORY 002", "Main"), id("HISTORY 003", "Main"), id("HISTORY 001", "Backup")];
    let unsaved = [id("HISTORY 004", "Main"), id("HISTORY 002", "Backup")];
    let first_check = id("HISTORY 001", "Main");

    let sorted = |mut ids: Vec<String>| {
        ids.sort();
        ids
    };
    let suggested = desk.suggest();
    let groups: Vec<Vec<String>> = suggested.iter().map(|c| sorted(c.sessions.clone())).collect();
    assert_eq!(groups, [sorted(unsaved.to_vec()), sorted(night.to_vec()), vec![old.clone()]]);
    let all: Vec<&String> = groups.iter().flatten().collect();
    assert!(![&first_check, &check, &written_up].iter().any(|s| all.contains(s)), "checks alone and sets already made");

    let [since, b2b, january] = &suggested[..] else { unreachable!() };
    assert_eq!((b2b.tracks, b2b.score), (10, 10 + 20 + 30 + 10));
    assert_eq!(
        b2b.reasons,
        [
            "10 tracks: at least 10 minutes of play",
            "from a drive you take to sets",
            "the recording Friday at the Warehouse has 3 of its tracks",
            "2 drives on the same night",
        ]
    );
    // When both windows hold it: after Main's previous pull, before Backup's.
    let window = |name: &str, drive: &str| got.sessions.iter().find(|s| s.id == id(name, drive)).unwrap().window;
    assert_eq!(b2b.window.after, window("HISTORY 002", "Main").after);
    assert_eq!(b2b.window.before, window("HISTORY 001", "Backup").before);
    assert_eq!((since.tracks, since.score), (5, 5 + 20 + 10));
    assert_eq!(since.window.after, window("HISTORY 004", "Main").after, "the later of the drives' last pulls");
    assert_eq!((january.tracks, january.score), (3, 3));
    assert_eq!(january.reasons, ["3 tracks: at least 3 minutes of play"]);
    assert!(january.window.exact);

    // Not a set: it's left out, until that's taken back.
    let dismiss = |undo: bool| {
        let req = DismissSessions { sessions: vec![old[..6].to_string()], undo };
        api::sessions_dismiss(&desk.store, &req, &Ctx::silent()).unwrap()
    };
    assert_eq!(dismiss(false), [old.as_str()]);
    assert_eq!(desk.suggest().len(), 2);
    let open = desk.sessions(&SessionsRequest { unassigned: true, ..Default::default() });
    assert!(!open.sessions.iter().any(|s| s.id == old || s.id == written_up));
    assert_eq!(open.sessions.len(), got.sessions.len() - 2);
    let all = desk.sessions(&SessionsRequest::default());
    let s = |id: &str| all.sessions.iter().find(|s| s.id == id).unwrap().clone();
    assert!(s(&old).dismissed && s(&written_up).sets.len() == 1);
    assert_eq!(dismiss(true), [old.as_str()]);
    assert_eq!(desk.suggest().len(), 3);
    let req = DismissSessions { sessions: vec!["HISTORY 001".into()], undo: false };
    assert_eq!(api::sessions_dismiss(&desk.store, &req, &Ctx::silent()).unwrap_err().code(), "invalid");
}

/// Saved by one pull, or with overlapping windows, sessions are one night only when what their
/// windows span is that short: otherwise each long enough is suggested on its own, saying the
/// night can't be told.
#[test]
fn sessions_are_only_grouped_when_their_windows_fit_one_night() {
    let desk = Desk::new();
    // A drive's first pull: its lists could be from any night before it.
    common::add_history(&desk.main, "HISTORY 001", &[1, 2, 3]);
    common::add_history(&desk.main, "HISTORY 002", &[4, 5, 6, 7]);
    common::add_history(&desk.main, "HISTORY 003", &[8]);
    let first = desk.pull(&desk.main);
    // Pulled again three days later: two lists from somewhere in those days.
    let manifest = desk.store.root().join(format!("snapshots/{}.json", first.id.to_hex()));
    let mut snapshot: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    snapshot["created"] = (first.created - chrono::TimeDelta::days(3)).to_rfc3339().into();
    std::fs::write(&manifest, serde_json::to_vec(&snapshot).unwrap()).unwrap();
    common::add_history(&desk.main, "HISTORY 004", &[9, 10, 11]);
    common::add_history(&desk.main, "HISTORY 005", &[12, 1, 2]);
    desk.pull(&desk.main);
    // And again straight after the next gig.
    common::add_history(&desk.main, "HISTORY 006", &[3, 4, 5]);
    common::add_history(&desk.main, "HISTORY 007", &[6, 7]);
    desk.pull(&desk.main);

    let got = desk.sessions(&SessionsRequest::default());
    let id = |name: &str| got.sessions.iter().find(|s| s.name == name).unwrap().id.clone();
    let suggested = desk.suggest();
    let mut groups: Vec<Vec<String>> = suggested
        .iter()
        .map(|c| {
            let mut ids = c.sessions.clone();
            ids.sort();
            ids
        })
        .collect();
    groups.sort();
    let mut tonight = vec![id("HISTORY 006"), id("HISTORY 007")];
    tonight.sort();
    let mut expected: Vec<Vec<String>> = ["HISTORY 001", "HISTORY 002", "HISTORY 004", "HISTORY 005"]
        .into_iter()
        .map(|name| vec![id(name)])
        .chain([tonight])
        .collect();
    expected.sort();
    assert_eq!(groups, expected, "the track check of the first pull is left out");
    for c in &suggested {
        let unknown = c.reasons.last().is_some_and(|r| r.starts_with("locallibrary can't tell which night"));
        assert_eq!(unknown, c.sessions.len() == 1, "{:?}", c.reasons);
    }
}

#[test]
fn the_sets_book_goes_with_copies_of_the_store() {
    let desk = Desk::new();
    let lib = desk.library();
    let t = lib.track(1).unwrap();
    let log = desk.import(
        &desk.log("Radio", &[(&t.title, &t.artist, "2026-10-02T20:00:00Z", "2026-10-02T20:05:00Z")]),
        Some(&desk.main),
        None,
    );
    let dest = desk.dir.path().join("copy");
    let copy = || offsite::copy(&desk.store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap();
    copy();
    let copied = Store::open(&dest).unwrap();
    assert_eq!(SetBook::load(&copied).unwrap().imported, std::slice::from_ref(&log));
    let req = DismissSessions { sessions: vec![log.id.clone()], undo: false };
    api::sessions_dismiss(&desk.store, &req, &Ctx::silent()).unwrap();
    copy();
    assert!(SetBook::load(&copied).unwrap().dismissed.contains(&log.id));
}

/// What rekordbox does when it imports a drive's history: the lists are gone from the drive.
fn rekordbox_imports_history(root: &Path) {
    let pdb = root.join(paths::EXPORT_PDB);
    let ids: Vec<u32> = pdb::read_library(&pdb).unwrap().history.iter().map(|h| h.id).collect();
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
    ed.remove_history_playlists(&ids).unwrap();
    ed.save(&pdb).unwrap();
}

/// A recording's report in which `tracks` are heard.
fn recording(title: &str, tracks: &[&Track]) -> MixReport {
    let play = |t: &Track| Play {
        track: t.clone(),
        start: 0.0,
        end: 300.0,
        track_start: 0.0,
        track_end: 300.0,
        tempo: 1.0,
        played_bpm: None,
        key_lock: None,
        semitones: 0.0,
        key: None,
        played_key: None,
        confidence: 0.9,
        votes: 100,
        presence: Vec::new(),
        presence_step: 2.0,
        jumps: 0,
        alternates: Vec::new(),
    };
    MixReport {
        mix: MixInfo {
            path: PathBuf::from(format!("/mixes/{title}.wav")),
            kind: "audio",
            title: title.into(),
            artist: None,
            genre: None,
            date: None,
            recorder: None,
            duration_secs: 3600.0,
            sample_rate: 44100,
            channels: 2,
            labels: Vec::new(),
            cover: None,
        },
        drive: "Main".into(),
        drive_id: None,
        library: Default::default(),
        plays: tracks.iter().map(|t| play(t)).collect(),
        transitions: Vec::new(),
        unidentified: Vec::new(),
        coverage: 0.9,
        waveform: Default::default(),
        tempo_range: 0.16,
        analysed_at: Utc::now(),
        elapsed_secs: 1.0,
    }
}

/// A set built from `session`.
fn set_from(session: &str) -> Set {
    let date = NaiveDate::from_ymd_opt(2025, 3, 1).unwrap();
    Set {
        id: locallibrary::sets::new_id(date).unwrap(),
        date,
        sessions: vec![session.to_string()],
        created_at: Utc::now(),
        updated_at: Utc::now(),
        ..Default::default()
    }
}
