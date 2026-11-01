use std::io::pipe;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use hegel::TestCase;
use hegel::generators as gs;
use loclib_plugin::{Manifest, Pending};
use serde_json::json;

use super::*;
use crate::Error;
use crate::error::{Action, Reason};
use crate::plugin::process::Host;
use crate::plugin::{Launch, Options};

/// A plugin granted `capabilities`.
fn granted(capabilities: &[&str]) -> Manifest {
    let list: Vec<String> = capabilities.iter().map(|c| format!("{c:?}")).collect();
    let head = "id = \"p\"\nname = \"P\"\nversion = \"0.1.0\"\nprotocol = \"^0.2\"";
    Manifest::parse(&format!("{head}\ncapabilities = [{}]\n", list.join(", "))).unwrap()
}

/// Reads with two drives, `USB` and `SD`, under `/Volumes`.
struct Drives;

impl Reads for Drives {
    fn read(&self, _: &str, params: Value, _: &Ctx<'_>) -> Result<Value, RpcFailure> {
        Ok(params)
    }

    fn drive(&self, spec: &str) -> Result<PathBuf> {
        match spec {
            "USB" | "SD" => Ok(PathBuf::from("/Volumes").join(spec)),
            other => Err(Error::NotFound(format!("drive {other:?}"))),
        }
    }
}

fn check(m: &Manifest, method: &str, params: Value) -> Result<Value, RpcFailure> {
    super::check(m, &|spec| Drives.drive(spec), method, params)
}

/// One read for each capability: refused without it, let through with it.
#[test]
fn each_read_needs_its_capability() {
    let reads = [
        ("tracks.query", "library:read"),
        ("tracks.analysis", "analysis:read"),
        ("art.get", "artwork:read"),
        ("drive.info", "drive:info"),
        ("doctor", "health:read"),
        ("store.info", "store:read"),
        ("settings.profile", "settings:read"),
        ("plays.stats", "plays:read"),
        ("sets.list", "sets:read"),
        ("account.status", "account:status"),
        ("collection.info", "collection:read"),
    ];
    let none = granted(&[]);
    for (method, capability) in reads {
        let e = check(&none, method, json!({})).unwrap_err();
        let said = format!("{method} needs the {capability} capability, which the p plugin doesn't have");
        assert_eq!((e.kind, e.message.as_str()), ("denied", said.as_str()));
        assert!(check(&granted(&[capability]), method, json!({})).is_ok(), "{method}");
    }
    // Writes, reads naming files here, and reads whose answers can't be bounded, never,
    // whatever is granted.
    let names: Vec<String> = Capability::NAMED.iter().map(Capability::to_string).collect();
    let all = granted(&names.iter().map(String::as_str).collect::<Vec<_>>());
    for method in ["edit.plan", "sync.apply", "version", "art.path", "edits.list", "plays.history", "drive.log", ""] {
        let e = check(&all, method, json!({})).unwrap_err();
        assert_eq!((e.code, e.kind), (code::METHOD_NOT_FOUND, "method_not_found"), "{method}");
    }
}

/// Each read takes only the params listed for it: a param it has that a plugin mustn't give (a
/// file, a server, a secret, another drive to read), or one it gains later, is refused.
#[test]
fn reads_take_only_the_params_listed() {
    let m = granted(&["library:read", "health:read", "collection:read"]);
    for (method, name) in [
        ("tracks.query", "nonsense"),
        ("drive.verify", "against"),
        ("collection.info", "db"),
        ("collection.info", "key"),
        ("track.show", "filter"),
        ("cues.list", "query"),
    ] {
        let mut params = json!({ name: "x" });
        if method != "collection.info" {
            params["drive"] = json!("USB");
        }
        let e = check(&m, method, params).unwrap_err();
        assert_eq!(e.message, format!("invalid params: a plugin doesn't give {method} `{name}`"));
    }
    assert!(check(&m, "drive.verify", json!({ "drive": "USB", "snapshot": "3lza" })).is_ok());
    let e = check(&m, "tracks.query", json!(["USB"])).unwrap_err();
    assert_eq!(e.message, "invalid params: expected an object");
}

/// Drives are found by the host, and the app given their roots.
#[test]
fn drives_are_found_by_the_host() {
    let m = granted(&["library:read", "health:read", "plays:read"]);
    // A root as the platform joins it (`\Volumes\USB` on Windows).
    let root = |name: &str| json!(Path::new("/Volumes").join(name));
    let given = check(&m, "tracks.query", json!({ "drive": "USB", "combined": true })).unwrap();
    assert_eq!((given["drive"].clone(), given["combined"].clone()), (root("USB"), json!(true)));
    assert_eq!(
        check(&m, "check", json!({ "drives": ["USB", "SD"] })).unwrap()["drives"],
        json!([root("USB"), root("SD")])
    );
    assert_eq!(check(&m, "plays.stats", json!({ "source": "SD" })).unwrap()["source"], root("SD"));
    assert_eq!(check(&m, "doctor", json!({ "drive": null })).unwrap()["drive"], Value::Null);
    let e = check(&m, "tracks.query", json!({ "drive": "/Volumes/USB" })).unwrap_err();
    assert_eq!((e.kind, e.message.as_str()), ("not_found", "not found: drive \"/Volumes/USB\""));
    let e = check(&m, "check", json!({ "drives": ["USB", "Elsewhere"] })).unwrap_err();
    assert_eq!(e.kind, "not_found");
    let e = check(&m, "tracks.query", json!({ "drive": 4 })).unwrap_err();
    assert_eq!(e.message, "invalid params: `drive` names a drive, which 4 doesn't");
}

#[test]
fn reads_come_in_pages_of_a_bounded_size() {
    let m = granted(&["library:read", "analysis:read", "artwork:read", "store:read", "plays:read"]);
    let limit = |method: &str, params: Value| check(&m, method, params).unwrap()["limit"].clone();
    assert_eq!(limit("tracks.query", json!({ "drive": "USB" })), 500);
    assert_eq!(limit("tracks.query", json!({ "drive": "USB", "limit": 20 })), 20);
    assert_eq!(limit("drive.library", json!({ "drive": "USB", "limit": 100_000 })), 500);
    assert_eq!(limit("drive.library", json!({ "drive": "USB", "limit": "all" })), 500);
    assert_eq!(limit("track.matches", json!({ "drive": "USB", "track": 1 })), 500);
    assert_eq!(limit("store.history", json!({ "library": "Main" })), 500);
    assert_eq!(limit("plays.stats", json!({ "source": "USB", "limit": 3 })), 3);
    assert_eq!(limit("playlists.tree", json!({ "drive": "USB" })), Value::Null, "only paged reads");

    let ids = |n: u32| (0..n).collect::<Vec<_>>();
    assert!(check(&m, "tracks.get", json!({ "drive": "USB", "ids": ids(1000) })).is_ok());
    let e = check(&m, "tracks.get", json!({ "drive": "USB", "ids": ids(1001) })).unwrap_err();
    assert_eq!(e.message, "invalid params: `ids` has 1001 items: at most 1000 at a time");
    assert!(check(&m, "track.fields", json!({ "drive": "USB", "tracks": ids(1001) })).is_err());
    assert!(check(&m, "track.show", json!({ "drive": "USB", "ids": ids(100) })).is_ok());
    assert!(check(&m, "cues.list", json!({ "drive": "USB", "paths": vec!["/a.mp3"; 101] })).is_err());
    assert!(check(&m, "tracks.analysis", json!({ "drive": "USB", "ids": ids(101) })).is_err());
    assert!(check(&m, "art.get", json!({ "drive": "USB", "tracks": ids(50) })).is_ok());
    assert!(check(&m, "art.get", json!({ "drive": "USB", "tracks": ids(51) })).is_err());
    assert!(check(&m, "art.get", json!({ "drive": "USB", "track": 1 })).is_ok());

    let waveform = |params: Value| check(&m, "track.analysis", params).unwrap()["waveform"].clone();
    assert_eq!(waveform(json!({ "drive": "USB", "track": 1 })), "none", "no waveforms unless asked for");
    assert_eq!(waveform(json!({ "drive": "USB", "track": 1, "waveform": "preview" })), "preview");
}

#[test]
fn errors_reach_plugins_without_paths_or_actions() {
    let roots: Vec<(String, String)> = [
        ("/Volumes/NO NAME", "NO NAME"),
        ("/Users/al", "~"),
        ("C:\\Users\\John Smith", "~"),
        ("C:/Users/John Smith", "~"),
        ("/var/store", "<store>"),
    ]
    .into_iter()
    .map(|(r, s)| (r.to_string(), s.to_string()))
    .collect();
    let path = "/Volumes/NO NAME/PIONEER/rekordbox/export.pdb";
    let io = Error::Io { path: path.into(), source: std::io::Error::other("gone") };
    let io = io
        .with("drives", ["/Volumes/NO NAME", "/Users/al/Music"])
        .action(Action::retry(json!({ "force": true }), "Force it"));
    let e = for_plugin(RpcFailure::from(io), &roots);
    assert_eq!((e.code, e.message.as_str()), (code::FAILED, "NO NAME/PIONEER/rekordbox/export.pdb: gone"));
    let data = serde_json::to_value(e.data.unwrap()).unwrap();
    let fields = json!({ "path": "NO NAME/PIONEER/rekordbox/export.pdb", "drives": ["NO NAME", "~/Music"] });
    assert_eq!(data, json!({ "kind": "io", "fields": fields }), "and no actions");

    let message =
        r"C:\Users\John Smith\x and key=/Users/al/y, file:///C:/Users/John Smith/z; /Users/alex and /var/store";
    let e = for_plugin(RpcFailure::from(Error::Conflict(message.into()).because(Reason::Diverged)), &roots);
    assert_eq!(e.message, r"~\x and key=~/y, file:///~/z; /Users/alex and <store>");
    assert_eq!(e.data.unwrap().reason.as_deref(), Some("diverged"));
}

/// The roots here are said otherwise: the temporary folder and the home folder at least.
#[test]
fn the_roots_here_are_known() {
    let roots = known_roots(None, &[]);
    let temp = std::env::temp_dir().to_string_lossy().trim_end_matches(['/', '\\']).to_string();
    assert!(roots.contains(&(temp, "<temp>".into())), "{roots:?}");
    let home = directories::BaseDirs::new().unwrap().home_dir().to_string_lossy().into_owned();
    assert!(roots.contains(&(home, "~".into())), "{roots:?}");
    assert!(roots.windows(2).all(|w| w[0].0.len() >= w[1].0.len()), "longest first");
}

/// A read as the spec has it: its method, the capability it needs, whether it reads a library,
/// and its other params.
type Read = (&'static str, &'static str, bool, &'static [(&'static str, Param)]);

/// What the host makes of a param: kept as it is or made something else, or refused.
type Made = Result<Option<Value>, (&'static str, String)>;

/// Every read a plugin may call, as the plugin host's spec has it: the capability it needs,
/// whether it reads a drive's library (and so takes `drive` and `combined`), and its other params.
const SPEC: &[Read] = {
    use Param::*;
    &[
        ("drive.library", "library:read", true, &[("offset", Plain), ("limit", Limit(500))]),
        ("playlists.tree", "library:read", true, &[]),
        ("tracks.get", "library:read", true, &[("ids", AtMost(1000))]),
        (
            "tracks.query",
            "library:read",
            true,
            &[("filter", Plain), ("playlist", Plain), ("sort", Plain), ("offset", Plain), ("limit", Limit(500))],
        ),
        ("track.show", "library:read", true, &[("ids", AtMost(100)), ("paths", AtMost(100))]),
        ("cues.list", "library:read", true, &[("ids", AtMost(100)), ("paths", AtMost(100))]),
        ("track.matches", "library:read", true, &[("track", Plain), ("limit", Limit(500))]),
        ("tags.list", "library:read", false, &[("drive", Drive)]),
        ("track.analysis", "analysis:read", true, &[("track", Plain), ("waveform", Or("none")), ("encoding", Plain)]),
        ("tracks.analysis", "analysis:read", true, &[("ids", AtMost(100)), ("waveform", Plain), ("encoding", Plain)]),
        ("track.fields", "analysis:read", true, &[("tracks", AtMost(1000)), ("plugin", Plain)]),
        ("art.get", "artwork:read", true, &[("track", Plain), ("tracks", AtMost(50))]),
        ("drive.info", "drive:info", false, &[("drive", Drive)]),
        ("drive.backups", "drive:info", false, &[("drive", Drive)]),
        ("drive.space", "drive:info", false, &[("drive", Drive)]),
        ("drives.list", "drive:info", false, &[("all", Plain)]),
        ("drives.mounted", "drive:info", false, &[]),
        ("drive.groups", "drive:info", false, &[]),
        ("fleet.status", "drive:info", false, &[("all", Plain), ("retired", Plain)]),
        (
            "doctor",
            "health:read",
            false,
            &[
                ("drive", Drive),
                ("hash_audio", Plain),
                ("by_sound", Plain),
                ("only", Plain),
                ("audio", Plain),
                ("analyze", Plain),
                ("sure_only", Plain),
            ],
        ),
        ("check", "health:read", false, &[("drives", Drives), ("players", Plain), ("audio", Plain)]),
        ("drive.verify", "health:read", false, &[("drive", Drive), ("snapshot", Plain)]),
        ("store.info", "store:read", false, &[]),
        ("store.copies", "store:read", false, &[]),
        ("library.list", "store:read", false, &[]),
        ("smart.list", "store:read", false, &[]),
        ("store.history", "store:read", false, &[("library", Plain), ("limit", Limit(500))]),
        ("snapshot.library", "store:read", false, &[("snapshot", Plain), ("offset", Plain), ("limit", Limit(500))]),
        ("settings.schema", "settings:read", false, &[]),
        ("settings.profile", "settings:read", false, &[]),
        ("players.models", "settings:read", false, &[]),
        ("players.show", "settings:read", false, &[]),
        ("settings.show", "settings:read", false, &[("drive", Drive)]),
        ("player.show", "settings:read", false, &[("drive", Drive)]),
        ("plays.stats", "plays:read", false, &[("source", Drive), ("since", Plain), ("limit", Limit(500))]),
        ("sets.played", "plays:read", false, &[("track", Plain)]),
        ("sets.list", "sets:read", false, &[("venue", Plain), ("kind", Plain), ("since", Plain), ("until", Plain)]),
        ("sets.get", "sets:read", false, &[("set", Plain), ("venue", Plain)]),
        ("sessions.list", "sets:read", false, &[("drive", Drive), ("unassigned", Plain)]),
        ("mix.reports.get", "sets:read", false, &[("id", Plain)]),
        ("account.status", "account:status", false, &[]),
        ("collection.info", "collection:read", false, &[]),
        ("collection.diff", "collection:read", false, &[("drive", Drive), ("playlist", Plain)]),
    ]
};

/// Methods no plugin calls, whatever it's granted: writes, reads naming files on this computer,
/// and reads whose answers can't be bounded.
const NEVER: &[&str] = &[
    "edit.plan",
    "edit.apply",
    "sync.apply",
    "plan.apply",
    "version",
    "art.path",
    "edits.list",
    "plays.history",
    "drive.log",
    "tracks.query ",
    "TRACKS.QUERY",
    "",
];

/// Params that name files, servers or secrets: never given by a plugin, to any read.
const REFUSED: &[&str] = &[
    "db",
    "file",
    "image",
    "files",
    "mix",
    "folders",
    "recording",
    "output",
    "path",
    "to",
    "location_root",
    "pds",
    "from",
    "client_id",
    "url",
    "key",
    "session_file",
];

/// The params a read takes, as the spec lists them: a library's first.
fn takes(library: bool, others: &[(&'static str, Param)]) -> Vec<(&'static str, Param)> {
    let library = if library { &[("drive", Param::Drive), ("combined", Param::Plain)][..] } else { &[] };
    library.iter().chain(others).copied().collect()
}

/// A value a plugin might give a param of the kind `param` (or of any kind): drives by name and
/// by path, lists around their cap, page sizes around theirs, and things of the wrong type.
#[hegel::test_helper]
fn given(tc: &TestCase, param: Option<Param>) -> Value {
    let drive = || {
        tc.draw(gs::sampled_from(vec![
            json!("USB"),
            json!("SD"),
            Value::Null,
            json!("USB"),
            json!("/Volumes/USB"),
            json!("Elsewhere"),
            Value::Null,
            json!(4),
        ]))
    };
    match param {
        Some(Param::Drive) => drive(),
        Some(Param::Drives) => match tc.draw(gs::booleans()) {
            true => Value::Array((0..tc.draw(gs::integers::<usize>().max_value(3))).map(|_| drive()).collect()),
            false => drive(),
        },
        Some(Param::AtMost(most)) => {
            // Mostly at the cap, or just past it.
            let n = tc.draw(gs::sampled_from(vec![most, most + 1, most, most + 1, 0, most * 2]));
            match tc.draw(gs::integers::<u8>().max_value(3)) {
                0 => json!("all"),
                _ => json!((0..n).collect::<Vec<_>>()),
            }
        }
        Some(Param::Limit(most)) => {
            let n = tc.draw(gs::integers::<u64>().max_value(most * 2));
            tc.draw(gs::sampled_from(vec![json!(n), json!(most), json!(u64::MAX), json!("all"), json!(-1), json!(2.5)]))
        }
        _ => tc.draw(gs::sampled_from(vec![json!("x"), json!(1), json!(true), Value::Null, json!(["/Volumes/USB"])])),
    }
}

/// What the host makes of a value given for a param of the kind `param`: kept as it's given
/// (`Ok(None)` when it's left out), made something else, or refused with a kind and message.
fn made(name: &str, param: Param, value: Option<&Value>) -> Made {
    let drive = |v: &Value| match v {
        Value::Null => Ok(Value::Null),
        Value::String(s) if s == "USB" || s == "SD" => Ok(json!(Path::new("/Volumes").join(s))),
        Value::String(s) => Err(("not_found", format!("not found: drive {s:?}"))),
        other => Err(("invalid_params", format!("invalid params: `{name}` names a drive, which {other} doesn't"))),
    };
    match (param, value) {
        (Param::Limit(most), v) => Ok(Some(json!(v.and_then(Value::as_u64).map_or(most, |n| n.min(most))))),
        (Param::Or(default), None) => Ok(Some(json!(default))),
        (_, None) => Ok(None),
        (Param::Drive, Some(v)) => drive(v).map(Some),
        (Param::Drives, Some(Value::Array(list))) => {
            list.iter().map(drive).collect::<Result<Vec<_>, _>>().map(|l| Some(json!(l)))
        }
        (Param::Drives, Some(v)) => drive(v).map(Some),
        (Param::AtMost(most), Some(Value::Array(list))) if list.len() > most => Err((
            "invalid_params",
            format!("invalid params: `{name}` has {} items: at most {most} at a time", list.len()),
        )),
        (_, Some(v)) => Ok(Some(v.clone())),
    }
}

/// What the host does with a call of `method` with `sent` (drawn as `params`, or not an object)
/// from a plugin granted `granted_ones`: exactly what the spec says.
fn as_the_spec_says(
    m: &Manifest,
    granted_ones: &[&str],
    method: &str,
    params: &serde_json::Map<String, Value>,
    object: bool,
) {
    let sent = if object { Value::Object(params.clone()) } else { json!(["USB"]) };
    let result = check(m, method, sent);
    let spec = SPEC.iter().find(|(name, ..)| *name == method);
    let Some(&(_, capability, library, others)) = spec else {
        let e = result.unwrap_err();
        assert_eq!((e.code, e.kind), (code::METHOD_NOT_FOUND, "method_not_found"), "{method:?}");
        return;
    };
    if !granted_ones.contains(&capability) {
        let e = result.unwrap_err();
        let said = format!("{method} needs the {capability} capability, which the p plugin doesn't have");
        assert_eq!((e.code, e.kind, e.message), (code::FAILED, "denied", said));
        return;
    }
    if !object {
        assert_eq!(result.unwrap_err().message, "invalid params: expected an object");
        return;
    }
    let listed = takes(library, others);
    let unlisted: Vec<&String> = params.keys().filter(|k| !listed.iter().any(|(name, _)| name == k)).collect();
    if !unlisted.is_empty() {
        let e = result.unwrap_err();
        let named = e.message.strip_prefix(&format!("invalid params: a plugin doesn't give {method} `")).unwrap();
        assert!(unlisted.iter().any(|k| named == format!("{k}`")), "{} names one of {unlisted:?}", e.message);
        assert_eq!(e.kind, "invalid_params");
        return;
    }
    let outcomes: Vec<(&str, Made)> =
        listed.iter().map(|&(name, param)| (name, made(name, param, params.get(name)))).collect();
    let refusals: Vec<&(&str, String)> = outcomes.iter().filter_map(|(_, o)| o.as_ref().err()).collect();
    match result {
        Ok(passed) => {
            assert!(refusals.is_empty(), "{method} {params:?} let through, though {refusals:?}");
            let expected: serde_json::Map<String, Value> =
                outcomes.into_iter().filter_map(|(name, o)| Some((name.to_string(), o.unwrap()?))).collect();
            assert_eq!(passed, Value::Object(expected), "{method} {params:?}");
            assert!(REFUSED.iter().all(|k| passed.get(k).is_none()));
        }
        Err(e) => {
            let refused = (e.kind, e.message.clone());
            assert!(
                refusals.iter().any(|r| (r.0, &r.1) == (refused.0, &refused.1)),
                "{method}: {refused:?} isn't one of {refusals:?}"
            );
        }
    }
}

/// For every read (and anything else) a plugin calls, granted any capabilities (mostly), with any
/// params (mostly its own), the host lets through exactly what the spec says: only the reads
/// listed, only when granted, only with the params listed for each (none naming a file, server or
/// secret), drives found by the host and never by path, lists and pages no bigger than their caps.
#[hegel::test(test_cases = 200)]
fn a_plugin_calls_only_what_the_spec_lets_it(tc: TestCase) {
    let capabilities: Vec<String> = Capability::NAMED.iter().map(Capability::to_string).collect();
    let granted_ones: Vec<&str> =
        capabilities.iter().map(String::as_str).filter(|_| tc.draw(gs::integers::<u8>().max_value(3)) > 0).collect();
    let m = granted(&granted_ones);
    let other = tc.draw(gs::text().max_size(12));
    let methods = SPEC.iter().map(|(m, ..)| *m).chain(NEVER.iter().copied()).chain([other.as_str()]);
    for method in methods {
        let listed =
            SPEC.iter().find(|(name, ..)| *name == method).map(|&(_, _, l, o)| takes(l, o)).unwrap_or_default();
        // Mostly the read's own params, with now and then one it doesn't take.
        let mut keys: Vec<&str> =
            listed.iter().map(|(name, _)| *name).filter(|_| tc.draw(gs::integers::<u8>().max_value(3)) > 0).collect();
        if tc.draw(gs::integers::<u8>().max_value(7)) == 0 {
            let others: Vec<&str> =
                REFUSED.iter().copied().chain(["drive", "combined", "nonsense", "against"]).collect();
            keys.push(tc.draw(gs::sampled_from(others)));
        }
        let mut params = serde_json::Map::new();
        for key in keys {
            let param = listed.iter().find(|(name, _)| *name == key).map(|(_, p)| *p);
            params.insert(key.to_string(), given(&tc, param));
        }
        let object = tc.draw(gs::integers::<u8>().max_value(15)) > 0;
        as_the_spec_says(&m, &granted_ones, method, &params, object);
    }
}

/// Roots here as [`known_roots`] has them, longest first: a store below the home folder, both
/// spellings of a Windows home, a drive with a space in its name and a temporary folder resolved.
fn roots() -> Vec<(String, String)> {
    let mut roots: Vec<(String, String)> = [
        ("/Users/al/Library/Application Support/locallibrary", "<store>"),
        ("C:\\Users\\John Smith", "~"),
        ("C:/Users/John Smith", "~"),
        ("/Volumes/NO NAME", "NO NAME"),
        ("/private/var/tmp", "<temp>"),
        ("/var/tmp", "<temp>"),
        ("/Users/al", "~"),
    ]
    .into_iter()
    .map(|(r, s)| (r.to_string(), s.to_string()))
    .collect();
    roots.sort_by_key(|(r, _)| std::cmp::Reverse(r.len()));
    roots
}

/// Text with paths in it, and the same text as a plugin is to get it: drawn as words (no paths),
/// roots followed by what ends them (a separator, a space, punctuation, or the text's end),
/// said from there, and roots that are only the start of another folder's name, kept.
#[hegel::test_helper]
fn text_with_paths(tc: &TestCase) -> (String, String) {
    let roots = roots();
    let (mut text, mut said) = (String::new(), String::new());
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(5)) {
        let (root, as_said) = &tc.draw(gs::sampled_from(&roots));
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => {
                let words = tc.draw(gs::text().alphabet("abz :=,").max_size(6));
                text.push_str(&words);
                said.push_str(&words);
            }
            1 => {
                let after =
                    tc.draw(gs::sampled_from(vec!["/", "\\", "/PIONEER/export.pdb", "\\Music", " ", ", ", ";", "\")"]));
                text.push_str(&format!("{root}{after}"));
                said.push_str(&format!("{as_said}{after}"));
            }
            // Another folder whose name starts with the root's last (`/Users/alex`): not one of
            // the roots, unless a shorter root is in it.
            _ if !roots.iter().any(|(r, _)| r.len() < root.len() && root.contains(&format!("{r}/"))) => {
                let more = tc.draw(gs::sampled_from(vec!["ex", "_2", ".old", "-b", "9"]));
                let kept = format!("{root}{more}");
                text.push_str(&kept);
                said.push_str(&kept);
            }
            _ => {}
        }
    }
    if tc.draw(gs::booleans()) {
        let (root, as_said) = &tc.draw(gs::sampled_from(&roots));
        text.push_str(root);
        said.push_str(as_said);
    }
    (text, said)
}

/// Whatever error a read fails with, a plugin gets its kind, reason and fields, never the
/// actions a front end offers, and each path under a root here said from there, in the message
/// and in every field however deep.
#[hegel::test(test_cases = 200)]
fn a_reads_error_reaches_a_plugin_without_actions_or_roots(tc: TestCase) {
    let (text, said) = text_with_paths(&tc);
    let (detail, detail_said) = text_with_paths(&tc);
    let kind = tc.draw(gs::integers::<u8>().max_value(4));
    let reason = tc.draw(gs::optional(gs::sampled_from(vec![Reason::Diverged, Reason::Uncertain])));
    let fields: Vec<(&str, u8)> = (0..tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|_| {
            (tc.draw(gs::sampled_from(vec!["drives", "against", "x"])), tc.draw(gs::integers::<u8>().max_value(3)))
        })
        .collect();
    let actions = tc.draw(gs::integers::<u8>().max_value(2));
    // The error with `text` and `detail` in it: as the read fails, and as the plugin is to get it.
    let error = |text: &str, detail: &str, actions: u8| {
        let e = match kind {
            0 => Error::Invalid(text.into()),
            1 => Error::NotFound(text.into()),
            2 => Error::Conflict(text.into()),
            3 => Error::Io { path: text.into(), source: std::io::Error::other(detail.to_string()) },
            _ => Error::Parse { path: text.into(), message: detail.into() },
        };
        let e = match reason {
            Some(r) => e.because(r),
            None => e,
        };
        let e = fields.iter().fold(e, |e, (key, shape)| match shape {
            0 => e.with(key, detail),
            1 => e.with(key, [text, detail]),
            2 => e.with(key, json!({ "deep": [{ "path": detail }] })),
            _ => e.with(key, 7),
        });
        let e = if actions > 0 { e.action(Action::retry(json!({ "force": true }), "Force it")) } else { e };
        if actions > 1 { e.action(Action::elevate("Run it as an administrator")) } else { e }
    };
    let got = for_plugin(RpcFailure::from(error(&text, &detail, actions)), &roots());
    let expected = RpcFailure::from(error(&said, &detail_said, 0));
    assert_eq!((got.code, got.message.as_str()), (code::FAILED, expected.message.as_str()));
    let data = serde_json::to_value(got.data.unwrap()).unwrap();
    let reason = expected.detail.reason.map(|r| serde_json::to_value(r).unwrap());
    let mut want = json!({ "kind": expected.kind, "fields": expected.detail.fields });
    if let Some(reason) = reason {
        want["reason"] = reason;
    }
    assert_eq!(data, want, "and no actions");
}

/// Reads for the quota's property: `track.show` holds its call until the test lets it go,
/// `drive.info` fails naming a file in the temporary folder (offering to run elevated), and
/// `playlists.tree` panics.
struct Held {
    entered: Mutex<Sender<()>>,
    release: Mutex<mpsc::Receiver<()>>,
    file: PathBuf,
}

impl Reads for Held {
    fn read(&self, method: &str, params: Value, _: &Ctx<'_>) -> Result<Value, RpcFailure> {
        match method {
            "track.show" => {
                self.entered.lock().unwrap().send(()).unwrap();
                self.release.lock().unwrap().recv().unwrap();
                Ok(json!("shown"))
            }
            "drive.info" => {
                let e = Error::Io { path: self.file.clone(), source: std::io::Error::other("gone") };
                Err(e.action(Action::elevate("Run it as an administrator")).into())
            }
            "playlists.tree" => panic!("a read that panics"),
            _ => Ok(params),
        }
    }
}

/// How long a call the test lets through may take to reach its read: a bound, not a wait.
const REACHED: Duration = Duration::from_secs(10);
/// How long a call beyond the quota may take to be refused, which it is at once: a bound that
/// fails a call let through instead (it would wait for the test) quickly.
const AT_ONCE: Duration = Duration::from_secs(2);

/// For any quota and any number of calls held under way, exactly the calls beyond the quota are
/// refused as busy, at once; those let through are answered once their reads finish, a read that
/// fails is answered without its actions or the path here, one that panics is answered too, and
/// once every call is answered the whole quota is free again.
#[hegel::test(test_cases = 30, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_plugin_has_at_most_its_quota_of_calls_under_way(tc: TestCase) {
    let quota = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let held = tc.draw(gs::integers::<usize>().max_value(7));
    let name = tc.draw(gs::sampled_from(vec!["export.pdb", "a b.txt", "x"]));
    let file = std::env::temp_dir().join("PIONEER").join(name);
    // As the plugin is told it, from wherever the temporary folder is (a drive root on Windows CI).
    let said = super::scrub(&file.display().to_string(), &super::known_roots(None, &[]));
    let ((entered_tx, entered), (release, release_rx)) = (mpsc::channel(), mpsc::channel());
    let reads = Held { entered: Mutex::new(entered_tx), release: Mutex::new(release_rx), file };
    let options = Options { callbacks: quota, ..Options::default() };
    let host = Arc::new(Host { reads: Arc::new(reads), options, turns: Mutex::default() });
    let launch = Launch { program: "nothing".into(), args: Vec::new(), env_remove: Vec::new() };
    let (host_in, plugin_out) = pipe().unwrap();
    let (plugin_in, host_out) = pipe().unwrap();
    let (host_end, incoming) = Peer::new(host_in, host_out);
    let (plugin, _calls) = Peer::new(plugin_in, plugin_out);
    let m = granted(&["library:read", "drive:info"]);
    let (slot, live) = (Arc::new(Slot::new(m, launch, host)), Arc::new(Live::new(None)));
    let answering =
        std::thread::spawn(move || answer(&Arc::downgrade(&slot), &slot.plugin, &live, &host_end, incoming));
    let busy = |e: RpcError| {
        assert_eq!(e.data.unwrap().kind, "busy");
        assert_eq!(e.message, format!("a plugin has at most {quota} calls under way at once"));
    };
    let hold = |n: usize| -> Vec<Pending> {
        let mut calls: Vec<Pending> = (0..n).map(|_| plugin.send_request("track.show", &json!({})).unwrap()).collect();
        for _ in 0..n.min(quota) {
            entered.recv_timeout(REACHED).expect("a call let through reaches its read");
        }
        for refused in calls.split_off(n.min(quota)) {
            busy(refused.wait_timeout::<Value>(AT_ONCE).expect("refused at once").unwrap_err());
        }
        calls
    };
    let let_go = |calls: Vec<Pending>| {
        calls.iter().for_each(|_| release.send(()).unwrap());
        for call in calls {
            assert_eq!(call.wait::<Value>().unwrap(), json!("shown"));
        }
    };

    // What a plugin only says (a notification) isn't a call: nothing answers it, and it takes no
    // room.
    plugin.notify("$/progress", &json!({ "id": 0, "event": { "event": "warning", "message": "x" } }));
    let under_way = hold(held);
    let failed = plugin.request::<_, Value>("drive.info", &json!({})).unwrap_err();
    if held >= quota {
        busy(failed);
    } else {
        let path = said.clone();
        assert_eq!(failed.message, format!("{path}: gone"));
        let data = serde_json::to_value(failed.data.unwrap()).unwrap();
        assert_eq!(data, json!({ "kind": "io", "fields": { "path": path } }), "and no actions");
    }
    let_go(under_way);
    let panicked = plugin.request::<_, Value>("playlists.tree", &json!({})).unwrap_err();
    assert_eq!(
        (panicked.code, panicked.message.as_str()),
        (code::INTERNAL, "internal error while handling \"playlists.tree\"")
    );
    // The whole quota is free again.
    let again = hold(quota);
    assert_eq!(again.len(), quota);
    let_go(again);
    plugin.close();
    answering.join().unwrap();
}
