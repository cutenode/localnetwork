//! Converting between export.pdb and OneLibrary, and spotting when the two disagree.

mod imperfect_drives;

use crate::common::{self, media::png};

use std::fs;

use locallibrary::api;
use locallibrary::artwork::{self, Artwork};
use locallibrary::convert::{self, ConvertOptions, Target};
use locallibrary::device::{Device, LibraryDrift, LineageOp};
use locallibrary::error::Reason;
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::{Error, anlz, backup, paths, pdb};

fn opts(to: Target, replace: bool) -> ConvertOptions {
    ConvertOptions { to, replace }
}

/// A twelve-track drive with artwork on two tracks and a history playlist.
fn drive(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let root = common::bigger_drive(dir, name);
    let mut d = Device::open_export(&root).unwrap();
    artwork::set_on_drive(&mut d, &[1, 2], &Artwork::from_bytes(&png(120, 90, 77)).unwrap(), &Ctx::silent()).unwrap();
    common::add_history(&root, "HISTORY 2026-10-04", &[3, 1, 4]);
    root
}

#[test]
fn builds_onelibrary_from_export_pdb() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path(), "A");
    let mut d = Device::open_export(&root).unwrap();
    assert!(!d.has_library_plus());

    let plan = convert::plan(&d, &opts(Target::OneLibrary, false), &Ctx::silent()).unwrap();
    assert_eq!((plan.tracks, plan.artwork, plan.history_playlists, plan.replaces), (12, 1, 1, false));
    let report = convert::apply(&plan, &mut d, &opts(Target::OneLibrary, false), &Ctx::silent()).unwrap();
    assert_eq!((report.tracks, report.artwork, report.history_playlists), (12, 1, 1));

    // Both databases now hold the same library, artwork included.
    let pdb_lib = d.library().unwrap();
    let plus = common::plus_library(&root);
    let drift = LibraryDrift::between(&pdb_lib, &plus);
    assert!(drift.is_empty(), "{drift:?}");
    assert!(d.info().drift.unwrap().is_empty());
    let art = plus.tracks.iter().find_map(|t| t.artwork_path.clone()).unwrap();
    assert!(art.contains("/b"), "{art}");
    assert!(d.path(&paths::key(&art)).is_file() && d.path(&artwork::medium_variant(&paths::key(&art))).is_file());
    let history = &plus.history[0];
    assert_eq!(history.name, "HISTORY 2026-10-04");
    let paths_of = |lib: &locallibrary::Library, ids: &[u32]| -> Vec<String> {
        ids.iter().map(|id| lib.track(*id).unwrap().file_path.clone()).collect()
    };
    assert_eq!(paths_of(&plus, &history.track_ids), paths_of(&pdb_lib, &pdb_lib.history[0].track_ids));
    assert_eq!(d.state().unwrap().head().unwrap().op, LineageOp::Convert);

    // SQLCipher accepts the new file.
    common::sqlcipher_reads(&root);

    // A second conversion needs --replace; undoing removes the new database.
    let refused = convert::plan(&d, &opts(Target::OneLibrary, false), &Ctx::silent());
    assert!(refused.is_err_and(|e| matches!(e.inner(), Error::Conflict(_)) && e.reason() == Some(Reason::Exists)));
    backup::restore(&mut d, report.backup.as_deref(), false).unwrap();
    assert!(!d.has_library_plus());
}

#[test]
fn builds_export_pdb_from_onelibrary() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path(), "A");
    common::onelibrary_only(&root);
    let mut d = Device::open_export(&root).unwrap();
    let plus = d.library().unwrap();

    let plan = convert::plan(&d, &opts(Target::DeviceLibrary, false), &Ctx::silent()).unwrap();
    assert_eq!((plan.tracks, plan.artwork, plan.history_playlists), (12, 1, 1));
    convert::apply(&plan, &mut d, &opts(Target::DeviceLibrary, false), &Ctx::silent()).unwrap();

    assert!(d.has_device_library());
    let pdb_lib = d.library().unwrap();
    let drift = LibraryDrift::between(&pdb_lib, &plus);
    assert!(drift.is_empty(), "{drift:?}");
    assert_eq!(pdb_lib.history.len(), 1);
    let art = pdb_lib.tracks.iter().find_map(|t| t.artwork_path.clone()).unwrap();
    assert!(art.contains("/a") && d.path(&paths::key(&art)).is_file(), "{art}");
    // An independent parser reads the new export.pdb, with none of the template's rows.
    let bytes = d.pdb_bytes().unwrap();
    assert_eq!(common::rekordcrate_rows(&bytes, rekordcrate::pdb::PageType::Tracks), 12);
    let db = pdb::parse_database(bytes, &d.path(paths::EXPORT_PDB)).unwrap();
    assert!(db.tracks.iter().all(|t| !t.title().is_empty()));
    assert_eq!(db.colors.len(), 8, "rekordbox's colours come with the template");
}

/// Regression: a OneLibrary built from export.pdb had no `cue` rows, so players that read only
/// OneLibrary showed converted tracks without cues. The rows are made from the analysis files.
#[test]
fn a_converted_onelibrary_has_the_analysis_files_cues() {
    use locallibrary::cue_edit::{self, CueEdit, CuePosition, HotCue, SetCue};
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    let mut d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let t = lib.tracks[0].clone();
    let set = |hot_cue, at, loop_beats: Option<f64>, name: &str, color: &str| {
        CueEdit::Set(SetCue {
            hot_cue,
            at,
            snap: false,
            loop_beats,
            name: Some(name.into()),
            color: Some(color.into()),
        })
    };
    for edit in [
        set(Some(HotCue(0)), CuePosition::Beat(9), None, "Intro", "aqua"),
        set(Some(HotCue(4)), CuePosition::Beat(33), Some(8.0), "Loop", "red"),
        set(None, CuePosition::Ms(20_000), None, "Memory", "green"),
    ] {
        let plan = cue_edit::plan(&d, &[t.id], &edit, &Ctx::silent()).unwrap();
        cue_edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    }
    let cues = anlz::read_cue_points(
        &fs::read(d.path(&paths::key(&t.analyze_path))).unwrap(),
        Some(&fs::read(d.path(&paths::key(&t.analyze_path.replace(".DAT", ".EXT")))).unwrap()),
    )
    .unwrap();
    assert_eq!(cues.len(), 3);

    let plan = convert::plan(&d, &opts(Target::OneLibrary, false), &Ctx::silent()).unwrap();
    let report = convert::apply(&plan, &mut d, &opts(Target::OneLibrary, false), &Ctx::silent()).unwrap();
    assert_eq!(report.cues, 3);
    let one = OneLibrary::open(&d).unwrap();
    let content = one.library().unwrap().tracks.iter().find(|p| p.file_path == t.file_path).unwrap().id;
    let mut rows: Vec<_> = one
        .cue_rows(content)
        .unwrap()
        .into_iter()
        .map(|r| (r.hot_cue, r.in_usec / 1000, r.out_usec.map(|o| o / 1000), r.comment, r.color, r.loop_beats))
        .collect();
    let mut want: Vec<_> = cues
        .iter()
        .map(|c| {
            let color = if c.hot_cue.is_some() { c.color_code } else { c.color_id };
            (c.hot_cue, i64::from(c.start_ms), c.end_ms.map(i64::from), c.comment.clone(), color, c.loop_beats)
        })
        .collect();
    rows.sort();
    want.sort();
    assert_eq!(rows, want);
    // As `cues` lists them: OneLibrary has a row for each.
    let listed = cue_edit::list(&d, &[t.id]).unwrap().remove(0);
    assert_eq!(listed.onelibrary_cues, Some(3));

    // Analysis files with no cue lists (synthesised here) give no rows, and no warning.
    let mut d = Device::open_export(common::bigger_drive(dir.path(), "B")).unwrap();
    let warnings = std::sync::Mutex::new(Vec::new());
    let collect = |e: &locallibrary::progress::Event| {
        if let locallibrary::progress::Event::Warning { message } = e {
            warnings.lock().unwrap().push(message.clone());
        }
    };
    let ctx = Ctx::new(&collect, Default::default());
    let plan = convert::plan(&d, &opts(Target::OneLibrary, false), &ctx).unwrap();
    let report = convert::apply(&plan, &mut d, &opts(Target::OneLibrary, false), &ctx).unwrap();
    assert_eq!(report.cues, 0);
    assert!(warnings.lock().unwrap().iter().all(|w| !w.contains("can't be read")), "{warnings:?}");
}

/// Regression: convert made its backup, marked the drive as interrupted and copied artwork before
/// building the new database, so one that failed (here a comment too long for an export.pdb row)
/// said "drive left unchanged" when it wasn't. Now it is.
/// A track export.pdb can't hold is refused when planning, by name. One that only gets that way
/// after the plan fails when applied, leaving the drive as it was.
#[test]
fn a_convert_that_fails_leaves_the_drive_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path(), "A");
    common::onelibrary_only(&root);
    let mut d = Device::open_export(&root).unwrap();
    let plan = convert::plan(&d, &opts(Target::DeviceLibrary, false), &Ctx::silent()).unwrap();
    assert_eq!(plan.artwork, 1);
    let one = OneLibrary::open(&d).unwrap();
    let long = "x".repeat(5000);
    one.connection().execute("UPDATE content SET djComment = ?1 WHERE content_id = 12", [&long]).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let before = common::Before::take(&root);
    convert::apply(&plan, &mut d, &opts(Target::DeviceLibrary, false), &Ctx::silent()).unwrap_err();
    before.untouched(&root).unwrap();
    let err = convert::plan(&d, &opts(Target::DeviceLibrary, false), &Ctx::silent()).unwrap_err().to_string();
    let title = d.library().unwrap().track(12).unwrap().display_name();
    assert!(err.contains(&format!("the track {title} can't be stored in export.pdb")), "{err}");
}

/// OneLibrary takes a name of any length; export.pdb doesn't. The plan used to accept one, and
/// applying it failed without saying which playlist.
#[test]
fn a_name_too_long_for_export_pdb_is_refused_when_planning() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path(), "A");
    common::onelibrary_only(&root);
    let d = Device::open_export(&root).unwrap();
    let mut one = OneLibrary::open(&d).unwrap();
    let folder = one.create_playlist("Sets", 0, true).unwrap();
    one.create_playlist(&"é".repeat(2500), folder, false).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let err = convert::plan(&d, &opts(Target::DeviceLibrary, false), &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "invalid");
    assert!(err.to_string().starts_with(&format!("the playlist Sets/{} can't be stored", "é".repeat(2500))), "{err}");
}

#[test]
fn replacing_onelibrary_fixes_drift_and_keeps_its_rows() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    let mut d = Device::open_export(&root).unwrap();
    assert!(d.info().drift.unwrap().is_empty());

    // OneLibrary drifts: a track goes missing, and every row carries a value export.pdb can't hold.
    let mut one = OneLibrary::open(&d).unwrap();
    one.connection().execute("UPDATE content SET masterDbId = 777", []).unwrap();
    let lost = one.library().unwrap().tracks[0].clone();
    one.remove_tracks(&[lost.id]).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let drift = d.info().drift.unwrap();
    assert_eq!(drift.only_in_device_library.len(), 1);
    assert!(!drift.playlists.is_empty(), "a playlist lost its entry");

    let plan = convert::plan(&d, &opts(Target::OneLibrary, true), &Ctx::silent()).unwrap();
    assert!(plan.replaces);
    convert::apply(&plan, &mut d, &opts(Target::OneLibrary, true), &Ctx::silent()).unwrap();
    assert!(d.info().drift.unwrap().is_empty());
    let one = OneLibrary::open(&d).unwrap();
    let kept: i64 =
        one.connection().query_row("SELECT count(*) FROM content WHERE masterDbId = 777", [], |r| r.get(0)).unwrap();
    assert_eq!(kept, 11, "rows OneLibrary had are kept whole; the missing one is rebuilt");
}

/// OneLibrary's database is built from export.pdb; building it again replaces it only when asked,
/// and the plan says it does.
#[test]
fn onelibrary_is_built_from_export_pdb_and_replaced_only_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let main = common::demo_drive(dir.path(), "Main");
    let convert = |replace| api::ConvertRequest {
        drive: common::path(&main),
        options: ConvertOptions { to: Target::OneLibrary, replace },
    };

    let plan = api::convert_plan(&convert(false), &Ctx::silent()).unwrap();
    assert_eq!(plan.summary, ["build exportLibrary.db from export.pdb on Main (2 tracks, 0 playlists)"]);
    assert!(!plan.destructive);
    assert_eq!(api::convert_apply(&convert(false), &Ctx::silent()).unwrap().tracks, 2);
    assert!(Device::open(&main).unwrap().has_library_plus());

    assert!(api::convert_plan(&convert(false), &Ctx::silent()).is_err(), "there's one already");
    let again = api::convert_plan(&convert(true), &Ctx::silent()).unwrap();
    assert!(again.destructive);
    assert!(again.summary[0].ends_with(", replacing the exportLibrary.db it has"), "{:?}", again.summary);
}
