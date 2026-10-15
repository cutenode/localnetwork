//! Property tests of the human-readable output: a command run with and without `--json` on the
//! same drives and store (or two copies of them, side by side) says the same things. Every name,
//! count, path and state the JSON holds (of those a person reads) is in the text, and the text
//! names no drive, library, playlist or track the JSON doesn't. The fixture is drawn: drives with
//! and without libraries, adopted or not, cloned, edited since their last pull, damaged,
//! cluttered, with recordings and orphaned files, a store with history, labels, copies, settings
//! and players, so each printer's branches are taken.

use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::Value;

use crate::support::{Env, Run, path};

/// One of rekordbox's demo samples, as its export brings them.
const SAMPLE: &str = "/Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES/House1.wav";

/// A track's audio on the template's drive A.
pub const TRACK_1: &str = "Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3";
pub const TRACK_2: &str = "Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3";

/// What a drawn fixture holds besides the template's drives (A, pulled; Club, not adopted; Blank,
/// empty). Each is made as a person, a player or the command line would leave it.
#[derive(Debug, Clone, Copy, Default)]
pub struct State {
    /// Club adopted, and pulled into a library of its own, Clubland.
    pub club_adopted: bool,
    pub club_pulled: bool,
    /// Blank made a clone of A (`sync A Blank`).
    pub cloned: bool,
    /// A playlist made on A since its pull: changes the store lacks, and a backup to undo.
    pub edited: bool,
    /// A second pull of A, with a message, and a label on the first.
    pub history: bool,
    /// A's first track damaged and its second gone.
    pub damaged: bool,
    /// What computers leave on A: Finder's files, the trash, audio outside the library, notes.
    pub clutter: bool,
    /// A recording made on a player, and an analysis file no track uses, on A.
    pub recording: bool,
    /// A OneLibrary database beside A's export.pdb.
    pub onelibrary: bool,
    /// A's role, and Club retired (when adopted).
    pub roles: bool,
    /// Settings in the store's profile that differ from A's, and remembered players.
    pub profile: bool,
    /// A drive whose export.pdb doesn't read.
    pub broken: bool,
    /// Copies of the store, one of them gone since.
    pub store_copy: bool,
    /// Track 1 on A rated, tagged, with cues and artwork.
    pub track_edits: bool,
    /// A player recorded a new history list on A since its pull: back from a set.
    pub played: bool,
    /// An operation on Club (when adopted) that didn't finish.
    pub interrupted: bool,
    /// export.pdb on A edited outside locallibrary: a second copy of track 1, in no playlist,
    /// and a playlist OneLibrary hasn't got.
    pub drift: bool,
    /// rekordbox's automatic playlist sync set up for A, and what rekordbox keeps of the drive.
    pub rekordbox_sync: bool,
    /// rekordbox exported over an edit recorded on A, bringing its demo samples (twice, in a
    /// playlist).
    pub exported_over: bool,
    /// A folder of playlists on A.
    pub folders: bool,
    /// A demo drive, not adopted, with no playlists.
    pub demo: bool,
    /// Blank (when a clone) edited on its own since it was synced.
    pub diverged: bool,
    /// Copies of A (when it has OneLibrary): one with only OneLibrary, one whose OneLibrary
    /// doesn't read.
    pub copies: bool,
    /// One of A's My Settings files cut short and another gone.
    pub odd_settings: bool,
    /// A copy of Club (pulled) as it would be made in Finder: two drives claim one identity.
    pub twin_club: bool,
    /// A clone of A that isn't plugged in.
    pub away: bool,
}

/// The flags with only those at `on` set, for [`State::from_flags`].
pub fn picked(on: &[usize]) -> Vec<bool> {
    (0..FLAGS).map(|i| on.contains(&i)).collect()
}

/// How many flags [`State::from_flags`] reads.
pub const FLAGS: usize = 26;

impl State {
    /// The parts of the fixture, one flag each in the order of the fields (Club is adopted when
    /// it's pulled, and pulled when it has a twin).
    pub fn from_flags(f: &[bool]) -> State {
        let f = |i: usize| f.get(i).copied().unwrap_or(false);
        State {
            club_adopted: f(0) || f(1) || f(24),
            club_pulled: f(1) || f(24),
            cloned: f(2),
            edited: f(3),
            history: f(4),
            damaged: f(5),
            clutter: f(6),
            recording: f(7),
            onelibrary: f(8),
            roles: f(9),
            profile: f(10),
            broken: f(11),
            store_copy: f(12),
            track_edits: f(13),
            played: f(14),
            interrupted: f(15),
            drift: f(16),
            rekordbox_sync: f(17),
            exported_over: f(18),
            folders: f(19),
            demo: f(20),
            diverged: f(21),
            copies: f(22),
            odd_settings: f(23),
            twin_club: f(24),
            away: f(25),
        }
    }
}

/// Damage keeps a file's size and modification time; the bytes differ.
fn damage(file: &Path) {
    let (good, modified) = (std::fs::read(file).unwrap(), std::fs::metadata(file).unwrap().modified().unwrap());
    std::fs::write(file, good.to_ascii_uppercase()).unwrap();
    std::fs::File::options().write(true).open(file).unwrap().set_modified(modified).unwrap();
}

/// A file left on a drive (or this computer), as of a fixed time: two copies of a fixture made a
/// moment apart hold the same.
pub fn write(path: &Path, bytes: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
    filetime::set_file_mtime(path, filetime::FileTime::from_unix_time(1_790_000_000, 0)).unwrap();
}

/// The template's drives and store, with what `s` adds.
pub fn build(s: State) -> Env {
    let fx = Env::from_template();
    let a = fx.volumes.join("A");
    let yes = |args: &[&str]| {
        let r = fx.run(&[&["--yes"], args].concat());
        assert_eq!(r.code, 0, "{args:?}\n{}\n{}", r.out, r.err);
    };
    if s.club_adopted {
        yes(&["drive", "adopt", "Club", "--name", "Club"]);
    }
    if s.club_pulled {
        yes(&["pull", "Club", "--library", "Clubland", "--new-library"]);
    }
    if s.onelibrary {
        yes(&["convert", "A", "--to", "onelibrary"]);
    }
    if s.cloned {
        yes(&["sync", "A", "Blank"]);
    }
    if s.track_edits {
        image::RgbImage::from_pixel(300, 300, image::Rgb([200, 40, 90])).save(fx.home("art.png")).unwrap();
        yes(&["track", "set", "A", "--track", "1", "--rating", "4"]);
        yes(&["tags", "add", "A", "Genre/Deep", "--track", "1"]);
        yes(&["cues", "set", "A", "--track", "1", "--hot", "B", "--at", "1.5"]);
        yes(&["cues", "set", "A", "--track", "1", "--memory", "--beat", "2", "--loop", "4"]);
        yes(&["art", "set", "A", path(&fx.home("art.png")), "--track", "1"]);
    }
    if s.history {
        let first = fx.json(&["library", "ls"])[0]["head"]["short_id"].as_str().unwrap().to_string();
        yes(&["store", "label", &first, "pre-gig"]);
        yes(&["playlist", "new", "A", "Warm-ups"]);
        yes(&["pull", "A", "--message", "take-two"]);
    }
    if s.edited {
        yes(&["playlist", "new", "A", "Friday"]);
    }
    if s.diverged && s.cloned {
        yes(&["playlist", "new", "Blank", "Blank-only"]);
    }
    if s.away {
        let away = fx.blank("Away");
        yes(&["sync", "A", "Away"]);
        std::fs::rename(&away, fx.home("away-drive")).unwrap();
    }
    if s.exported_over {
        // An edit recorded, then rekordbox's export over it, a while later.
        let pdb = std::fs::read(a.join(crate::support::PDB)).unwrap();
        yes(&["playlist", "new", "A", "Encore"]);
        std::fs::write(a.join(crate::support::PDB), pdb).unwrap();
        let mut ed = rekordbox_pdb::PdbEditor::from_file(a.join(crate::support::PDB)).unwrap();
        let mut ids = Vec::new();
        for file in [SAMPLE, &SAMPLE.replace("House1", "House1 copy")] {
            let sample = rekordbox_pdb::NewTrack {
                artist: Some("rekordbox".into()),
                ..rekordbox_pdb::NewTrack::new("House 1", file)
            };
            ids.push(ed.add_track(&sample).unwrap());
            write(&a.join(file.trim_start_matches('/')), "demo sample");
        }
        let samples = ed.create_playlist("Samples", 0, false).unwrap();
        ed.set_playlist_entries(samples, &ids).unwrap();
        ed.save(a.join(crate::support::PDB)).unwrap();
        filetime::set_file_mtime(a.join(crate::support::PDB), filetime::FileTime::from_unix_time(1_800_000_000, 0))
            .unwrap();
    }
    if s.folders {
        yes(&["playlist", "new", "A", "Crates", "--folder"]);
        yes(&["playlist", "new", "A", "Crates/Deep Cuts"]);
    }
    if s.demo {
        crate::support::demo_drive(&fx.volumes, "Demo");
    }
    if s.damaged {
        damage(&a.join(TRACK_1));
        std::fs::remove_file(a.join(TRACK_2)).unwrap();
    }
    if s.clutter {
        write(&a.join(".DS_Store"), "finder");
        write(&a.join(".Trashes/501/old.mp3"), "trash");
        for i in 1..=6 {
            write(&a.join(format!("Music/{i}.mp3")), "loose");
        }
        write(&a.join("notes.txt"), "set notes");
    }
    if s.recording {
        write(&a.join("ALPHATHETA REC/REC001.WAV"), "cut off before its header");
        // More analysis files nothing uses than a listing shows.
        for i in 0..21 {
            write(&a.join(format!("PIONEER/USBANLZ/P0FF/0000BE{i:02}/ANLZ0000.DAT")), "PMAI analysis nothing uses");
        }
    }
    if s.roles {
        yes(&["drive", "role", "A", "primary"]);
        if s.club_adopted {
            yes(&["drive", "retire", "Club"]);
        }
    }
    if s.profile {
        yes(&["settings", "set", "quantize=off", "lcd_brightness=5"]);
        yes(&["settings", "set", "sync=on", "--drive", "A"]);
        yes(&["players", "set", "CDJ-3000,XDJ-AZ"]);
        yes(&["players", "set", "CDJ-2000NXS2", "--library", "A"]);
    }
    if s.broken {
        write(&fx.volumes.join("Broken/PIONEER/rekordbox/export.pdb"), "not a database");
    }
    if s.played {
        crate::support::play(&a, "HISTORY 002", &[2, 1]);
    }
    if s.interrupted && s.club_adopted {
        let state = fx.volumes.join("Club/.locallibrary/state.json");
        let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&state).unwrap()).unwrap();
        v["pending"] = serde_json::json!({
            "op": "sync",
            "started": "2026-10-01T21:30:00Z",
            "description": "sync from A",
        });
        std::fs::write(&state, v.to_string()).unwrap();
    }
    if s.drift {
        let mut ed = rekordbox_pdb::PdbEditor::from_file(a.join(crate::support::PDB)).unwrap();
        let copy = rekordbox_pdb::NewTrack {
            artist: Some("Loopmasters".into()),
            album: Some("Demo Album".into()),
            genre: Some("House".into()),
            mix_name: "Extended".into(),
            comment: "a second copy".into(),
            date_added: "2024-01-02".into(),
            year: 2020,
            bitrate: 320,
            duration: 172,
            play_count: 3,
            rating: 7,
            color_id: 2,
            tempo: 12_800,
            ..rekordbox_pdb::NewTrack::new("Demo Track 1 (Extended)", format!("/{TRACK_1}"))
        };
        ed.add_track(&copy).unwrap();
        // And a track of its own, with a comma in its title, a key that isn't one, and no tempo.
        let only = rekordbox_pdb::NewTrack {
            key: Some("Xyz".into()),
            ..rekordbox_pdb::NewTrack::new("Only, In PDB", "/Contents/Only.mp3")
        };
        ed.add_track(&only).unwrap();
        write(&a.join("Contents/Only.mp3"), "only in export.pdb");
        ed.create_playlist("Only PDB", 0, false).unwrap();
        // Track 2 gone from export.pdb, so that (with OneLibrary) only OneLibrary has it.
        if s.onelibrary {
            ed.remove_tracks(&[2]).unwrap();
        }
        ed.save(a.join(crate::support::PDB)).unwrap();
    }
    if s.rekordbox_sync {
        let sync =
            r#"<Sync DBID="-12" AutomaticSync="1"><Playlists><NODE Name="Set"/><NODE Name="All"/></Playlists></Sync>"#;
        write(&a.join("PIONEER/rekordbox/playlists3.sync"), sync);
        write(&a.join("PIONEER/rekordbox/playlists3Plus.sync"), &sync.replace("-12", "7"));
        let info = r#"{"uuid":"x","info":[{"device_id":"x","device_name":"MAIN","background_color":"3",
            "background_color_libplus":"4","device_filesystem":"FAT32","backup_pc_id":"1","backup_pc_name":"Studio"}]}"#;
        write(&a.join("PIONEER/DeviceLibBackup/rbDevLibBaInfo_1.json"), info);
    }
    if s.copies && s.onelibrary {
        for (name, gone) in
            [("Plus", "PIONEER/rekordbox/export.pdb"), ("Garbled", "PIONEER/rekordbox/exportLibrary.db")]
        {
            let copy = fx.volumes.join(name);
            crate::support::copy_dir(&a, &copy);
            std::fs::remove_dir_all(copy.join(".locallibrary")).unwrap();
            std::fs::remove_file(copy.join(gone)).unwrap();
            let _ = std::fs::remove_file(copy.join("PIONEER/rekordbox/exportExt.pdb"));
        }
        write(&fx.volumes.join("Garbled/PIONEER/rekordbox/exportLibrary.db"), "not a database");
    }
    if s.odd_settings {
        write(&a.join("PIONEER/DEVSETTING.DAT"), "short");
        let _ = std::fs::remove_file(a.join("PIONEER/DJMMYSETTING.DAT"));
    }
    if s.twin_club {
        crate::support::copy_dir(&fx.volumes.join("Club"), &fx.volumes.join("Club2"));
    }
    if s.store_copy {
        yes(&["store", "copy", path(&fx.home("store-copy"))]);
        yes(&["store", "copy", path(&fx.home("gone"))]);
        std::fs::remove_dir_all(fx.home("gone")).unwrap();
    }
    fx
}

/// A copy of `fx`'s drives, store and home in a folder of its own: the same drives and libraries
/// (by id too), recorded where `fx`'s were.
pub fn twin(fx: &Env) -> Env {
    let root = fx.volumes.parent().unwrap();
    // First every file a moment older than FAT's 2 s: the hash cache distrusts a hash taken within
    // 2 s of a file's mtime, so until then the two copies, run side by side but not in step, could
    // count different files verified.
    let secs = || filetime::FileTime::now().unix_seconds();
    let mtimes = walkdir::WalkDir::new(root).into_iter().flatten().filter_map(|e| e.metadata().ok());
    let now = secs();
    let newest =
        mtimes.map(|m| filetime::FileTime::from_last_modification_time(&m).unix_seconds()).filter(|t| *t <= now);
    if let Some(newest) = newest.max() {
        while secs() <= newest + 2 {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    let template = crate::files::Template::of(root);
    let other = Env::from_template();
    let to = other.volumes.parent().unwrap();
    for dir in ["volumes", "store", "home"] {
        let _ = std::fs::remove_dir_all(to.join(dir));
    }
    template.write(to);
    other
}

/// The values at `path` in `v`: keys separated by dots, `[]` each element of a list (`[..n]` the
/// first n, `[k=v]` or `[k!=v]` those whose `k` is or isn't `v`), `#` how many it holds, `*` each value of an object and `&` each of its
/// keys.
pub fn at(v: &Value, path: &str) -> Vec<Value> {
    let shown = |v: Option<&Value>| match v {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => "null".into(),
    };
    let mut here = vec![v.clone()];
    for key in path.split('.').filter(|k| !k.is_empty()) {
        let mut next = Vec::new();
        for v in here {
            let filter = key.strip_prefix('[').and_then(|k| k.strip_suffix(']')).filter(|k| !k.is_empty());
            match (key, v) {
                ("[]", Value::Array(list)) => next.extend(list),
                ("#", Value::Array(list)) => next.push(Value::from(list.len())),
                (_, Value::Array(list)) if key.starts_with("[..") => {
                    let n: usize = key[3..key.len() - 1].parse().expect("[..N] takes a number");
                    next.extend(list.into_iter().take(n));
                }
                (_, Value::Array(list)) if filter.is_some() => {
                    let f = filter.unwrap();
                    let (k, want, is) = match f.split_once("!=") {
                        Some((k, w)) => (k, w, false),
                        None => {
                            let (k, w) = f.split_once('=').expect("a filter is k=v or k!=v");
                            (k, w, true)
                        }
                    };
                    next.extend(list.into_iter().filter(|e| (shown(e.get(k)) == want) == is));
                }
                ("*", Value::Object(o)) => next.extend(o.into_iter().map(|(_, v)| v)),
                ("&", Value::Object(o)) => next.extend(o.into_iter().map(|(k, _)| Value::from(k))),
                (k, Value::Object(mut o)) => next.extend(o.remove(k)),
                _ => {}
            }
        }
        here = next;
    }
    here
}

/// A value as the text shows it: a string as it is, a number in decimal; nothing for the rest.
pub fn text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// What `spec` says the human text must hold, from the JSON: each entry is a path (see [`at`]),
/// or a path in braces with words around it (`{[].tracks} tracks`), each value at the path filled
/// in. A leading `~` takes only the first 24 characters of each value, for columns that cut long
/// values short (found anywhere in the text; the rest as words of their own, see [`names`]).
pub fn expected(json: &Value, spec: &[&str]) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for s in spec {
        let (cut, s) = match s.strip_prefix('~') {
            Some(rest) => (true, rest),
            None => (false, *s),
        };
        let (before, path, after) = match (s.find('{'), s.rfind('}')) {
            (Some(i), Some(j)) => (&s[..i], &s[i + 1..j], &s[j + 1..]),
            _ => ("", s, ""),
        };
        for v in at(json, path).iter().filter_map(text) {
            let v = if cut { v.chars().take(24).collect() } else { v };
            out.push((format!("{before}{v}{after}"), cut));
        }
    }
    out
}

/// Whether `name` is in `text` as a word of its own, not part of a longer word.
pub fn names(text: &str, name: &str) -> bool {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    // Only an end that is itself part of a word needs a break beside it.
    let (starts, ends) = (word(name.chars().next()), word(name.chars().next_back()));
    text.match_indices(name).any(|(i, _)| {
        let joined_before = starts && word(text[..i].chars().next_back());
        let joined_after = ends && word(text[i + name.len()..].chars().next());
        !joined_before && !joined_after
    })
}

/// The drives, playlists and tracks of a fixture, and the store's labels and messages, by name:
/// what a text may name only when its JSON does. Names of three characters or more (shorter
/// ones are words of the text's own too).
pub fn universe(fx: &Env) -> Vec<String> {
    let mut out: Vec<String> = [
        "Club",
        "Blank",
        "Broken",
        "Friday",
        "Warm-ups",
        "Only PDB",
        "pre-gig",
        "take-two",
        "Encore",
        "Samples",
        "House 1",
        "Crates",
        "Deep Cuts",
        "Blank-only",
        "Only, In PDB",
        "Demo",
        "Plus",
        "Garbled",
        "Club2",
        "Away",
        "Clubland",
        "Late Set",
    ]
    .map(String::from)
    .into();
    for drive in ["A", "Club"] {
        if let Ok(v) = serde_json::from_str::<Value>(&fx.run(&["--json", "tracks", drive]).out) {
            out.extend(at(&v, "tracks.[].title").iter().filter_map(text));
        }
        if let Ok(v) = serde_json::from_str::<Value>(&fx.run(&["--json", "playlist", "ls", drive]).out) {
            out.extend(at(&v, "[].name").iter().filter_map(text));
        }
    }
    // The playlist "Set" shares its name with a word the hints start with ("Set with ...").
    out.retain(|n| n.chars().count() >= 3 && n != "Set");
    out.sort();
    out.dedup();
    out
}

/// A command's two runs, as a failure shows them.
pub fn said(args: &[&str], human: &Run, json: &Run) -> String {
    let json_out: String = json.out.chars().take(6000).collect();
    format!(
        "{args:?}\nhuman exited {}:\n{}{}\n--json exited {}:\n{json_out}{}",
        human.code, human.out, human.err, json.code, json.err
    )
}

/// The checks of [`agree`], of runs already made: `human` without `--json`, `json` with it.
pub fn agree_runs(args: &[&str], human: &Run, json: &Run, spec: &[&str], universe: &[String]) -> Option<Value> {
    let said = || said(args, human, json);
    assert_eq!(human.code, json.code, "{}", said());
    assert!(!human.err.contains("panicked at") && !json.err.contains("panicked at"), "{}", said());
    // A JSON document, or (`watch`) one a line: the last says how it came out.
    let docs: Vec<Value> = serde_json::Deserializer::from_str(&json.out)
        .into_iter::<Value>()
        .collect::<Result<_, _>>()
        .unwrap_or_else(|e| panic!("not JSON ({e}): {}", said()));
    let v = match docs.len() {
        1 => docs[0].clone(),
        _ => Value::Array(docs),
    };
    if [1, 2].contains(&human.code) {
        let message = v["error"]["message"].as_str().unwrap_or_else(|| panic!("no message: {}", said()));
        // No command at all: a person gets the usage, a program the error.
        let usage = spec.contains(&PRINTS_USAGE) && human.err.contains("Usage: locallibrary");
        assert!(usage || human.err.contains(&format!("error: {message}")), "{}", said());
        return None;
    }
    // A read of every mounted drive (or library): each says what a read of one drive says.
    let reads: Vec<&Value> = match v.as_array() {
        Some(list)
            if !list.is_empty() && list.iter().all(|r| r.get("result").is_some() && r.get("drive").is_some()) =>
        {
            list.iter().collect()
        }
        _ => Vec::new(),
    };
    let mut want = Vec::new();
    // With nothing to do, a change says so in a line rather than showing its plan.
    let nothing = v["outcome"] == "nothing_to_do";
    match reads.is_empty() {
        true if nothing => {}
        true => want.extend(expected(&v, spec)),
        false => {
            for r in reads.iter().filter(|r| !r["result"].is_null()) {
                want.extend(expected(&r["result"], spec));
            }
        }
    }
    for (w, cut) in &want {
        let says = if *cut { human.out.contains(w.as_str()) } else { names(&human.out, w) };
        assert!(says, "the text doesn't say {w:?}: {}", said());
    }
    // What the command line names, the text may say back.
    let known = |n: &str| args.iter().any(|a| names(a, n));
    for n in universe.iter().filter(|n| !known(n)) {
        if names(&human.out, n) {
            // Under `--json` a warning goes to stderr, keeping stdout for the JSON.
            assert!(names(&json.out, n) || names(&json.err, n), "the text names {n:?}, the JSON doesn't: {}", said());
        }
    }
    Some(v)
}

/// `args` run as it is and with `--json` on the same drives and store: the same exit status; a
/// failure's message said the same way; a success's text holding what `spec` picks out of its
/// JSON (see [`expected`]), and naming nothing of `universe` the JSON or `args` don't. The JSON, when it
/// succeeded.
// Only props_cli_asked, which is Unix-only, uses it so far.
#[cfg_attr(not(unix), allow(dead_code))]
pub fn agree(fx: &Env, args: &[&str], spec: &[&str], universe: &[String]) -> Option<Value> {
    let words: Vec<String> = args.iter().map(|w| fill(fx, w)).collect();
    let args: Vec<&str> = words.iter().map(String::as_str).collect();
    let human = run_in(fx, &args);
    let json = run_in(fx, &[&["--json"], &args[..]].concat());
    agree_runs(&args, &human, &json, spec, universe)
}

/// `a()` and `b()` at once, `b` in a thread of its own: each command costs about the same and
/// mostly waits on the process it runs, so commands on two copies of a fixture take half as long
/// side by side. A panic in either is the caller's.
pub fn side_by_side<A, B: Send>(a: impl FnOnce() -> A, b: impl FnOnce() -> B + Send) -> (A, B) {
    std::thread::scope(|s| {
        let b = s.spawn(b);
        let a = a();
        (a, b.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
    })
}

/// `json` with `from`'s folder written as `to`'s: what a run in a copy of a fixture says, as the
/// run in the original would.
pub fn as_in(json: Run, from: &Env, to: &Env) -> Run {
    let root = |fx: &Env| path(fx.volumes.parent().unwrap()).to_string();
    let (from, to) = (root(from), root(to));
    // A message can quote a path as Debug does (`{spec:?}`), doubling Windows' backslashes, and the
    // JSON escapes them again, so the folders are found in every spelling they're written in.
    let debug = |s: &str| format!("{s:?}").trim_matches('"').to_string();
    let escaped = |s: &str| serde_json::to_string(s).unwrap().trim_matches('"').to_string();
    let (debug_from, debug_to) = (debug(&from), debug(&to));
    let out = json.out.replace(&escaped(&debug_from), &escaped(&debug_to)).replace(&escaped(&from), &escaped(&to));
    let err = json.err.replace(&debug_from, &debug_to).replace(&from, &to);
    Run { out, err, code: json.code }
}

/// [`agree`] of each of `reads` in turn, run as it is in `h` and with `--json` in `j` (a
/// [`twin`] of `h`): each copy runs them all in the same order, the two side by side.
fn reads_agree(h: &Env, j: &Env, reads: &[(&[&str], &[&str])], universe: &[String]) {
    let all = |fx: &Env, json: &[&str]| -> Vec<Run> {
        let run = |args: &[&str]| {
            let words: Vec<String> = args.iter().map(|w| fill(fx, w)).collect();
            run_in(fx, &[json, &words.iter().map(String::as_str).collect::<Vec<_>>()].concat())
        };
        reads.iter().map(|(args, _)| run(args)).collect()
    };
    let (human, json) = side_by_side(|| all(h, &[]), || all(j, &["--json"]));
    for (((args, spec), human), json) in reads.iter().zip(human).zip(json) {
        let words: Vec<String> = args.iter().map(|w| fill(h, w)).collect();
        let args: Vec<&str> = words.iter().map(String::as_str).collect();
        agree_runs(&args, &human, &as_in(json, j, h), spec, universe);
    }
}

/// A word of a command with its placeholder filled in for `fx`: `@h/x` the file x on this
/// computer, `@v/x` the drive x's mount path.
pub fn fill(fx: &Env, word: &str) -> String {
    match word {
        w if w.starts_with("@h/") => path(&fx.home(&w[3..])).to_string(),
        w if w.starts_with("@v/") => path(&fx.volumes.join(&w[3..])).to_string(),
        w => w.to_string(),
    }
}

/// [`Env::run`], but with `@empty` among the words on a computer with no drives and no store yet,
/// with `@one` with only drive A plugged in, with `@default-store` without a store given, and with
/// `@sudo` or `@sudo-me` as under sudo.
pub fn run_in(fx: &Env, args: &[&str]) -> Run {
    let mut command = fx.command();
    let marker = args.iter().copied().find(|a| ["@empty", "@one", "@default-store", "@sudo", "@sudo-me"].contains(a));
    let args: Vec<&str> = args.iter().copied().filter(|a| Some(*a) != marker).collect();
    match marker {
        // A store of its own for each command line: a write would make one for the rest. Named
        // without `--json`, so both runs of a line name the same store when they say there's none.
        Some("@empty") => {
            std::fs::create_dir_all(fx.home("no-drives")).unwrap();
            let words: Vec<&str> = args.iter().copied().filter(|a| *a != "--json").collect();
            let store = fx.home(&format!("no-store {}", words.join(" ").replace('/', "|")));
            command.env("LOCALLIBRARY_VOLUMES", fx.home("no-drives")).env("LOCALLIBRARY_STORE", store);
        }
        // Only drive A plugged in (the folder of drives holds a link to it, or on Windows, where
        // a symlink takes a privilege, a copy of it made the first time).
        Some("@one") => {
            let one = fx.home("one-drive");
            if !one.exists() {
                std::fs::create_dir_all(&one).unwrap();
                #[cfg(unix)]
                std::os::unix::fs::symlink(fx.volumes.join("A"), one.join("A")).unwrap();
                #[cfg(not(unix))]
                crate::support::copy_dir(&fx.volumes.join("A"), &one.join("A"));
            }
            command.env("LOCALLIBRARY_VOLUMES", one);
        }
        // No store given: the default one, in the commands' own home folder.
        Some("@default-store") => {
            command.env_remove("LOCALLIBRARY_STORE");
        }
        // Run with sudo by someone else (a uid that owns nothing here), or by the user who owns
        // the store.
        Some("@sudo") => {
            command.env_remove("LOCALLIBRARY_STORE").env("SUDO_UID", "4242");
        }
        Some("@sudo-me") => {
            // SAFETY: getuid only reads this process's user id.
            #[cfg(unix)]
            let me = unsafe { libc::getuid() }.to_string();
            // Windows has no uids: only Unix reads the value, to see whose the store is.
            #[cfg(not(unix))]
            let me = String::new();
            command.env_remove("LOCALLIBRARY_STORE").env("SUDO_UID", me);
        }
        _ => return fx.run(&args),
    }
    let out = command.args(&args).output().unwrap();
    Run {
        out: String::from_utf8(out.stdout).unwrap(),
        err: String::from_utf8(out.stderr).unwrap(),
        code: out.status.code().unwrap(),
    }
}

/// A spec's entry for a command line a person is shown the usage for, as the error.
pub const PRINTS_USAGE: &str = "(prints the usage)";

/// What each read of the drives must say, from its JSON.
const DRIVE_READS: &[(&[&str], &[&str])] = &[
    (
        &["status"],
        &[
            "libraries.[].name",
            "saved ({libraries.[unsaved=false].head.short_id})",
            "mounted.[].drive.name",
            "  {mounted.[settings_drift!=0].settings_drift} setting(s) differ",
            "offline.[].name",
            "duplicates.[].name",
        ],
    ),
    (&["status", "--all", "--retired"], &["libraries.[].name", "mounted.[].drive.name", "offline.[].name"]),
    (&["drive", "ls"], &["[].library", "[].root", "{[].summary.tracks} tracks", "{[].summary.playlists} playlists"]),
    (&["drive", "ls", "--all"], &["[].library", "[].root"]),
    (&["drive", "ls", "--known", "--retired"], &["[].name", "[].library", "[].mounted"]),
    (&["drive", "list"], &["[].name", "[].library"]),
    (&["drive"], &["[].library", "[].root"]),
    (
        &["drive", "info", "A"],
        &[
            "root",
            "id",
            "{summary.tracks} tracks",
            "{summary.playlists} playlists",
            "{summary.folders} folders",
            "{summary.artists} artists",
            "save #{pdb.sequence}",
            "settings_files.[]",
            "({lineage_len} recorded states)",
        ],
    ),
    (&["drive", "info"], &["root", "{summary.tracks} tracks", "settings_files.[]"]),
    (&["drive", "Club"], &["root", "{summary.tracks} tracks", "{summary.playlists} playlists"]),
    (&["info", "Blank"], &["root"]),
    (
        &["drive", "log", "A"],
        &["name", "id", "role", "last_mount", "in {log.writes} operation(s)", "{log.clean_ejects} clean eject(s)"],
    ),
    (&["drive", "log", "Club"], &["name", "id", "role", "last_mount"]),
    (&["drive", "groups"], &["[].library", "[].members.[]"]),
    (&["drive", "space", "A"], &["drive", "{categories.[].files} files", "loose_audio.[..5].path", "fits.[].name"]),
    (&["drive", "space"], &["drive", "{categories.[].files} files"]),
    (&["drive", "backups", "A"], &["[].id", "[].reason"]),
    (&["drive", "backups"], &["[].id", "[].reason"]),
    (&["drive", "recordings", "A"], &["[].path"]),
    (&["drive", "verify", "A"], &["drive", "{files} files", "damaged.[].path", "missing.[].path", "tracks.[].name"]),
    (&["drive", "verify", "Club", "--all"], &["drive", "{files} files", "missing.[].path"]),
    (&["drive", "player", "A"], &["drive", "{onelibrary.colour}"]),
    (&["drive", "recognise", "Blank"], &["snapshot.library", "snapshot.snapshot.short_id", "drive_by_content.name"]),
    (&["drive", "recognise", "Club"], &["snapshot.library", "drive_by_content.name"]),
    (&["undo", "--list"], &["drives.[].backups.[].id", "drives.[].backups.[].reason"]),
    (&["undo", "A", "--list"], &["drives.[].backups.[].id"]),
    (&["drive", "fsck", "A"], &[]),
    // After a check, what the drive's log and the registry say of it.
    (&["drive", "ls", "--known"], &["[].name", "[].log.problems.[..1].detail"]),
    (&["drive", "log", "A"], &["name", "log.last_scrub.summary", "log.problems.[..10].detail"]),
    (&["status"], &["libraries.[].name", "mounted.[].drive.name", "offline.[].name"]),
    // The top-level forms of the drive commands.
    (&["drives"], &["[].library", "[].root"]),
    (&["info", "A"], &["root"]),
    (&["space", "A"], &["drive"]),
    (&["backups", "A"], &["[].id"]),
    (&["recordings", "A"], &["[].path"]),
    (&["verify", "Blank"], &["drive"]),
    (&["fsck", "A"], &[]),
    (&["disks"], &[]),
    // A computer with no drives and no store.
    (&["@empty", "status"], &[]),
    (&["@empty", "drive", "ls"], &[]),
    (&["@empty", "drive", "ls", "--known"], &[]),
    (&["@empty", "drive", "groups"], &[]),
    (&["@empty", "drive", "info"], &[]),
    (&["@empty", "drive", "verify"], &[]),
];

/// The commands that read drives (`drive ...`, `status`, `undo --list`) say what their JSON says,
/// whatever the drives and the store hold.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(flags = vec![true; FLAGS])]
#[hegel::explicit_test_case(flags = vec![false; FLAGS])]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 0).collect::<Vec<bool>>())]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 1).collect::<Vec<bool>>())]
#[hegel::explicit_test_case(flags = picked(&[14]))]
#[hegel::explicit_test_case(flags = picked(&[2, 4]))]
#[hegel::explicit_test_case(flags = picked(&[2, 4, 8, 16, 21, 22]))]
fn drive_reads_say_what_their_json_says(tc: TestCase) {
    let flags = tc.draw(gs::vecs(gs::booleans()).min_size(FLAGS).max_size(FLAGS));
    let fx = build(State::from_flags(&flags));
    let universe = universe(&fx);
    reads_agree(&fx, &twin(&fx), DRIVE_READS, &universe);
}

/// What each read of the store, its libraries and the settings must say, from its JSON.
const STORE_READS: &[(&[&str], &[&str])] = &[
    (&["library", "ls"], &["[].name", "{[].snapshots} snapshot(s)", "latest {[].head.short_id}", "[].drives.[]"]),
    (&["library"], &["[].name", "{[].snapshots} snapshot(s)"]),
    (
        &["library", "log"],
        &[
            "libraries.&",
            "libraries.*.[].short_id",
            "{libraries.*.[].summary.tracks} tracks",
            "[{libraries.*.[].labels.[]}]",
            "from {libraries.*.[].source.drive_name}",
            "libraries.*.[].message",
        ],
    ),
    (&["library", "log", "A", "-n", "1"], &["libraries.&", "libraries.*.[].short_id"]),
    (&["library", "A"], &["libraries.&", "libraries.*.[].short_id"]),
    (&["library", "Nope"], &[]),
    (&["log", "Club"], &["libraries.*.[].short_id"]),
    (
        &["store"],
        &["root", "{stats.objects} files", "snapshots  {stats.snapshots}", "{stats.drives} known", "libraries.[].name"],
    ),
    (&["store", "copies"], &["[].path", "{[].snapshots} snapshot(s)", "in {[].objects} file(s)", "[].problems.[]"]),
    (&["store", "verify"], &["corrupt.[]", "missing.[]"]),
    (&["players"], &["default.[]", "libraries.[].name", "libraries.[].models.[]"]),
    (&["players", "show"], &["default.[]", "libraries.[].models.[]"]),
    (&["players", "models"], &["[].name"]),
    (&["players", "models", "CDJ-3000"], &["name", "source"]),
    (&["players", "models", "DJM-900NXS2"], &["name", "source"]),
    (&["players", "models", "XDJ-AZ"], &["name", "source"]),
    (&["players", "models", "CDJ-2000"], &["name", "source"]),
    (&["players", "models", "XDJ-RX2"], &["name", "source"]),
    (&["players", "models", "Nope"], &[]),
    (&["settings"], &["base.&", "base.*", "drives.*.name", "drives.&", "drives.*.values.&", "drives.*.values.*"]),
    (&["settings", "schema"], &["[].key", "[].choices.[].id"]),
    (&["settings", "show", "A"], &["drive", "drift.[].key", "drift.[].expected"]),
    (&["settings", "show"], &["drive", "drift.[].key"]),
    (&["settings", "Club"], &["drive", "drift.[].key"]),
    (&["--store", "@h/store-copy", "store", "info"], &["root", "copy_of.from", "libraries.[].name"]),
    (&["@empty", "library", "ls"], &[]),
    (&["@empty", "library", "log"], &[]),
    (&["@empty", "players"], &[]),
    (&["@empty", "store", "copies"], &[]),
];

/// The commands that read the store, its libraries, the settings profile and the players say
/// what their JSON says, whatever the store holds.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(flags = vec![true; FLAGS])]
#[hegel::explicit_test_case(flags = vec![false; FLAGS])]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 0).collect::<Vec<bool>>())]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 1).collect::<Vec<bool>>())]
fn store_reads_say_what_their_json_says(tc: TestCase) {
    let flags = tc.draw(gs::vecs(gs::booleans()).min_size(FLAGS).max_size(FLAGS));
    let fx = build(State::from_flags(&flags));
    let universe = universe(&fx);
    reads_agree(&fx, &twin(&fx), STORE_READS, &universe);
}

/// What each read of a library's tracks, and the checks of a drive, must say, from its JSON.
const TRACK_READS: &[(&[&str], &[&str])] = &[
    (&["tracks", "A"], &["~tracks.[].title", "tracks.[].key", "{total} track(s)"]),
    (&["tracks", "Club", "-n", "3", "--sort", "-bpm"], &["~tracks.[].title", "of {total} track(s)"]),
    (&["tracks"], &["~tracks.[].title"]),
    (&["tracks", "A", "-s", "demo 1", "-f", "bpm:>=100"], &["~tracks.[].title"]),
    (&["tracks", "A", "-p", "Set", "--format", "tsv"], &["tracks.[].title", "tracks.[].artist"]),
    (
        &[
            "tracks",
            "A",
            "--format",
            "csv",
            "--columns",
            "id,title,artist,album,genre,label,key,bpm,rating,color,year,plays,added,time,bitrate,path,comment",
        ],
        &["tracks.[].title", "tracks.[].file_path", "tracks.[].comment", "tracks.[].date_added", "tracks.[].bitrate"],
    ),
    (&["tracks", "--format", "csv"], &["tracks.[].title"]),
    (&["tracks", "--format", "tsv", "--columns", "name,genre,label,rating,color"], &["tracks.[].artist"]),
    (&["playlist", "ls", "A"], &["[].name"]),
    (&["playlists", "Club"], &["[].name"]),
    (&["playlist", "Blank"], &["[].name"]),
    (
        &["track", "show", "A", "--track", "1"],
        &[
            "[].track.title",
            "[].track.file_path",
            "[].playlists.[]",
            "[].history.[]",
            "[].tags.[]",
            "{[].analysis.beats} beats",
            "{[].analysis.hot_cues} hot",
            "{[].analysis.memory_cues} memory",
        ],
    ),
    (&["track", "show", "A", "--track", "2"], &["[].track.title", "[].track.file_path"]),
    (&["track", "show", "Club", "--filter", "bpm:>=125"], &["[].track.title", "[].track.file_path", "[].playlists.[]"]),
    (&["art", "missing", "A"], &["[].name", "#{[].id}"]),
    (&["art"], &["[].name"]),
    (&["art", "Club"], &["[].name"]),
    (
        &["diff", "A", "Club", "--all"],
        &[
            "left",
            "right",
            "added_tracks.[].name",
            "removed_tracks.[].name",
            "added_playlists.[]",
            "removed_playlists.[]",
        ],
    ),
    (&["diff", "A", "Blank"], &["left", "right", "changed_playlists.[].path"]),
    (&["diff", "Club", "snapshot:0000"], &[]),
    (
        &["diff", "--summary", "A", "Blank"],
        &["left.label", "right.label", "{left.tracks} tracks", "{left.playlists} playlists", "evidence.[].detail"],
    ),
    (&["compare", "A", "Club"], &["left.label", "right.label", "{right.tracks} tracks", "evidence.[].detail"]),
    (
        &["doctor", "A", "--all"],
        &[
            "drive",
            "root",
            "{tracks} tracks",
            "missing.[].track.name",
            "missing.[].path",
            "orphans.[].path",
            "unfiled.[].name",
            "duplicates.[].tracks.[].track.file_path",
            "large_files.[].path",
        ],
    ),
    (&["doctor"], &["drive", "{tracks} tracks", "missing.[..25].path", "orphans.[..15].path"]),
    (&["doctor", "Club", "--only", "orphans,unfiled,duplicates"], &["drive", "unfiled.[].name"]),
    (&["check"], &["items.[].drive", "items.[].message"]),
    (
        &["check", "A", "Blank", "--players", "CDJ-3000,CDJ-2000NXS2", "--verbose"],
        &["items.[].drive", "items.[].message"],
    ),
    (&["check", "Club", "--players", "XDJ-AZ"], &["items.[].message", "unloadable.[].tracks.[].name"]),
    (&["track", "show", "A", "--track", "3"], &["[].track.title", "[].track.genre", "[].track.comment"]),
    (&["track", "show", "A", "--track", "4"], &["[].track.title", "[].track.key"]),
    (&["tracks", "A", "--columns", "title,artist,album,comment,path,bpm,key"], &["~tracks.[].artist"]),
    (&["tracks", "A", "--format", "csv", "--columns", "title,bpm,key"], &["tracks.[].key"]),
    (&["playlist", "ls", "Demo"], &["[].name"]),
    (&["playlist", "ls", "Plus"], &["[].name"]),
    (&["drive", "info", "Plus"], &["root"]),
    (&["drive", "info", "Garbled"], &["root"]),
    (&["doctor", "Garbled", "--only", "drift"], &["drive"]),
    (&["diff", "--summary", "Blank", "A"], &["left.label", "right.label", "evidence.[].detail"]),
    (&["diff", "--summary", "A", "Club"], &["left.label", "right.label"]),
    (&["diff", "--summary", "Club", "Club2"], &["left.label", "right.label"]),
    (&["diff", "A", "Blank", "--playlist", "Set"], &["left", "right"]),
    (&["@empty", "tracks"], &[]),
    (&["@empty", "doctor"], &[]),
    (&["doctor", "A", "--only", "missing,large-files,audio,drift,compatibility"], &["drive", "large_files.[].path"]),
    (&["diff", "--summary", "A", "A"], &["left.label"]),
    (&["doctor", "A", "--hash-audio"], &["drive", "duplicates.[].tracks.[].track.file_path"]),
    (&["diff", "--summary", "Club", "A"], &["left.label", "right.label"]),
];

/// The commands that list a library's tracks and playlists, show a track, and check a drive say
/// what their JSON says, whatever the drives hold.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(flags = vec![true; FLAGS])]
#[hegel::explicit_test_case(flags = vec![false; FLAGS])]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 0).collect::<Vec<bool>>())]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 1).collect::<Vec<bool>>())]
#[hegel::explicit_test_case(flags = picked(&[14]))]
#[hegel::explicit_test_case(flags = picked(&[2, 4]))]
#[hegel::explicit_test_case(flags = picked(&[2, 4, 8, 16, 21, 22]))]
fn track_reads_say_what_their_json_says(tc: TestCase) {
    let flags = tc.draw(gs::vecs(gs::booleans()).min_size(FLAGS).max_size(FLAGS));
    let fx = build(State::from_flags(&flags));
    let other = twin(&fx);
    // A file FAT32 can't hold (sparse: it takes no room), made in each copy as `twin` would read
    // it whole.
    for fx in [&fx, &other] {
        std::fs::File::create(fx.volumes.join("A/Contents/big.mov")).unwrap().set_len(4 << 30).unwrap();
    }
    // Both copies run these reads too, so each is as the other when the reads start.
    let (universe, _) = side_by_side(|| universe(&fx), || universe(&other));
    reads_agree(&fx, &other, TRACK_READS, &universe);
}

/// Command lines each taking another way through the dispatch: the hidden top-level forms, the
/// player (refused without a terminal), the moved `set`, writing files elsewhere, flags that
/// don't go together, and no command at all.
const DISPATCH: &[(&[&str], &[&str])] = &[
    (&[], &[PRINTS_USAGE]),
    (&["tui"], &[]),
    (&["tui", "A"], &[]),
    (&["man"], &[]),
    (&["man", "-o", "@h/man"], &[]),
    (&["rpc", "--emit-schema", "@h/schema"], &[]),
    (&["history", "A"], &[]),
    (&["stats", "A"], &[]),
    (&["analysis", "bench", "@h/none"], &[]),
    (&["set", "A"], &[]),
    (&["sets", "--no-eject"], &[]),
    (&["store", "fetch", "not-a-code"], &[]),
    (&["settings", "import", "A", "--pds", "https://pds.example.com"], &[]),
    (&["settings", "import", "A", "--replace"], &[]),
    (&["format", "A", "--dry-run"], &[]),
    (&["drive", "format", "A", "--filesystem", "fat32", "--cluster-size", "64KiB", "--dry-run"], &[]),
    (&["drive", "format", "A", "--cluster-size", "lots"], &[]),
    (&["drive", "disks"], &[]),
    (&["drive", "Nope"], &[]),
    (&["drive", "info", "Nope"], &[]),
    (&["sync", "A"], &[]),
    (&["sync", "A", "--library", "A", "--dry-run"], &[]),
    (&["pull", "A", "--library", "Club", "--label", "x"], &[]),
    (&["pull", "--force"], &[]),
    (&["xml", "--replace"], &[]),
    (&["art", "get", "A", "2", "-o", "@h/none.jpg"], &[]),
    (&["art", "unembed", "--list", "--backup", "x"], &[]),
    (&["players", "set", "NOPE-1"], &[]),
    (&["players", "clear", "--library", "Nope"], &[]),
    (&["library", "rename", "Nope", "Other"], &[]),
    (&["library", "push", "Nope"], &[]),
    (&["doctor", "--fix-orphans"], &[]),
    (&["check", "Nope"], &[]),
    (&["merge", "A", "A"], &[]),
    (&["convert", "Blank", "--to", "onelibrary"], &[]),
    (&["clean", "Blank"], &[]),
    (&["undo", "Blank"], &[]),
    (&["store", "label", "zz", "x"], &[]),
    (&["store", "unlabel", "never-given"], &[]),
    (&["settings", "set", "quantize"], &[]),
    (&["settings", "set", "quantize=sideways"], &[]),
    (&["watch", "--uninstall"], &[]),
    (&["@empty", "--yes", "setup", "--library", "Fresh"], &[]),
    (&["@empty", "--yes", "art", "auto"], &[]),
    (&["@sudo-me", "provision", "Blank", "--library", "A", "--dry-run"], &[]),
    (&["@one", "tracks"], &["~tracks.[].title"]),
    (&["@one", "drive", "info"], &["root"]),
    (&["diff", "--summary", "--playlist", "Set", "A", "Blank"], &[]),
    (&["compare", "A", "Blank", "--playlist", "Set"], &[]),
    (&["@default-store", "drive", "format", "A", "--dry-run"], &[]),
    (&["@sudo", "drive", "format", "A", "--dry-run"], &[]),
    (&["@sudo", "provision", "Blank", "--library", "A", "--dry-run"], &[]),
    (&["--store", "@h/cover.png", "status"], &[]),
    (&["settings", "import", "--from-account"], &[]),
    (&["analysis", "bench", "A", "--keys", "@h/none"], &[]),
    (&["sync", "Clubland", "--library", "A"], &[]),
    (&["pull", "Clubland", "--library", "A"], &[]),
];

/// Every way through the command dispatch says the same with and without `--json`: the same exit
/// status, and an error's message said either way.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(flags = vec![false; FLAGS])]
#[hegel::explicit_test_case(flags = vec![true; FLAGS])]
fn dispatch_says_the_same_either_way(tc: TestCase) {
    let flags = tc.draw(gs::vecs(gs::booleans()).min_size(FLAGS).max_size(FLAGS));
    let fx = build(State::from_flags(&flags));
    let universe = universe(&fx);
    reads_agree(&fx, &twin(&fx), DISPATCH, &universe);
}
