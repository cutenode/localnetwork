//! Which drive a request means, and what it's told when it means none or several: a registered
//! drive found where it was last seen, a known one that isn't plugged in, a drive named like a
//! folder here, and the drives a command given none could mean, listed as a table, until they all
//! belong to one library, whose lead clone it then means.

use crate::common::{self, path};
use crate::props_api_drives::{saved_copy, saved_demo};
use crate::props_api_jobs::{bare_export, own_identity};

use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
use locallibrary::api;
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::error::Reason;
use locallibrary::fleet::{self, Role};
use locallibrary::store::Store;
use locallibrary::{Error, sync};

/// Where a registered drive is now, against where the registry last saw it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Place {
    /// Where it was last seen, in a folder inside the volumes folder, where drives aren't looked
    /// for.
    Nested,
    /// Unplugged.
    Gone,
    /// Last seen outside the volumes folder.
    Outside,
    /// Another drive is where it was last seen.
    Replaced,
}

/// `word` with each letter's case drawn.
fn any_case(tc: &TestCase, word: &str) -> String {
    word.chars()
        .map(|c| if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() })
        .collect()
}

/// Mounted drives (rekordbox exports or adopted blank ones, one named like a folder here) and four
/// drives the store knows, one at each kind of place: each, by name in any case or by its id's
/// first characters, is found where it was last seen while that's inside the volumes folder and
/// it's still there; otherwise it's known but not mounted, with when and where it was last seen,
/// and what is mounted (only the drives locallibrary would use, the one found among them). A word
/// no drive answers to says what is mounted too, or that nothing is. A drive named like a folder
/// here is refused as ambiguous; the folder is meant by its path.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn a_drive_is_found_where_it_was_last_seen_or_said_to_be_missing(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let volumes = dunce_canonicalize(&common::isolate(dir.path(), Some(store.root())));
    let e = api::resolve_drive("Nobody").unwrap_err();
    assert!(
        e.to_string()
            .ends_with("matches \"Nobody\" (use a path, a drive name, or an id); no rekordbox drive is mounted"),
        "{e}"
    );

    // One drive named like a folder in the working directory (the crate's), and maybe others.
    let mut names = vec![tc.draw(gs::sampled_from(vec!["src", "tests"]))];
    names.extend(tc.draw(gs::vecs(gs::sampled_from(vec!["Main", "Club"])).max_size(2).unique(true)));
    let mut ours: Vec<(String, PathBuf)> = Vec::new();
    for name in &names {
        let root = volumes.join(format!("{name} volume"));
        match tc.draw(gs::booleans()) {
            true => bare_export(&root),
            false => std::fs::create_dir(&root).unwrap(),
        }
        Device::open(&root).unwrap().adopt(Some(name)).unwrap();
        ours.push((name.to_string(), root));
    }
    // A blank volume, which isn't one of ours.
    common::blank(&volumes, "BLANK");

    let places = [
        (Place::Nested, "Bag stick"),
        (Place::Gone, "Lost stick"),
        (Place::Outside, "Far stick"),
        (Place::Replaced, "Old stick"),
    ];
    let mut known = Vec::new();
    for (place, name) in places {
        let seen_at = match place {
            Place::Outside => dir.path().join("elsewhere").join(name),
            _ => volumes.join("Bag").join(name),
        };
        std::fs::create_dir_all(&seen_at).unwrap();
        let seen_at = dunce_canonicalize(&seen_at);
        let mut d = Device::open(&seen_at).unwrap();
        d.adopt(Some(name)).unwrap();
        sync::register_drive(&store, &d, None).unwrap();
        match place {
            Place::Nested | Place::Outside => {}
            Place::Gone => std::fs::remove_dir_all(&seen_at).unwrap(),
            Place::Replaced => {
                std::fs::remove_dir_all(&seen_at).unwrap();
                std::fs::create_dir(&seen_at).unwrap();
                Device::open(&seen_at).unwrap().adopt(Some("Pretender")).unwrap();
            }
        }
        known.push((place, name, seen_at, d.id().unwrap()));
    }
    // Found where it was last seen, a drive is as good as mounted.
    let listed: Vec<(String, PathBuf)> = ours
        .iter()
        .cloned()
        .chain(known.iter().filter(|k| k.0 == Place::Nested).map(|k| (k.1.to_string(), k.2.clone())))
        .collect();
    let mounted = |e: &Error| {
        let said = e.to_string();
        for (name, root) in &listed {
            assert!(said.contains(&format!("{name} ({})", root.display())), "{said} lists {name}");
        }
        assert!(!said.contains("BLANK") && !said.contains("Pretender"), "{said}");
    };
    for (place, name, seen_at, id) in &known {
        let spec = match tc.draw(gs::booleans()) {
            true => any_case(&tc, name),
            false => id.to_string()[..tc.draw(gs::integers::<usize>().min_value(4).max_value(8))].to_string(),
        };
        match (place, api::resolve_drive(&spec)) {
            (Place::Nested, Ok(d)) => assert_eq!(d.root(), seen_at),
            (Place::Nested, Err(e)) => panic!("{spec:?} isn't found where it was last seen: {e}"),
            (_, Ok(d)) => panic!("{spec:?} found at {}, though it isn't there", d.root().display()),
            (_, Err(e)) => {
                assert!(matches!(e, Error::NotFound(_)), "{e}");
                assert!(e.to_string().contains(&format!("{name} is known but not mounted; last seen ")), "{e}");
                assert!(e.to_string().contains(&format!(" at {};", seen_at.display())), "{e}");
                mounted(&e);
            }
        }
    }
    let e = api::resolve_drive("Nobody").unwrap_err();
    assert!(e.to_string().contains("no mounted drive matches \"Nobody\""), "{e}");
    mounted(&e);

    for (name, root) in &ours {
        let here = Path::new(name).is_dir();
        match api::resolve_drive(name) {
            Ok(d) => {
                assert!(!here, "{name} is a folder here too");
                assert_eq!(d.root(), dunce_canonicalize(root));
            }
            Err(e) => {
                assert!(here, "{e}");
                assert!(e.to_string().contains("is both a folder here and the drive at"), "{e}");
                let folder = api::resolve_drive(&format!("./{name}")).unwrap();
                assert_eq!(folder.root(), dunce_canonicalize(Path::new(name)));
            }
        }
    }
    locallibrary::testing::volumes_on_this_thread(None);
    locallibrary::testing::store_root_on_this_thread(None);
}

/// The lines of a candidate table in a refusal: those after the first.
fn table(e: &Error) -> Vec<String> {
    e.to_string().lines().skip(1).map(str::to_string).collect()
}

/// Each line names a candidate (name, mount path, role, library) in columns, aligned.
fn assert_table(e: &Error, want: &[(String, PathBuf, Option<Role>, Option<&str>)]) {
    let lines = table(e);
    assert_eq!(lines.len(), want.len(), "{e}");
    let mut column = None;
    for (name, root, role, library) in want {
        let root = path(root);
        let line = lines.iter().find(|l| l.contains(&root)).unwrap_or_else(|| panic!("{root} in {lines:?}"));
        let at = line.find(&root).unwrap();
        assert_eq!(*column.get_or_insert(at), at, "the mount paths line up: {lines:?}");
        assert!(line.trim_start().starts_with(name.as_str()), "{line:?}");
        let rest: Vec<&str> = line[at + root.len()..].split_whitespace().collect();
        assert_eq!(rest[0], role.map_or("-", Role::name), "{line:?}");
        assert_eq!(rest[1..].join(" "), library.unwrap_or("(no library)"), "{line:?}");
    }
}

/// Move a mounted drive out of the volumes folder (unplugging it), or back.
fn unplug(root: &Path, away: &Path) {
    std::fs::create_dir_all(away.parent().unwrap()).unwrap();
    std::fs::rename(root, away).unwrap();
}

/// One, then two or three rekordbox drives in no library, then with drives of two libraries,
/// then only Main's two clones: one alone is the drive meant; else a command given no drive (or
/// a group of drives) is refused, listing the
/// candidates as a table of name, mount path, role and library, until the mounted drives of a
/// library are all that's mounted; then a read reads the library once, its name means its
/// clones, each once beside a
/// clone named too, and its lead clone: the newest, the primary when the two diverged, none when
/// they diverged with no primary (a write to both then goes to the first named), and none when
/// both are archive or retired.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_command_given_no_drive_lists_the_drives_it_could_mean(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = saved_copy(dir.path());
    // Named apart from its library, so that "Main" means the library.
    fleet::rename(Some(&store), &mut Device::open(&main).unwrap(), "Home", false).unwrap();
    let volumes = main.parent().unwrap().to_path_buf();
    let away = dir.path().join("away");
    unplug(&main, &away.join("Main"));

    // Drives in no library.
    let names =
        tc.draw(gs::vecs(gs::sampled_from(vec!["Gig", "Radio", "Spare stick"])).min_size(2).max_size(3).unique(true));
    let mut loose = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let root = volumes.join(name);
        bare_export(&root);
        if i == 0 {
            // Alone, a drive in no library is a group of its own, and the drive meant.
            let group = api::drive_group(Some(&store), None).unwrap();
            assert_eq!((group.library, group.drives), (None, vec![dunce_canonicalize(&root)]));
            assert_eq!(api::default_drive(Some(&store), "check").unwrap().root(), dunce_canonicalize(&root));
        }
        if tc.draw(gs::booleans()) {
            Device::open(&root).unwrap().adopt(Some(&format!("{name} drive"))).unwrap();
        }
        let d = Device::open(&root).unwrap();
        loose.push((d.name(), d.root().to_path_buf(), None, None));
    }
    let e = api::default_drive(Some(&store), "refresh").unwrap_err();
    assert_eq!(e.reason(), Some(Reason::ChooseDrive), "{e}");
    assert!(e.to_string().starts_with("say which drive to refresh ("), "{e}");
    assert!(e.to_string().contains(&format!(": {} rekordbox drives are mounted", names.len())), "{e}");
    assert_table(&e, &loose);
    for store in [Some(&store), None] {
        let e = api::drive_group(store, None).unwrap_err();
        assert!(e.to_string().contains("none belongs to a library in the store yet"), "{e}");
        assert_table(&e, &loose);
    }

    // Main back, where its volume isn't called Main either, and a drive of Club, each with a role
    // or none.
    let main = dunce_canonicalize(&volumes).join("Home");
    unplug(&away.join("Main"), &main);
    let club = volumes.join("Club");
    bare_export(&club);
    let library = store.create_library("Club").unwrap().id;
    let mut d = Device::open(&club).unwrap();
    d.adopt(Some("Club")).unwrap();
    d.state_mut().unwrap().library_id = Some(library);
    d.save_state().unwrap();
    sync::register_drive(&store, &d, None).unwrap();
    let roles = [None, Some(Role::Primary), Some(Role::Backup)];
    let mut all = loose.clone();
    for (root, name, lib) in [(&main, "Home", "Main"), (&club, "Club", "Club")] {
        let role = tc.draw(gs::sampled_from(&roles[..]).print_as_debug());
        fleet::set_role(&store, Device::open(root).unwrap().id().unwrap(), role).unwrap();
        all.push((name.to_string(), dunce_canonicalize(root), role, Some(lib)));
    }
    let e = api::default_drive(Some(&store), "check").unwrap_err();
    assert!(e.to_string().starts_with("say which library (Club, Main) or drive to check"), "{e}");
    assert_table(&e, &all);
    let e = api::drive_group(Some(&store), None).unwrap_err();
    assert!(e.to_string().starts_with("say which library ("), "{e}");
    assert!(e.to_string().contains("drives of 2 libraries are mounted"), "{e}");
    assert_table(&e, &all);

    // Only Main's clones.
    for (_, root, ..) in &loose {
        std::fs::remove_dir_all(root).unwrap();
    }
    std::fs::remove_dir_all(&club).unwrap();
    let backup = volumes.join("Backup");
    saved_demo().1.write(&backup);
    own_identity(&store, &mut Device::open(&backup).unwrap(), "Backup");
    let backup = dunce_canonicalize(&backup);
    let ids = [&main, &backup].map(|r| Device::open(r).unwrap().id().unwrap());
    for id in ids {
        fleet::set_role(&store, id, None).unwrap();
    }
    // A read given no drive reads the library once.
    let reads: Vec<String> = api::library_reads(Some(&store)).into_iter().map(|c| c.name).collect();
    assert_eq!(reads, ["Main"]);
    let group = api::library_clones(&any_case(&tc, "main")).unwrap().expect("a library");
    let mut clones = group.drives.clone();
    clones.sort();
    assert_eq!(clones, [backup.clone(), main.clone()]);
    let named = tc.draw(gs::sampled_from(vec![main.clone(), backup.clone()]));
    let (drives, left_out) = api::expand_libraries(&[path(&named), "Main".into()]).unwrap();
    let other = if named == main { &backup } else { &main };
    assert_eq!((drives, left_out), (vec![path(&named), path(other)], Vec::<String>::new()));
    // Archive and retired clones aren't the library's lead.
    let kept: Vec<bool> = (0..2).map(|_| tc.draw(gs::booleans())).collect();
    for (id, archive) in ids.iter().zip(&kept) {
        match archive {
            true => fleet::set_role(&store, *id, Some(Role::Archive)).map(|_| ()).unwrap(),
            false => fleet::retire(&store, *id, false).map(|_| ()).unwrap(),
        }
    }
    let e = api::resolve_drive("main").map(|d| d.root().to_path_buf()).unwrap_err();
    assert!(e.to_string().contains("a mounted drive of library \"Main\" but archive and retired ones"), "{e}");
    let (_, left_out) = api::expand_libraries(&["main".into()]).unwrap();
    assert_eq!(left_out.len(), 2, "{left_out:?}");
    for (id, archive) in ids.iter().zip(&kept) {
        match archive {
            true => fleet::set_role(&store, *id, None).map(|_| ()).unwrap(),
            false => fleet::retire(&store, *id, true).map(|_| ()).unwrap(),
        }
    }
    // Both move on: the primary leads, and with none, there is no lead.
    common::export_new_track(&main, "Artist", "At home");
    common::export_new_track(&backup, "Artist", "At the club");
    let primary = tc.draw(gs::optional(gs::sampled_from(vec![0usize, 1])));
    if let Some(p) = primary {
        fleet::set_role(&store, ids[p], Some(Role::Primary)).unwrap();
    }
    let roots = [&main, &backup];
    match (primary, api::resolve_drive("Main")) {
        (Some(p), Ok(lead)) => assert_eq!(lead.root(), roots[p]),
        (None, Err(e)) => assert_eq!(e.reason(), Some(Reason::Diverged), "{e}"),
        (p, got) => panic!("primary {p:?}: {:?}", got.map(|d| d.root().to_path_buf())),
    }
    let once = api::libraries_once(vec![path(&named), path(other)]).unwrap();
    let want = primary.map_or(path(&named), |p| path(roots[p]));
    assert_eq!(once, [want]);
}
