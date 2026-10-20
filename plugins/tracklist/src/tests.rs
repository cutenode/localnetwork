use std::io::pipe;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use hegel::TestCase;
use hegel::generators as gs;
use loclib_model::model::Track;
use loclib_model::test_util::track;
use loclib_plugin::{Capability, Incoming, Manifest, PROTOCOL_VERSION, Peer, method};

use super::*;

/// The most tracks the host gives a plugin in a page of `drive.library`.
const PAGE: usize = 500;

/// A read callback: its method and params.
type Call = (String, Value);

/// The plugin served on a thread, over pipes, past `hello`, with a host whose drive `USB` has a
/// library and that answers its reads as locallibrary does.
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

/// A read as the host answers a plugin: only the params it may give (no `combined`), a drive
/// it knows, and `drive.library`'s tracks a page at a time.
fn read(lib: &Library, method: &str, params: Value) -> Result<Value, RpcError> {
    if method != "drive.library" {
        return Err(RpcError::failed("denied", format!("{method} needs a capability the plugin doesn't have")));
    }
    let Value::Object(params) = params else { return Err(RpcError::invalid_params("expected an object")) };
    if let Some(name) = params.keys().find(|k| !["drive", "offset", "limit"].contains(&k.as_str())) {
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
    fn plan_on(&self, drive: Option<&str>, params: Value) -> Result<OpPlan, RpcError> {
        let op = json!({ "operation": "import", "params": params, "context": { "drive": drive } });
        self.host.request(method::PLAN_OP, &op)
    }

    fn plan(&self, params: Value) -> Result<OpPlan, RpcError> {
        self.plan_on(Some("USB"), params)
    }

    fn report(&self, params: Value) -> Result<Report, RpcError> {
        let run = json!({ "command": "match", "params": params, "context": { "drive": "USB" } });
        self.host.request(method::RUN_COMMAND, &run)
    }

    /// Closes the plugin's input: it stops. Answers the reads it called back for.
    fn stop(self) -> Vec<Call> {
        self.host.close();
        self.served.join().unwrap();
        self.answering.join().unwrap();
        Arc::try_unwrap(self.calls).unwrap().into_inner().unwrap()
    }
}

fn playlist(id: u32, parent_id: Option<u32>, name: &str, is_folder: bool, track_ids: &[u32]) -> Playlist {
    Playlist { id, parent_id, name: name.into(), is_folder, sort_order: id, track_ids: track_ids.to_vec() }
}

fn set_entries(into: &str, paths: &[&str]) -> Value {
    json!({ "op": "set_entries", "playlist": into, "paths": paths, "create": true })
}

fn write(dir: &Path, name: &str, text: &str) -> String {
    let file = dir.join(name);
    std::fs::write(&file, text).unwrap();
    file.to_string_lossy().into_owned()
}

fn kind(e: &RpcError) -> (&str, &str) {
    (e.kind().unwrap_or_default(), e.message.as_str())
}

/// Bicep's Glue, and six tracks called In Charge.
fn club() -> Library {
    let mut tracks = vec![track(1, "Bicep", "Glue", "/Contents/Bicep/Glue.mp3")];
    let artists = ["Four Tet", "Floating Points", "Ross From Friends", "Kettama", "Overmono", "Joy O"];
    for (id, artist) in (2..).zip(artists) {
        tracks.push(track(id, artist, "In Charge", &format!("/Contents/{artist}/In Charge.mp3")));
    }
    Library { tracks, ..Default::default() }
}

/// It plans and may shorten a playlist: replacing one's entries with a shorter list does.
#[test]
fn the_manifest_is_a_first_party_one_that_reads_and_plans() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    assert!(manifest.validate(true).is_ok());
    assert!(manifest.validate(false).is_err(), "only a plugin locallibrary ships has a top-level CLI command");
    let capabilities = [Capability::LibraryRead, Capability::LibraryPlan, Capability::LibraryDelete];
    assert_eq!(manifest.capabilities, capabilities);
    let (command, operation) = (&manifest.contributes.commands[0], &manifest.contributes.operations[0]);
    assert_eq!((command.name.as_str(), command.cli.as_deref()), ("match", None));
    assert_eq!((operation.name.as_str(), operation.cli.as_deref()), ("import", Some("playlist import")));
    let s = start(Library::default());
    let described: Manifest = s.host.request(method::DESCRIBE, &Value::Null).unwrap();
    assert_eq!(described, manifest);
    s.stop();
}

/// As `playlist import` showed it: the found track fills the playlist, and the ambiguous and
/// missing entries are warned of, with up to five candidates. A choice settles one, and with
/// `append` the playlist's entries stay first.
#[test]
fn a_list_fills_the_playlist_with_what_is_found_and_warns_of_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let list = write(dir.path(), "list.txt", "Bicep - Glue\nIn Charge\nNobody - Nothing\n");
    let mut lib = club();
    let s = start(lib.clone());
    let plan = s.plan(json!({ "file": list, "into": "Imported" })).unwrap();
    assert_eq!(plan.summary, format!("{list}: 3 entr(ies) as Text: 1 found, 1 ambiguous, 1 missing"));
    let candidates = "#2 Four Tet - In Charge, #3 Floating Points - In Charge, #4 Ross From Friends - In Charge, \
                      #5 Kettama - In Charge, #6 Overmono - In Charge";
    let warnings = [
        format!("2. In Charge could be {candidates}: left out unless one is chosen"),
        "3. Nobody - Nothing isn't on the drive".to_string(),
    ];
    assert_eq!(plan.warnings, warnings);
    assert_eq!(plan.edits, [set_entries("Imported", &["/Contents/Bicep/Glue.mp3"])]);
    assert_eq!(plan.inputs, [PathBuf::from(&list)]);
    s.stop();

    lib.playlists = vec![playlist(1, None, "Imported", false, &[1])];
    let s = start(lib);
    let plan = s.plan(json!({ "file": list, "into": "Imported", "choices": { "1": 7 }, "append": true })).unwrap();
    assert_eq!(plan.summary, format!("{list}: 3 entr(ies) as Text: 2 found, 0 ambiguous, 1 missing"));
    assert_eq!(plan.warnings, ["3. Nobody - Nothing isn't on the drive"]);
    let paths = ["/Contents/Bicep/Glue.mp3", "/Contents/Bicep/Glue.mp3", "/Contents/Joy O/In Charge.mp3"];
    assert_eq!(plan.edits, [set_entries("Imported", &paths)]);
    s.stop();
}

/// Without `append` the list replaces the playlist's entries, though it has fewer.
#[test]
fn a_shorter_list_replaces_the_playlists_entries() {
    let dir = tempfile::tempdir().unwrap();
    let list = write(dir.path(), "list.txt", "Bicep - Glue\n");
    let mut lib = club();
    lib.playlists = vec![playlist(1, None, "Imported", false, &[2, 3, 4, 5])];
    let s = start(lib);
    let plan = s.plan(json!({ "file": list, "into": "Imported" })).unwrap();
    assert_eq!(plan.edits, [set_entries("Imported", &["/Contents/Bicep/Glue.mp3"])]);
    s.stop();
}

#[test]
fn a_spotify_csv_fills_the_playlist_with_the_tracks_found() {
    let dir = tempfile::tempdir().unwrap();
    let csv = "\"Track Name\",\"Artist Name(s)\",\"Duration (ms)\"\n\"Glue\",\"Bicep\",300000\n\
               \"In Charge\",\"Overmono\",300000\n\"Not on the drive\",\"Nobody\",1000\n";
    let list = write(dir.path(), "spotify.csv", csv);
    let s = start(club());
    let plan = s.plan(json!({ "file": list, "into": "From Spotify" })).unwrap();
    assert_eq!(plan.summary, format!("{list}: 3 entr(ies) as Csv: 2 found, 0 ambiguous, 1 missing"));
    assert_eq!(plan.warnings, ["3. \"Not on the drive\",\"Nobody\",1000 isn't on the drive"]);
    let paths = ["/Contents/Bicep/Glue.mp3", "/Contents/Overmono/In Charge.mp3"];
    assert_eq!(plan.edits, [set_entries("From Spotify", &paths)]);
    s.stop();
}

/// A list that played a track twice keeps both, an entry whose length is off waits for a
/// choice, and `into` is exactly a path: a playlist of that name in a folder is neither filled
/// nor appended to.
#[test]
fn a_list_keeps_its_repeats_and_fills_exactly_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = ("/Contents/Bicep/Glue.mp3", "/Contents/Burial/Archangel.mp3");
    let tracks = vec![track(1, "Bicep", "Glue", a), track(2, "Burial", "Archangel", b)];
    let shows = || vec![playlist(1, None, "Shows", true, &[]), playlist(2, Some(1), "Friday", false, &[2])];
    let m3u = format!(
        "#EXTM3U\n/Users/me/Music{a}\n/Users/me/Music{b}\n/Users/me/Music{a}\n#EXTINF:5000,Archangel\n/nowhere/x.mp3\n"
    );
    let list = write(dir.path(), "set.m3u8", &m3u);
    let s = start(Library { tracks: tracks.clone(), playlists: shows(), ..Default::default() });
    let report = s.report(json!({ "file": list })).unwrap();
    assert_eq!(report.entries[3].status, Status::Ambiguous { candidates: vec![2] }, "its length is off");
    let report = s.report(json!({ "file": list, "choices": { "3": 2 } })).unwrap();
    assert_eq!(report.entries[3].status, Status::Found { track_id: 2, how: MatchKind::Chosen });
    let plan = s.plan(json!({ "file": list, "into": "Friday", "choices": { "3": 2 } })).unwrap();
    assert_eq!(plan.edits, [set_entries("Friday", &[a, b, a, b])]);
    let plan = s.plan(json!({ "file": list, "into": "Friday", "append": true })).unwrap();
    assert_eq!(plan.edits, [set_entries("Friday", &[a, b, a])], "the nested namesake is left alone");
    s.stop();

    let mut playlists = shows();
    playlists.push(playlist(3, None, "Friday", false, &[1, 2, 1, 2]));
    let s = start(Library { tracks, playlists, ..Default::default() });
    let plan = s.plan(json!({ "file": list, "into": " Friday/", "append": true })).unwrap();
    assert_eq!(plan.edits, [set_entries(" Friday/", &[a, b, a, b, a, b, a])]);
    s.stop();
}

/// A rekordbox XML playlist is the one of exactly its path: of two whose paths differ only in
/// case, asking for neither is refused.
#[test]
fn an_xml_playlist_is_the_one_of_exactly_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <COLLECTION Entries="2">
    <TRACK TrackID="1" Name="Glue" Artist="Bicep" Location="file://localhost/Volumes/USB/Contents/Bicep/Glue.mp3"/>
    <TRACK TrackID="2" Name="In Charge" Artist="Kettama" Location="file://localhost/Volumes/USB/Contents/Kettama/In%20Charge.mp3"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="2">
      <NODE Name="Set" Type="1" KeyType="0" Entries="1"><TRACK Key="1"/></NODE>
      <NODE Name="SET" Type="1" KeyType="0" Entries="1"><TRACK Key="2"/></NODE>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>
"#;
    let list = write(dir.path(), "rekordbox.xml", xml);
    let s = start(club());
    let plan = s.plan(json!({ "file": list, "into": "Imported", "playlist": " /SET/ " })).unwrap();
    assert_eq!(plan.summary, format!("{list}: 1 entr(ies) as RekordboxXml: 1 found, 0 ambiguous, 0 missing"));
    assert_eq!(plan.edits, [set_entries("Imported", &["/Contents/Kettama/In Charge.mp3"])]);
    let all = s.report(json!({ "file": list })).unwrap();
    assert_eq!((all.format, all.summary.found), (Format::RekordboxXml, 2), "the whole collection");
    for name in ["set", "Nowhere"] {
        let e = s.plan(json!({ "file": list, "into": "Imported", "playlist": name })).unwrap_err();
        assert_eq!(kind(&e), ("not_found", format!("not found: playlist {name:?} in {list}").as_str()));
    }
    s.stop();
}

/// A list none of whose tracks are on the drive plans nothing, saying so, as `playlist import`
/// did; a choice must name an entry the list has and a track the drive has.
#[test]
fn a_list_of_nothing_the_drive_has_plans_nothing_and_a_bad_choice_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let list = write(dir.path(), "list.txt", "Nobody - Nothing\n");
    let s = start(club());
    for append in [false, true] {
        let plan = s.plan(json!({ "file": list, "into": "Set", "append": append })).unwrap();
        assert_eq!(plan.summary, format!("{list}: 1 entr(ies) as Text: 0 found, 0 ambiguous, 1 missing"));
        assert_eq!(
            plan.warnings,
            ["1. Nobody - Nothing isn't on the drive", "none of the list's tracks are on the drive"]
        );
        assert!(plan.edits.is_empty(), "{plan:?}");
    }
    let e = s.plan(json!({ "file": list, "into": "Set", "choices": { "0": 999 } })).unwrap_err();
    assert_eq!(kind(&e), ("not_found", "not found: track #999"));
    let e = s.plan(json!({ "file": list, "into": "Set", "choices": { "5": 1 } })).unwrap_err();
    assert_eq!(kind(&e), ("invalid", "no entry 5 in the list"));
    s.stop();
}

/// The `match` command answers what loclib-tracklist finds, as `playlist import --json`'s report
/// did.
#[test]
fn the_command_answers_each_entry_as_loclib_tracklist_finds_it() {
    let dir = tempfile::tempdir().unwrap();
    let text = "1. Bicep - Glue (5:00)\n2. In Charge\n3. Glue by Bicep\n4. Nobody - Nothing\n";
    let list = write(dir.path(), "set.txt", text);
    let lib = club();
    let s = start(lib.clone());
    let report = s.report(json!({ "file": list })).unwrap();
    let entries = tracklist::parse(text.as_bytes(), Some("set.txt")).unwrap();
    assert_eq!(report, Report::new(Format::Text, tracklist::match_entries(&lib, &entries)));
    assert_eq!((report.summary.found, report.summary.ambiguous, report.summary.missing), (2, 1, 1));
    s.stop();
}

/// The library comes a page at a time, from the drive the user is on, as its players read it.
#[test]
fn the_library_is_read_a_page_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let list = write(dir.path(), "list.txt", "Bicep - Glue\n");
    let mut tracks: Vec<Track> =
        (1..=1200).map(|id| track(id, "Someone", &format!("Track {id}"), &format!("/Contents/{id}.mp3"))).collect();
    tracks.push(track(1201, "Bicep", "Glue", "/Contents/Bicep/Glue.mp3"));
    let s = start(Library { tracks, ..Default::default() });
    let plan = s.plan(json!({ "file": list, "into": "Imported" })).unwrap();
    assert_eq!(plan.edits, [set_entries("Imported", &["/Contents/Bicep/Glue.mp3"])]);
    let calls = s.stop();
    let reads: Vec<Call> =
        [0, 500, 1000].iter().map(|o| ("drive.library".into(), json!({ "drive": "USB", "offset": o }))).collect();
    assert_eq!(calls, reads);
}

#[test]
fn what_cant_be_read_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let list = write(dir.path(), "list.txt", "Bicep - Glue\n");
    let s = start(club());
    let e = s.plan_on(None, json!({ "file": list, "into": "Imported" })).unwrap_err();
    assert_eq!(kind(&e), ("invalid_params", "invalid params: say which drive"));
    let e = s.plan_on(Some("Other"), json!({ "file": list, "into": "Imported" })).unwrap_err();
    assert_eq!(kind(&e), ("not_found", "not found: drive \"Other\""));
    let gone = dir.path().join("gone.txt");
    let e = s.plan(json!({ "file": gone, "into": "Imported" })).unwrap_err();
    assert_eq!(e.kind(), Some("io"));
    assert!(e.message.starts_with(&format!("{}: ", gone.display())), "{}", e.message);
    let table = write(dir.path(), "list.csv", "Artist,Album\nBicep,Glue\n");
    let e = s.plan(json!({ "file": table, "into": "Imported" })).unwrap_err();
    assert_eq!(kind(&e), ("invalid", "no title or file column in the table's header (found: Artist, Album)"));
    let e = s.plan(json!({ "file": list, "into": "Imported", "playlist": "Set" })).unwrap_err();
    assert_eq!(kind(&e), ("invalid", "not a rekordbox XML file (no DJ_PLAYLISTS element)"));
    let e = s.plan(json!({ "into": "Imported" })).unwrap_err();
    assert_eq!(e.kind(), Some("invalid_params"));
    s.stop();
}

// Properties: drawn tracklists of each format against drawn libraries.

/// The ways a drawn tracklist is written.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    Lines,
    M3u,
    Spotify,
    RekordboxText,
    RekordboxXml,
}

/// A tracklist of `shape` naming `picks` (`None` for a track no drive has): its file name and
/// text.
fn tracklist(shape: Shape, picks: &[Option<&Track>]) -> (&'static str, String) {
    let nobody = track(0, "Nobody", "Nothing", "/Contents/Nobody/Nothing.mp3");
    let picked = picks.iter().map(|p| p.unwrap_or(&nobody));
    match shape {
        Shape::Lines => ("list.txt", picked.map(|t| format!("{} - {}\n", t.artist, t.title)).collect()),
        Shape::M3u => {
            let lines = picked.map(|t| format!("#EXTINF:300,{}\n/Users/me/Music{}\n", t.display_name(), t.file_path));
            ("set.m3u8", format!("#EXTM3U\n{}", lines.collect::<String>()))
        }
        Shape::Spotify => {
            let rows = picked.map(|t| format!("\"{}\",\"{}\",300000\n", t.title, t.artist));
            (
                "spotify.csv",
                format!("\"Track Name\",\"Artist Name(s)\",\"Duration (ms)\"\n{}", rows.collect::<String>()),
            )
        }
        Shape::RekordboxText => {
            let rows = picked.enumerate().map(|(i, t)| format!("{}\t{}\t{}\n", i + 1, t.title, t.artist));
            ("rekordbox.txt", format!("#\tTrack Title\tArtist\n{}", rows.collect::<String>()))
        }
        Shape::RekordboxXml => {
            let tracks: String = picked
                .enumerate()
                .map(|(i, t)| {
                    let at = t.file_path.replace(' ', "%20");
                    let (title, artist) = (&t.title, &t.artist);
                    format!(
                        "<TRACK TrackID=\"{}\" Name=\"{title}\" Artist=\"{artist}\" Location=\"file://localhost/Volumes/USB{at}\"/>\n",
                        i + 1
                    )
                })
                .collect();
            let keys: String = (1..=picks.len()).map(|key| format!("<TRACK Key=\"{key}\"/>")).collect();
            let first = if picks.is_empty() { String::new() } else { "<TRACK Key=\"1\"/>".into() };
            let xml = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<DJ_PLAYLISTS Version=\"1.0.0\">\n<COLLECTION Entries=\"{}\">\n\
                 {tracks}</COLLECTION>\n<PLAYLISTS><NODE Type=\"0\" Name=\"ROOT\" Count=\"2\">\n\
                 <NODE Name=\"Set\" Type=\"1\" KeyType=\"0\" Entries=\"{}\">{keys}</NODE>\n\
                 <NODE Name=\"Other\" Type=\"1\" KeyType=\"0\" Entries=\"1\">{first}</NODE>\n\
                 </NODE></PLAYLISTS>\n</DJ_PLAYLISTS>\n",
                picks.len(),
                picks.len()
            );
            ("rekordbox.xml", xml)
        }
    }
}

/// What loclib-tracklist makes of the list in `bytes` (named `name`, at `file`) against `lib`,
/// with `choices` made, as `playlist import` matched it; or why it can't be read (its error's kind
/// and message).
fn imported(
    lib: &Library,
    bytes: &[u8],
    name: &str,
    file: &str,
    playlist: Option<&str>,
    choices: &[(usize, u32)],
) -> Result<Report, (&'static str, String)> {
    let unreadable = |e: tracklist::Error| ("invalid", e.to_string());
    let (format, entries) = match playlist {
        Some(p) => {
            let lists = tracklist::xml_playlists(bytes).map_err(unreadable)?;
            let entries = model::pick(lists, p.trim().trim_matches('/'))
                .ok_or_else(|| ("not_found", format!("not found: playlist {p:?} in {file}")))?;
            (Format::RekordboxXml, entries)
        }
        None => (tracklist::detect(bytes, Some(name)), tracklist::parse(bytes, Some(name)).map_err(unreadable)?),
    };
    let mut entries = tracklist::match_entries(lib, &entries);
    for &(i, id) in choices {
        let Some(m) = entries.get_mut(i) else { return Err(("invalid", format!("no entry {i} in the list"))) };
        if lib.track(id).is_none() {
            return Err(("not_found", format!("not found: track #{id}")));
        }
        m.status = Status::Found { track_id: id, how: MatchKind::Chosen };
    }
    Ok(Report::new(format, entries))
}

/// For any library (namesakes included) and any tracklist of each format naming some of its
/// tracks and some it hasn't, the `match` command answers what loclib-tracklist matches, with the
/// choices made, and the import plans the playlist at exactly `into` filled with the tracks found,
/// in the list's order (after its entries, with `append`), warning of each entry left out and
/// planning nothing when none is found; a list that can't be read, a playlist it hasn't, a bad
/// choice and a missing drive are refused, saying why.
#[hegel::test(test_cases = 100)]
fn any_tracklist_imports_what_loclib_tracklist_matches(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(6));
    let artists = || gs::sampled_from(vec!["Bicep", "Kettama", "Overmono"]);
    let titles = || gs::sampled_from(vec!["Glue", "In Charge", "Apricots"]);
    let tracks: Vec<Track> = (1..=n)
        .map(|id| {
            let (artist, title) = (tc.draw(artists()), tc.draw(titles()));
            track(id, artist, title, &format!("/Contents/{artist}/{title} {id}.mp3"))
        })
        .collect();
    let some = || gs::vecs(gs::integers::<u32>().min_value(1).max_value(n)).min_size(1).max_size(4);
    let mut playlists = Vec::new();
    if tc.draw(gs::weighted_booleans(0.7)) {
        playlists.push(playlist(1, None, "Imported", false, &tc.draw(some())));
    }
    if tc.draw(gs::weighted_booleans(0.7)) {
        let ids = tc.draw(some());
        playlists.extend([playlist(2, None, "Shows", true, &[]), playlist(3, Some(2), "Imported", false, &ids)]);
    }
    let lib = Library { tracks, playlists, ..Default::default() };

    let shape = tc.draw(gs::sampled_from(vec![
        Shape::Lines,
        Shape::M3u,
        Shape::Spotify,
        Shape::RekordboxText,
        Shape::RekordboxXml,
    ]));
    let picks: Vec<Option<u32>> =
        tc.draw(gs::vecs(gs::optional(gs::integers::<u32>().min_value(1).max_value(n))).max_size(6));
    let picked: Vec<Option<&Track>> = picks.iter().map(|p| p.and_then(|id| lib.track(id))).collect();
    let (name, text) = tracklist(shape, &picked);
    let dir = tempfile::tempdir().unwrap();
    let gone = tc.draw(gs::weighted_booleans(0.03));
    let file = match gone {
        true => dir.path().join(name).to_string_lossy().into_owned(),
        false => write(dir.path(), name, &text),
    };
    let from = match shape {
        Shape::RekordboxXml => {
            tc.draw(gs::sampled_from(vec![None, Some("Set"), Some(" /Set/ "), Some("set"), Some("Nowhere")]))
        }
        _ => tc.draw(gs::weighted_booleans(0.05)).then_some("Set"),
    };
    // Choices for some entries, and now and then one that's bad (a refusal names the first it
    // finds, so there's only ever one).
    let mut choices: Vec<(usize, u32)> = Vec::new();
    if !picks.is_empty() && tc.draw(gs::weighted_booleans(0.3)) {
        let choice = gs::tuples!(
            gs::integers::<usize>().max_value(picks.len() - 1),
            gs::integers::<u32>().min_value(1).max_value(n)
        );
        for (i, id) in tc.draw(gs::vecs(choice).min_size(1).max_size(2)) {
            if !choices.iter().any(|c| c.0 == i) {
                choices.push((i, id));
            }
        }
    }
    if tc.draw(gs::weighted_booleans(0.1)) {
        let bad = match tc.draw(gs::booleans()) && !picks.is_empty() {
            true => (0, n + 1),
            false => (picks.len(), 1),
        };
        choices.retain(|c| c.0 != bad.0);
        choices.push(bad);
    }
    let into = tc.draw(gs::sampled_from(vec!["Imported", "Imported", " Imported/", "Shows/Imported", "Elsewhere"]));
    let append = tc.draw(gs::booleans());
    let on_drive = tc.draw(gs::weighted_booleans(0.95));

    let mut params = json!({ "file": file, "into": into, "append": append });
    if let Some(from) = from {
        params["playlist"] = json!(from);
    }
    if !choices.is_empty() {
        let chosen: serde_json::Map<String, Value> = choices.iter().map(|(i, id)| (i.to_string(), json!(id))).collect();
        params["choices"] = Value::Object(chosen);
    }
    let s = start(lib.clone());
    let planned = s.plan_on(on_drive.then_some("USB"), params.clone());
    let report = on_drive.then(|| {
        let mut asked = params.clone();
        let asked_obj = asked.as_object_mut().unwrap();
        asked_obj.remove("into");
        asked_obj.remove("append");
        s.report(asked)
    });
    s.stop();

    let foreseen = match (on_drive, gone) {
        (false, _) => Err(("invalid_params", "invalid params: say which drive".to_string())),
        (true, true) => Err(("io", format!("{file}: "))),
        (true, false) => imported(&lib, text.as_bytes(), name, &file, from, &choices),
    };
    let (planned, foreseen) = match (planned, foreseen) {
        // The command refuses as the import does.
        (Err(e), Err((kind, message))) => {
            for e in std::iter::once(e).chain(report.map(Result::unwrap_err)) {
                assert_eq!(e.kind(), Some(kind), "{e:?}");
                assert!(e.message.starts_with(&message), "{e:?}, not {message}");
            }
            return;
        }
        (Ok(plan), Ok(foreseen)) => (plan, foreseen),
        (planned, foreseen) => panic!("planned {planned:?}, not {foreseen:?}"),
    };
    assert_eq!(report.map(|r| r.unwrap()), Some(foreseen.clone()));

    // Each track picked that the drive has is found, unless it has a namesake (a list naming it
    // by path finds it all the same), or the user chose for it.
    for ((i, pick), m) in picks.iter().enumerate().zip(&foreseen.entries) {
        let namesakes = |id: u32| {
            let t = lib.track(id).unwrap();
            lib.tracks.iter().filter(|o| o.artist == t.artist && o.title == t.title).count()
        };
        let by_path = matches!(shape, Shape::M3u | Shape::RekordboxXml);
        match pick {
            _ if choices.iter().any(|c| c.0 == i) => {}
            Some(id) if by_path || namesakes(*id) == 1 => assert_eq!(m.status.track_id(), Some(*id), "{m:?}"),
            Some(_) => assert!(matches!(m.status, Status::Ambiguous { .. }), "{m:?}"),
            None => assert_eq!(m.status, Status::Missing, "{m:?}"),
        }
    }

    let s = &foreseen.summary;
    let summary = format!(
        "{file}: {} entr(ies) as {:?}: {} found, {} ambiguous, {} missing",
        s.entries, foreseen.format, s.found, s.ambiguous, s.missing
    );
    assert_eq!(planned.summary, summary);
    let mut warnings: Vec<String> = Vec::new();
    for (n, m) in (1..).zip(&foreseen.entries) {
        let raw = m.entry.raw.trim();
        match &m.status {
            Status::Found { .. } => {}
            Status::Missing => warnings.push(format!("{n}. {raw} isn't on the drive")),
            Status::Ambiguous { candidates } => {
                let named: Vec<String> = candidates
                    .iter()
                    .take(5)
                    .map(|&id| format!("#{id} {}", lib.track(id).unwrap().display_name()))
                    .collect();
                warnings.push(format!("{n}. {raw} could be {}: left out unless one is chosen", named.join(", ")));
            }
        }
    }
    let paths = |ids: &[u32]| ids.iter().map(|&id| lib.track(id).unwrap().file_path.clone()).collect::<Vec<_>>();
    let found = paths(&foreseen.track_ids());
    if found.is_empty() {
        warnings.push("none of the list's tracks are on the drive".into());
    }
    assert_eq!(planned.warnings, warnings);
    // The playlist at exactly that path, never its namesake elsewhere.
    let there = match into.trim_matches([' ', '/']) {
        "Imported" => lib.playlists.iter().find(|p| p.id == 1),
        "Shows/Imported" => lib.playlists.iter().find(|p| p.id == 3),
        _ => None,
    };
    let kept = match (append, there) {
        (true, Some(p)) => paths(&p.track_ids),
        _ => Vec::new(),
    };
    let edits = match found.is_empty() {
        true => Vec::new(),
        false => {
            vec![
                json!({ "op": "set_entries", "playlist": into, "paths": ([kept, found.clone()].concat()), "create": true }),
            ]
        }
    };
    assert_eq!(planned.edits, edits);
    assert_eq!(planned.inputs, [PathBuf::from(&file)]);
}
