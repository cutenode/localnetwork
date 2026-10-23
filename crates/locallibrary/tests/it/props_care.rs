//! Looking after a drive, through the service layer: firmware updates put in its root, clutter the
//! OS left tidied away, and the name and colour players show. Each takes exactly the files it
//! names, refuses what the device wouldn't accept, and leaves the library alone.

use crate::common::{self, path};

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, DriveRef};
use locallibrary::backup;
use locallibrary::device::Device;
use locallibrary::error::Reason;
use locallibrary::firmware;
use locallibrary::player::{self, DbIdentity};
use locallibrary::progress::Ctx;
use locallibrary::tidy::{TidyKind, TidyOptions};

/// The twelve-track drive USB, the only drive lookups find on this thread, and no store.
fn usb() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(&common::isolate(dir.path(), None), "USB");
    (dir, root)
}

/// Update file names: every model's, in any case, and some that aren't.
const UPDATES: &[(&str, Option<&str>)] = &[
    ("CDJ3000Xv110.UPD", Some("CDJ-3000X")),
    ("cdj3kv321.upd", Some("CDJ-3000")),
    ("XDJAZv105.UPD", Some("XDJ-AZ")),
    ("OPUSQUADv130.UPD", Some("OPUS-QUAD")),
    ("DJM-A9_v140.UPD", Some("DJM-A9")),
    ("DJM-V10_v120.upd", Some("DJM-V10")),
    ("CDJ3000Xv110 (1).zip", None),
    ("firmware.UPD", None),
];

#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn firmware_goes_in_the_root_only_as_the_device_takes_it(tc: TestCase) {
    let (dir, root) = usb();
    let library = common::library_files(&root);
    let (name, model) = tc.draw(gs::sampled_from(UPDATES));
    let file = dir.path().join(name);
    let bytes = tc.draw(gs::binary().min_size(1).max_size(256));
    fs::write(&file, &bytes).unwrap();
    // Update files already on the drive, and an AppleDouble one that isn't.
    let others: Vec<&str> =
        UPDATES.iter().map(|(n, _)| *n).filter(|n| n.to_ascii_uppercase().ends_with(".UPD") && *n != name).collect();
    let present: Vec<&str> = others.into_iter().filter(|_| tc.draw(gs::weighted_booleans(0.3))).collect();
    for n in &present {
        fs::write(root.join(n), b"old update").unwrap();
    }
    fs::write(root.join("._DJM-A9_v100.UPD"), b"\0\x05\x16\x07").unwrap();
    let mut listed: Vec<String> = present.iter().map(|s| s.to_string()).collect();
    listed.sort();
    assert_eq!(firmware::existing_updates(&root), listed);

    let (replace, force) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let req = api::FirmwareRequest { drive: path(&root), file, replace, force };
    let plan = api::firmware_plan(&req).unwrap();
    assert_eq!(plan.model.map(|m| m.model), model);
    assert_eq!(firmware::model_for(name).map(|m| m.model), model);
    let single = plan.model.is_some_and(|m| m.single_file);
    let unknown = model.is_none();
    let crowded = single && !present.is_empty() && !replace;
    assert_eq!(plan.problems.iter().any(|p| p.contains("isn't named like")), unknown);
    assert_eq!(plan.problems.iter().any(|p| p.contains("only one update file")), crowded);
    assert_eq!(plan.warnings, plan.problems);
    assert_eq!(plan.existing.len(), if replace { present.len() } else { 0 });
    assert_eq!(plan.destructive, !plan.existing.is_empty());

    match api::firmware_apply(&req, &Ctx::silent()) {
        Err(e) => {
            assert!(!plan.problems.is_empty() && !force, "{e}");
            assert_eq!(e.reason(), Some(Reason::FirmwareProblems), "{e}");
            assert_eq!(firmware::existing_updates(&root), listed, "nothing written");
        }
        Ok(r) => {
            assert!(plan.problems.is_empty() || force);
            assert_eq!(fs::read(&r.written).unwrap(), bytes);
            assert_eq!(r.removed.len(), plan.existing.len());
            let mut want: BTreeSet<String> = if replace { BTreeSet::new() } else { listed.iter().cloned().collect() };
            want.insert(name.to_string());
            let now = firmware::existing_updates(&root);
            let want: Vec<String> = want.into_iter().filter(|n| n.to_ascii_uppercase().ends_with(".UPD")).collect();
            assert_eq!(now.iter().cloned().collect::<BTreeSet<_>>(), want.into_iter().collect());
            let drive = DriveRef { drive: path(&root) };
            let remove = api::firmware_remove_plan(&drive).unwrap();
            let size: u64 = now.iter().map(|n| fs::metadata(root.join(n)).unwrap().len()).sum();
            assert_eq!((&remove.files, remove.bytes, remove.noop), (&now, size, now.is_empty()));
            assert_eq!(api::firmware_remove(&drive, &Ctx::silent()).unwrap(), now);
            assert!(firmware::existing_updates(&root).is_empty());
        }
    }
    assert_eq!(common::library_files(&root), library, "the library is untouched");
}

/// Clutter the OS leaves, with what isn't clutter beside it.
const CLUTTER: &[(&str, Option<TidyKind>)] = &[
    (".DS_Store", Some(TidyKind::DsStore)),
    ("Contents/A/.DS_Store", Some(TidyKind::DsStore)),
    ("Contents/A/Thumbs.db", Some(TidyKind::Thumbnails)),
    ("Contents/A/._Track.mp3", Some(TidyKind::AppleDouble)),
    ("Contents/A/._not apple double.mp3", None),
    (".Spotlight-V100/Store-V2/store.db", Some(TidyKind::Spotlight)),
    (".fseventsd/0000", Some(TidyKind::FsEvents)),
    (".TemporaryItems/folders.501/x", Some(TidyKind::TemporaryItems)),
    (".Trashes/501/old.mp3", Some(TidyKind::Trash)),
    ("$RECYCLE.BIN/S-1/x.mp3", Some(TidyKind::Trash)),
    ("Contents/A/.locallibrary-tmp-1234", Some(TidyKind::TempFiles)),
    ("Contents/A/notes.txt", None),
    (".locallibrary/.DS_Store", None),
];

#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn tidying_removes_exactly_the_clutter(tc: TestCase) {
    let (_dir, root) = usb();
    let mut want: BTreeSet<String> = BTreeSet::new();
    let mut trash = 0u64;
    let empty_trash = tc.draw(gs::booleans());
    let chosen: Vec<(&str, Option<TidyKind>)> = tc.draw(gs::subsequences(CLUTTER));
    for (key, kind) in &chosen {
        let p = root.join(key);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let body: Vec<u8> = match kind {
            Some(TidyKind::AppleDouble) => [&[0u8, 5, 0x16, 7][..], &[0; 28]].concat(),
            _ => vec![1; 10 + key.len()],
        };
        fs::write(&p, &body).unwrap();
        if *kind == Some(TidyKind::Trash) {
            trash += body.len() as u64;
        }
        // Folders of clutter go whole: what's planned is the folder.
        let item = match key.split('/').next().unwrap() {
            top if [".Spotlight-V100", ".fseventsd", ".TemporaryItems", ".Trashes", "$RECYCLE.BIN"].contains(&top) => {
                top.to_string()
            }
            _ => key.to_string(),
        };
        if kind.is_some() && (*kind != Some(TidyKind::Trash) || empty_trash) {
            want.insert(item);
        }
    }
    let library = common::library_files(&root);
    let req = api::TidyRequest { drive: path(&root), options: TidyOptions { empty_trash } };
    let plan = api::tidy_plan(&req, &Ctx::silent()).unwrap();
    let planned: BTreeSet<String> = plan.items.iter().map(|i| i.path.clone()).collect();
    assert_eq!(planned, want);
    assert_eq!(plan.trash_bytes, trash);
    assert_eq!(plan.bytes, plan.items.iter().map(|i| i.bytes).sum::<u64>());
    let by_kind = plan.by_kind();
    assert_eq!(by_kind.iter().map(|k| k.1).sum::<usize>(), plan.items.iter().map(|i| i.files).sum::<usize>());
    assert!(by_kind.windows(2).all(|w| w[0].0 < w[1].0));
    assert_eq!((plan.noop, plan.destructive), (want.is_empty(), !want.is_empty()));

    let r = api::tidy_apply(&req, &Ctx::silent()).unwrap();
    assert!(r.failed.is_empty());
    assert_eq!((r.removed, r.bytes), (plan.items.len(), plan.bytes));
    for (key, kind) in &chosen {
        let gone = kind.is_some() && (*kind != Some(TidyKind::Trash) || empty_trash);
        assert_eq!(!root.join(key).exists(), gone, "{key}");
    }
    assert_eq!(common::library_files(&root), library);
    assert!(api::tidy_plan(&req, &Ctx::silent()).unwrap().noop);
}

/// The name and colour players show are refused unless they're valid (a name 1 to 32 printable
/// characters, a colour 0 to 8, at least one of them), and then on a drive without a OneLibrary
/// database, where players read them, saying so; otherwise they go into OneLibrary as given, and
/// undoing gives the old ones back.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn the_name_players_show_is_checked_set_and_undone(tc: TestCase) {
    let (_dir, root) = usb();
    let plus = tc.draw(gs::weighted_booleans(0.8));
    if plus {
        common::add_onelibrary(&root);
    }
    if tc.draw(gs::booleans()) {
        Device::open_export(&root).unwrap().adopt(Some("USB")).unwrap();
    }
    let before = player::read(&Device::open_export(&root).unwrap()).onelibrary;
    let name = tc.draw(gs::optional(gs::sampled_from(vec![
        "DJ Ünï",
        "  Club  ",
        "",
        "   ",
        "thirty-three characters is too long",
        "tab\there",
    ])));
    let colour = tc.draw(gs::optional(gs::integers::<i64>().min_value(-1).max_value(9)));
    let valid_name = name.is_none_or(|n| {
        let n = n.trim();
        !n.is_empty() && n.chars().count() <= player::MAX_NAME && !n.chars().any(char::is_control)
    });
    let valid = (name.is_some() || colour.is_some()) && valid_name && colour.is_none_or(|c| (0..=8).contains(&c));
    let req = api::SetPlayerIdentity { drive: path(&root), name: name.map(Into::into), colour };
    let refused = (valid && !plus).then_some(Reason::NoOneLibrary);
    assert_eq!(api::player_set_plan(&req).err().map(|e| e.reason()), (!(valid && plus)).then_some(refused));
    match api::player_set(&req) {
        Err(e) => assert_eq!((valid && plus, e.reason()), (false, refused), "{name:?}, {colour:?}: {e}"),
        Ok(now) => {
            assert!(valid && plus);
            let old = before.clone().unwrap_or_default();
            let want =
                DbIdentity { name: name.map(|n| n.trim().to_string()).or(old.name), colour: colour.or(old.colour) };
            assert_eq!(now.onelibrary, Some(want.clone()));
            assert_eq!(player::db_identity(&root), Some(want.clone()));
            let mut d = Device::open_export(&root).unwrap();
            if d.is_adopted() {
                assert_eq!(d.state().unwrap().player, Some(want.clone()));
            }
            let shown = player::db_identity(&root);
            backup::restore(&mut d, None, false).unwrap();
            player::after_undo(&mut d, shown).unwrap();
            assert_eq!(player::read(&d).onelibrary, before);
        }
    }
}
