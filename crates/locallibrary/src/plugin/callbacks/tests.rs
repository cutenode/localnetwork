use std::path::{Path, PathBuf};

use loclib_plugin::Manifest;
use serde_json::json;

use super::*;
use crate::Error;
use crate::error::{Action, Reason};

/// A plugin granted `capabilities`.
fn granted(capabilities: &[&str]) -> Manifest {
    let list: Vec<String> = capabilities.iter().map(|c| format!("{c:?}")).collect();
    let head = "id = \"p\"\nname = \"P\"\nversion = \"0.1.0\"\nprotocol = \"^0.1\"";
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
    super::check(m, &Drives, method, params)
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
    let roots = known_roots();
    let temp = std::env::temp_dir().to_string_lossy().trim_end_matches(['/', '\\']).to_string();
    assert!(roots.contains(&(temp, "<temp>".into())), "{roots:?}");
    let home = directories::BaseDirs::new().unwrap().home_dir().to_string_lossy().into_owned();
    assert!(roots.contains(&(home, "~".into())), "{roots:?}");
    assert!(roots.windows(2).all(|w| w[0].0.len() >= w[1].0.len()), "longest first");
}
