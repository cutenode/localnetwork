//! A set's and a mix's exports, as `locallibrary sets export SET --format traktor --drive DRIVE`
//! and `locallibrary mix RECORDING --drive DRIVE --format serato --export DIR` run them: the drive's
//! library narrowed to one playlist of the tracks played on it, in order, written as the drive's
//! exports write a library.

use super::*;

const DEMO_1: &str = "/Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3";
const DEMO_2: &str = "/Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3";
const FLAC: &str = "/Contents/Other/lossless.flac";
const ONLY_PLUS: &str = "/Contents/Plus/Only Plus.mp3";

/// A track played: on the drive by its file, or (with no file, or one the drive hasn't) not.
struct Heard {
    artist: &'static str,
    title: &'static str,
    file: Option<String>,
}

fn on_drive(file: &str) -> Heard {
    Heard { artist: "Loopmasters", title: "On the drive", file: Some(file.into()) }
}

/// Typed in by hand: no file.
const TYPED: Heard = Heard { artist: "Floorplan", title: "Never Grow Old", file: None };

fn elsewhere() -> Heard {
    Heard { artist: "", title: "Gone", file: Some("/Contents/Gone.mp3".into()) }
}

/// `sets.get`'s answer for a set of `heard`, as the host gives a plugin it.
fn set(heard: &[Heard]) -> Value {
    let tracks: Vec<Value> = heard
        .iter()
        .map(|h| json!({ "artist": h.artist, "title": h.title, "file": h.file, "bpm": 0.0, "origin": "session" }))
        .collect();
    json!({ "id": "1a2b", "date": "2026-10-03", "title": "Friday", "venue": "Warehouse", "tracks": tracks })
}

/// `mix.reports.get`'s answer for a mix of `heard`: each play's track as the drive's library has
/// it.
fn mix(heard: &[Heard]) -> Value {
    let plays: Vec<Value> = heard
        .iter()
        .map(|h| {
            let path = h.file.clone().unwrap_or_default();
            json!({ "track": Track { artist: h.artist.into(), title: h.title.into(), file_path: path, ..Track::default() }, "start": 0.0 })
        })
        .collect();
    json!({ "mix": { "title": "Live at the Warehouse", "date": "2026-10-04", "kind": "audio" }, "plays": plays })
}

/// What a drive's export gives loclib-interop, but with one playlist, `name`, of the tracks at
/// `files` in that order, and only those tracks.
fn played(drive: &Drive, name: &str, files: &[&str]) -> Expected {
    let mut want = expected(drive, &[]);
    let id = |file: &str| want.library.tracks.iter().find(|t| paths::file_key(&t.file_path) == paths::file_key(file));
    let ids: Vec<u32> = files.iter().map(|f| id(f).unwrap().id).collect();
    want.library.tracks.retain(|t| ids.contains(&t.id));
    want.library.playlists =
        vec![Playlist { id: 1, parent_id: None, name: name.into(), is_folder: false, sort_order: 0, track_ids: ids }];
    want.library.history.clear();
    want.library.history_playlists = 0;
    want
}

/// `drive`, with the store keeping `answer` as `id`.
fn kept(mut drive: Drive, id: &str, answer: Value) -> Drive {
    drive.kept.insert(id.into(), answer);
    drive
}

/// A set's tracks on the drive go into a Traktor playlist named for its day and title, in play
/// order, one played twice twice, found by file whatever its case; those that aren't there are
/// left out, each named, and only the tracks played are in the collection, with their beat grids
/// and cues.
#[test]
fn a_set_is_a_playlist_of_its_tracks_on_the_drive() {
    let heard = [on_drive(&DEMO_2.to_uppercase()), TYPED, on_drive(ONLY_PLUS), on_drive(DEMO_2), elsewhere()];
    let drive = kept(drive(), "1a2b", set(&heard));
    let session = start(&drive);
    let output = drive.dir.path().join("for traktor");
    let source = json!({ "drive": "USB", "set": "1a2b" });
    let planned = session.export_of("traktor", source.clone(), &output, json!({}), true).unwrap();
    let done = session.export_of("traktor", source, &output, json!({}), false).unwrap();
    assert_eq!(done, planned);
    assert_eq!(session.calls("sets.get"), [json!({ "set": "1a2b" }), json!({ "set": "1a2b" })]);
    let analysis = session.calls("tracks.analysis");
    assert_eq!(analysis[0]["ids"], json!([2, PLUS]), "only the tracks played are read");

    let nml = std::fs::read_to_string(output.join("collection.nml")).unwrap();
    let list = &nml[nml.find("<NODE TYPE=\"PLAYLIST\"").unwrap()..];
    assert!(list.starts_with("<NODE TYPE=\"PLAYLIST\" NAME=\"2026-10-03 Friday\"><PLAYLIST ENTRIES=\"3\""), "{list}");
    let order: Vec<usize> = ["Demo Track 2.mp3", "Only Plus.mp3"].iter().map(|f| list.find(f).unwrap()).collect();
    assert!(order[0] < order[1] && list.rfind("Demo Track 2.mp3").unwrap() > order[1], "{list}");

    let want = played(&drive, "2026-10-03 Friday", &[DEMO_2, ONLY_PLUS, DEMO_2]);
    let left_out = vec![
        "2. Floorplan - Never Grow Old isn't on the drive: left out".to_string(),
        "5. Gone isn't on the drive: left out".to_string(),
    ];
    let asked =
        Asked { software: Software::Traktor, root: drive.root.clone(), skip: false, tags: false, on_drive: false };
    is_built(&want, &asked, &drive, &output, &done, left_out);
    assert_eq!(done.counts["tracks"], 2);
    session.stop();
}

/// A mix's plays are found the same way, its playlist named for its day and title: a Serato crate
/// listing the track twice, and an Engine playlist, which holds a track once, saying so.
#[test]
fn a_mix_is_a_playlist_of_its_plays_on_the_drive() {
    let heard = [on_drive(DEMO_1), on_drive(FLAC), on_drive(DEMO_1), elsewhere()];
    let drive = kept(drive(), "live", mix(&heard));
    let session = start(&drive);
    let source = json!({ "drive": "USB", "mix": "live" });
    let name = "2026-10-04 Live at the Warehouse";
    let want = played(&drive, name, &[DEMO_1, FLAC, DEMO_1]);
    let left_out = || vec!["4. Gone isn't on the drive: left out".to_string()];
    for (format, software) in [("serato", Software::Serato), ("engine", Software::Engine)] {
        let output = drive.dir.path().join(format);
        let done = session.export_of(format, source.clone(), &output, json!({}), false).unwrap();
        let asked = Asked { software, root: drive.root.clone(), skip: false, tags: false, on_drive: false };
        is_built(&want, &asked, &drive, &output, &done, left_out());
        assert_eq!(done.counts["playlists"], 1);
    }
    let crate_file = drive.dir.path().join(format!("serato/_Serato_/Subcrates/{name}.crate"));
    assert!(crate_file.is_file());
    assert_eq!(session.calls("mix.reports.get"), [json!({ "id": "live" }), json!({ "id": "live" })]);
    session.stop();
}

/// A mix with neither a day nor a title still gets a playlist with a name.
#[test]
fn a_mix_without_a_name_is_called_mix() {
    let mut answer = mix(&[on_drive(DEMO_1)]);
    answer["mix"] = json!({ "title": "", "kind": "audio" });
    let drive = kept(drive(), "live", answer);
    let session = start(&drive);
    let output = drive.dir.path().join("out");
    let source = json!({ "drive": "USB", "mix": "live" });
    session.export_of("traktor", source, &output, json!({}), false).unwrap();
    let nml = std::fs::read_to_string(output.join("collection.nml")).unwrap();
    assert!(nml.contains("<NODE TYPE=\"PLAYLIST\" NAME=\"Mix\">"), "{nml}");
    session.stop();
}

/// Without a drive there's nothing for the files to point at: refused, saying how to name one,
/// before anything is read. With none of its tracks on the drive, or a set the store hasn't, it's
/// not found.
#[test]
fn a_set_needs_its_tracks_on_a_drive() {
    let drive = kept(kept(drive(), "1a2b", set(&[TYPED, elsewhere()])), "live", mix(&[]));
    let session = start(&drive);
    let output = drive.dir.path().join("out");
    for (format, source, what) in [
        ("traktor", json!({ "drive": "", "set": "1a2b" }), "set"),
        ("engine", json!({ "drive": "", "mix": "live" }), "mix"),
    ] {
        let e = session.export_of(format, source, &output, json!({}), true).unwrap_err();
        assert_eq!((e.code, e.kind()), (code::INVALID_PARAMS, Some("invalid")));
        let said = format!("a {what}'s export points at its tracks' audio: name the drive they're on");
        assert_eq!(e.message, said);
    }
    assert!(session.calls.lock().unwrap().is_empty(), "nothing is read");
    for (format, source, said) in [
        ("serato", json!({ "drive": "USB", "set": "1a2b" }), "none of the set's tracks are on the drive"),
        ("traktor", json!({ "drive": "USB", "mix": "live" }), "none of the mix's tracks are on the drive"),
        ("traktor", json!({ "drive": "USB", "set": "nope" }), "not found: \"nope\""),
    ] {
        let e = session.export_of(format, source, &output, json!({}), true).unwrap_err();
        assert_eq!((e.kind(), e.message.as_str()), (Some("not_found"), said));
    }
    assert!(!output.exists());
    session.stop();
}

/// For any set or mix of tracks on the drive (any of them, in any order, any of them more than
/// once, their files in any case), typed in or gone from it, with or without a drive, in any of
/// the formats with any options its schema takes: an export without a drive, or with none of the
/// tracks on it, is refused; otherwise the dry run names exactly the files then written, and
/// they're what loclib-interop builds of the drive's library narrowed to one playlist of the
/// tracks found, in play order, each play left out named in a warning of its own.
#[hegel::test(test_cases = 60)]
fn any_set_or_mix_exports_the_tracks_played_on_the_drive(tc: TestCase) {
    let mix_of = tc.draw(gs::booleans());
    let software = tc.draw(gs::sampled_from(vec![Software::Traktor, Software::Serato, Software::Engine]));
    let what = if mix_of { "mix" } else { "set" };
    let format = serde_json::to_value(software).unwrap().as_str().unwrap().to_string();
    let choices = vec![Some(DEMO_1), Some(DEMO_2), Some(FLAC), Some(ONLY_PLUS), None, None];
    let picks = tc.draw(gs::vecs(gs::sampled_from(choices)).max_size(8));
    let heard: Vec<Heard> = picks
        .iter()
        .enumerate()
        .map(|(i, pick)| match pick {
            Some(file) if tc.draw(gs::booleans()) => on_drive(&file.to_lowercase()),
            Some(file) => on_drive(file),
            None if i % 2 == 0 => TYPED,
            None => elsewhere(),
        })
        .collect();
    let (id, answer) = if mix_of { ("live", mix(&heard)) } else { ("1a2b", set(&heard)) };
    let drive = kept(drive(), id, answer);
    let named = tc.draw(gs::weighted_booleans(0.9));
    let mut source = json!({ "drive": if named { "USB" } else { "" } });
    source[what] = json!(id);

    let root = tc.draw(gs::weighted_booleans(0.3)).then(|| drive.dir.path().join("Music Copy"));
    let mut options = json!({});
    if let Some(root) = &root {
        options["location_root"] = json!(root);
    }
    if software != Software::Engine && tc.draw(gs::booleans()) {
        options["skip_analysis"] = json!(tc.draw(gs::booleans()));
    }
    let tags = software == Software::Serato && root.is_none() && tc.draw(gs::booleans());
    if tags {
        options["write_tags"] = json!(true);
    }
    let on_drive_paths = software == Software::Engine && tc.draw(gs::booleans());
    if on_drive_paths {
        options["paths"] = json!("drive");
    }
    let manifest = Manifest::parse(MANIFEST).unwrap();
    let schema = manifest.contributes.exporters.iter().find(|e| e.format == format).unwrap().options.clone();
    assert!(check_params(schema.as_ref(), &options).is_ok(), "{options}");

    let session = start(&drive);
    let output = drive.dir.path().join("out");
    let planned = session.export_of(&format, source.clone(), &output, options.clone(), true);
    assert!(!output.exists(), "a dry run writes nothing");
    let found: Vec<&str> = picks.iter().flatten().copied().collect();
    if !named || found.is_empty() {
        let e = planned.unwrap_err();
        let said = match named {
            false => {
                format!("a {what}'s export points at its tracks' audio: name the drive they're on")
            }
            true => format!("none of the {what}'s tracks are on the drive"),
        };
        assert_eq!(e.message, said);
        assert_eq!(e.kind(), Some(if named { "not_found" } else { "invalid" }));
        session.stop();
        return;
    }
    let planned = planned.unwrap();
    let done = session.export_of(&format, source, &output, options.clone(), false).unwrap();
    session.stop();
    assert_eq!(done, planned);
    let mut named_files = paths(&done);
    named_files.sort();
    let written_paths: Vec<PathBuf> = files(&output).into_iter().map(|(path, _)| path).collect();
    assert_eq!(written_paths, named_files, "the dry run named what's written, and only that");

    let name = if mix_of { "2026-10-04 Live at the Warehouse" } else { "2026-10-03 Friday" };
    let want = played(&drive, name, &found);
    let left_out: Vec<String> = heard
        .iter()
        .enumerate()
        .filter(|(i, _)| picks[*i].is_none())
        .map(|(i, h)| {
            let name = [h.artist, h.title].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" - ");
            format!("{}. {name} isn't on the drive: left out", i + 1)
        })
        .collect();
    let skip = options["skip_analysis"] == true;
    let asked = Asked { software, root: root.unwrap_or(drive.root.clone()), skip, tags, on_drive: on_drive_paths };
    is_built(&want, &asked, &drive, &output, &done, left_out);
}
