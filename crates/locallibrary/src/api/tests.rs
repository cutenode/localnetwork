use super::*;

#[test]
fn a_format_never_erases_the_disk_holding_the_store_it_is_given() {
    // Nothing in the library's own tests names a store for drive lookups: the store given
    // is all the plan knows of.
    assert!(default_store_root().is_none());
    let dir = tempfile::tempdir().unwrap();
    // As the OS reports it (`E:\` on Windows, which `canonicalize` would make `\\?\E:\`).
    let mount = device::dunce_canonicalize(dir.path());
    let mut disk = crate::format::tests::sample_disk();
    disk.volumes[0].mount = Some(mount.clone());
    let req = FormatRequest { drive: "disk4".into(), options: FormatOptions::default() };
    let ctx = Ctx::silent();
    let refused = |store_root: &Path| match plan_format(store_root, disk.clone(), &req, &ctx) {
        Err(e) => e.to_string().contains("it holds"),
        Ok(_) => false,
    };

    let store = Store::open_or_init(mount.join("store")).unwrap();
    assert!(refused(store.root()), "a store on the disk");
    // One that doesn't open (or isn't there yet) is kept all the same.
    assert!(refused(&mount.join("not yet")));
    // A store elsewhere doesn't stop it.
    let elsewhere = tempfile::tempdir().unwrap();
    assert!(!refused(elsewhere.path()));
}

/// Regression: formatting never looked at the role, so an archive or retired drive was erased
/// (and `provision --format` erased one before its push was refused).
#[test]
fn a_format_keeps_archive_and_retired_drives_unless_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mount = device::dunce_canonicalize(&dir.path().join("Vault"));
    std::fs::create_dir(&mount).unwrap();
    let mut vault = Device::open(&mount).unwrap();
    vault.adopt(Some("Vault")).unwrap();
    sync::register_drive(&store, &vault, None).unwrap();
    let id = vault.id().unwrap();
    let mut disk = crate::format::tests::sample_disk();
    disk.volumes[0].mount = Some(mount);
    let req = FormatRequest { drive: "disk4".into(), options: FormatOptions::default() };
    let plan = || plan_format(store.root(), disk.clone(), &req, &Ctx::silent());

    crate::fleet::set_role(&store, id, Some(crate::fleet::Role::Archive)).unwrap();
    let err = plan().unwrap_err();
    assert_eq!(err.detail().reason, Some(Reason::Archive), "{err}");
    crate::fleet::set_role(&store, id, Some(crate::fleet::Role::Backup)).unwrap();
    crate::fleet::retire(&store, id, false).unwrap();
    assert_eq!(plan().unwrap_err().detail().reason, Some(Reason::Retired));

    // Allowed, the plan says what it erases. (On Linux a plan's steps come from lsblk, which
    // knows no such disk.)
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        op::allow_archive_writes(true);
        let allowed = plan();
        op::allow_archive_writes(false);
        let erased = &allowed.unwrap().erases[0];
        assert_eq!(
            (erased.drive_name.as_deref(), erased.role, erased.retired),
            (Some("Vault"), Some(crate::fleet::Role::Backup), true)
        );
    }
}

/// A format plan is refused for the disk holding the store it's given, wherever on the disk
/// the store is and whether or not it's there yet, and for an archive or retired drive the
/// store knows; a store elsewhere doesn't stop it.
#[hegel::test(test_cases = 20)]
fn a_format_is_refused_for_the_store_s_disk_and_kept_drives(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (dir, elsewhere) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::create_dir(dir.path().join("Disk")).unwrap();
    let mount = device::dunce_canonicalize(&dir.path().join("Disk"));
    let mut disk = crate::format::tests::sample_disk();
    disk.volumes[0].mount = Some(mount.clone());
    let on_disk = tc.draw(gs::booleans());
    let depth: Vec<&str> =
        tc.draw(gs::vecs(gs::sampled_from(&["store", "My Store", ".hidden"][..])).min_size(1).max_size(3));
    // As the temporary folder is spelled, through a symlink on macOS (/var for /private/var),
    // where the OS reports the mount by its real path.
    let base = if on_disk { dir.path().join("Disk") } else { elsewhere.path().to_path_buf() };
    let store_root = depth.iter().fold(base, |p, d| p.join(d));
    let made = tc.draw(gs::booleans());
    let store = made.then(|| Store::open_or_init(&store_root).unwrap());
    let role = match (&store, on_disk) {
        (Some(store), false) => {
            let mut d = Device::open(&mount).unwrap();
            d.adopt(Some("Vault")).unwrap();
            sync::register_drive(store, &d, None).unwrap();
            let id = d.id().unwrap();
            match tc.draw(gs::integers::<u8>().max_value(2)) {
                0 => None,
                1 => {
                    crate::fleet::set_role(store, id, Some(crate::fleet::Role::Archive)).unwrap();
                    Some(Reason::Archive)
                }
                _ => {
                    crate::fleet::retire(store, id, false).unwrap();
                    Some(Reason::Retired)
                }
            }
        }
        _ => None,
    };
    let req = FormatRequest { drive: "disk4".into(), options: FormatOptions::default() };
    match plan_format(&store_root, disk, &req, &Ctx::silent()) {
        Ok(plan) => {
            assert!(!on_disk && role.is_none());
            assert!(plan.destructive && plan.summary[0].starts_with("erase"), "{:?}", plan.summary);
            assert!(plan.plan.protected.contains(&fsutil::resolved(&store_root)));
        }
        Err(e) if on_disk => assert!(e.to_string().contains("it holds"), "{e}"),
        Err(e) if role.is_some() => assert_eq!(e.detail().reason, role, "{e}"),
        // Planning a format reads the disk with the system's tools, which may know none
        // such (lsblk on Linux): that isn't a refusal for the store.
        Err(e) => assert!(!e.to_string().contains("it holds"), "{e}"),
    }
}

/// Regression: `format <name>` took any mounted volume's label, so an unrelated disk carrying
/// the label of a drive that isn't plugged in was planned (and confirmed by retyping it).
#[test]
fn a_disk_to_erase_is_named_by_an_adopted_drive_never_a_label() {
    let dir = tempfile::tempdir().unwrap();
    crate::testing::volumes_on_this_thread(Some(vec![dir.path().to_path_buf()]));
    let path = |name: &str| device::dunce_canonicalize(&dir.path().join(name));
    // An external disk labelled Backup; the stick adopted as "Backup" isn't plugged in.
    std::fs::create_dir(dir.path().join("Backup")).unwrap();
    let err = resolve("Backup", false, false).unwrap_err();
    assert!(err.to_string().contains("is the label of the volume at"), "{err}");
    assert_eq!(resolve("Backup", true, false).unwrap().root(), path("Backup"), "other commands still take labels");

    // Now it is: the adopted drive is the one meant, where a label would be ambiguous.
    std::fs::create_dir(dir.path().join("STICK")).unwrap();
    Device::open(dir.path().join("STICK")).unwrap().adopt(Some("Backup")).unwrap();
    assert_eq!(resolve("backup", false, false).unwrap().root(), path("STICK"));
    assert!(resolve("Backup", true, false).unwrap_err().to_string().contains("matches 2 drives"));
    // An adopted drive's volume label isn't its name either.
    assert!(resolve("STICK", false, false).is_err());
    crate::testing::volumes_on_this_thread(None);
}

/// Regression: for erasing, a bare word no adopted drive has was taken as a folder in the
/// working directory, though it was the label of the volume meant.
#[test]
fn a_disk_to_erase_is_never_a_folder_here_named_like_a_label() {
    let dir = tempfile::tempdir().unwrap();
    crate::testing::volumes_on_this_thread(Some(vec![dir.path().to_path_buf()]));
    // Tests run in the crate's folder, which holds `src`; so does a volume labelled src.
    std::fs::create_dir(dir.path().join("src")).unwrap();
    let err = resolve("src", false, false).unwrap_err();
    assert!(err.to_string().contains("is the label of the volume at"), "{err}");
    assert_eq!(resolve("./src", false, false).unwrap().root(), device::dunce_canonicalize(Path::new("src")));
    crate::testing::volumes_on_this_thread(None);
}

/// For erasing, a word names a drive locallibrary adopted under that name (in any case), never a
/// volume's label: a label alone is refused, naming the volume, several adopted drives answering
/// to it are refused as ambiguous, and no drive at all isn't found. Other commands take a
/// volume's label as well as an adopted name.
#[hegel::test(test_cases = 40)]
fn a_disk_to_erase_is_named_by_adopted_name_alone(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    crate::testing::volumes_on_this_thread(Some(vec![dir.path().to_path_buf()]));
    let labels =
        tc.draw(gs::vecs(gs::sampled_from(vec!["Backup", "STICK", "Gig"])).min_size(1).max_size(3).unique(true));
    // (root, label, adopted name)
    let mut mounted: Vec<(PathBuf, &str, Option<&str>)> = Vec::new();
    for label in labels {
        std::fs::create_dir(dir.path().join(label)).unwrap();
        let name = tc.draw(gs::optional(gs::sampled_from(vec!["Backup", "Main"])));
        if let Some(n) = name {
            Device::open(dir.path().join(label)).unwrap().adopt(Some(n)).unwrap();
        }
        mounted.push((device::dunce_canonicalize(&dir.path().join(label)), label, name));
    }
    let word = tc.draw(gs::sampled_from(vec!["backup", "BACKUP", "Main", "stick", "Gig", "Nowhere"]));
    let lower = word.to_lowercase();
    let is = |s: &str| s.to_lowercase() == lower;
    let adopted: Vec<&PathBuf> = mounted.iter().filter(|m| m.2.is_some_and(is)).map(|m| &m.0).collect();
    let labelled: Vec<&PathBuf> = mounted.iter().filter(|m| is(m.1) || m.2.is_some_and(is)).map(|m| &m.0).collect();
    let found = |labels: bool| resolve(word, labels, false).map(|d| d.root().to_path_buf());
    match (adopted.as_slice(), found(false)) {
        ([one], Ok(root)) => assert_eq!(&&root, one),
        ([], Err(e)) => match mounted.iter().find(|m| is(m.1) || m.2.is_some_and(is)) {
            Some(m) => {
                let label = format!("{word:?} is the label of the volume at {}", m.0.display());
                assert!(e.to_string().contains(&label), "{e}");
            }
            None => assert!(matches!(e, Error::NotFound(_)), "{e}"),
        },
        (many, Err(e)) if many.len() > 1 => {
            assert!(e.to_string().contains(&format!("matches {} drives", many.len())), "{e}")
        }
        (want, got) => panic!("{word:?}: want {want:?}, got {got:?}"),
    }
    match (labelled.as_slice(), found(true)) {
        ([one], Ok(root)) => assert_eq!(&&root, one),
        ([], Err(e)) => assert!(matches!(e, Error::NotFound(_)), "{e}"),
        (many, Err(e)) if many.len() > 1 => {
            assert!(e.to_string().contains(&format!("matches {} drives", many.len())), "{e}")
        }
        (want, got) => panic!("{word:?} with labels: want {want:?}, got {got:?}"),
    }
    crate::testing::volumes_on_this_thread(None);
}

/// While drive lookups name folders, the machine's disks are hidden with its drives: listing
/// them, planning a format by any name, applying a plan made before the folders were named and
/// applying a format confirmed by its identity are all refused, before any disk is looked at.
#[hegel::test(test_cases = 10)]
fn disks_stay_hidden_while_drives_are(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let drive =
        tc.draw(gs::sampled_from(vec!["disk999".to_string(), "Backup".to_string(), dir.path().display().to_string()]));
    let req = FormatRequest { drive: drive.clone(), options: FormatOptions::default() };
    let planned = plan_format(&dir.path().join("elsewhere"), crate::format::tests::sample_disk(), &req, &Ctx::silent());
    crate::testing::volumes_on_this_thread(Some(vec![dir.path().join("volumes")]));
    let hidden = |e: Error| assert!(e.to_string().contains("LOCALLIBRARY_VOLUMES is set"), "{e}");
    hidden(list_disks(store.root()).unwrap_err());
    hidden(format_plan(store.root(), &req, &Ctx::silent()).unwrap_err());
    // Planning reads the disk with the system's tools, which may know none such (lsblk on Linux).
    if let Ok(planned) = planned {
        let with_store = tc.draw(gs::booleans());
        hidden(planned.apply(with_store.then_some(&store), &Ctx::silent()).unwrap_err());
    }
    let confirmed = FormatApply { drive, identity: "disk999".into(), options: FormatOptions::default() };
    hidden(format_apply(store.root(), &confirmed, &Ctx::silent()).unwrap_err());
    crate::testing::volumes_on_this_thread(None);
}

#[cfg(unix)]
use crate::format::tests::fake_tools;

/// A drive on a disk of its own, adopted and in the store: a store with it, and the drive.
#[cfg(unix)]
fn drive_on_a_disk(fake: &mut fake_tools::FakeTools, store_on_it: bool) -> (Store, Device) {
    std::fs::create_dir(fake.dir().join("DNA")).unwrap();
    let mut d = Device::open(fake.dir().join("DNA")).unwrap();
    d.adopt(Some("DNA")).unwrap();
    let store_root = if store_on_it { d.root().join("store") } else { fake.dir().join("store") };
    let store = Store::open_or_init(store_root).unwrap();
    sync::register_drive(&store, &d, None).unwrap();
    (store, d)
}

/// A drive's file system check, through the API, runs the platform's steps for its volume and
/// goes in the drive's logbook in the store: the verdict as the last file-system check, and
/// errors found as a problem too.
#[cfg(unix)]
#[hegel::test(test_cases = 10)]
fn a_drives_file_system_check_goes_in_its_logbook(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = fake_tools::FakeTools::new();
    let (store, d) = drive_on_a_disk(&mut fake, false);
    let (volume, fs) = fake.volume_at(d.root(), "vfat", true);
    let repair = tc.draw(gs::booleans());
    let steps = crate::fscheck::steps(d.root(), Some(&volume), Some(&fs), repair).unwrap();
    let consistent = tc.draw(gs::booleans());
    for (i, c) in [steps.unmount.as_ref(), Some(&steps.check), steps.mount.as_ref()].into_iter().flatten().enumerate() {
        let args: Vec<&str> = c.args.iter().map(String::as_str).collect();
        match (i, consistent) {
            (_, true) | (0, false) if c != &steps.check => fake.answer(&c.program, &args, ""),
            (_, true) => fake.answer(&c.program, &args, "File system check exit code is 0."),
            _ => fake.refuse(&c.program, &args, "File system check exit code is 8."),
        }
    }

    let req = FsCheckRequest { drive: d.root().display().to_string(), repair };
    let report = drive_fs_check(Some(&store), &req).unwrap();
    assert_eq!(report.ok, consistent);
    let log = store.drives().unwrap().shift_remove(&d.id().unwrap()).unwrap().log;
    let last = log.last_fs_check.unwrap();
    assert_eq!((last.ok, last.summary.as_str()), (consistent, report.summary().as_str()));
    let problems: Vec<(&str, &str)> = log.problems.iter().map(|p| (p.kind.as_str(), p.detail.as_str())).collect();
    let summary = report.summary();
    assert_eq!(problems, if consistent { vec![] } else { vec![("fs_errors", summary.as_str())] });
}

/// Ejecting a drive, through the API, plans the platform's commands for its root and applies
/// them, counting a clean eject in the drive's logbook; a drive whose disk holds the store is
/// refused before anything runs, as unmounting it would take the store away.
#[cfg(unix)]
#[hegel::test(test_cases = 10)]
fn ejecting_a_drive_is_planned_applied_and_counted(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = fake_tools::FakeTools::new();
    let store_on_it = tc.draw(gs::weighted_booleans(0.3));
    let (store, d) = drive_on_a_disk(&mut fake, store_on_it);
    fake.volume_at(d.root(), "vfat", true);
    let cmds = crate::eject::commands(d.root()).unwrap();
    for c in &cmds {
        fake.answer(&c.program, &c.args.iter().map(String::as_str).collect::<Vec<_>>(), "");
    }
    // By path: by name, a drive is looked for among the system's mounted volumes.
    let drives = vec![d.root().display().to_string()];
    let before = fake.calls().len();

    let planned = drive_eject_plan(Some(store.root()), &EjectDrives { drives: drives.clone() });
    let ejects = || store.drives().unwrap().shift_remove(&d.id().unwrap()).unwrap().log.clean_ejects;
    if store_on_it {
        assert!(matches!(&planned, Err(Error::Invalid(m)) if m.contains("which holds")), "{planned:?}");
        let ran = fake.calls().split_off(before);
        assert!(ran.iter().all(|c| !cmds.contains(c)), "{ran:?}");
        assert_eq!(ejects(), 0);
        return;
    }
    let planned = planned.unwrap();
    let shown: Vec<String> = cmds.iter().map(|c| c.display()).collect();
    assert_eq!(planned.plan.drives[0].commands, shown);
    assert_eq!(planned.summary, ["eject DNA: unmount it and power it off"]);
    let reports = if tc.draw(gs::booleans()) {
        planned.apply(Some(&store), &Ctx::silent()).unwrap()
    } else {
        drive_eject(Some(&store), &EjectDrives { drives }).unwrap()
    };
    assert_eq!(reports.iter().map(|r| r.commands.clone()).collect::<Vec<_>>(), [shown]);
    assert_eq!(ejects(), 1);
}

/// A drive's disk is formatted through the API as planned, the plan made by the drive's path:
/// the planned erase runs, and the drive keeps its name and identity on the new volume.
/// Confirmed with another identity, nothing is erased.
#[cfg(target_os = "macos")]
#[hegel::test(test_cases = 10)]
fn a_drives_disk_is_formatted_as_planned_through_the_api(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = fake_tools::FakeTools::new();
    let (store, d) = drive_on_a_disk(&mut fake, false);
    fake.volume_at(d.root(), "vfat", true);
    let drive = d.root().display().to_string();
    let req = FormatRequest { drive: drive.clone(), options: FormatOptions::default() };
    let planned = format_plan(store.root(), &req, &Ctx::silent()).unwrap();
    assert_eq!(planned.plan.keeps_name.as_deref(), Some("DNA"));
    let erase = ["eraseDisk", "MS-DOS FAT32", planned.plan.label.as_str(), "MBR", "disk4"];
    fake.then_after("diskutil", &erase);
    // The new volume, mounted somewhere else.
    let new = fake.dir().join("NEW");
    std::fs::create_dir(&new).unwrap();
    let new = device::dunce_canonicalize(&new);
    let listing = serde_json::json!({ "AllDisksAndPartitions": [{ "DeviceIdentifier": "disk4", "Partitions": [
            { "DeviceIdentifier": "disk4s1", "Size": 63_000_000_000u64, "MountPoint": new }] }] });
    let info = serde_json::json!({ "DeviceIdentifier": "disk4", "Size": 64_000_000_000u64, "MediaName": "Ultra Fit" });
    fake.answer("diskutil", &["list", "-plist", "disk4"], &listing.to_string());
    fake.answer("diskutil", &["info", "-plist", "disk4"], &info.to_string());
    fake.answer("diskutil", &["info", "-plist", "disk4s1"], "{}");

    let confirmed = tc.draw(gs::weighted_booleans(0.8));
    let identity = if confirmed { planned.plan.identity.clone() } else { format!("{}0", planned.plan.identity) };
    let applied = match tc.draw(gs::booleans()) {
        true if confirmed => planned.apply(Some(&store), &Ctx::silent()),
        _ => format_apply(
            store.root(),
            &FormatApply { drive, identity, options: FormatOptions::default() },
            &Ctx::silent(),
        ),
    };
    let erased = fake.calls().iter().any(|c| c.args.first().is_some_and(|a| a == "eraseDisk"));
    if !confirmed {
        assert!(matches!(applied, Err(Error::StalePlan(_))), "{applied:?}");
        assert!(!erased);
        return;
    }
    let report = applied.unwrap();
    assert!(erased);
    assert_eq!((report.mount.as_deref(), report.drive_name.as_deref()), (Some(new.as_path()), Some("DNA")));
    assert_eq!(Device::open(&new).unwrap().id(), d.id());
}

/// The disks offered for formatting are the system's external ones, each with why it can't be
/// formatted: the one holding the store can't, saying so.
#[cfg(target_os = "macos")]
#[hegel::test(test_cases = 6)]
fn the_disk_holding_the_store_is_listed_as_not_to_be_formatted(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = fake_tools::FakeTools::new();
    let store_on_it = tc.draw(gs::booleans());
    let (store, d) = drive_on_a_disk(&mut fake, store_on_it);
    fake.volume_at(d.root(), "vfat", true);
    let external = serde_json::json!({ "WholeDisks": ["disk4"] }).to_string();
    fake.answer("diskutil", &["list", "-plist", "external", "physical"], &external);

    let listed = list_disks(store.root()).unwrap();
    assert_eq!(listed.iter().map(|v| v.disk.id.as_str()).collect::<Vec<_>>(), ["disk4"]);
    let holds = format!("it holds {}", store.root().display());
    assert_eq!(listed[0].problem, store_on_it.then_some(holds));
}

/// Regression: `provision --format` showed each of the format's warnings twice, in the format's
/// plan and again in its own.
#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "a plan on Linux asks lsblk what's mounted on the disk, and the sample disk isn't there"
)]
fn a_provision_plan_keeps_the_formats_warnings_in_the_formats_plan() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let req = FormatRequest { drive: "disk4".into(), options: FormatOptions::default() };
    crate::format::THREAD_OS.with(|o| o.set(Some("linux")));
    let format = plan_format(store.root(), crate::format::tests::sample_disk(), &req, &Ctx::silent());
    crate::format::THREAD_OS.with(|o| o.set(None));
    let untested = "formatting hasn't been tested on Linux yet; check the result";
    // Copied from a drive, as the store has no library to write.
    let main = dir.path().join("Main");
    std::fs::create_dir(&main).unwrap();
    let req = ProvisionRequest {
        target: "disk4".into(),
        like: Some(main.display().to_string()),
        library: None,
        name: None,
        format: true,
        format_options: FormatOptions::default(),
        role: None,
        player_name: None,
        player_colour: None,
        settings: false,
        verify: false,
        eject: false,
    };
    let plan = drives::plan_provision(&store, &req, None, Some(format.unwrap())).unwrap();
    let once = |w: &[String]| w.iter().filter(|w| *w == untested).count() == 1;
    assert!(once(&plan.format.as_ref().unwrap().warnings), "{:?}", plan.format);
    assert!(plan.plan.warnings.is_empty(), "its own: {:?}", plan.plan.warnings);
    assert!(once(&plan.warnings), "every warning, once: {:?}", plan.warnings);
}

/// Provisioning with a format plans the format's steps first, and applies only with the plan's
/// `format.identity`: none, or another disk's, is refused before anything is erased. Confirmed,
/// the disk is erased and the new volume gets the drive's name and a copy of the library; one the
/// system didn't mount stops the provision at the format, and an eject the system refuses stops
/// it at the eject, the steps before it done; one it takes is reported.
#[cfg(target_os = "macos")]
#[hegel::test(test_cases = 8)]
#[hegel::explicit_test_case(mounted = true, eject = true, ejects = false, confirmed = "the plan's")]
#[hegel::explicit_test_case(mounted = true, eject = true, ejects = true, confirmed = "the plan's")]
#[hegel::explicit_test_case(mounted = false, eject = false, ejects = false, confirmed = "the plan's")]
fn prop_a_provision_formats_only_the_confirmed_disk(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = fake_tools::FakeTools::new();
    let (store, d) = drive_on_a_disk(&mut fake, false);
    fake.volume_at(d.root(), "vfat", true);
    let source = fake.demo_drive("Source");
    let mounted = tc.draw(gs::weighted_booleans(0.8));
    let eject = tc.draw(gs::booleans());
    let ejects = tc.draw(gs::booleans());
    let req = ProvisionRequest {
        target: d.root().display().to_string(),
        like: Some(source.display().to_string()),
        library: None,
        name: Some("Gig".into()),
        format: true,
        format_options: FormatOptions::default(),
        role: Some("backup".into()),
        player_name: None,
        player_colour: None,
        settings: false,
        verify: false,
        eject,
    };
    let planned = provision_plan(&store, &req, &Ctx::silent()).unwrap();
    let format = planned.plan.format.clone().unwrap();
    assert!(planned.destructive);
    assert!(planned.plan.steps.iter().any(|s| s == "give it the name \"Gig\""), "{:?}", planned.plan.steps);
    assert_eq!(
        planned.plan.steps.last().map(String::as_str),
        Some(if eject { "eject it" } else { "mark it as a backup drive" })
    );
    let erase = ["eraseDisk", "MS-DOS FAT32", format.label.as_str(), "MBR", "disk4"];
    fake.then_after("diskutil", &erase);
    let new = fake.dir().join("NEW");
    std::fs::create_dir(&new).unwrap();
    let new = device::dunce_canonicalize(&new);
    // The system knows the new volume, and ejects it, or doesn't.
    if ejects {
        let volume = serde_json::json!({ "DeviceIdentifier": "disk4s1", "ParentWholeDisk": "disk4", "MountPoint": new,
                "FilesystemType": "msdos", "FilesystemName": "MS-DOS FAT32" });
        fake.answer("diskutil", &["info", "-plist", &new.to_string_lossy()], &volume.to_string());
        for c in crate::eject::commands(&new).unwrap() {
            fake.answer(&c.program, &c.args.iter().map(String::as_str).collect::<Vec<_>>(), "");
        }
    }
    let mut part = serde_json::json!({ "DeviceIdentifier": "disk4s1", "Size": 63_000_000_000u64 });
    if mounted {
        part["MountPoint"] = serde_json::json!(new);
    }
    let listing =
        serde_json::json!({ "AllDisksAndPartitions": [{ "DeviceIdentifier": "disk4", "Partitions": [part] }] });
    let info = serde_json::json!({ "DeviceIdentifier": "disk4", "Size": 64_000_000_000u64, "MediaName": "Ultra Fit" });
    fake.answer("diskutil", &["list", "-plist", "disk4"], &listing.to_string());
    fake.answer("diskutil", &["info", "-plist", "disk4"], &info.to_string());
    fake.answer("diskutil", &["info", "-plist", "disk4s1"], "{}");

    let erased = || fake.calls().iter().any(|c| c.args.first().is_some_and(|a| a == "eraseDisk"));
    let confirmed = tc.draw(gs::sampled_from(vec!["none", "another", "the plan's"]));
    let identity = match confirmed {
        "none" => None,
        "another" => Some(format!("{}0", format.identity)),
        _ => Some(format.identity.clone()),
    };
    let applied = provision_apply(&store, &req, identity.as_deref(), &Ctx::silent());
    match confirmed {
        "none" => assert!(matches!(&applied, Err(Error::Invalid(m)) if m.contains("format.identity")), "{applied:?}"),
        "another" => assert!(matches!(&applied, Err(Error::StalePlan(_))), "{applied:?}"),
        _ => {}
    }
    if identity.as_deref() != Some(format.identity.as_str()) {
        assert!(!erased());
        return;
    }
    let report = applied.unwrap();
    assert!(erased());
    let stopped = report.stopped.as_ref().map(|s| s.step.as_str());
    if !mounted {
        assert_eq!(stopped, Some("format"), "{:?}", report.stopped);
        assert!(report.stopped.unwrap().message.contains("wasn't mounted"));
        return;
    }
    let made = Device::open(&new).unwrap();
    assert_eq!(made.name(), "Gig");
    assert_eq!(made.library().unwrap(), Device::open(&source).unwrap().library().unwrap());
    let role = store.drives().unwrap().shift_remove(&made.id().unwrap()).and_then(|r| r.role);
    assert_eq!(role, Some(crate::fleet::Role::Backup));
    assert_eq!(report.drive.and_then(|i| i.name).as_deref(), Some("Gig"));
    assert_eq!(stopped, (eject && !ejects).then_some("eject it"), "{:?}", report.stopped);
    assert_eq!(report.eject.map(|e| e.root), (eject && ejects).then_some(new));
}

/// Getting a saved drive on a disk ready for a set ejects it once the check passes: the plan
/// names the drive (warning where ejecting hasn't been tested), and applied, the check finds the
/// drive ready for the CDJ-3000, then the job runs the system's eject, says the drive can be
/// pulled out, and counts the eject in its logbook.
#[cfg(target_os = "macos")]
#[hegel::test(test_cases = 4)]
fn prop_a_set_job_ejects_the_drive_once_it_is_checked(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = fake_tools::FakeTools::new();
    let root = device::dunce_canonicalize(&fake.demo_drive("DNA"));
    for t in Device::open(&root).unwrap().library().unwrap().tracks {
        let audio = crate::paths::to_fs(&root, &crate::paths::key(&t.file_path));
        std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
        std::fs::write(audio, "audio").unwrap();
    }
    let drive = root.display().to_string();
    let store = Store::open_or_init(fake.dir().join("store")).unwrap();
    fake.volume_at(&root, "vfat", true);
    adopt_drive(Some(&store), &AdoptDrive { drive: drive.clone(), name: Some("DNA".into()) }).unwrap();
    pull(&store, &PullRequest { drive: drive.clone(), ..Default::default() }, &Ctx::silent()).unwrap();
    for c in crate::eject::commands(&root).unwrap() {
        fake.answer(&c.program, &c.args.iter().map(String::as_str).collect::<Vec<_>>(), "");
    }

    let os = tc.draw(gs::sampled_from(vec![None, Some("linux"), Some("windows")]));
    crate::testing::os_on_this_thread(os);
    let req = SetJobRequest { drives: vec![drive], players: vec!["cdj-3000".into()], ..Default::default() };
    let plan = set_job_plan(&store, &req, &Ctx::silent());
    crate::testing::os_on_this_thread(None);
    let plan = plan.unwrap();
    let eject = plan.plan.steps.iter().find(|s| s.step == StepKind::Eject).unwrap();
    assert_eq!(eject.summary, ["eject DNA once the check passes"]);
    assert_eq!(eject.warnings.len(), usize::from(os.is_some()), "{:?}", eject.warnings);

    let report = plan.apply(Some(&store), &Ctx::silent()).unwrap();
    assert!(!report.problems && report.stopped.is_none(), "{:?}", report.stopped);
    let check = report.steps.iter().find(|s| s.step == StepKind::Check).unwrap();
    assert_eq!(check.done, ["ready"], "{:?}", report.check);
    let last = report.steps.last().unwrap();
    assert_eq!((last.step, &last.done[..]), (StepKind::Eject, &["DNA ejected; it can be pulled out".to_string()][..]));
    let id = Device::open(&root).unwrap().id().unwrap();
    assert_eq!(store.drives().unwrap()[&id].log.clean_ejects, 1);
}

/// A disk to format is named by the system's identifier for it as well as by the drive mounted
/// on it: both plans erase the same disk the same way, keeping the drive's name, under the label
/// asked for (else the drive's name).
#[cfg(target_os = "macos")]
#[hegel::test(test_cases = 4)]
fn prop_a_disk_to_format_is_found_by_its_identifier_too(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = fake_tools::FakeTools::new();
    let (store, d) = drive_on_a_disk(&mut fake, false);
    fake.volume_at(d.root(), "vfat", true);
    let label = tc.draw(gs::optional(gs::sampled_from(vec!["GIG", "DNA 2"])));
    let options = FormatOptions { label: label.map(String::from), ..FormatOptions::default() };
    let plan = |drive: String| {
        let req = FormatRequest { drive, options: options.clone() };
        format_plan(store.root(), &req, &Ctx::silent()).unwrap().plan
    };
    let (by_id, by_path) = (plan("disk4".into()), plan(d.root().display().to_string()));
    for p in [&by_id, &by_path] {
        assert_eq!((p.disk.id.as_str(), p.keeps_name.as_deref()), ("disk4", Some("DNA")));
    }
    assert_eq!((&by_id.identity, &by_id.label), (&by_path.identity, &by_path.label));
    assert_eq!(by_id.label, label.unwrap_or("DNA"));
}

/// The store drive lookups consult by default is kept safe like the store a plan is given: a
/// drive whose disk holds it isn't ejected, though the plan was given another store.
#[cfg(unix)]
#[hegel::test(test_cases = 6)]
fn prop_the_default_store_s_disk_is_kept_too(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = fake_tools::FakeTools::new();
    let (store, d) = drive_on_a_disk(&mut fake, true);
    fake.volume_at(d.root(), "vfat", true);
    let given = fake.dir().join("elsewhere");
    let default = tc.draw(gs::sampled_from(vec!["none", "the given one", "the drive's"]));
    let root = match default {
        "none" => None,
        "the given one" => Some(given.clone()),
        _ => Some(store.root().to_path_buf()),
    };
    crate::testing::store_root_on_this_thread(root);
    let planned =
        drive_eject_plan(Some(given.as_path()), &EjectDrives { drives: vec![d.root().display().to_string()] });
    crate::testing::store_root_on_this_thread(None);
    match default {
        "the drive's" => {
            assert!(matches!(&planned, Err(Error::Invalid(m)) if m.contains("which holds")), "{planned:?}")
        }
        _ => assert_eq!(planned.unwrap().plan.drives.len(), 1),
    }
}

/// Settings imported with no drive come from rekordbox's own folder on this computer: its My
/// Settings files become the profile's base, and importing them again changes nothing. A computer
/// without that folder has none to give.
#[cfg(unix)]
#[hegel::test(test_cases = 4)]
#[hegel::explicit_test_case(installed = true)]
fn prop_settings_are_imported_from_rekordbox_on_this_computer(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let home = tempfile::tempdir().unwrap();
    // SAFETY: nextest gives each test a process of its own, and no other thread of it reads the
    // environment while the test runs.
    unsafe {
        std::env::set_var("HOME", home.path());
        std::env::remove_var("XDG_CONFIG_HOME");
    }
    let dir = match cfg!(target_os = "macos") {
        true => home.path().join("Library/Application Support/Pioneer/rekordbox6"),
        false => home.path().join(".config/Pioneer/rekordbox6"),
    };
    let installed = tc.draw(gs::booleans());
    if installed {
        std::fs::create_dir_all(&dir).unwrap();
        let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rekordcrate/demo_tracks/PIONEER");
        for f in settings::SettingsFile::ALL {
            std::fs::copy(demo.join(f.file_name()), dir.join(f.file_name())).unwrap();
        }
    }
    let store = Store::open_or_init(home.path().join("store")).unwrap();
    let planned = settings_import_plan(&store, &ImportSettings { drive: None });
    if !installed {
        let e = planned.map(|p| p.noop).unwrap_err();
        assert!(matches!(&e, Error::NotFound(m) if m.contains("rekordbox's settings folder")), "{e}");
        return;
    }
    let planned = planned.unwrap();
    let read = settings::read_all(&FlatSettings(&dir));
    assert_eq!((planned.plan.from.as_str(), planned.plan.values), ("rekordbox on this computer", read.values.len()));
    assert!(!planned.noop);
    let profile = settings_import(&store, &ImportSettings::default()).unwrap();
    assert_eq!(profile.base.len(), planned.plan.changes.len());
    assert!(settings_import_plan(&store, &ImportSettings::default()).unwrap().noop, "imported already");
}
