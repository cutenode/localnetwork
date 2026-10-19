//! Converting drives whose images, My Tags and databases aren't quite what rekordbox leaves:
//! images shared by several tracks or gone from the drive, an `exportExt.pdb` that's missing,
//! damaged or links tracks the new database won't list, analysis files that can't be read, names
//! `export.pdb` can't hold, a drive that changed since the plan, and artwork bigger than the drive.

use crate::common::{self, Warnings};

use indexmap::IndexMap;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::artwork::medium_variant;
use locallibrary::convert::{self, ConvertOptions, ConvertPlan, ConvertReport, Target};
use locallibrary::device::Device;
use locallibrary::edit::Edit;
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::{Library, paths, pdb, tags};

/// The twelve-track drive (or rekordbox's two-track demo export) `name` in `dir`, made once and
/// written out for each case, which is quicker than making it.
fn drive(dir: &Path, name: &str, demo: bool) -> PathBuf {
    static BIGGER: OnceLock<common::Template> = OnceLock::new();
    static DEMO: OnceLock<common::Template> = OnceLock::new();
    let make: fn(&Path, &str) -> PathBuf = if demo { common::demo_drive } else { common::bigger_drive };
    let cell = if demo { &DEMO } else { &BIGGER };
    let template = cell.get_or_init(|| common::Template::of(&make(tempfile::tempdir().unwrap().path(), "T")));
    template.write(&dir.join(name))
}

fn file(root: &Path, device_path: &str) -> std::path::PathBuf {
    paths::to_fs(root, &paths::key(device_path))
}

/// Plan and apply a conversion: the plan, the report and the warnings applying it gave.
fn convert(root: &Path, opts: &ConvertOptions) -> (ConvertPlan, ConvertReport, Vec<String>) {
    let mut d = Device::open_export(root).unwrap();
    let warnings = Warnings::default();
    let plan = convert::plan(&d, opts, &Ctx::silent()).unwrap();
    let report = convert::apply(&plan, &mut d, opts, &warnings.ctx()).unwrap();
    (plan, report, warnings.said())
}

/// The library the converted database holds.
fn made(root: &Path, to: Target) -> Library {
    match to {
        Target::OneLibrary => common::plus_library(root),
        Target::DeviceLibrary => pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap(),
    }
}

/// How the images get there.
#[derive(Debug, Clone, Copy)]
enum Way {
    /// A new OneLibrary from export.pdb.
    ToOneLibrary,
    /// OneLibrary rebuilt, keeping the images it had.
    RebuildingOneLibrary,
    /// A new export.pdb from OneLibrary.
    ToExportPdb,
}

/// Images come across once each: tracks sharing one share its copy under the new database's
/// name (with its smaller variant when the drive has one), a track whose image is gone from the
/// drive gets none (the plan says how many are missing), and a rebuilt OneLibrary keeps the images
/// it had.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn images_come_across_once_each(tc: TestCase) {
    for way in [Way::ToOneLibrary, Way::RebuildingOneLibrary, Way::ToExportPdb] {
        images_come_across_once_each_with(&tc, way);
    }
}

/// One case of [`images_come_across_once_each`].
#[hegel::test_helper]
fn images_come_across_once_each_with(tc: &TestCase, way: Way) {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path(), "A", false);
    let ids: Vec<u32> = common::library(&root).tracks.iter().map(|t| t.id).collect();
    // One picture on two or more tracks, perhaps another on one more.
    let mut shared = tc.draw(gs::vecs(gs::sampled_from(ids.clone())).min_size(2).max_size(4).unique(true));
    shared.sort();
    common::media::set_art(&root, &shared);
    let others: Vec<u32> = ids.iter().copied().filter(|id| !shared.contains(id)).collect();
    let lone = tc.draw(gs::sampled_from(others));
    let second = !matches!(way, Way::RebuildingOneLibrary) || tc.draw(gs::booleans());
    if second {
        common::media::set_art(&root, &[lone]);
    }
    match way {
        Way::ToOneLibrary => {}
        Way::RebuildingOneLibrary => common::add_onelibrary(&root),
        Way::ToExportPdb => common::onelibrary_only(&root),
    }
    let to = match way {
        Way::ToExportPdb => Target::DeviceLibrary,
        _ => Target::OneLibrary,
    };
    let source = match to {
        Target::OneLibrary => pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap(),
        Target::DeviceLibrary => common::plus_library(&root),
    };
    let art_of = |id: u32| source.track(id).and_then(|t| t.artwork_path.clone()).unwrap();
    // The shared picture's smaller variant gone, the other picture gone.
    let medium_gone = tc.draw(gs::booleans());
    if medium_gone {
        fs::remove_file(paths::to_fs(&root, &medium_variant(&paths::key(&art_of(shared[0]))))).unwrap();
    }
    // Always gone for a new database (its builder then has no image to copy), at times for a rebuild.
    let gone = second && (!matches!(way, Way::RebuildingOneLibrary) || tc.draw(gs::booleans()));
    if gone {
        fs::remove_file(file(&root, &art_of(lone))).unwrap();
    }
    let old = (matches!(way, Way::RebuildingOneLibrary)).then(|| common::plus_library(&root));
    let there: IndexMap<String, String> = source
        .tracks
        .iter()
        .filter_map(|t| t.artwork_path.as_ref().map(|a| (paths::file_key(&t.file_path), a.clone())))
        .filter(|(_, a)| file(&root, a).is_file())
        .collect();

    let opts = ConvertOptions { to, replace: matches!(way, Way::RebuildingOneLibrary) };
    let (plan, report, _) = convert(&root, &opts);
    let distinct: std::collections::BTreeSet<String> = there.values().map(|a| paths::file_key(a)).collect();
    assert_eq!(plan.artwork, distinct.len());
    let warned = plan.warnings.iter().any(|w| w.starts_with("1 artwork image(s) the tracks point at are missing"));
    assert_eq!(warned, gone, "{:?}", plan.warnings);

    let after = made(&root, to);
    let by_file: IndexMap<String, Option<String>> =
        after.tracks.iter().map(|t| (paths::file_key(&t.file_path), t.artwork_path.clone())).collect();
    let mut copies: IndexMap<String, String> = IndexMap::new();
    for t in &source.tracks {
        let got = by_file[&paths::file_key(&t.file_path)].clone();
        let kept = old.as_ref().and_then(|o| {
            let own = o
                .tracks
                .iter()
                .find(|x| paths::file_key(&x.file_path) == paths::file_key(&t.file_path))?
                .artwork_path
                .clone()?;
            file(&root, &own).is_file().then_some(own)
        });
        match (there.get(&paths::file_key(&t.file_path)), kept) {
            // A rebuilt OneLibrary keeps the image it had.
            (_, Some(own)) => assert_eq!(got.as_deref(), Some(own.as_str()), "{}", t.file_path),
            (None, None) => assert_eq!(got, None, "{} had no image on the drive", t.file_path),
            (Some(src), None) => {
                let got = got.unwrap_or_else(|| panic!("{} lost its image", t.file_path));
                assert!(file(&root, &got).is_file(), "{got}");
                assert_ne!(paths::file_key(&got), paths::file_key(src), "under the new database's name");
                // One copy per picture, its smaller variant beside it when the drive had one.
                let prev = copies.insert(paths::file_key(src), got.clone());
                assert!(prev.as_ref().is_none_or(|p| *p == got), "{src} copied twice: {prev:?} and {got}");
                let medium = |p: &str| paths::to_fs(&root, &medium_variant(&paths::key(p))).is_file();
                assert_eq!(medium(&got), medium(src), "{got}'s smaller variant");
            }
        }
    }
    if old.is_none() {
        assert_eq!(report.artwork, copies.len());
    }
}

/// Give OneLibrary's tracks `ids` My Tag "Mood/Dark".
fn tag_in_onelibrary(root: &Path, ids: &[u32]) {
    let one = OneLibrary::open_root(root).unwrap();
    one.connection()
        .execute_batch("INSERT INTO myTag VALUES (901, 0, 'Mood', 1, 0), (902, 0, 'Dark', 0, 901);")
        .unwrap();
    for id in ids {
        one.connection().execute("INSERT INTO myTag_content VALUES (902, ?1)", [id]).unwrap();
    }
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
}

/// What the drive's `exportExt.pdb` is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Ext {
    Missing,
    Damaged,
    Kept,
}

/// My Tags go where an `exportExt.pdb` lets them, whichever way a drive is converted: a damaged
/// one is left exactly as it is (the plan and the conversion say its tags aren't converted), a
/// missing one isn't made (the plan says tags have nowhere to go), and links in one to tracks a
/// rebuilt `export.pdb` won't list go, as the plan says.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
fn my_tags_go_where_export_ext_lets_them(tc: TestCase) {
    for ext in [Ext::Missing, Ext::Damaged, Ext::Kept] {
        for (to, replace, plus) in [
            (Target::OneLibrary, false, false),
            (Target::DeviceLibrary, false, false),
            (Target::DeviceLibrary, false, true),
            (Target::DeviceLibrary, true, true),
        ] {
            my_tags_go_where_export_ext_lets_them_with(&tc, ext, ConvertOptions { to, replace }, plus);
        }
    }
}

/// One case of [`my_tags_go_where_export_ext_lets_them`]: `plus` when export.pdb stays beside
/// OneLibrary.
#[hegel::test_helper]
fn my_tags_go_where_export_ext_lets_them_with(tc: &TestCase, ext: Ext, opts: ConvertOptions, plus: bool) {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path(), "A", true);
    let mut d = Device::open_export(&root).unwrap();
    let pdb_files: IndexMap<u32, String> =
        d.library().unwrap().tracks.iter().map(|t| (t.id, t.file_path.clone())).collect();
    let ids: Vec<u32> = pdb_files.keys().copied().collect();
    let mut removed = None;
    // OneLibrary's tracks with My Tags, when converting from it.
    let mut plus_tagged = Vec::new();
    let tagged = tc.draw(gs::vecs(gs::sampled_from(ids.clone())).min_size(1).max_size(2).unique(true));
    let plan = tags::plan(&d, &tags::Edit::Add { tag: "Genre/Deep".into(), tracks: tagged.clone() }).unwrap();
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let ext_bytes = fs::read(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    if opts.to == Target::DeviceLibrary {
        match plus {
            true => common::add_onelibrary(&root),
            false => common::onelibrary_only(&root),
        }
        let one_ids: Vec<u32> = common::plus_library(&root).tracks.iter().map(|t| t.id).collect();
        plus_tagged = tc.draw(gs::vecs(gs::sampled_from(one_ids.clone())).min_size(1).max_size(2).unique(true));
        tag_in_onelibrary(&root, &plus_tagged);
        // A rebuild from a OneLibrary that lost a track drops that track's links.
        if opts.replace {
            let mut one = OneLibrary::open_root(&root).unwrap();
            let lose = tc.draw(gs::sampled_from(one_ids.clone()));
            plus_tagged.retain(|id| *id != lose);
            removed = Some(one.library().unwrap().track(lose).unwrap().file_path.clone());
            one.remove_tracks(&[lose]).unwrap();
            fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
        }
    }
    let ext_path = root.join(paths::EXPORT_EXT_PDB);
    match ext {
        Ext::Missing => drop(fs::remove_file(&ext_path)),
        Ext::Damaged => fs::write(&ext_path, b"not a database").unwrap(),
        Ext::Kept => fs::write(&ext_path, &ext_bytes).unwrap(),
    }

    let (plan, report, warnings) = convert(&root, &opts);
    let says = |what: &str| plan.warnings.iter().any(|w| w.contains(what));
    let damaged = match (opts.to, opts.replace, plus) {
        (Target::DeviceLibrary, false, false) | (Target::DeviceLibrary, true, _) => {
            "exportExt.pdb is left as it is, so its My Tags may name the wrong tracks"
        }
        _ => "its My Tags aren't converted",
    };
    assert_eq!(says(damaged), ext == Ext::Damaged, "{ext:?} {opts:?}: {:?}", plan.warnings);
    assert_eq!(warnings.iter().any(|w| w.ends_with(damaged)), ext == Ext::Damaged, "{ext:?} {opts:?}: {warnings:?}");
    match ext {
        Ext::Damaged => {
            assert_eq!(fs::read(&ext_path).unwrap(), b"not a database", "left as it is");
            assert_eq!((plan.tagged_tracks, report.tagged_tracks), (0, 0));
        }
        Ext::Missing => {
            assert!(!ext_path.exists(), "none is made");
            let nowhere = says("have My Tags, but the drive has no exportExt.pdb for them");
            assert_eq!(nowhere, !plus_tagged.is_empty(), "{:?}", plan.warnings);
            assert_eq!(report.tagged_tracks, 0);
        }
        Ext::Kept => {
            assert!(rekordbox_pdb::ExtDatabase::from_file(&ext_path).is_ok(), "it still reads");
            if opts.to == Target::OneLibrary {
                assert_eq!((plan.tagged_tracks, report.tagged_tracks), (tagged.len(), tagged.len()));
            }
            // Links to tracks the new export.pdb won't list go, and the plan says how many: all of
            // them when there's no export.pdb to follow them from, else those of the track lost.
            let lost =
                |id: &u32| removed.as_deref().is_some_and(|f| paths::file_key(&pdb_files[id]) == paths::file_key(f));
            let dropped = match (opts.to, plus) {
                (Target::DeviceLibrary, false) => tagged.len(),
                (Target::DeviceLibrary, true) if opts.replace => tagged.iter().filter(|id| lost(id)).count(),
                _ => 0,
            };
            let said = format!(
                "{dropped} My Tag link(s) in exportExt.pdb are for tracks the new export.pdb won't list; they go"
            );
            assert_eq!(plan.warnings.contains(&said), dropped > 0, "{said:?} in {:?}", plan.warnings);
        }
    }
    let after = made(&root, opts.to);
    let source = match opts.to {
        Target::OneLibrary => pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap(),
        Target::DeviceLibrary => common::plus_library(&root),
    };
    let files =
        |l: &Library| l.tracks.iter().map(|t| paths::file_key(&t.file_path)).collect::<std::collections::BTreeSet<_>>();
    assert!(files(&after).is_superset(&files(&source)), "every track comes across");
}

/// What a conversion can't do is said before anything changes, or stops it with the drive as it
/// was: a name or track too long for `export.pdb` (refused when planning, naming it), a drive
/// changed since the plan (stale), and artwork too big for the drive (the plan says it won't fit,
/// and applying it is refused). What it can do with less is said: analysis files that can't be
/// read give their tracks no cues, and rebuilding OneLibrary names the playlists, entries and hot
/// cue bank lists only it had.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn what_a_conversion_cant_do_is_said(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Ctx::silent();

    // Too long for export.pdb, by kind: a track's comment, a playlist's name, a history list's.
    let long = "x".repeat(tc.draw(gs::integers::<usize>().min_value(5_000).max_value(9_000)));
    for (n, kind) in ["track", "playlist", "history list"].into_iter().enumerate() {
        let root = drive(dir.path(), &format!("Long {n}"), false);
        common::onelibrary_only(&root);
        let mut one = OneLibrary::open_root(&root).unwrap();
        let id = tc.draw(gs::sampled_from(one.library().unwrap().tracks.iter().map(|t| t.id).collect::<Vec<_>>()));
        match kind {
            "track" => {
                one.connection()
                    .execute("UPDATE content SET djComment = ?1 WHERE content_id = ?2", (&long, id))
                    .unwrap();
            }
            "playlist" => {
                one.create_playlist(&long, 0, false).unwrap();
            }
            _ => {
                one.create_history(&long, 0, false).unwrap();
            }
        }
        fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
        let d = Device::open_export(&root).unwrap();
        let err = convert::plan(&d, &ConvertOptions { to: Target::DeviceLibrary, replace: false }, &ctx).unwrap_err();
        assert!(err.to_string().contains(&format!("the {kind} ")), "{kind}: {err}");
        assert!(err.to_string().contains("can't be stored in export.pdb"), "{err}");
    }

    // Changed since the plan.
    let root = drive(dir.path(), "Changed", false);
    let to = ConvertOptions { to: Target::OneLibrary, replace: false };
    let plan = convert::plan(&Device::open_export(&root).unwrap(), &to, &ctx).unwrap();
    for n in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(2)) {
        common::export_new_track(&root, "Later", &format!("Track {n}"));
    }
    let before = common::Before::take(&root);
    let err = convert::apply(&plan, &mut Device::open_export(&root).unwrap(), &to, &ctx).unwrap_err();
    assert_eq!(err.code(), "stale_plan", "{err}");
    before.untouched(&root).unwrap();

    // Sparse only on Unix: NTFS would have to find the room for it.
    #[cfg(unix)]
    {
        // Artwork bigger than the drive (a sparse file: it takes no room itself).
        let root = drive(dir.path(), "Big", false);
        common::media::set_art(&root, &[1]);
        let art =
            pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().track(1).unwrap().artwork_path.clone().unwrap();
        let free = locallibrary::fsutil::available_space(&root).unwrap();
        fs::File::options().write(true).open(file(&root, &art)).unwrap().set_len(2 * free + (1 << 30)).unwrap();
        let mut d = Device::open_export(&root).unwrap();
        let plan = convert::plan(&d, &to, &ctx).unwrap();
        assert!(!plan.fits, "{plan:?}");
        let full = format!("needs about {} bytes but only {} are free", plan.needed_bytes, plan.free_space.unwrap());
        assert!(plan.warnings.contains(&full), "{:?}", plan.warnings);
        let err = convert::apply(&plan, &mut d, &to, &ctx).unwrap_err();
        assert!(matches!(err, locallibrary::Error::NoSpace { needed, .. } if needed == plan.needed_bytes), "{err}");
        assert!(!d.has_library_plus());
    }

    // Analysis files that can't be read (rekordbox's demo export's are real ones).
    let root = drive(dir.path(), "Garbled", true);
    let lib = common::library(&root);
    let garbled: Vec<u32> = lib.tracks.iter().map(|t| t.id).filter(|_| tc.draw(gs::booleans())).collect();
    for id in &garbled {
        let dat = file(&root, &lib.track(*id).unwrap().analyze_path);
        for f in [dat.clone(), dat.with_extension("EXT")] {
            fs::write(f, b"garbage").unwrap();
        }
    }
    let (_, report, warnings) = convert(&root, &to);
    let said = warnings.iter().find(|w| w.contains("have analysis files that can't be read"));
    match garbled.first() {
        None => assert!(said.is_none(), "{warnings:?}"),
        Some(first) => {
            let w = said.unwrap_or_else(|| panic!("{warnings:?}"));
            assert!(w.starts_with(&format!("{} track(s)", garbled.len())), "{w}");
            assert!(w.contains(&lib.track(*first).unwrap().display_name()), "{w}");
        }
    }
    assert_eq!(report.tracks, lib.tracks.len());

    // Rebuilding OneLibrary names what only it had.
    let root = drive(dir.path(), "Rebuilt", false);
    // A playlist both have, with an entry only OneLibrary has.
    let files: Vec<String> = common::library(&root).tracks.iter().map(|t| t.file_path.clone()).collect();
    let k = tc.draw(gs::integers::<usize>().min_value(1).max_value(files.len() - 1));
    common::apply(&root, &[Edit::SetEntries { playlist: "Both".into(), paths: files[..k].to_vec(), create: true }]);
    common::add_onelibrary(&root);
    let mut one = OneLibrary::open_root(&root).unwrap();
    let plus = one.library().unwrap();
    let both = plus.find_playlist("Both").unwrap();
    let extra = plus.tracks.iter().find(|t| !both.track_ids.contains(&t.id)).unwrap().id;
    one.add_to_playlist(both.id, extra).unwrap();
    let both = "Both";
    let names: Vec<String> =
        (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(7))).map(|n| format!("Booth {n}")).collect();
    for name in &names {
        one.create_playlist(name, 0, false).unwrap();
    }
    let lists: Vec<String> =
        (0..tc.draw(gs::integers::<usize>().max_value(2))).map(|n| format!("HISTORY 9{n}")).collect();
    for list in &lists {
        one.create_history(list, 0, false).unwrap();
    }
    let banks = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    for n in 0..banks {
        one.create_hot_cue_bank_list(&format!("Drops {n}"), 0, false).unwrap();
    }
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let rebuild = ConvertOptions { to: Target::OneLibrary, replace: true };
    let plan = convert::plan(&Device::open_export(&root).unwrap(), &rebuild, &ctx).unwrap();
    let dropped =
        plan.warnings.iter().find(|w| w.starts_with("rebuilding drops what only exportLibrary.db has")).unwrap();
    for name in names.iter().take(5) {
        assert!(dropped.contains(name.as_str()), "{name}: {dropped}");
    }
    assert_eq!(dropped.contains(&format!(", +{} more", names.len().saturating_sub(5))), names.len() > 5, "{dropped}");
    assert!(dropped.ends_with(&format!("; entries only it has in {both}")), "{both}: {dropped}");
    assert_eq!(dropped.contains(&format!("the history list(s) {}", lists.join(", "))), !lists.is_empty(), "{dropped}");
    assert!(
        plan.warnings.contains(&format!("its {banks} hot cue bank list(s) go too (export.pdb has none)")),
        "{:?}",
        plan.warnings
    );
}
