//! Importers: what a file says about tracks found on the drive, added to them as an edit plan, and
//! the plays it logged kept as sessions.

use locallibrary::api::DriveRef;
use locallibrary::cue_edit::{self, CueChange, CueRef};
use locallibrary::edit::{Edit, TrackFields};
use locallibrary::harmony::Camelot;
use locallibrary::op::Planned;
use locallibrary::plugin::PluginImport;
use locallibrary::query::TrackSelector;
use locallibrary::sets::SetBook;
use locallibrary::{Device, Track};

use super::*;

/// The echo manifest with an importer, `given`, providing `provides` (TOML's list): the test
/// plugin answers a JSON file as it is.
fn given(provides: &str) -> String {
    ECHO.replace(
        "[[contributes.providers]]",
        &format!(
            "[[contributes.importers]]\nformat = \"given\"\ntitle = \"Given\"\nprovides = {provides}\n\
             options = {{ type = \"object\", properties = {{ blob = {{ type = \"string\" }} }} }}\n\n\
             [[contributes.providers]]"
        ),
    )
}

const EVERYTHING: &str = r#"["fields", "cues", "tags", "sessions"]"#;

impl Fixture {
    /// Imports `read` (what the importer answers) with `options`, from `read.json`.
    fn import(&self, read: &Value, options: Value) -> Result<Planned<PluginImport>> {
        let file = self._dir.path().join("read.json");
        std::fs::write(&file, read.to_string()).unwrap();
        self.plugins.import_plan("echo", "given", &file, "USB", options, Some(&self.store), &Ctx::silent())
    }

    /// The drive's tracks, the first without a key and the second without cues.
    fn bare(&self) -> Vec<Track> {
        let tracks = common::library(&self.drive).tracks;
        let ids = |t: &Track| TrackSelector { ids: vec![t.id], ..TrackSelector::default() };
        let clear = TrackFields { key: Some(String::new()), ..TrackFields::default() };
        let none = CueChange::Delete(CueRef::All);
        common::apply(
            &self.drive,
            &[
                Edit::EditTracks { tracks: ids(&tracks[0]), fields: clear },
                Edit::Cues { tracks: vec![tracks[1].file_path.clone()], change: none },
            ],
        );
        common::library(&self.drive).tracks
    }
}

/// What a file says the tracks it finds have, as the importer read it, and the plays it logged.
fn read(t: &[Track]) -> Value {
    json!({
        "tracks": [
            { "match": { "path": t[0].file_path }, "key": "8A", "bpm": 99.0 },
            {
                "match": { "artist": t[1].artist, "title": t[1].title },
                "cues": [{ "hot_cue": "A", "start_ms": 1000 }, { "start_ms": 5000, "name": "drop" }],
                "my_tags": ["Genre/Imported", " "],
            },
            { "match": { "title": "Nothing on this drive" }, "key": "1A" },
        ],
        "sessions": [
            {
                "name": "Friday",
                "started": "2026-10-03T22:00:00+02:00",
                "plays": [
                    { "artist": t[0].artist, "title": t[0].title, "path": t[0].file_path },
                    { "artist": "Someone", "title": "Else", "started_at": "2026-10-03T22:05:00+02:00" },
                ],
            },
            { "plays": [{ "artist": "No", "title": "Time" }] },
        ],
        "warnings": ["a line it couldn't read"],
    })
}

/// The tracks a file names are found on the drive and added to, as one edit plan, journaled: a key
/// where a track has none (a BPM it has kept), cues in slots it has none in, and My Tags. Its
/// sessions are kept in the store. Those it doesn't find, and what's there already, are left out,
/// saying so; the same file again adds nothing more, and keeps its sessions as they were.
#[test]
fn what_a_file_says_is_added_to_the_tracks_it_finds() {
    let f = Fixture::new(|dir| vec![spec(dir, &given(EVERYTHING), &[])], Options::default());
    let t = f.bare();
    let read = read(&t);
    let journal = || api::edits_list(&DriveRef { drive: common::path(&f.drive) }).unwrap().len();
    let before = journal();
    let planned = f.import(&read, Value::Null).unwrap();
    let file = f._dir.path().join("read.json");
    assert_eq!(&*planned.operation, "echo.import.given");
    assert_eq!(planned.summary[0], format!("import 2 of 3 track(s) found on USB from {}", file.display()));
    assert!(
        planned.summary.contains(&"keep 1 session(s) of 2 play(s), 1 of them found on the drive".to_string()),
        "{:?}",
        planned.summary
    );
    let warnings = [
        "read 2: left out, a play has no time and nor has the session",
        "1 of the 3 track(s) left out: 1 aren't on the drive, 0 could be several",
        "Given: a line it couldn't read",
    ];
    assert_eq!(planned.warnings, warnings);
    assert_eq!((planned.tracks.entries, planned.tracks.found, planned.tracks.missing), (3, 2, 1));
    assert!(!planned.destructive && !planned.noop);
    planned.apply(Some(&f.store), &Ctx::silent()).unwrap();

    let after = common::library(&f.drive).tracks;
    assert_eq!((after[0].key.clone(), after[0].bpm), (Camelot::parse("8A").unwrap().note(), t[0].bpm));
    let device = Device::open(&f.drive).unwrap();
    let cues = &cue_edit::list(&device, &[t[1].id]).unwrap()[0].cues;
    let said: Vec<_> = cues.iter().map(|c| (c.hot_cue.as_deref(), c.start_ms, c.name.as_str())).collect();
    assert_eq!(said, [(None, 5000, "drop"), (Some("A"), 1000, "")], "memory cues first, as a drive lists them");
    let tags = locallibrary::tags::read(&device).unwrap();
    let tagged = tags.categories.iter().flat_map(|c| &c.tags).find(|g| g.path == "Genre/Imported").unwrap();
    assert_eq!(tagged.tracks.iter().map(|r| r.id).collect::<Vec<_>>(), [t[1].id]);
    assert_eq!(journal(), before + 1, "journaled");
    let book = SetBook::load(&f.store).unwrap();
    let [session] = &book.imported[..] else { panic!("one session: {:?}", book.imported) };
    assert_eq!((session.name.as_str(), session.tool.as_str(), &session.file), ("Friday", "echo.given", &file));
    let plays: Vec<_> = session.plays.iter().map(|p| (p.entry.file.clone(), p.started.to_rfc3339())).collect();
    let at = |t: &str| chrono::DateTime::parse_from_rfc3339(t).unwrap().to_utc().to_rfc3339();
    assert_eq!(
        plays,
        [(Some(t[0].file_path.clone()), at("2026-10-03T22:00:00+02:00")), (None, at("2026-10-03T22:05:00+02:00"))]
    );
    assert_eq!(session.plays[1].entry.name(), "Someone - Else");

    let again = f.import(&read, Value::Null).unwrap();
    assert!(again.edits.is_noop(), "{:?}", again.summary);
    assert!(again.warnings.contains(&"2 cue(s) left out: their tracks have one there already, or end before".into()));
    assert_eq!(again.sessions[0].id, session.id, "the same log is the same session");
    again.apply(Some(&f.store), &Ctx::silent()).unwrap();
    assert_eq!(SetBook::load(&f.store).unwrap().imported.len(), 1);
}

/// What an importer answers in a file in its scratch folder is read as it would be in its answer:
/// a file outside the folder, or that isn't an answer, is refused.
#[test]
fn a_big_import_comes_in_a_file() {
    let f = Fixture::new(|dir| vec![spec(dir, &given(EVERYTHING), &[])], Options::default());
    let t = f.bare();
    let inline = f.import(&read(&t), Value::Null).unwrap();
    let blob = f.import(&read(&t), json!({ "blob": "read.json" })).unwrap();
    assert_eq!((blob.summary.clone(), blob.warnings.clone()), (inline.summary.clone(), inline.warnings.clone()));
    let e = f.import(&read(&t), json!({ "blob": "../read.json" })).err().unwrap();
    assert!(failure(e).contains("its answer: \"../read.json\" isn't a file's name in the scratch folder"));
    let e = f.import(&json!({ "tracks": 5 }), json!({ "blob": "read.json" })).err().unwrap();
    assert!(failure(e).contains("its answer in read.json isn't one: invalid type: integer `5`"));
}

/// An importer's answer with what its manifest doesn't say it provides is refused, and so is a
/// time that isn't one.
#[test]
fn an_importer_answers_only_what_it_provides() {
    let f = Fixture::new(|dir| vec![spec(dir, &given(r#"["tags"]"#), &[])], Options::default());
    let t = common::library(&f.drive).tracks;
    let refused = |read: Value| failure(f.import(&read, Value::Null).err().unwrap());
    let key = json!({ "tracks": [{ "match": { "path": t[0].file_path }, "key": "8A" }] });
    assert!(refused(key).contains("it read fields, which its manifest doesn't say it provides"));
    assert!(refused(read(&t)).contains("it read fields"));
    let sessions = json!({ "sessions": [{ "plays": [] }] });
    assert!(refused(sessions).contains("it read sessions, which its manifest doesn't say it provides"));

    let f = Fixture::new(|dir| vec![spec(dir, &given(EVERYTHING), &[])], Options::default());
    let time = json!({ "sessions": [{ "plays": [{ "artist": "A", "title": "B", "started_at": "Friday" }] }] });
    let e = failure(f.import(&time, Value::Null).err().unwrap());
    assert!(e.contains("its session 0: \"Friday\" isn't a time (RFC 3339)"), "{e}");
}

/// However many cues a file has for a track, they're one edit of it, made at once.
#[test]
fn a_tracks_cues_are_one_edit() {
    let f = Fixture::new(|dir| vec![spec(dir, &given(EVERYTHING), &[])], Options::default());
    let t = f.bare();
    let hot = ["A", "B", "C", "D", "E", "F", "G", "H"].map(|h| json!({ "hot_cue": h, "start_ms": 1000 }));
    let memory = (0..200).map(|i| json!({ "start_ms": 2000 + i * 150 }));
    let cues: Vec<Value> = hot.into_iter().chain(memory).collect();
    assert!(t[1].duration_secs * 1000 > 2000 + 200 * 150, "the track is long enough");
    let read = json!({ "tracks": [{ "match": { "path": t[1].file_path }, "cues": cues }] });
    let planned = f.import(&read, Value::Null).unwrap();
    assert_eq!(planned.edits.changes, ["set 208 cue(s) on 1 track(s)"]);
    planned.apply(Some(&f.store), &Ctx::silent()).unwrap();
    let device = Device::open(&f.drive).unwrap();
    assert_eq!(cue_edit::list(&device, &[t[1].id]).unwrap()[0].cues.len(), 208);
    let journal = api::edits_list(&DriveRef { drive: common::path(&f.drive) }).unwrap();
    let last = journal.last().unwrap();
    assert_eq!(last.changes, ["set 208 cue(s) on 1 track(s)"], "journaled as one edit");
}

/// Of a track's cues, those the cue edit can't set (a slot that isn't one, a colour, a loop
/// length or a name it doesn't take, a loop past the end), and every one for a track that can't
/// hold cues (without analysis files), are left out, saying so for each track and why; the rest
/// are set, as one edit.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_cue_the_edit_cant_set_is_left_out(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let f = Fixture::new(|dir| vec![spec(dir, &given(EVERYTHING), &[])], Options::default());
    let t = f.bare();
    let end = t[1].duration_secs * 1000;
    let wrong = [
        (json!({ "hot_cue": "Z", "start_ms": 1500 }), "hot cues are A-H, not \"Z\""),
        (json!({ "hot_cue": "B", "start_ms": 2000, "color": "chartreuse" }), "unknown colour \"chartreuse\""),
        (json!({ "hot_cue": "C", "start_ms": 3000, "loop_beats": 3.3 }), "not 3.3"),
        (json!({ "hot_cue": "D", "start_ms": end - 1000, "loop_beats": 512 }), "is past the end"),
        (json!({ "start_ms": 4000, "name": "x".repeat(0x10001) }), "65536 bytes"),
    ];
    let wrong: Vec<_> = wrong.into_iter().filter(|_| tc.draw(gs::booleans())).collect();
    let good = [json!({ "hot_cue": "A", "start_ms": 1000 }), json!({ "start_ms": 5000, "name": "kept" })];
    let mut cues: Vec<Value> = wrong.iter().map(|(c, _)| c.clone()).chain(good).collect();
    let first = tc.draw(gs::integers::<usize>().max_value(cues.len() - 1));
    cues.swap(0, first);
    // And at times a track without analysis files, which can't hold the cue given it.
    let unanalysed = tc.draw(gs::booleans());
    if unanalysed {
        std::fs::remove_file(locallibrary::paths::to_fs(&f.drive, &locallibrary::paths::key(&t[0].analyze_path)))
            .unwrap();
    }
    let read = json!({ "tracks": [
        { "match": { "path": t[1].file_path }, "cues": cues },
        { "match": { "path": t[0].file_path }, "cues": [{ "hot_cue": "H", "start_ms": 1000 }] },
    ] });
    let planned = f.import(&read, Value::Null).unwrap();
    let said = |t: &Track| planned.warnings.iter().find(|w| w.starts_with(&format!("{}: ", t.display_name())));
    match said(&t[1]) {
        None => assert!(wrong.is_empty(), "{:?}", planned.warnings),
        Some(said) => {
            assert!(said.starts_with(&format!("{}: {} cue(s) left out: ", t[1].display_name(), wrong.len())));
            for (_, why) in &wrong {
                assert!(said.contains(why), "{why}: {said}");
            }
        }
    }
    assert_eq!(said(&t[0]).is_some_and(|w| w.contains("can't hold cues")), unanalysed, "{:?}", planned.warnings);
    let t0 = (!unanalysed).then_some("set 1 cue(s) on 1 track(s)");
    let changes: Vec<&str> = std::iter::once("set 2 cue(s) on 1 track(s)").chain(t0).collect();
    assert_eq!(planned.edits.changes, changes);
    planned.apply(Some(&f.store), &Ctx::silent()).unwrap();
    let device = Device::open(&f.drive).unwrap();
    let cues = &cue_edit::list(&device, &[t[1].id]).unwrap()[0].cues;
    let set: Vec<_> = cues.iter().map(|c| (c.hot_cue.as_deref(), c.start_ms)).collect();
    assert_eq!(set, [(None, 5000), (Some("A"), 1000)]);
}

/// A session a file has twice (the same plays at the same times) is kept once.
#[test]
fn a_session_twice_in_a_file_is_kept_once() {
    let f = Fixture::new(|dir| vec![spec(dir, &given(EVERYTHING), &[])], Options::default());
    let log =
        json!({ "name": "Friday", "plays": [{ "artist": "A", "title": "B", "started_at": "2026-10-03T22:00:00Z" }] });
    let read = json!({ "sessions": [log, { "name": "Again", "plays": log["plays"] }] });
    let planned = f.import(&read, Value::Null).unwrap();
    assert_eq!(planned.sessions.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Friday"]);
    assert_eq!(planned.warnings, ["Again: left out, its plays are another session's"]);
}

/// The file is one on this computer, and the importer one the plugin has.
#[test]
fn an_import_reads_a_file_the_plugin_has_an_importer_for() {
    let f = Fixture::new(|dir| vec![spec(dir, &given(EVERYTHING), &[])], Options::default());
    let plan = |importer: &str, file: &Path| {
        f.plugins.import_plan("echo", importer, file, "USB", Value::Null, Some(&f.store), &Ctx::silent())
    };
    let e = plan("given", Path::new("read.json")).err().unwrap();
    assert_eq!(e.to_string(), "the file to import must be an absolute path, not read.json");
    let e = plan("given", f._dir.path()).err().unwrap();
    assert_eq!(e.to_string(), format!("{} isn't a file", f._dir.path().display()));
    let e = plan("nope", f._dir.path()).err().unwrap();
    assert_eq!(e.code(), "not_found", "{e}");
    // Echo's own importer reads a list of audio files, a line each.
    let list = f._dir.path().join("list.txt");
    let t = common::library(&f.drive).tracks;
    std::fs::write(&list, format!("{}\n/Contents/nowhere.mp3\n", t[0].file_path)).unwrap();
    let planned = plan("lines", &list).unwrap();
    assert_eq!(planned.summary[0], format!("import 1 of 2 track(s) found on USB from {}", list.display()));
    // Without a store, sessions have nowhere to go.
    let read =
        json!({ "sessions": [{ "plays": [{ "artist": "A", "title": "B", "started_at": "2026-10-03T22:00:00Z" }] }] });
    let file = f._dir.path().join("read.json");
    std::fs::write(&file, read.to_string()).unwrap();
    let e = f.plugins.import_plan("echo", "given", &file, "USB", Value::Null, None, &Ctx::silent()).err().unwrap();
    assert_eq!(e.to_string(), "sessions are kept in the store, and there's none");
}

/// For any answer an importer gives, the plan counts the tracks it finds on the drive as the
/// matcher finds them by file, keeps each session with a time for every play, and applied, only
/// adds: every track keeps the key, BPM, cues and My Tags it had, a track without a key gets the
/// first one given for it, and every tag given a track found is on it.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_import_only_adds_to_what_tracks_have(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let f = Fixture::new(|dir| vec![spec(dir, &given(EVERYTHING), &[])], Options::default());
    let before = f.bare();
    let device = Device::open(&f.drive).unwrap();
    let cues_of = |t: &Track| {
        let listed = cue_edit::list(&device, &[t.id]).unwrap().remove(0).cues;
        listed.into_iter().map(|c| (c.hot_cue, c.start_ms)).collect::<Vec<_>>()
    };
    let had: Vec<_> = before.iter().map(cues_of).collect();
    let codes: Vec<String> = (1..=12).flat_map(|n| [format!("{n}A"), format!("{n}B")]).collect();
    let n = tc.draw(gs::integers::<usize>().max_value(5));
    let (mut tracks, mut found, mut keys) = (Vec::new(), 0, vec![None; before.len()]);
    for _ in 0..n {
        let on = tc.draw(gs::optional(gs::integers::<usize>().max_value(before.len() - 1)));
        let path = on.map_or_else(|| "/Contents/nowhere/x.mp3".to_string(), |i| before[i].file_path.clone());
        let key = tc.draw(gs::optional(gs::sampled_from(codes.clone())));
        let hot = || gs::optional(gs::sampled_from(vec!["A", "D", "H"]));
        let cues: Vec<Value> = (0..tc.draw(gs::integers::<usize>().max_value(2)))
            .map(|_| json!({ "hot_cue": tc.draw(hot()), "start_ms": tc.draw(gs::integers::<u32>().max_value(9_000)) }))
            .collect();
        let tag = format!("Genre/Imp{}", tc.draw(gs::integers::<u8>().max_value(2)));
        if let Some(i) = on {
            found += 1;
            keys[i] = keys[i].clone().or(key.clone());
        }
        tracks.push(json!({ "match": { "path": path }, "key": key, "cues": cues, "my_tags": [tag], "on": on }));
    }
    // And somewhere among them, what each track has already: another key (for one with a key), a
    // BPM, and a cue in each of its hot cue slots and at each place it has one.
    for (i, t) in before.iter().enumerate() {
        let key = Camelot::parse(&t.key).map(|k| k.parallel().code());
        let cues: Vec<Value> = had[i].iter().map(|(hot, at)| json!({ "hot_cue": hot, "start_ms": at + 50 })).collect();
        let over = json!({ "match": { "path": t.file_path }, "key": key, "bpm": t.bpm + 1.0, "cues": cues, "on": i });
        tracks.insert(tc.draw(gs::integers::<usize>().max_value(tracks.len())), over);
        found += 1;
    }
    let n = tracks.len();
    let logs: Vec<Value> = (0..tc.draw(gs::integers::<usize>().max_value(2)))
        .map(|i| {
            let timed = tc.draw(gs::booleans());
            let plays: Vec<Value> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)))
                .map(|j| {
                    let at = timed.then(|| format!("2026-10-03T22:0{j}:00Z"));
                    json!({ "artist": "A", "title": format!("T{i}{j}"), "started_at": at })
                })
                .collect();
            json!({ "plays": plays, "timed": timed })
        })
        .collect();
    let read = json!({ "tracks": tracks, "sessions": logs });
    let planned = f.import(&read, Value::Null).unwrap();
    assert_eq!((planned.tracks.entries, planned.tracks.found, planned.tracks.missing), (n, found, n - found));
    let kept = logs.iter().filter(|l| l["timed"] == true).count();
    assert_eq!(planned.sessions.len(), kept);
    assert!(!planned.destructive, "an import only adds");
    if !planned.noop {
        planned.apply(Some(&f.store), &Ctx::silent()).unwrap();
    }

    let after = common::library(&f.drive).tracks;
    let tags = locallibrary::tags::read(&Device::open(&f.drive).unwrap()).unwrap();
    for (i, (b, a)) in before.iter().zip(&after).enumerate() {
        let key = match (b.key.is_empty(), &keys[i]) {
            (true, Some(code)) => Camelot::parse(code).unwrap().note(),
            _ => b.key.clone(),
        };
        assert_eq!((&a.key, a.bpm), (&key, b.bpm), "track {i}");
        let now = cues_of(a);
        assert!(had[i].iter().all(|c| now.contains(c)), "track {i} kept its cues: {:?} -> {now:?}", had[i]);
        for tag in tracks.iter().filter(|t| t["on"] == json!(i)).filter_map(|t| t["my_tags"][0].as_str()) {
            let on = tags.categories.iter().flat_map(|c| &c.tags).find(|g| g.path == tag);
            assert!(on.is_some_and(|g| g.tracks.iter().any(|r| r.id == a.id)), "track {i} has {tag}");
        }
    }
    assert_eq!(SetBook::load(&f.store).unwrap().imported.len(), kept);
}
