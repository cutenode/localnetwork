//! The drive registry against a model: drives registered, renamed, given roles, retired, logged
//! and forgotten in any order. A library has at most one primary, archive and retired drives are
//! never written to unasked, listings and groups say what the model does, and a copied drive
//! (identity file and all) is spotted and given an identity of its own.

use crate::common;

use indexmap::IndexMap;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::error::Reason;
use locallibrary::fleet::{self, Check, Role};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
struct Drive {
    name: String,
    registered: bool,
    role: Option<Role>,
    retired: bool,
    writes: u32,
    bytes: u64,
    ejects: u32,
    problems: usize,
    scrubbed: Option<bool>,
}

/// Drive names, sharing letters in another case so that a name can mean several drives. The
/// service layer's model of the registry (`props_api_drives`) names its drives these too.
pub(crate) const NAMES: &[&str] = &["Main", "main", "Club", "Backup", "Club USB"];

/// Why a write to a registered drive is refused, if it is: a retired or archive drive is kept as
/// it is, unless the write is forced.
pub(crate) fn kept(retired: bool, role: Option<Role>) -> Option<Reason> {
    match (retired, role) {
        (true, _) => Some(Reason::Retired),
        (false, Some(Role::Archive)) => Some(Reason::Archive),
        _ => None,
    }
}

/// `role` given to a drive of a library whose drives have `roles`: a second primary makes the
/// first a backup.
pub(crate) fn demote<'a>(roles: impl Iterator<Item = &'a mut Option<Role>>, role: Option<Role>) {
    if role == Some(Role::Primary) {
        roles.filter(|r| **r == Some(Role::Primary)).for_each(|r| *r = Some(Role::Backup));
    }
}

#[hegel::test(test_cases = 15, suppress_health_check = [HealthCheck::TooSlow])]
fn the_registry_matches_the_model(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut devices: Vec<Device> = Vec::new();
    let mut model: IndexMap<Uuid, Drive> = IndexMap::new();
    for i in 0..3 {
        let mut d = Device::open(common::blank(dir.path(), &format!("d{i}"))).unwrap();
        let name = tc.draw(gs::sampled_from(NAMES));
        d.adopt(Some(name)).unwrap();
        model.insert(d.id().unwrap(), Drive { name: name.into(), ..Default::default() });
        devices.push(d);
    }
    let ids: Vec<Uuid> = model.keys().copied().collect();
    // Most drives start registered, so roles and names meet.
    for (i, id) in ids.iter().enumerate() {
        if tc.draw(gs::weighted_booleans(0.8)) {
            sync::register_drive(&store, &devices[i], None).unwrap();
            model[id].registered = true;
        }
    }

    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(12)) {
        let i = tc.draw(gs::integers::<usize>().max_value(ids.len() - 1));
        let id = ids[i];
        match tc.draw(gs::integers::<u8>().max_value(7)) {
            0 => {
                sync::register_drive(&store, &devices[i], None).unwrap();
                model[&id].registered = true;
            }
            1 => {
                let name = tc.draw(gs::sampled_from(NAMES));
                let r = fleet::rename(Some(&store), &mut devices[i], name, false).unwrap();
                assert_eq!((r.id, r.old_name.as_str()), (id, model[&id].name.as_str()));
                model[&id].name = name.into();
                model[&id].registered = true;
            }
            2 => {
                // Primary twice as often: a second primary demotes the first.
                let roles = vec![
                    Some(Role::Primary),
                    Some(Role::Primary),
                    Some(Role::Backup),
                    Some(Role::Set),
                    Some(Role::Archive),
                    None,
                ];
                let role = tc.draw(gs::sampled_from(roles));
                let r = fleet::set_role(&store, id, role);
                match model[&id].registered {
                    false => assert!(r.is_err()),
                    true => {
                        assert_eq!(r.unwrap().role, role);
                        demote(model.values_mut().filter(|m| m.registered).map(|m| &mut m.role), role);
                        model[&id].role = role;
                    }
                }
            }
            3 => {
                let undo = tc.draw(gs::booleans());
                let r = fleet::retire(&store, id, undo);
                assert_eq!(r.is_ok(), model[&id].registered);
                if model[&id].registered {
                    model[&id].retired = !undo;
                }
            }
            4 => {
                let bytes = tc.draw(gs::integers::<u64>().max_value(1 << 20));
                fleet::record_write(&store, &devices[i], bytes).unwrap();
                fleet::record_eject(&store, id).unwrap();
                if model[&id].registered {
                    model[&id].writes += 1;
                    model[&id].bytes += bytes;
                    model[&id].ejects += 1;
                }
            }
            5 => {
                let n = tc.draw(gs::integers::<usize>().max_value(60));
                for k in 0..n {
                    fleet::record_problem(&store, id, "damaged_files", format!("problem {k}")).unwrap();
                }
                let ok = tc.draw(gs::booleans());
                fleet::record_check(&store, id, Check::Scrub, ok, "scrubbed").unwrap();
                if model[&id].registered {
                    model[&id].problems = (model[&id].problems + n).min(50);
                    model[&id].scrubbed = Some(ok);
                }
            }
            6 => {
                // By name, which may be ambiguous; by id it never is.
                let by_name = tc.draw(gs::booleans());
                let spec = if by_name { model[&id].name.clone() } else { id.to_string() };
                let answering: Vec<Uuid> = model
                    .iter()
                    .filter(|(o, m)| {
                        m.registered && if by_name { m.name.eq_ignore_ascii_case(&spec) } else { **o == id }
                    })
                    .map(|(o, _)| *o)
                    .collect();
                let r = fleet::forget(&store, &spec);
                match answering.as_slice() {
                    [one] => {
                        assert_eq!(r.unwrap().id, *one);
                        model[one] = Drive { name: model[one].name.clone(), ..Default::default() };
                    }
                    _ => assert!(r.is_err(), "{spec:?} answers to {answering:?}"),
                }
            }
            _ => {
                let force = tc.draw(gs::booleans());
                let kept = model[&id].registered && kept(model[&id].retired, model[&id].role).is_some();
                assert_eq!(fleet::check_target(Some(&store), &devices[i], force).is_err(), kept && !force);
            }
        }

        // The registry says what the model does.
        let drives = store.drives().unwrap();
        let registered: Vec<Uuid> = model.iter().filter(|(_, m)| m.registered).map(|(id, _)| *id).collect();
        let mut held: Vec<Uuid> = drives.keys().copied().collect();
        let mut want = registered.clone();
        held.sort();
        want.sort();
        assert_eq!(held, want);
        for id in &registered {
            let (rec, m) = (&drives[id], &model[id]);
            assert_eq!((rec.name.as_str(), rec.role, rec.log.retired.is_some()), (m.name.as_str(), m.role, m.retired));
            assert_eq!((rec.log.writes, rec.log.bytes_written, rec.log.clean_ejects), (m.writes, m.bytes, m.ejects));
            assert_eq!(rec.log.problems.len(), m.problems);
            assert_eq!(rec.log.last_scrub.as_ref().map(|c| c.ok), m.scrubbed);
        }
        let primaries = drives.values().filter(|r| r.role == Some(Role::Primary)).count();
        assert!(primaries <= 1, "one primary per library");
        let primary = fleet::primary_of(&store, None).unwrap().map(|r| r.id);
        let want_primary = model.iter().find(|(_, m)| m.registered && m.role == Some(Role::Primary) && !m.retired);
        assert_eq!(primary, want_primary.map(|(id, _)| *id));

        // Listings: retired drives only when asked, mounted ones first, by name.
        for include_retired in [false, true] {
            let known = fleet::known(&store, &devices, include_retired).unwrap();
            let mut listed: Vec<Uuid> = known.iter().map(|k| k.id).collect();
            let mut want: Vec<Uuid> =
                registered.iter().copied().filter(|id| include_retired || !model[id].retired).collect();
            let names: Vec<String> = known.iter().map(|k| k.name.to_lowercase()).collect();
            assert!(names.is_sorted(), "{names:?}");
            assert!(known.iter().all(|k| k.mounted.is_some()));
            listed.sort();
            want.sort();
            assert_eq!(listed, want);
        }
    }
}

/// A drive copied file for file, identity and all: both claim one id, the copy is told apart by
/// that, and given an identity of its own it no longer is one. A drive that lost its identity is
/// recognised by what it holds, and takes the identity back.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn copies_are_told_apart_and_lost_identities_found(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut main = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    main.adopt(Some(tc.draw(gs::sampled_from(&["Main", "Ünï USB"][..])))).unwrap();
    sync::pull(&store, &mut main, &PullOptions::default(), &Ctx::silent()).unwrap();
    let copies = tc.draw(gs::integers::<usize>().min_value(1).max_value(2));
    let mut mounted = vec![Device::open(main.root()).unwrap()];
    for n in 0..copies {
        let root = dir.path().join(format!("Copy {n}"));
        common::copy_dir(main.root(), &root);
        mounted.push(Device::open(&root).unwrap());
    }
    let dupes = fleet::duplicates(&mounted, &|_| None);
    assert_eq!(dupes.len(), 1);
    assert_eq!(dupes[0].id, main.id().unwrap());
    assert_eq!(dupes[0].roots.len(), copies + 1);
    assert!(mounted.iter().all(|d| fleet::is_copy(d, &mounted)));

    let named = tc.draw(gs::optional(gs::sampled_from(vec!["Spare", "Set"])));
    let mut ids = vec![main.id().unwrap()];
    for d in mounted.iter_mut().skip(1) {
        let id = fleet::reidentify(Some(&store), d, named).unwrap().id.unwrap();
        assert!(!ids.contains(&id), "a new identity");
        assert_eq!(d.state().unwrap().library_id, main.state().unwrap().library_id, "the library link stays");
        if named.is_none() {
            assert_eq!(d.name(), format!("{} (copy)", main.name()));
        }
        ids.push(id);
    }
    let mounted: Vec<Device> = mounted.iter().map(|d| Device::open(d.root()).unwrap()).collect();
    assert!(fleet::duplicates(&mounted, &|_| None).is_empty());
    assert!(!mounted.iter().any(|d| fleet::is_copy(d, &mounted)));

    // Main loses its identity file: what it holds says which drive it is.
    std::fs::remove_dir_all(main.state_dir()).unwrap();
    let mut lost = Device::open(main.root()).unwrap();
    let found = fleet::recognise(&store, &lost, &Ctx::silent()).unwrap();
    assert!(found.snapshot.is_some());
    assert_eq!(found.best().map(|r| r.id), Some(ids[0]));
    let others: Vec<Device> = mounted[1..].iter().map(|d| Device::open(d.root()).unwrap()).collect();
    let info = fleet::adopt_as(&store, &mut lost, &ids[0].to_string(), &others, &Ctx::silent()).unwrap();
    assert_eq!(info.id, Some(ids[0]));
    // Its identity is in use while it's mounted: no other drive may take it.
    let taken = fleet::adopt_as(
        &store,
        &mut Device::open(others[0].root()).unwrap(),
        &ids[0].to_string(),
        &[lost],
        &Ctx::silent(),
    );
    assert!(taken.is_err());
}

/// Roles read back from their names in any case (`gig` is `set`); two sightings are of one disk
/// only by a serial number, and of one volume by its id; and of drives claiming one identity, the
/// one on the disk or volume the identity was given on is the original, when only one is.
#[hegel::test(test_cases = 40, suppress_health_check = [HealthCheck::TooSlow])]
fn roles_and_hardware_tell_drives_apart(tc: TestCase) {
    use locallibrary::fleet::Hardware;
    for role in Role::ALL {
        let spelled = match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => role.name().to_uppercase(),
            1 => format!("  {} ", role.name()),
            _ => role.name().to_string(),
        };
        assert_eq!(Role::parse(&spelled).unwrap(), role);
    }
    assert_eq!(Role::parse("GiG").unwrap(), Role::Set);
    assert!(Role::parse(tc.draw(gs::sampled_from(vec!["spare", "", "primaryx"]))).is_err());

    let serials = vec![None, Some("4C530001123456"), Some("0000000000"), Some("4C530001999999")];
    let hw = |tc: &TestCase| Hardware {
        model: tc.draw(gs::sampled_from(vec!["Cruzer", "Extreme"])).into(),
        serial: tc.draw(gs::sampled_from(serials.clone())).map(Into::into),
        volume_uuid: tc
            .draw(gs::optional(gs::sampled_from(vec!["ABCD-1234", "abcd-1234", "FFFF-0000"])))
            .map(Into::into),
        size: tc.draw(gs::sampled_from(vec![64u64, 128])),
    };
    let (a, b) = (hw(&tc), hw(&tc));
    assert_eq!(a.same_disk(&b), b.same_disk(&a));
    assert_eq!(a.same_disk(&b), a.serial.is_some() && a.serial == b.serial && a.model == b.model && a.size == b.size);
    let volume = |h: &Hardware| h.volume_uuid.as_ref().map(|v| v.to_lowercase());
    assert_eq!(a.same_volume(&b), volume(&a).is_some() && volume(&a) == volume(&b));

    // Copies of one drive: the hardware each is seen on now, and what its identity recorded.
    let dir = tempfile::tempdir().unwrap();
    let mut first = Device::open(common::blank(dir.path(), "d0")).unwrap();
    first.adopt(Some("Main")).unwrap();
    first.state_mut().unwrap().hardware = Some(a.clone());
    first.save_state().unwrap();
    let mut roots = vec![first.root().to_path_buf()];
    let mut seen = vec![a.clone()];
    for n in 1..tc.draw(gs::integers::<usize>().min_value(2).max_value(3)) {
        let root = dir.path().join(format!("d{n}"));
        common::copy_dir(first.root(), &root);
        roots.push(Device::open(&root).unwrap().root().to_path_buf());
        seen.push(hw(&tc));
    }
    let mounted: Vec<Device> = roots.iter().map(|r| Device::open(r).unwrap()).collect();
    let on = |root: &std::path::Path| roots.iter().position(|r| r == root).map(|i| seen[i].clone());
    let dupes = fleet::duplicates(&mounted, &on);
    assert_eq!(dupes.len(), 1);
    let hits: Vec<&std::path::PathBuf> =
        roots.iter().zip(&seen).filter(|(_, h)| h.same_disk(&a) || h.same_volume(&a)).map(|(r, _)| r).collect();
    assert_eq!(dupes[0].original.as_ref(), (hits.len() == 1).then(|| hits[0]));
}
