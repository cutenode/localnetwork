//! History lists on request (`include_history`): Traktor, Serato and Engine DJ get them as
//! playlists in a folder `History`, as loclib-interop's `with_history` makes them, with a warning
//! that they stop being history there.

use super::*;

/// What the core exports gave loclib-interop for `drive` with its history lists as playlists:
/// the library narrowed to `only` (or, `named`, to no playlists at all), then its history lists
/// added, each with its tracks and their analysis.
fn with_history(drive: &Drive, only: &[&str], named: bool) -> Expected {
    let mut all = drive.library.clone();
    let old = all.number_for_files();
    let only: Vec<String> = only.iter().map(|s| s.to_string()).collect();
    let base = match named {
        true => Library { playlists: Vec::new(), tracks: Vec::new(), ..all.clone() },
        false => all.clone().subset(&only).unwrap(),
    };
    let library = loclib_interop::with_history(base, &all.history, &all.tracks);
    let analysis = library
        .tracks
        .iter()
        .filter_map(|t| Some((t.id, drive.analysis.get(old.get(&t.id).unwrap_or(&t.id))?.clone())))
        .collect();
    Expected { library, analysis }
}

const WARNING: &str = "1 history list(s) go as playlists in the folder History";

/// The whole library, its history list too, in each format that keeps no history: the files
/// loclib-interop writes for it, a warning saying so, and notes naming the option.
#[test]
fn history_lists_go_as_playlists_on_request() {
    let drive = drive();
    let session = start(&drive);
    let want = with_history(&drive, &[], false);
    let history: Vec<&str> = want.library.playlists.iter().skip(4).map(|p| p.name.as_str()).collect();
    assert_eq!(history, ["History", "HISTORY 001"]);
    for format in ["traktor", "serato", "engine"] {
        let output = drive.dir.path().join(format);
        let done = session.export(format, &output, json!({ "include_history": true }), false).unwrap();
        let out = resolved(&output);
        let export = want.build(&drive.root, false, |s| match format {
            "traktor" => traktor::build(s, &output, &Ctx::silent()).unwrap(),
            "serato" => serato::build::<Error>(s, &output, None, &Ctx::silent()).unwrap(),
            _ => engine::build(s, &out, PathStyle::Relative, &Ctx::silent()).unwrap(),
        });
        assert_eq!(written(&output), contents(&export, &out, &output), "{format}");
        assert_eq!((export.playlists, export.folders), (4, 2), "{format}");
        let software: Software = serde_json::from_value(json!(format)).unwrap();
        let warned = format!("{WARNING}: in {} they're playlists, not history", software.name());
        assert_eq!(done.warnings.first(), Some(&warned), "{format}");
        if format != "serato" {
            assert!(
                done.notes.iter().any(|n| n.contains("history lists go only as playlists (include_history)")
                    || n.contains("History lists go only as playlists (include_history)")),
                "{:?}",
                done.notes
            );
        }
    }
    session.stop();
}

/// A history list named alone goes instead of the playlists (as `export --history` asks), and
/// with playlists named, beside them, each with only its own tracks.
#[test]
fn named_history_lists_go_alone_or_beside_playlists() {
    let drive = drive();
    let session = start(&drive);
    let output = drive.dir.path().join("out");
    let options = json!({ "include_history": true });
    let alone = json!({ "drive": "USB", "history": ["HISTORY 001"] });
    session.export_of("traktor", alone, &output, options.clone(), false).unwrap();
    let want = with_history(&drive, &[], true);
    let ids: Vec<u32> = want.library.tracks.iter().map(|t| t.id).collect();
    assert_eq!(ids, [1]);
    let export = want.build(&drive.root, false, |s| traktor::build(s, &output, &Ctx::silent()).unwrap());
    assert_eq!(written(&output), contents(&export, &output, &output));

    let beside = json!({ "drive": "USB", "playlists": ["Booth"], "history": ["HISTORY 001"] });
    let options = json!({ "include_history": true, "skip_analysis": true });
    session.export_of("traktor", beside, &output, options, false).unwrap();
    let want = with_history(&drive, &["Booth"], false);
    let names: Vec<&str> = want.library.playlists.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Booth", "History", "HISTORY 001"]);
    let export = want.build(&drive.root, true, |s| traktor::build(s, &output, &Ctx::silent()).unwrap());
    assert_eq!(written(&output), contents(&export, &output, &output));
    session.stop();
}

/// History lists named without the option, one the drive hasn't, and the option with a set (which
/// has no history lists, so it isn't read) are refused, before anything is written.
#[test]
fn history_lists_are_asked_for() {
    let drive = drive();
    let session = start(&drive);
    let output = drive.dir.path().join("out");
    let named = json!({ "drive": "USB", "history": ["HISTORY 001"] });
    let unknown = json!({ "drive": "USB", "history": ["HISTORY 404"] });
    let set = json!({ "drive": "USB", "set": "1a2b" });
    let on = json!({ "include_history": true });
    let refusals = [
        session.export_of("engine", named, &output, json!({}), true),
        session.export_of("serato", unknown, &output, on.clone(), true),
        session.export_of("traktor", set, &output, on, true),
    ];
    let messages: Vec<String> = refusals.into_iter().map(|r| r.unwrap_err().message).collect();
    assert_eq!(
        messages,
        [
            "Engine DJ doesn't take history lists: include_history exports them as playlists",
            "not found: history list \"HISTORY 404\"",
            "include_history is for a drive's lists, not a set or a mix",
        ]
    );
    assert!(!output.exists());
    assert!(session.calls("sets.get").is_empty());
    session.stop();
}
