//! The plugin served to a fake host that answers its `collection.*` reads from a library made in
//! a temporary folder: rekordbox's folders, a music folder and a backup folder.

use std::io::Read as _;
use std::path::PathBuf;
use std::sync::Once;

use loclib_plugin::test_util::FakeHost;
use loclib_plugin::{
    CollectionCopy, CollectionLocations, CollectionStatus, CollectionTrack, CollectionTracks, CommandPlan, Manifest,
    RpcError, View, method,
};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;

mod commands;

/// The library's id, as `djmdProperty.DBID` holds it.
pub(crate) const DB_ID: &str = "2354715273";

/// What the fake host's copy of `master.db` holds.
pub(crate) const MASTER_DB: &[u8] = b"a copy of master.db, as of one moment";

/// `rekordbox3.settings` with two sign-in tokens among its values, as JUCE writes it.
pub(crate) const SETTINGS_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\r\n<PROPERTIES>\r\n  \
<VALUE name=\"masterDbDirectory\" val=\"/Users/dj/Library/Pioneer/rekordbox\"/>\r\n  \
<VALUE name=\"RekordboxToken\" val=\"SECRET-RB-TOKEN\"/>\r\n  <VALUE name=\"lang\" val=\"en\"/>\r\n  \
<VALUE name=\"dropboxREFRESHtoken\" val=\"SECRET-DROPBOX\"/>\r\n  <VALUE name=\"Tokenless\" val=\"no\"/>\r\n\
</PROPERTIES>\r\n";

/// [`SETTINGS_XML`] without its tokens: the same bytes, but the lines holding them.
pub(crate) const SETTINGS_STRIPPED: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\r\n<PROPERTIES>\r\n  \
<VALUE name=\"masterDbDirectory\" val=\"/Users/dj/Library/Pioneer/rekordbox\"/>\r\n  <VALUE name=\"lang\" val=\"en\"/>\r\n\
</PROPERTIES>\r\n";

/// `activations.xml` with the AlphaTheta account in it.
pub(crate) const ACTIVATIONS_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\r\n<ACTIVATIONS>\r\n  \
<rekordbox_account account=\"dj@example.com\" account_info=\"x\" enc=\"SECRET-ACCOUNT\"/>\r\n  \
<product name=\"rekordbox\" plan=\"free\"/>\r\n</ACTIVATIONS>\r\n";

pub(crate) const ACTIVATIONS_STRIPPED: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\r\n<ACTIVATIONS>\r\n  \
<product name=\"rekordbox\" plan=\"free\"/>\r\n</ACTIVATIONS>\r\n";

/// Every backup made here is dated and timed by `SOURCE_DATE_EPOCH` (2026-09-21 14:13:20 UTC),
/// so its name and bytes don't depend on the clock.
pub(crate) fn stamped() {
    static STAMPED: Once = Once::new();
    // SAFETY: nextest runs each test in a process of its own, and this runs before its first
    // plugin thread starts.
    STAMPED.call_once(|| unsafe { std::env::set_var("SOURCE_DATE_EPOCH", "1790000000") });
}

/// A library in a temporary folder: rekordbox's files (`rekordbox/`, its `share/`, `settings/`),
/// the music (`music/`) and an empty backup folder (`backups/`).
pub(crate) struct Fixture {
    pub dir: TempDir,
    pub at: CollectionLocations,
    pub dest: PathBuf,
    pub tracks: Vec<CollectionTrack>,
}

impl Fixture {
    pub(crate) fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// The zip a backup made here today has.
    pub(crate) fn zip(&self) -> PathBuf {
        backup::zip_in(&self.dest)
    }
}

/// Writes `bytes` to `rel` in `dir`, making its folders.
pub(crate) fn put(dir: &std::path::Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Six tracks: one named by its tags whose analysis was redone (`ANLZ0001` beside a stale
/// `ANLZ0000`), one with non-ASCII tags and a long file name sharing the first's artwork, a
/// deleted one, one whose file is missing, a streaming one, and one with no tags whose analysis
/// path leads out of `share`. Beside them, files rekordbox's backup leaves out.
pub(crate) fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let data = root.join("rekordbox");
    let share = data.join("share");
    let settings = root.join("settings");
    for name in ["masterPlaylists6.xml", "automixPlaylist6.xml", "networkAnalyze6.db", "playlists3.sync"] {
        put(&data, name, format!("the library's {name}").as_bytes());
    }
    // rekordbox's own copies, which its backup leaves out.
    for name in ["master.db", "master.db-wal", "master.backup.db", "datafile.edb", "product.db"] {
        put(&data, name, b"live");
    }
    put(&settings, "rekordbox3.settings", SETTINGS_XML.as_bytes());
    put(&settings, "activations.xml", ACTIVATIONS_XML.as_bytes());
    for name in
        ["PlaySettings.xml", "LightingDB/user.db3", "MYSETTING.DAT", "KeyMappings/rekordbox_0000000000030.mappings"]
    {
        put(&settings, name, format!("the settings' {name}").as_bytes());
    }
    for name in ["cue_personal_trend.json", "rb_rtkn", "FxUnitSettings7.xml", "KeyMappings/notes.txt", "pad/1.xml"] {
        put(&settings, name, b"left out");
    }
    for ext in ["DAT", "EXT", "2EX", "3EX"] {
        put(&share, &format!("PIONEER/USBANLZ/46b/3baea/ANLZ0000.{ext}"), b"stale analysis");
        put(&share, &format!("PIONEER/USBANLZ/46b/3baea/ANLZ0001.{ext}"), format!("analysis {ext}").as_bytes());
        put(&share, &format!("PIONEER/USBANLZ/27f/142bd/ANLZ0000.{ext}"), b"a deleted track's analysis");
    }
    for ext in ["DAT", "EXT"] {
        put(&share, &format!("PIONEER/USBANLZ/111/222/ANLZ0000.{ext}"), format!("analysis {ext}").as_bytes());
    }
    for size in ["", "_s", "_m"] {
        put(&share, &format!("PIONEER/Artwork/46b/3baea/artwork{size}.jpg"), format!("artwork{size}").as_bytes());
    }
    put(root, "escape.DAT", b"outside share");
    let music = root.join("music");
    let lizard = put(&music, "TikTok/Lizard (Once).mp3", b"lizard audio");
    let long = put(&music, "A file name far longer than thirty-five characters.flac", "ł".repeat(40).as_bytes());
    let six = put(&music, "six.wav", b"six audio");
    let track = |id: &str, path: &std::path::Path| CollectionTrack {
        id: id.into(),
        uuid: Some(format!("uuid-{id}")),
        path: path.to_string_lossy().into_owned(),
        ..CollectionTrack::default()
    };
    let tracks = vec![
        CollectionTrack {
            artist: Some("Oliver Tree".into()),
            album: Some("Unknown Album".into()),
            analysis_path: Some("/PIONEER/USBANLZ/46b/3baea/ANLZ0001.DAT".into()),
            image_path: Some("/PIONEER/Artwork/46b/3baea/artwork.jpg".into()),
            ..track("40822045", &lizard)
        },
        CollectionTrack {
            artist: Some("Łukasz & Mø 💀".into()),
            album: Some("Über \"Alles\"".into()),
            analysis_path: Some("/PIONEER/USBANLZ/111/222/ANLZ0000.DAT".into()),
            image_path: Some("/PIONEER/Artwork/46b/3baea/artwork.jpg".into()),
            ..track("2", &long)
        },
        CollectionTrack {
            deleted: true,
            analysis_path: Some("/PIONEER/USBANLZ/27f/142bd/ANLZ0000.DAT".into()),
            ..track("3", &music.join("deleted.mp3"))
        },
        track("4", &music.join("gone.mp3")),
        CollectionTrack { path: "tidal:tracks:123".into(), ..track("5", &music) },
        CollectionTrack { analysis_path: Some("/PIONEER/../../escape.DAT".into()), ..track("6", &six) },
    ];
    let dest = root.join("backups");
    std::fs::create_dir(&dest).unwrap();
    let at = CollectionLocations {
        data: Some(data.clone()),
        master_db: Some(data.join("master.db")),
        master_db_exists: true,
        share: Some(share),
        settings: Some(settings),
    };
    Fixture { dir, at, dest, tracks }
}

/// The plugin, served to a host answering its reads of `f`'s library: rekordbox 7.2.14, not
/// running; a copy of `master.db` holding [`MASTER_DB`]; the tracks in one page.
pub(crate) fn host(f: &Fixture) -> FakeHost {
    let host = FakeHost::start(MANIFEST, serve);
    host.read("collection.locations", f.at.clone());
    host.read("collection.status", CollectionStatus { running: false, version: Some("7.2.14.0323".into()) });
    let tracks = CollectionTracks { db_id: Some(DB_ID.into()), total: f.tracks.len(), tracks: f.tracks.clone() };
    host.on_read("collection.tracks", move |p| match p["offset"].as_u64() {
        Some(0) => Ok(json!(tracks)),
        _ => Ok(json!(CollectionTracks { tracks: Vec::new(), ..tracks.clone() })),
    });
    host.on_read("collection.copy", |p| {
        let into = PathBuf::from(p["into"].as_str().unwrap());
        std::fs::write(&into, MASTER_DB).unwrap();
        let kept = p["with_credentials"] == json!(true);
        let copied = CollectionCopy {
            bytes: MASTER_DB.len() as u64,
            db_id: Some(DB_ID.into()),
            db_version: Some("6000".into()),
            credentials_removed: u64::from(!kept),
        };
        Ok(json!(copied))
    });
    host
}

/// Runs `command` with `params`, the settings `settings` and the backup folder `f.dest`.
pub(crate) fn run(
    host: &FakeHost,
    f: &Fixture,
    command: &str,
    params: Value,
    settings: Value,
) -> Result<Value, RpcError> {
    let mut settings = settings;
    settings["destination"] = json!(f.dest);
    let req = json!({ "command": command, "params": params, "context": { "settings": settings } });
    host.request(method::RUN_COMMAND, &req)
}

/// `backup`'s plan.
pub(crate) fn plan(host: &FakeHost, f: &Fixture, settings: Value) -> Result<CommandPlan, RpcError> {
    let mut settings = settings;
    settings["destination"] = json!(f.dest);
    let req = json!({ "command": "backup", "dry_run": true, "context": { "settings": settings } });
    host.request(method::RUN_COMMAND, &req)
}

/// The names of the zip at `path`'s entries, in order.
pub(crate) fn names(path: &std::path::Path) -> Vec<String> {
    let zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    zip.file_names().map(|n| n.unwrap().into_owned()).collect()
}

/// The zip at `path`'s entry `name`.
pub(crate) fn entry(path: &std::path::Path, name: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut bytes = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
    bytes
}

pub(crate) fn as_view(value: Value) -> View {
    serde_json::from_value(value).unwrap()
}

#[test]
fn the_manifest_is_one_locallibrary_ships() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    assert!(manifest.unknown().is_empty(), "{:?}", manifest.unknown());
    manifest.validate(true).unwrap();
    assert_eq!(manifest.capabilities, [loclib_plugin::Capability::CollectionFiles]);
}
