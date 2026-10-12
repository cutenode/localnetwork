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
