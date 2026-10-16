//! Exports for other DJ software (Traktor, Serato, Engine DJ) from drawn drives: the plan names
//! every file the export then writes, at its size, holding the drive's whole library (or the
//! playlists chosen); nothing is written to the drive, files already there are only replaced
//! when asked, and what a format can't take is refused up front.

use crate::{common, drawn};

use std::fs;

use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
use locallibrary::convert;
use locallibrary::device::Device;
use locallibrary::export::PathStyle;
use locallibrary::interop::{self, InteropOptions, Software};
use locallibrary::progress::Ctx;
use locallibrary::{Library, paths};

/// The tracks an export of `only` holds: those of the chosen playlists and everything below them.
fn chosen_tracks(lib: &Library, only: &[String]) -> std::collections::BTreeSet<u32> {
    if only.is_empty() {
        return lib.tracks.iter().map(|t| t.id).collect();
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut queue: Vec<u32> = only.iter().map(|q| lib.find_playlist(q).unwrap().id).collect();
    while let Some(id) = queue.pop() {
        queue.extend(lib.children(Some(id)).iter().map(|c| c.id));
        ids.extend(lib.playlist(id).unwrap().track_ids.iter().copied());
    }
    ids
}

#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn an_export_writes_what_it_planned(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (root, lib) = drawn::drive(&tc, dir.path(), "A");
    if tc.draw(gs::weighted_booleans(0.3)) {
        common::add_onelibrary(&root);
    }
    let gone: Vec<u32> =
        drawn::some(&tc, &lib.tracks.iter().map(|t| t.id).collect::<Vec<_>>()).into_iter().take(2).collect();
    for id in &gone {
        fs::remove_file(paths::to_fs(&root, &paths::key(&lib.track(*id).unwrap().file_path))).unwrap();
    }
    let d = Device::open_export(&root).unwrap();
    let combined = convert::combined_library(&d).unwrap();
    let software =
        tc.draw(gs::sampled_from(vec![Software::Traktor, Software::Serato, Software::Engine]).print_as_debug());
    let names: Vec<String> = combined.playlists.iter().map(|p| combined.playlist_path(p.id)).collect();
    let only: Vec<String> =
        drawn::some(&tc, &names).into_iter().filter(|q| combined.find_playlist(q).is_some()).take(2).collect();
    let opts = InteropOptions {
        only: only.clone(),
        skip_analysis: tc.draw(gs::booleans()),
        write_tags: software == Software::Serato && tc.draw(gs::booleans()) && gone.is_empty(),
        paths: if software == Software::Engine && tc.draw(gs::booleans()) {
            PathStyle::Drive
        } else {
            PathStyle::Relative
        },
        ..Default::default()
    };
    // Canonical, as the formats name the files they write.
    let out = locallibrary::device::dunce_canonicalize(dir.path()).join("Export");

    // On the drive itself, never.
    assert!(interop::plan(&d, software, &root.join("Export"), &opts, &Ctx::silent()).is_err());
    let plan = interop::plan(&d, software, &out, &opts, &Ctx::silent()).unwrap();
    let chosen = chosen_tracks(&combined, &only);
    assert_eq!(plan.tracks, chosen.len());
    assert!(plan.files.iter().all(|f| !f.exists && f.path.starts_with(&out)));
    let missing = gone.iter().filter(|id| chosen.contains(id)).count();
    let warned = plan.warnings.iter().find(|w| w.contains("have no audio"));
    assert_eq!(
        warned.map(|w| w.starts_with(&format!("{missing} track(s)"))),
        (missing > 0).then_some(true),
        "{:?}",
        plan.warnings
    );
    if opts.skip_analysis {
        assert_eq!((plan.beat_grids, plan.cues), (0, 0));
    }
    let library_before = common::library_files(&root);
    // The placeholder audio isn't MPEG, so Serato's tag can't go into it: it's copied as it is.
    assert!(plan.files.iter().all(|f| !f.what.ends_with("with Serato's cue tag")), "{:#?}", plan.files);
    let done = interop::apply(&d, software, &out, &opts, &Ctx::silent()).unwrap();
    for f in &done.files {
        assert_eq!(fs::metadata(&f.path).unwrap().len(), f.bytes, "{}", f.path.display());
    }
    assert_eq!(done.files.len(), plan.files.len());
    assert_eq!(common::library_files(&root), library_before, "the drive isn't written to");

    // Again: refused while the files are there, unless replacing them.
    let again = interop::plan(&d, software, &out, &opts, &Ctx::silent()).unwrap();
    assert!(again.files.iter().all(|f| f.exists));
    assert!(interop::apply(&d, software, &out, &opts, &Ctx::silent()).is_err());
    interop::apply(&d, software, &out, &InteropOptions { replace: true, ..opts.clone() }, &Ctx::silent()).unwrap();

    // What the format can't take is refused.
    if software != Software::Serato {
        assert!(
            interop::plan(
                &d,
                software,
                &out,
                &InteropOptions { write_tags: true, ..Default::default() },
                &Ctx::silent()
            )
            .is_err()
        );
    }
    if software != Software::Engine {
        let drive_paths = InteropOptions { paths: PathStyle::Drive, ..Default::default() };
        assert!(interop::plan(&d, software, &out, &drive_paths, &Ctx::silent()).is_err());
    }
}
