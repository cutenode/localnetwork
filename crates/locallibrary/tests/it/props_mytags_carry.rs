//! My Tags carried between databases: drawn tags given to drawn tracks of rekordbox's demo export
//! (exportExt.pdb, perhaps OneLibrary too) are read back as given, both databases agree, and a
//! OneLibrary built or added to from export.pdb, or merged into another drive, brings every tag
//! with its tracks.

use crate::common;

use indexmap::IndexMap;
use std::collections::BTreeSet;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::convert::{self, ConvertOptions, Target};
use locallibrary::device::Device;
use locallibrary::merge::{self, MergeOptions};
use locallibrary::progress::Ctx;
use locallibrary::tags::{self, Edit};

const TAGS: &[&str] = &["Genre/Deep", "genre/deep", "Situation/Peak", "Components/Vocal", "Genre/Ünï Ñ"];

/// Every tag (folded) with the files of its tracks.
fn held(d: &Device) -> IndexMap<String, BTreeSet<String>> {
    let t = tags::read(d).unwrap();
    let mut out = IndexMap::new();
    for c in &t.categories {
        for tag in &c.tags {
            let files: BTreeSet<String> = tag.tracks.iter().map(|r| r.file_path.to_lowercase()).collect();
            if !files.is_empty() {
                out.insert(tag.path.to_lowercase(), files);
            }
        }
    }
    out
}

#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn tags_given_are_carried_wherever_the_tracks_go(tc: TestCase) {
    // With exportExt.pdb alone, and with OneLibrary beside it.
    for both in [false, true] {
        carry(&tc, both);
    }
}

#[hegel::test_helper]
fn carry(tc: &TestCase, both: bool) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Demo");
    if both {
        common::add_onelibrary(&root);
    }
    let mut d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let mut model: IndexMap<String, BTreeSet<String>> = IndexMap::new();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let tag = tc.draw(gs::sampled_from(TAGS));
        let tracks: Vec<u32> = lib.tracks.iter().map(|t| t.id).filter(|_| tc.draw(gs::booleans())).collect();
        let remove = tc.draw(gs::weighted_booleans(0.25));
        let edit = if remove {
            Edit::Remove { tag: tag.into(), tracks: tracks.clone() }
        } else {
            Edit::Add { tag: tag.into(), tracks: tracks.clone() }
        };
        let Ok(plan) = tags::plan(&d, &edit) else {
            // Nothing to take off, or no tracks given.
            assert!(tracks.is_empty() || remove);
            continue;
        };
        tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
        let files = tracks.iter().map(|id| lib.track(*id).unwrap().file_path.to_lowercase());
        let slot = model.entry(tag.to_lowercase()).or_default();
        if remove {
            for f in files {
                slot.remove(&f);
            }
        } else {
            slot.extend(files);
        }
    }
    model.retain(|_, v| !v.is_empty());
    assert_eq!(held(&d), model);
    // A tag given is in both databases. (The test helper's OneLibrary has none of
    // exportExt.pdb's own categories, so they may still differ elsewhere.)
    let read = tags::read(&d).unwrap();
    for c in &read.categories {
        for t in c.tags.iter().filter(|t| model.contains_key(&t.path.to_lowercase())) {
            assert_eq!(t.sources.len(), if both { 2 } else { 1 }, "{}", t.path);
        }
    }

    // OneLibrary built from export.pdb (or added to) gets them.
    let opts = ConvertOptions { to: Target::OneLibrary, replace: tc.draw(gs::booleans()) };
    let plan = convert::plan(&d, &opts, &Ctx::silent()).unwrap();
    convert::apply(&plan, &mut d, &opts, &Ctx::silent()).unwrap();
    let mut d = Device::open_export(&root).unwrap();
    assert_eq!(held(&d), model);
    // Built afresh, it holds exactly exportExt.pdb's.
    if opts.replace || !both {
        let read = tags::read(&d).unwrap();
        assert!(read.differences.is_empty(), "{:?}", read.differences);
    }
    // export.pdb rebuilt from OneLibrary: exportExt.pdb's links follow their tracks to new ids.
    let back = ConvertOptions { to: Target::DeviceLibrary, replace: true };
    let plan = convert::plan(&d, &back, &Ctx::silent()).unwrap();
    convert::apply(&plan, &mut d, &back, &Ctx::silent()).unwrap();
    let d = Device::open_export(&root).unwrap();
    assert_eq!(held(&d), model);

    // Merged into another drive with exportExt.pdb, the new tracks bring theirs.
    let target_root = dir.path().join("Target");
    common::copy_dir(&common::fixtures().join("rekordcrate/demo_tracks"), &target_root);
    let _ = std::fs::remove_dir_all(target_root.join("Contents"));
    let mut ed = rekordbox_pdb::PdbEditor::from_file(target_root.join(locallibrary::paths::EXPORT_PDB)).unwrap();
    let ids: Vec<u32> = ed.database().unwrap().tracks.iter().map(|t| t.id).collect();
    ed.remove_tracks(&ids).unwrap();
    ed.save(target_root.join(locallibrary::paths::EXPORT_PDB)).unwrap();
    let mut target = Device::open_export(&target_root).unwrap();
    let mplan = merge::plan(&d, &target, &MergeOptions::default(), &Ctx::silent()).unwrap();
    let report = merge::apply(&mplan, &d, &mut target, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(report.tagged_tracks, mplan.tagged_tracks);
    let target = Device::open_export(&target_root).unwrap();
    let carried: IndexMap<String, usize> = held(&target).into_iter().map(|(k, v)| (k, v.len())).collect();
    let want: IndexMap<String, usize> = model.iter().map(|(k, v)| (k.clone(), v.len())).collect();
    assert_eq!(carried, want);
}
