use std::collections::BTreeSet;
use std::io::pipe;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use hegel::TestCase;
use hegel::generators as gs;
use loclib_model::model::Library;
use loclib_model::test_util::track;
use loclib_plugin::{Capability, Incoming, Manifest, PROTOCOL_VERSION, Peer, When, method};

use super::*;

/// The most tracks the host gives a plugin in a page of `drive.library`.
const PAGE: usize = 500;

const SAMPLES: &str = "/Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES";

/// A read callback: its method and params.
type Call = (String, Value);

/// The plugin served on a thread, over pipes, past `hello`, with a host whose drive `USB` has a
/// library (both its databases') and that answers its reads as locallibrary does.
struct Session {
    host: Peer,
    served: JoinHandle<()>,
    answering: JoinHandle<()>,
    calls: Arc<Mutex<Vec<Call>>>,
}

fn start(lib: Library) -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let calls: Arc<Mutex<Vec<Call>>> = Arc::default();
    let (to, log) = (host.clone(), calls.clone());
    let answering = std::thread::spawn(move || {
        for msg in incoming {
            let Incoming::Request(req) = msg else { continue };
            log.lock().unwrap().push((req.method.clone(), req.params.clone()));
            to.respond(&req.id, read(&lib, &req.method, req.params));
        }
    });
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "locallibrary", "version": "0.1.0" } });
    host.request::<_, Value>(method::HELLO, &hello).unwrap();
    Session { host, served, answering, calls }
}

/// A read as the host answers a plugin: only the params it may give, a drive it knows, and
/// `drive.library`'s tracks a page at a time.
fn read(lib: &Library, method: &str, params: Value) -> Result<Value, RpcError> {
    if method != "drive.library" {
        return Err(RpcError::failed("denied", format!("{method} needs a capability the plugin doesn't have")));
    }
    let Value::Object(params) = params else { return Err(RpcError::invalid_params("expected an object")) };
    if let Some(name) = params.keys().find(|k| !["drive", "combined", "offset", "limit"].contains(&k.as_str())) {
        return Err(RpcError::invalid_params(format!("a plugin doesn't give {method} `{name}`")));
    }
    if params["drive"] != "USB" {
        return Err(RpcError::failed("not_found", format!("not found: drive {}", params["drive"])));
    }
    let offset = params.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = params.get("limit").and_then(Value::as_u64).map_or(PAGE, |n| (n as usize).min(PAGE));
    let tracks: Vec<Track> = lib.tracks.iter().skip(offset).take(limit).cloned().collect();
    let page = json!({ "offset": offset, "count": tracks.len(), "total": lib.tracks.len() });
    Ok(json!({ "summary": lib.summary(), "library": Library { tracks, ..lib.clone() }, "page": page }))
}

impl Session {
    fn plan_on(&self, drive: Option<&str>) -> Result<OpPlan, RpcError> {
        let op = json!({ "operation": "clean", "context": { "drive": drive } });
        self.host.request(method::PLAN_OP, &op)
    }

    fn plan(&self) -> OpPlan {
        self.plan_on(Some("USB")).unwrap()
    }

    fn check(&self) -> Findings {
        let run = json!({ "check": "tracks", "context": { "drive": "USB" } });
        self.host.request(method::RUN_CHECK, &run).unwrap()
    }

    /// Closes the plugin's input: it stops. Answers the reads it called back for.
    fn stop(self) -> Vec<Call> {
        self.host.close();
        self.served.join().unwrap();
        self.answering.join().unwrap();
        Arc::try_unwrap(self.calls).unwrap().into_inner().unwrap()
    }
}

fn delete(paths: &[&str]) -> Value {
    json!({ "op": "delete_tracks", "tracks": { "paths": paths } })
}

/// Two demo tracks (spelled as rekordbox does, and not), one of them twice on one file, one
/// sharing its file with a track of the DJ's, and the DJ's own: one by rekordbox, one called
/// House 1.
fn drive() -> Library {
    let tracks = vec![
        track(1, "rekordbox", "House 1", &format!("{SAMPLES}/House1.wav")),
        track(2, " REKORDBOX ", "techno  1", &format!("{SAMPLES}/Techno1.wav")),
        track(3, "rekordbox", "House 1", &format!("{SAMPLES}/house1.WAV")),
        track(4, "rekordbox", "Keeper", "/Contents/rekordbox/Keeper.mp3"),
        track(5, "Bicep", "House 1", "/Contents/Bicep/House 1.mp3"),
        track(6, "rekordbox", "Breaks 1", "/Contents/rekordbox/Keeper.mp3"),
        // Only OneLibrary lists it: the combined library gives it an id from 2^31.
        track(1 << 31, "rekordbox", "Breaks 2", &format!("{SAMPLES}/Breaks2.wav")),
    ];
    Library { tracks, ..Default::default() }
}

/// Only plugins locallibrary ships have a top-level command; removing tracks needs
/// `library:delete`.
#[test]
fn the_manifest_is_a_first_party_one_with_a_check_and_a_destructive_operation() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    assert!(manifest.validate(true).is_ok());
    assert!(manifest.validate(false).is_err(), "only a plugin locallibrary ships has a top-level CLI command");
    let capabilities = [Capability::LibraryRead, Capability::LibraryPlan, Capability::LibraryDelete];
    assert_eq!(manifest.capabilities, capabilities);
    let check = &manifest.contributes.checks[0];
    assert_eq!((check.name.as_str(), check.default_on, check.when.as_slice()), ("tracks", true, &[When::Check][..]));
    let operation = &manifest.contributes.operations[0];
    assert_eq!((operation.name.as_str(), operation.cli.as_deref()), ("clean", Some("clean")));
    let s = start(Library::default());
    let described: Manifest = s.host.request(method::DESCRIBE, &Value::Null).unwrap();
    assert_eq!(described, manifest);
    s.stop();
}

/// The demo tracks go by file, each file once, OneLibrary's own too, reading both databases;
/// the DJ's tracks stay, and so does a demo track sharing a file with one of them, warned of.
#[test]
fn clean_deletes_the_demo_tracks_by_file_and_leaves_the_rest() {
    let s = start(drive());
    let plan = s.plan();
    assert_eq!(plan.summary, "remove 4 of rekordbox's demo track(s) and their files");
    let samples = [format!("{SAMPLES}/House1.wav"), format!("{SAMPLES}/Techno1.wav"), format!("{SAMPLES}/Breaks2.wav")];
    assert_eq!(plan.edits, [delete(&samples.each_ref().map(String::as_str))]);
    let shared =
        "1 demo track(s) share their audio file with another track and stay: deleting them would delete it too";
    assert_eq!(plan.warnings, [shared]);
    assert!(plan.inputs.is_empty());
    let calls = s.stop();
    assert!(calls.iter().all(|(m, p)| m == "drive.library" && p["combined"] == true), "{calls:?}");
}

/// The check finds what `clean` removes, fixed by it; a drive without any has nothing to report
/// or remove.
#[test]
fn the_check_finds_what_clean_removes() {
    let s = start(drive());
    let Findings { findings } = s.check();
    let [CheckFinding { finding, fix }] = findings.as_slice() else { panic!("one finding: {findings:?}") };
    assert_eq!((finding.level, finding.check.as_str()), (Level::Info, "tracks"));
    assert_eq!(finding.message, "4 of rekordbox's demo track(s), which its exports put back");
    assert_eq!(finding.tracks, [1, 2, 3, 1 << 31]);
    assert_eq!(fix.as_ref(), Some(&Fix { operation: "clean".into(), params: Value::Null }));
    s.stop();

    let mut lib = drive();
    lib.tracks.retain(|t| !is_demo(t) || t.id == 6);
    let s = start(lib);
    assert_eq!(s.check(), Findings::default());
    let plan = s.plan();
    assert_eq!(plan.summary, "no demo tracks from rekordbox on the drive");
    assert!(plan.edits.is_empty());
    s.stop();
}

/// Without a drive there's nothing to read.
#[test]
fn it_needs_a_drive() {
    let s = start(drive());
    let e = s.plan_on(None).unwrap_err();
    assert_eq!(e.message, "invalid params: say which drive");
    s.stop();
}

/// For any library, `clean` deletes exactly the files of the demo tracks no other track shares,
/// each once, and the check names exactly those tracks; every other track's file is left out.
#[hegel::test(test_cases = 100)]
fn clean_deletes_exactly_the_demo_tracks_files(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().max_value(12));
    let names = || {
        gs::sampled_from(vec![
            ("rekordbox", "House 1"),
            ("Rekordbox", "house  4"),
            ("rekordbox ", "Breaks 3"),
            ("rekordbox", "House 10"),
            ("Bicep", "Techno 1"),
            ("Kettama", "Glue"),
        ])
    };
    let files = || gs::sampled_from(vec!["House1.wav", "house1.wav", "Breaks3.wav", "Glue.mp3", "Techno.mp3"]);
    let tracks: Vec<Track> = (1..=n)
        .map(|id| {
            let (artist, title) = tc.draw(names());
            track(id, artist, title, &format!("{SAMPLES}/{}", tc.draw(files())))
        })
        .collect();
    let lib = Library { tracks, ..Default::default() };
    let demo = |t: &&Track| {
        let name = (normalize(&t.artist), normalize(&t.title));
        ["house 1", "house 4", "breaks 3"].iter().any(|title| name == ("rekordbox".into(), title.to_string()))
    };
    let kept: BTreeSet<String> = lib.tracks.iter().filter(|t| !demo(t)).map(|t| file_key(&t.file_path)).collect();
    let going: Vec<&Track> =
        lib.tracks.iter().filter(demo).filter(|t| !kept.contains(&file_key(&t.file_path))).collect();

    let s = start(lib.clone());
    let plan = s.plan();
    let deleted: Vec<String> = match plan.edits.as_slice() {
        [] => Vec::new(),
        [edit] => serde_json::from_value(edit["tracks"]["paths"].clone()).unwrap(),
        edits => panic!("one edit: {edits:?}"),
    };
    let keys: Vec<String> = deleted.iter().map(|p| file_key(p)).collect();
    assert_eq!(keys.iter().collect::<BTreeSet<_>>().len(), keys.len(), "each file once: {deleted:?}");
    let want: BTreeSet<String> = going.iter().map(|t| file_key(&t.file_path)).collect();
    assert_eq!(keys.into_iter().collect::<BTreeSet<_>>(), want);
    let found: Vec<u32> = s.check().findings.iter().flat_map(|f| f.finding.tracks.clone()).collect();
    assert_eq!(found, going.iter().map(|t| t.id).collect::<Vec<_>>());
    s.stop();
}
