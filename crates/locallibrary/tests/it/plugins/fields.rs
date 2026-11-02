//! Analyzers' fields (`echo.bytes`) beyond track listings: smart playlist rules, edit selectors
//! and sorting a playlist on the drive take those the plugins installed declare, as they're
//! declared, and read what the analyzers worked out.

use locallibrary::api::{DriveRef, EditRequest, SetSmartPlaylist};
use locallibrary::edit::{Edit, EditOptions};
use locallibrary::query::TrackSelector;

use super::*;

/// The fixture's two tracks with audio files of 2000 and 500 bytes, analyzed for their size when
/// `analyzed`.
fn sized(f: &Hosted, analyzed: bool) -> Vec<locallibrary::Track> {
    let tracks = common::library(&f.drive).tracks;
    assert_eq!(tracks.len(), 2);
    for (t, size) in tracks.iter().zip([2000, 500]) {
        std::fs::write(locallibrary::paths::to_fs(&f.drive, &locallibrary::paths::key(&t.file_path)), "x".repeat(size))
            .unwrap();
    }
    if analyzed {
        f.plugins.analyze("echo", "size", "USB", None, &f.store, &Ctx::silent()).unwrap();
    }
    tracks
}

fn rule(filter: &str, sort: Option<&str>) -> SetSmartPlaylist {
    let sort = sort.map(String::from);
    SetSmartPlaylist { path: "Smart/Big".into(), filter: filter.into(), sort, limit: None, drives: Vec::new() }
}

/// The playlist's tracks on the drive, by id.
fn entries(f: &Hosted, path: &str) -> Vec<u32> {
    common::library(&f.drive).find_playlist(path).unwrap().track_ids.clone()
}

/// A rule naming a plugin's field is kept when an analyzer installed declares it, and the value
/// it's given fits; applied, it reads what the analyzer worked out, sorted by it too.
#[test]
fn a_smart_rule_takes_a_plugins_field() {
    let f = Hosted::new(Fixture::echo());
    let tracks = sized(&f, true);
    api::smart_set(&f.store, &rule("echo.bytes:>100", Some("echo.bytes"))).unwrap();
    let e = api::smart_set(&f.store, &rule("echo.byte:>1000", None)).unwrap_err().to_string();
    assert!(e.contains("there's no field \"echo.byte\" (did you mean echo.bytes?)"), "{e}");
    let e = api::smart_set(&f.store, &rule("echo.bytes:big", None)).unwrap_err().to_string();
    assert!(e.contains("it's a number"), "{e}");
    let e = api::smart_set(&f.store, &rule("bpm:>1", Some("-echo.byte"))).unwrap_err().to_string();
    assert!(e.contains("did you mean echo.bytes?"), "{e}");

    let plan = api::smart_apply_plan(&f.store, &DriveRef { drive: "USB".into() }, &Ctx::silent()).unwrap();
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    plan.apply(Some(&f.store), &Ctx::silent()).unwrap();
    assert_eq!(entries(&f, "Smart/Big"), [tracks[1].id, tracks[0].id]);
}

/// Without the analyzer's values, a rule naming its field says so: when no plugin installed
/// declares it, applying it is refused; when nothing was analyzed yet, the plan warns.
#[test]
fn a_smart_rule_on_a_field_says_when_it_cant_be_read() {
    let f = Hosted::new(Fixture::echo());
    sized(&f, false);
    api::smart_set(&f.store, &rule("echo.bytes:>1000", None)).unwrap();
    let plan = api::smart_apply_plan(&f.store, &DriveRef { drive: "USB".into() }, &Ctx::silent()).unwrap();
    assert_eq!(plan.warnings, ["no track here has a value for echo.bytes: analyze the tracks first"]);

    // A rule kept while a plugin was installed that isn't any more.
    let mut rules = f.store.smart_playlists().unwrap();
    rules.playlists[0].filter = "other.bytes:>1000".into();
    f.store.save_smart_playlists(&rules).unwrap();
    let e = api::smart_apply_plan(&f.store, &DriveRef { drive: "USB".into() }, &Ctx::silent()).unwrap_err();
    assert!(e.to_string().contains("there's no field \"other.bytes\" (no plugin installed has it)"), "{e}");
    let e = api::smart_set(&f.store, &rule("other.bytes:>1000", None)).unwrap_err();
    assert!(e.to_string().contains("(no plugin installed has it)"), "{e}");
}

/// An edit's selector and a playlist sorted on the drive take a plugin's field.
#[test]
fn edits_select_and_sort_by_a_plugins_field() {
    let f = Hosted::new(Fixture::echo());
    let tracks = sized(&f, true);
    let edit = |edits: Vec<Edit>| {
        let req = EditRequest { drive: "USB".into(), edits, options: EditOptions::default() };
        api::edit_plan(&req, &Ctx::silent()).and_then(|p| p.apply(Some(&f.store), &Ctx::silent()))
    };
    let add = |filter: &str| Edit::AddTracks {
        playlist: "Sized".into(),
        tracks: TrackSelector { filter: Some(filter.into()), ..TrackSelector::default() },
        position: None,
        sort: None,
        allow_repeats: false,
    };
    edit(vec![Edit::CreatePlaylist { path: "Sized".into(), folder: false }, add("echo.bytes:<1000")]).unwrap();
    assert_eq!(entries(&f, "Sized"), [tracks[1].id]);
    edit(vec![add("echo.bytes:>1000")]).unwrap();
    assert_eq!(entries(&f, "Sized"), [tracks[1].id, tracks[0].id]);
    edit(vec![Edit::SortPlaylist { playlist: "Sized".into(), by: "-echo.bytes".into() }]).unwrap();
    assert_eq!(entries(&f, "Sized"), [tracks[0].id, tracks[1].id]);
    let e = edit(vec![Edit::SortPlaylist { playlist: "Sized".into(), by: "other.bytes".into() }]).unwrap_err();
    assert!(e.to_string().contains("(no plugin installed has it)"), "{e}");
}
