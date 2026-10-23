//! Model-based tests of the service layer's drive management: the registry (adopting, naming,
//! roles, retiring, forgetting, copies and the identities they carry, unplugging), checked after
//! every step against a model of the drives mounted and the drives registered; looking after a
//! drive (a contact, set recordings; `props_care` has the rest); checking and repairing a saved
//! drive's files; and setting up a new drive. Drives are folders in a volumes folder named for the
//! test's thread, and so is the store.

use crate::common::{self, Template, path};
use crate::props_fleet::{self, NAMES};

use indexmap::IndexMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, DriveRef};
use locallibrary::device::dunce_canonicalize;
use locallibrary::error::Reason;
use locallibrary::fleet::Role;
use locallibrary::progress::{CancelToken, Ctx, Event, Progress, Scope};
use locallibrary::store::{DriveRecord, Store};
use locallibrary::{Error, Result};
use uuid::Uuid;

// ---------------------------------------------------------------------------------------------
// The registry
// ---------------------------------------------------------------------------------------------

/// Roles as a request gives them: every name, in any case, the old `gig`, and one that isn't.
const ROLES: &[Option<&str>] =
    &[Some("primary"), Some(" Backup "), Some("GIG"), Some("set"), Some("ARCHIVE"), Some("headliner"), None];

/// A drive folder: in the volumes folder (mounted) or moved out of it (unplugged).
struct Slot {
    root: PathBuf,
    id: Uuid,
    name: String,
    mounted: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Rec {
    name: String,
    role: Option<Role>,
    retired: bool,
}

/// What a spec means, as the model works it out.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Means {
    /// The adopted drive mounted in this slot.
    Mounted(usize),
    /// The registry's record.
    Registered(Uuid),
    NotFound,
    /// Several drives answer to it.
    Refused,
}

struct Registry {
    _dir: tempfile::TempDir,
    store: Store,
    volumes: PathBuf,
    away: PathBuf,
    slots: Vec<Slot>,
    registry: IndexMap<Uuid, Rec>,
    folders: usize,
}

impl Registry {
    fn new() -> Registry {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("store")).unwrap();
        locallibrary::testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
        let (volumes, away) = (dir.path().join("volumes"), dir.path().join("away"));
        for d in [&volumes, &away] {
            std::fs::create_dir(d).unwrap();
        }
        // Drive roots come back canonical (`/private/var/...` on macOS).
        let (volumes, away) = (dunce_canonicalize(&volumes), dunce_canonicalize(&away));
        locallibrary::testing::volumes_on_this_thread(Some(vec![volumes.clone()]));
        let mut r =
            Registry { _dir: dir, store, volumes, away, slots: Vec::new(), registry: IndexMap::new(), folders: 0 };
        // Two drives to start with, one registered, so most steps have a drive to act on.
        r.adopt("Main", true);
        r.adopt("Club", false);
        r
    }

    /// A new, empty folder in the volumes folder: a blank drive.
    fn blank(&mut self) -> PathBuf {
        self.folders += 1;
        let root = self.volumes.join(format!("v{}", self.folders));
        std::fs::create_dir(&root).unwrap();
        root
    }

    /// Adopt a blank drive as `name`, registered or not.
    fn adopt(&mut self, name: &str, registered: bool) {
        let root = self.blank();
        let store = registered.then_some(&self.store);
        let info = api::adopt_drive(store, &api::AdoptDrive { drive: path(&root), name: Some(name.into()) }).unwrap();
        assert_eq!(info.name.as_deref(), Some(name));
        self.slots.push(Slot { root, id: info.id.expect("adopted"), name: name.into(), mounted: true });
        if registered {
            self.register(self.slots.len() - 1);
        }
    }

    fn mounted(&self) -> impl Iterator<Item = (usize, &Slot)> {
        self.slots.iter().enumerate().filter(|(_, s)| s.mounted)
    }

    fn is_mounted(&self, id: Uuid) -> bool {
        self.mounted().any(|(_, s)| s.id == id)
    }

    /// Note the slot's identity in the registry, as registering a drive does: its name changes,
    /// its role and whether it's retired stay.
    fn register(&mut self, i: usize) {
        let (id, name) = (self.slots[i].id, &self.slots[i].name);
        self.registry.entry(id).or_insert(Rec { name: name.clone(), role: None, retired: false }).name = name.clone();
    }

    /// Why a write to the slot's drive is refused, if it is: plans that write a drive refuse
    /// a retired or archive one (unless told to write to it).
    fn kept(&self, i: usize) -> Option<Reason> {
        let rec = self.registry.get(&self.slots[i].id)?;
        props_fleet::kept(rec.retired, rec.role)
    }

    /// Registered drives by name (any case), else by id prefix of 4 characters or more.
    fn records_named(&self, spec: &str) -> Vec<Uuid> {
        let lower = spec.trim().to_lowercase();
        let by_name: Vec<Uuid> =
            self.registry.iter().filter(|(_, r)| r.name.to_lowercase() == lower).map(|(id, _)| *id).collect();
        match by_name.is_empty() && lower.len() >= 4 {
            true => self.registry.keys().filter(|id| id.to_string().starts_with(&lower)).copied().collect(),
            false => by_name,
        }
    }

    fn find_record(&self, spec: &str) -> Means {
        match self.records_named(spec).as_slice() {
            [] => Means::NotFound,
            [id] => Means::Registered(*id),
            _ => Means::Refused,
        }
    }

    /// The drive a spec names, identity first: a mounted drive by its path, name or id prefix,
    /// else the registry's record.
    fn named(&self, spec: &str) -> Means {
        if let Some((i, _)) = self.mounted().find(|(_, s)| path(&s.root) == spec) {
            return Means::Mounted(i);
        }
        let lower = spec.to_lowercase();
        let matches: Vec<usize> = self
            .mounted()
            .filter(|(_, s)| {
                s.name.to_lowercase() == lower
                    || (spec.len() >= 4 && s.id.to_string().starts_with(&lower))
                    || s.root.file_name().unwrap().to_string_lossy().to_lowercase() == lower
            })
            .map(|(i, _)| i)
            .collect();
        match matches.as_slice() {
            [] => self.find_record(spec),
            [i] => Means::Mounted(*i),
            _ => Means::Refused,
        }
    }

    /// What `drive_find` (and `drive_log`, and `drive_forget` before it forgets) answers.
    fn find(&self, spec: &str) -> Means {
        match self.named(spec) {
            Means::Mounted(i) if self.registry.contains_key(&self.slots[i].id) => Means::Registered(self.slots[i].id),
            Means::Mounted(_) if self.records_named(spec).is_empty() => Means::NotFound,
            Means::Mounted(_) => Means::Refused,
            m => m,
        }
    }

    /// What the registry writes that take a drive (`drive_role`, `drive_retire`) act on,
    /// registering a mounted drive first.
    fn record(&mut self, spec: &str) -> Means {
        match self.named(spec) {
            Means::Mounted(i) => {
                self.register(i);
                Means::Registered(self.slots[i].id)
            }
            m => m,
        }
    }

    /// A spec for some drive: a mounted drive's path, a name, or an identity (whole, or its first
    /// eight characters).
    #[hegel::test_helper]
    fn spec(&self, tc: &TestCase) -> String {
        let ids: Vec<Uuid> = self.slots.iter().map(|s| s.id).chain(self.registry.keys().copied()).collect();
        let mounted: Vec<PathBuf> = self.mounted().map(|(_, s)| s.root.clone()).collect();
        match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 | 1 if !mounted.is_empty() => path(&tc.draw(gs::sampled_from(&mounted))),
            2 if !ids.is_empty() => {
                let id = tc.draw(gs::sampled_from(&ids)).to_string();
                if tc.draw(gs::booleans()) { id } else { id[..8].to_uppercase() }
            }
            _ => tc.draw(gs::sampled_from(NAMES)).to_string(),
        }
    }

    /// A mounted slot, if there is one.
    #[hegel::test_helper]
    fn mounted_slot(&self, tc: &TestCase) -> Option<usize> {
        let mounted: Vec<usize> = self.mounted().map(|(i, _)| i).collect();
        (!mounted.is_empty()).then(|| tc.draw(gs::sampled_from(mounted)))
    }

    /// `result` is the record `means` says, with what the model has for it; or the refusal it
    /// says.
    fn agrees(&self, what: &str, means: Means, result: Result<DriveRecord>) {
        match (means, result) {
            (Means::Registered(id), Ok(rec)) => {
                assert_eq!(rec.id, id, "{what}");
                let model = &self.registry[&id];
                assert_eq!(
                    (&rec.name, rec.role, rec.log.retired.is_some()),
                    (&model.name, model.role, model.retired),
                    "{what}"
                );
            }
            (Means::NotFound, Err(e)) => assert!(matches!(e.inner(), Error::NotFound(_)), "{what}: {e}"),
            (Means::Refused, Err(e)) => assert!(matches!(e.inner(), Error::Invalid(_)), "{what}: {e}"),
            (means, result) => panic!("{what}: the model says {means:?}, the API {result:?}"),
        }
    }

    /// The fleet view agrees with the model: the drives mounted, their records and copies, and
    /// (with `all`) the registered drives that aren't.
    fn check_fleet(&self, tc: &TestCase) {
        let (all, retired) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
        let st = api::fleet_status(Some(&self.store), &api::FleetRequest { all, retired }, &Ctx::silent()).unwrap();
        let mut roots: Vec<PathBuf> = st.mounted.iter().map(|d| d.status.drive.root.clone()).collect();
        let mut want: Vec<PathBuf> = self.mounted().map(|(_, s)| s.root.clone()).collect();
        roots.sort();
        want.sort();
        assert_eq!(roots, want, "the drives mounted");
        for d in &st.mounted {
            let slot = self.slots.iter().find(|s| s.mounted && s.root == d.status.drive.root).unwrap();
            assert_eq!(d.status.drive.id, Some(slot.id));
            assert_eq!(d.status.drive.name.as_deref(), Some(slot.name.as_str()));
            let rec = self.registry.get(&slot.id);
            assert_eq!((d.role, d.log.is_some()), (rec.and_then(|r| r.role), rec.is_some()), "{}", slot.name);
            let mut twins: Vec<PathBuf> = self
                .mounted()
                .filter(|(_, s)| s.id == slot.id && s.root != slot.root)
                .map(|(_, s)| s.root.clone())
                .collect();
            let mut same = d.same_identity_as.clone();
            twins.sort();
            same.sort();
            assert_eq!(same, twins, "the copies of {}", slot.name);
            assert!(!d.read_only && d.exported_over_edits == 0);
        }
        let mut shared: Vec<Uuid> = st.duplicates.iter().map(|d| d.id).collect();
        let mut want: Vec<Uuid> = self.registry.keys().chain(self.slots.iter().map(|s| &s.id)).copied().collect();
        want.sort();
        want.dedup();
        want.retain(|id| self.mounted().filter(|(_, s)| s.id == *id).count() > 1);
        shared.sort();
        assert_eq!(shared, want, "the identities several mounted drives hold");
        let mut offline: Vec<Uuid> = st.offline.iter().map(|k| k.id).collect();
        let mut want: Vec<Uuid> = (self.registry.iter())
            .filter(|(id, r)| all && (retired || !r.retired) && !self.is_mounted(**id))
            .map(|(id, _)| *id)
            .collect();
        offline.sort();
        want.sort();
        assert_eq!(offline, want, "the registered drives not mounted (all: {all}, retired: {retired})");
        assert!(st.libraries.is_empty());
        assert!(api::drive_sets(&self.store).unwrap().is_empty(), "no drive is in a library");
    }

    /// The registry agrees with the model, and so does what a spec finds.
    fn check(&self, tc: &TestCase) {
        let retired = tc.draw(gs::booleans());
        let req = api::FleetRequest { all: true, retired };
        let listed = |r: &Rec| retired || !r.retired;
        let known = api::drives_known(&self.store, &req).unwrap();
        let mut ids: Vec<Uuid> = known.iter().map(|k| k.id).collect();
        let mut want: Vec<Uuid> = self.registry.iter().filter(|(_, r)| listed(r)).map(|(id, _)| *id).collect();
        ids.sort();
        want.sort();
        assert_eq!(ids, want, "the registered drives");
        for k in &known {
            let r = &self.registry[&k.id];
            assert_eq!((&k.name, k.role, k.mounted.is_some()), (&r.name, r.role, self.is_mounted(k.id)));
        }
        assert!(known.windows(2).all(|w| w[0].mounted.is_some() || w[1].mounted.is_none()), "mounted ones first");

        let mounted: Vec<PathBuf> = api::drives_mounted().into_iter().map(|m| m.root).collect();
        assert!(self.mounted().all(|(_, s)| mounted.contains(&s.root)), "{mounted:?}");

        let spec = self.spec(tc);
        let drive = DriveRef { drive: spec.clone() };
        self.agrees(&format!("drive_find({spec:?})"), self.find(&spec), api::drive_find(&self.store, &drive));
        self.agrees(&format!("drive_log({spec:?})"), self.find(&spec), api::drive_log(&self.store, &drive));
        // What a role would change, written nowhere: a mounted drive the registry doesn't have
        // shows as it would be registered.
        let shown = api::drive_to_change(&self.store, &drive);
        match self.named(&spec) {
            Means::Mounted(i) if !self.registry.contains_key(&self.slots[i].id) => {
                let rec = shown.unwrap();
                assert_eq!((rec.id, &rec.name, rec.role), (self.slots[i].id, &self.slots[i].name, None));
                assert!(!self.store.drives().unwrap().contains_key(&rec.id), "nothing is written");
            }
            Means::Mounted(i) => self.agrees("drive_to_change", Means::Registered(self.slots[i].id), shown),
            m => self.agrees("drive_to_change", m, shown),
        }
    }
}

#[hegel::state_machine]
impl Registry {
    /// Adopting a blank drive gives it an identity, registered when there's a store.
    #[rule(weight = 2)]
    fn adopt_new(&mut self, tc: TestCase) {
        let name = tc.draw(gs::sampled_from(NAMES)).to_string();
        let registered = tc.draw(gs::weighted_booleans(0.7));
        self.adopt(&name, registered);
    }

    /// A rename is planned (an empty name refused, the same name nothing to do, an archive or
    /// retired drive kept as it is, the volume label it would get) and renames the drive, and
    /// its record when there's a store; a volume label can't be written to a folder.
    #[rule]
    fn rename(&mut self, tc: TestCase) {
        tc.assume(self.mounted().next().is_some());
        let spec = self.spec(&tc);
        let pad = tc.draw(gs::sampled_from(&["", " ", "\t"][..]));
        let to = match tc.draw(gs::weighted_booleans(0.1)) {
            true => pad.to_string(),
            false => format!("{pad}{}{pad}", tc.draw(gs::sampled_from(NAMES))),
        };
        let with_store = tc.draw(gs::booleans());
        let label = tc.draw(gs::weighted_booleans(0.2));
        let req = api::RenameDrive { drive: spec.clone(), name: to.clone(), label };
        let plan = api::drive_rename_plan(&req);
        // A drive's name, as `resolve_one_drive` takes it: an adopted drive's name or a volume's.
        let target = match self.named(&spec) {
            Means::Mounted(i) => Some(i),
            _ => None,
        };
        let Some(i) = target.filter(|_| !to.trim().is_empty()) else {
            assert!(plan.is_err(), "renaming {spec:?} to {to:?}");
            return;
        };
        if let Some(why) = self.kept(i) {
            assert_eq!(plan.expect_err("refused").reason(), Some(why));
            return;
        }
        let plan = plan.unwrap();
        assert_eq!(plan.noop, to.trim() == self.slots[i].name && !label, "{:?}", plan.summary);
        assert_eq!((plan.from.as_str(), plan.to.as_str()), (self.slots[i].name.as_str(), to.trim()));
        let store = with_store.then_some(&self.store);
        if label {
            // The volume label it would get, which can't be written to a folder.
            assert_eq!(plan.label, Some(locallibrary::fleet::volume_label(to.trim())));
            let e = plan.apply(store, &Ctx::silent()).expect_err("refused");
            assert!(e.to_string().contains("LOCALLIBRARY_VOLUMES"), "{e}");
            return;
        }
        let report = match tc.draw(gs::booleans()) {
            true => plan.apply(store, &Ctx::silent()).unwrap(),
            false => api::drive_rename(store, &req).unwrap(),
        };
        assert_eq!(
            (report.id, report.old_name.as_str(), report.label),
            (self.slots[i].id, self.slots[i].name.as_str(), None)
        );
        self.slots[i].name = to.trim().to_string();
        if with_store {
            self.register(i);
        }
    }

    /// A role is set on the drive named (registering a mounted one), an unknown one refused
    /// before anything is written; a library has one primary.
    #[rule]
    fn set_role(&mut self, tc: TestCase) {
        tc.assume(!self.slots.is_empty() || !self.registry.is_empty());
        let spec = self.spec(&tc);
        let role = tc.draw(gs::sampled_from(ROLES));
        let got = api::drive_role(&self.store, &api::SetRole { drive: spec.clone(), role: role.map(Into::into) });
        let parsed = match role.map(Role::parse).transpose() {
            Ok(r) => r,
            Err(_) => {
                let e = got.unwrap_err();
                assert!(e.to_string().contains("unknown role"), "{e}");
                return;
            }
        };
        let means = self.record(&spec);
        if let Means::Registered(id) = means {
            // Every drive here is in no library, so they share one primary.
            props_fleet::demote(self.registry.values_mut().map(|r| &mut r.role), parsed);
            self.registry[&id].role = parsed;
        }
        self.agrees(&format!("drive_role({spec:?}, {role:?})"), means, got);
    }

    /// Retiring (or, undone, bringing back) the drive named, registering a mounted one.
    #[rule]
    fn retire(&mut self, tc: TestCase) {
        tc.assume(!self.slots.is_empty() || !self.registry.is_empty());
        let spec = self.spec(&tc);
        let undo = tc.draw(gs::booleans());
        let got = api::drive_retire(&self.store, &api::RetireDrive { drive: spec.clone(), undo });
        let means = self.record(&spec);
        if let Means::Registered(id) = means {
            self.registry[&id].retired = !undo;
        }
        self.agrees(&format!("drive_retire({spec:?}, {undo})"), means, got);
    }

    /// Forgetting takes a registered drive out of the registry (its own identity file stays).
    #[rule]
    fn forget(&mut self, tc: TestCase) {
        tc.assume(!self.slots.is_empty() || !self.registry.is_empty());
        let spec = self.spec(&tc);
        let means = self.find(&spec);
        let got = api::drive_forget(&self.store, &DriveRef { drive: spec.clone() });
        self.agrees(&format!("drive_forget({spec:?})"), means, got);
        if let Means::Registered(id) = means {
            self.registry.shift_remove(&id);
        }
    }

    /// A drive unplugged (its folder moved out of the volumes folder), or plugged back in.
    #[rule(weight = 2)]
    fn unplug_or_plug(&mut self, tc: TestCase) {
        tc.assume(!self.slots.is_empty());
        let i = tc.draw(gs::integers::<usize>().max_value(self.slots.len() - 1));
        let s = &mut self.slots[i];
        let to = match s.mounted {
            true => self.away.join(s.root.file_name().unwrap()),
            false => self.volumes.join(s.root.file_name().unwrap()),
        };
        std::fs::rename(&s.root, &to).unwrap();
        s.root = to;
        s.mounted = !s.mounted;
    }

    /// A drive's files copied onto another: the copy carries its identity file along.
    #[rule]
    fn copy(&mut self, tc: TestCase) {
        let i = self.mounted_slot(&tc);
        tc.assume(i.is_some());
        let i = i.unwrap();
        let root = self.volumes.join(format!("v{}", self.folders + 1));
        self.folders += 1;
        common::copy_dir(&self.slots[i].root, &root);
        let (id, name) = (self.slots[i].id, self.slots[i].name.clone());
        self.slots.push(Slot { root, id, name, mounted: true });
    }

    /// A copy (or, forced, any drive) gets an identity of its own, by its path only; the role
    /// stays with the identity, which the plan warns about.
    #[rule]
    fn reidentify(&mut self, tc: TestCase) {
        let i = self.mounted_slot(&tc);
        tc.assume(i.is_some());
        let i = i.unwrap();
        let force = tc.draw(gs::booleans());
        let name = tc.draw(gs::optional(gs::sampled_from(NAMES))).map(str::to_string);
        let with_store = tc.draw(gs::booleans());
        let store = with_store.then_some(&self.store);
        let by_name = api::Reidentify { drive: self.slots[i].name.clone(), name: None, force: true };
        assert!(api::drive_reidentify_plan(store, &by_name).unwrap_err().to_string().contains("mount path"));
        let req = api::Reidentify { drive: path(&self.slots[i].root), name: name.clone(), force };
        let old = self.slots[i].id;
        let copy = self.mounted().any(|(j, s)| j != i && s.id == old);
        let plan = api::drive_reidentify_plan(store, &req);
        if !force && !copy {
            let e = plan.expect_err("refused");
            assert_eq!(e.reason(), Some(Reason::HasIdentity), "{e}");
            return;
        }
        if let Some(why) = self.kept(i) {
            assert_eq!(plan.expect_err("refused").reason(), Some(why));
            return;
        }
        let plan = plan.unwrap();
        let role = self.registry.get(&old).and_then(|r| r.role).filter(|_| with_store);
        assert_eq!(plan.warnings.len(), usize::from(role.is_some()), "{:?}", plan.warnings);
        let name = name.unwrap_or_else(|| format!("{} (copy)", self.slots[i].name));
        assert_eq!((plan.plan.id, &plan.plan.name), (old, &name));
        let info = match tc.draw(gs::booleans()) {
            true => plan.apply(store, &Ctx::silent()).unwrap(),
            false => api::drive_reidentify(store, &req).unwrap(),
        };
        let id = info.id.unwrap();
        assert!(id != old && self.slots.iter().all(|s| s.id != id), "a new identity");
        assert_eq!(info.name.as_deref(), Some(name.as_str()));
        self.slots[i].id = id;
        self.slots[i].name = name;
        if with_store {
            self.register(i);
        }
    }

    /// A drive (a blank one, or one adopted already) takes a registered drive's identity: refused
    /// for a retired or archive drive, while a mounted drive holds it, or when the drive has
    /// another of its own.
    #[rule]
    fn adopt_as(&mut self, tc: TestCase) {
        tc.assume(!self.registry.is_empty());
        let target = match tc.draw(gs::booleans()) {
            true => self.mounted_slot(&tc),
            false => None,
        };
        let root = match target {
            Some(i) => self.slots[i].root.clone(),
            None => self.blank(),
        };
        // Mostly a registered drive, often one that isn't plugged in: a drive that lost its
        // identity file, plugged in again.
        let away: Vec<Uuid> = self.registry.keys().filter(|id| !self.is_mounted(**id)).copied().collect();
        let ids: Vec<Uuid> = match away.is_empty() || tc.draw(gs::booleans()) {
            true => self.registry.keys().copied().collect(),
            false => away,
        };
        let known = match ids.is_empty() || tc.draw(gs::weighted_booleans(0.3)) {
            true => self.spec(&tc),
            false => {
                let id = tc.draw(gs::sampled_from(&ids));
                match tc.draw(gs::booleans()) {
                    true => self.registry[&id].name.clone(),
                    false => id.to_string()[..8].to_string(),
                }
            }
        };
        let got = api::drive_adopt_as(
            &self.store,
            &api::AdoptAs { drive: path(&root), known: known.clone() },
            &Ctx::silent(),
        );
        let what = format!("adopting {} as {known:?}", root.display());
        let id = match self.find_record(&known) {
            Means::Registered(id) => id,
            means => {
                let e = got.expect_err("refused");
                match means {
                    Means::NotFound => assert!(matches!(e.inner(), Error::NotFound(_)), "{what}: {e}"),
                    _ => assert!(matches!(e.inner(), Error::Invalid(_)), "{what}: {e}"),
                }
                return;
            }
        };
        // Applying a plan refuses to write a retired or archive drive first.
        if let Some(why) = target.and_then(|i| self.kept(i)) {
            assert_eq!(got.expect_err("refused").reason(), Some(why), "{what}");
            return;
        }
        if self.mounted().any(|(j, s)| s.id == id && Some(j) != target) {
            assert_eq!(got.expect_err("refused").reason(), Some(Reason::IdentityInUse), "{what}");
            return;
        }
        if target.is_some_and(|i| self.slots[i].id != id) {
            assert_eq!(got.expect_err("refused").reason(), Some(Reason::HasIdentity), "{what}");
            return;
        }
        let info = got.unwrap();
        let name = self.registry[&id].name.clone();
        assert_eq!((info.id, info.name.as_deref()), (Some(id), Some(name.as_str())), "{what}");
        match target {
            Some(i) => self.slots[i].name = name,
            None => self.slots.push(Slot { root, id, name, mounted: true }),
        }
    }

    /// The registry, and what a spec finds, agree with the model after every step.
    #[invariant(always_run)]
    fn the_registry_agrees(&self, tc: TestCase) {
        self.check(&tc);
    }

    /// So does the fleet view, now and then (each mounted drive costs it a call to the OS's disk
    /// tool).
    #[invariant]
    fn the_fleet_agrees(&self, tc: TestCase) {
        self.check_fleet(&tc);
    }
}

/// Random sequences of registry operations through the service layer (adopting drives, with
/// a store or without; renaming, roles, retiring, forgetting; unplugging and plugging drives
/// back in; copying a drive, identity file and all, and giving copies identities of their own or
/// a known drive's) agree with a model of the mounted drives and the registry: the fleet view,
/// the registered drives and what every kind of spec (a path, a name, an id or its prefix)
/// finds.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn registry_operations_agree_with_a_model(tc: TestCase) {
    hegel::stateful::machine(Registry::new()).steps(10).run(tc);
}

// ---------------------------------------------------------------------------------------------
// Looking after a drive
// ---------------------------------------------------------------------------------------------

/// rekordcrate's demo export.
pub(crate) fn demo_export() -> &'static Template {
    static DEMO: OnceLock<Template> = OnceLock::new();
    DEMO.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        Template::of(&common::demo_drive(dir.path(), "Main"))
    })
}

/// AppleDouble's magic number, which a `._` file macOS writes starts with.
const APPLE_DOUBLE: &[u8] = &[0x00, 0x05, 0x16, 0x07, 0, 0, 0, 0];

/// Contacts as a request gives them, valid or not.
const CONTACTS: &[&str] = &["dj@example.com", "  +44 7700 900123 ", "Sam/Alex", "", "   ", "line\nbreak"];

/// A blank drive, Main, in a volumes folder named for this thread, with no store.
fn blank_drive() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::blank(&common::isolate(dir.path(), None), "Main");
    (dir, root)
}

/// A contact is refused when it's blank or holds a control character; otherwise it's written
/// (replacing the one there, which the plan says) onto the demo export, with its playlist when
/// asked.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_contact_is_written_with_its_playlist(tc: TestCase) {
    let (_dir, root) = blank_drive();
    let root: &Path = &demo_export().write(&root);
    let file = root.join(locallibrary::player::CONTACT_FILE);
    let had = tc.draw(gs::booleans());
    if had {
        std::fs::write(&file, b"an old contact").unwrap();
    }
    let contact = tc.draw(gs::sampled_from(CONTACTS)).to_string();
    let playlist = tc.draw(gs::booleans());
    let req = api::SetContact { drive: path(root), contact: contact.clone(), playlist };
    let plan = api::drive_contact_plan(&req, &Ctx::silent());
    let valid = !contact.trim().is_empty() && !contact.chars().any(char::is_control);
    assert_eq!(plan.is_ok(), valid, "{contact:?}");
    let Ok(plan) = plan else { return };
    let named = format!("IF FOUND: {}", contact.trim().replace('/', "-"));
    assert_eq!((plan.plan.replaces, plan.plan.playlist.as_deref()), (had, playlist.then_some(named.as_str())));
    let report = api::drive_contact(&req, &Ctx::silent()).unwrap();
    assert_eq!(report.playlist, plan.plan.playlist);
    assert!(std::fs::read_to_string(&file).unwrap().contains(contact.trim()));
    let lists: Vec<String> = locallibrary::device::Device::open(root)
        .unwrap()
        .library()
        .unwrap()
        .playlists
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(lists.contains(&named), playlist, "{lists:?}");
}

/// Recordings are listed oldest first, `._` files and other files left out; collecting the ones
/// asked for (by path, in any case, or all of them) copies each, deleting it from the drive only
/// when asked, and one that isn't there is refused before anything is copied.
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn recordings_are_listed_and_collected_as_asked(tc: TestCase) {
    let (dir, root) = blank_drive();
    let root: &Path = &root;
    let sets = &dir.path().join("Sets");
    let folder = root.join("ALPHATHETA REC");
    std::fs::create_dir_all(&folder).unwrap();
    let now = std::time::SystemTime::now();
    let mut on_drive = Vec::new();
    for (i, name) in ["REC001.WAV", "REC002.WAV", "rec003.wav"].into_iter().enumerate() {
        if tc.draw(gs::booleans()) {
            let p = folder.join(name);
            std::fs::write(&p, common::media::wav_of(8_000, 16, 1, 0.05 * (i + 1) as f64)).unwrap();
            let ended = now - std::time::Duration::from_secs(3600 * (3 - i as u64));
            std::fs::File::options().write(true).open(&p).unwrap().set_modified(ended).unwrap();
            on_drive.push(format!("ALPHATHETA REC/{name}"));
        }
    }
    std::fs::write(folder.join("._REC001.WAV"), APPLE_DOUBLE).unwrap();
    std::fs::write(folder.join("notes.txt"), b"set list").unwrap();
    let listed: Vec<String> =
        api::recordings_list(&DriveRef { drive: path(root) }).unwrap().into_iter().map(|r| r.path).collect();
    assert_eq!(listed, on_drive, "oldest first");

    let mut only: Vec<String> = Vec::new();
    for r in &on_drive {
        if tc.draw(gs::booleans()) {
            let spelled = if tc.draw(gs::booleans()) { r.to_lowercase() } else { format!("/{r}") };
            only.push(spelled);
        }
    }
    let missing = tc.draw(gs::weighted_booleans(0.2));
    if missing {
        only.push("ALPHATHETA REC/REC009.WAV".into());
    }
    let delete = tc.draw(gs::booleans());
    let options = locallibrary::recordings::CollectOptions {
        to: sets.to_path_buf(),
        only: only.clone(),
        tracklist: Default::default(),
        delete,
    };
    let req = api::CollectRecordings { drive: path(root), options };
    let plan = api::recordings_collect_plan(&req);
    if missing {
        assert!(matches!(plan, Err(Error::NotFound(_))), "{only:?}");
        return;
    }
    let plan = plan.unwrap();
    let chosen: Vec<String> = match only.is_empty() {
        true => on_drive.clone(),
        false => only
            .iter()
            .map(|o| on_drive.iter().find(|r| r.eq_ignore_ascii_case(o.trim_start_matches('/'))).unwrap().clone())
            .collect(),
    };
    let planned: Vec<String> = plan.plan.recordings.iter().map(|r| r.path.clone()).collect();
    assert_eq!(planned, chosen);
    let sizes: u64 = chosen.iter().map(|r| std::fs::metadata(root.join(r)).unwrap().len()).sum();
    assert_eq!((plan.plan.bytes, plan.noop, plan.destructive), (sizes, chosen.is_empty(), delete));
    let originals: Vec<Vec<u8>> = chosen.iter().map(|r| std::fs::read(root.join(r)).unwrap()).collect();
    let report = api::recordings_collect(&req, &Ctx::silent()).unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(report.collected.iter().map(|c| c.recording.clone()).collect::<Vec<_>>(), chosen);
    for (c, bytes) in report.collected.iter().zip(&originals) {
        assert_eq!(&std::fs::read(&c.copied_to).unwrap(), bytes, "{}", c.recording);
        assert_eq!(root.join(&c.recording).exists(), !delete, "{}", c.recording);
    }
}

// ---------------------------------------------------------------------------------------------
// Checking a drive's files
// ---------------------------------------------------------------------------------------------

/// The demo export adopted as Main and saved in a store: the store's files and the drive's,
/// made once.
pub(crate) fn saved_demo() -> &'static (Template, Template) {
    static SAVED: OnceLock<(Template, Template)> = OnceLock::new();
    SAVED.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("store")).unwrap();
        let root = common::demo_drive(dir.path(), "Main");
        // Written well before the pull, so the snapshot tells a rewrite since by its later mtime.
        common::age(&root);
        common::adopt(Some(&store), &root, "Main");
        let pull = api::PullRequest { drive: path(&root), ..Default::default() };
        api::pull(&store, &pull, &Ctx::silent()).unwrap();
        (Template::of(store.root()), Template::of(&root))
    })
}

/// A copy of the saved demo: a store named for this thread and Main mounted in `volumes`.
pub(crate) fn saved_copy(dir: &Path) -> (Store, PathBuf) {
    let (store, drive) = saved_demo();
    let store = Store::open(store.write(&dir.join("store"))).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    let volumes = dir.join("volumes");
    let root = drive.write(&volumes.join("Main"));
    locallibrary::testing::volumes_on_this_thread(Some(vec![dunce_canonicalize(&volumes)]));
    (store, dunce_canonicalize(&root))
}

/// Damaging and deleting a saved drive's audio and analysis files: a check against the store
/// finds exactly the files damaged and missing, and notes them in the drive's logbook; repairing
/// them puts back every byte, as a write; then the check is clean. Around it, the drive's state
/// is recognised as the snapshot saved, its space is weighed with the store and without, and checking
/// its file system or ejecting it (refused for a folder) leaves the logbook as it was.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_check_finds_the_damage_done_and_a_repair_mends_it(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (store, root) = saved_copy(dir.path());
    let ctx = Ctx::silent();
    let drive = DriveRef { drive: path(&root) };
    let id = locallibrary::device::Device::open(&root).unwrap().id().unwrap();
    let log = || store.drives().unwrap()[&id].log.clone();

    let seen = api::drive_recognise(Some(&store), &drive, &ctx).unwrap();
    assert_eq!(seen.snapshot.as_ref().map(|s| s.library.as_str()), Some("Main"));
    assert!(seen.best().is_none(), "its own identity isn't another drive's");

    let files = common::library_files(&root);
    let audio_and_analysis: Vec<String> = files
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| {
            let class = locallibrary::paths::classify(k);
            matches!(class, Some(locallibrary::paths::FileClass::Audio | locallibrary::paths::FileClass::Analysis))
        })
        .collect();
    let (mut damaged, mut missing) = crate::drawn::damage(&tc, &root, &audio_and_analysis, &audio_and_analysis);
    let verify = |snapshot: Option<&str>, store: Option<&Store>| {
        let req =
            api::VerifyRequest { drive: path(&root), snapshot: snapshot.map(Into::into), against: vec![path(&root)] };
        api::drive_verify(store, &req, &ctx)
    };
    assert!(matches!(verify(Some("abcd"), None), Err(Error::Invalid(_))), "a snapshot needs the store");
    assert!(verify(Some("ffffffff"), Some(&store)).is_err(), "no such snapshot");
    let report = verify(None, Some(&store)).unwrap();
    let keys = |p: &[locallibrary::scrub::FileProblem]| {
        let mut k: Vec<String> = p.iter().map(|p| p.path.clone()).collect();
        k.sort();
        k
    };
    damaged.sort();
    missing.sort();
    // The state's fingerprint covers every library file but the audio's content: any other
    // change, and a missing file, means no snapshot holds exactly this state. The check still
    // finds every file gone, by the snapshot the drive last held, whose databases it still has.
    let audio = |k: &String| locallibrary::paths::classify(k) == Some(locallibrary::paths::FileClass::Audio);
    let exact = missing.is_empty() && damaged.iter().all(audio);
    assert_eq!(report.snapshot.is_some(), exact, "the snapshot of exactly this state");
    assert_eq!((keys(&report.damaged), keys(&report.missing)), (damaged.clone(), missing.clone()));
    assert!(report.unreadable.is_empty());
    let clean = damaged.is_empty() && missing.is_empty();
    let kinds: Vec<&str> = [(!damaged.is_empty(), "damaged_files"), (!missing.is_empty(), "missing_files")]
        .into_iter()
        .filter_map(|(n, k)| n.then_some(k))
        .collect();
    let noted = log();
    assert_eq!(noted.last_scrub.as_ref().map(|c| c.ok), Some(clean));
    assert_eq!(noted.problems.iter().map(|p| p.kind.as_str()).collect::<Vec<_>>(), kinds);

    let problems: Vec<_> = report.damaged.into_iter().chain(report.missing).collect();
    let req = api::RepairRequest { drive: path(&root), problems, against: Vec::new() };
    let repaired = api::drive_repair(Some(&store), &req, &ctx).unwrap();
    let mut done = repaired.repaired.clone();
    done.sort();
    let mut want: Vec<String> = damaged.iter().chain(&missing).cloned().collect();
    want.sort();
    assert_eq!(done, want);
    assert!(repaired.failed.is_empty(), "{:?}", repaired.failed);
    assert_eq!(log().writes, 1, "a repair is a write");
    assert_eq!(common::library_files(&root), files, "every byte is back");
    assert!(verify(None, Some(&store)).unwrap().is_clean());
    assert_eq!(log().last_scrub.map(|c| c.ok), Some(true));

    let with = api::drive_space(Some(&store), &drive, &ctx).unwrap();
    let without = api::drive_space(None, &drive, &ctx).unwrap();
    let sizes =
        |r: &locallibrary::space::SpaceReport| r.categories.iter().map(|c| (c.kind, c.bytes)).collect::<Vec<_>>();
    assert_eq!(sizes(&with), sizes(&without));
    assert!(with.fits.is_empty() && without.fits.is_empty(), "no other drive of its library");
    assert!(with.forecast.is_none(), "one snapshot is no trend");

    let before = log();
    let fs = api::drive_fs_check(Some(&store), &api::FsCheckRequest { drive: path(&root), repair: false });
    assert!(fs.unwrap_err().to_string().contains("LOCALLIBRARY_VOLUMES"));
    assert!(api::drive_eject(Some(&store), &api::EjectDrives { drives: vec![path(&root)] }).is_err());
    let after = log();
    assert_eq!(
        (after.clean_ejects, after.last_fs_check.is_some(), after.problems.len()),
        (before.clean_ejects, false, before.problems.len())
    );
}

// ---------------------------------------------------------------------------------------------
// Setting up a new drive
// ---------------------------------------------------------------------------------------------

/// Roles a request may give a new drive.
const NEW_ROLES: &[Option<&str>] = &[None, Some("backup"), Some("Archive")];

/// Setting up a drive (blank, or holding an export it replaces, which the plan warns about;
/// pushed from the store's one library, named or not, or copied from Main) plans exactly the
/// steps the request asks for, in order, and refuses what can't be done before anything is
/// written (an unknown role or library; formatting, with the machine's disks hidden). Applied,
/// it runs them until the first that can't run here (the name players show, with no OneLibrary
/// database in the library; ejecting a folder), and leaves the drive named, registered with its
/// role and holding Main's library, whatever library it held before.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn setting_up_a_drive_runs_the_steps_asked_for(tc: TestCase) {
    // The plan's warnings are checked exactly, so the platform's own (ejecting is untested on
    // Linux and Windows) would fail the cases that eject there: answer as macOS on every platform.
    locallibrary::testing::os_on_this_thread(Some("macos"));
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = saved_copy(dir.path());
    let target = dunce_canonicalize(&common::blank(main.parent().unwrap(), "New"));
    // A request that doesn't say writes My Settings and checks the drive afterwards.
    let unsaid: api::ProvisionRequest = serde_json::from_value(serde_json::json!({ "target": path(&target) })).unwrap();
    assert!(unsaid.settings && unsaid.verify && !unsaid.format && !unsaid.eject);
    // A drive holding an export already, which the new library replaces.
    let over = tc.draw(gs::weighted_booleans(0.3));
    if over {
        let pdb = target.join(locallibrary::paths::EXPORT_PDB);
        std::fs::create_dir_all(pdb.parent().unwrap()).unwrap();
        std::fs::copy(main.join(locallibrary::paths::EXPORT_PDB), pdb).unwrap();
    }
    let like = tc.draw(gs::booleans());
    let library = match tc.draw(gs::weighted_booleans(0.15)) {
        true => Some("Radio"),
        false => tc.draw(gs::sampled_from(&[None, Some("main")][..])),
    };
    let name = tc.draw(gs::sampled_from(&[None, Some("Spare")][..]));
    let role = match tc.draw(gs::weighted_booleans(0.15)) {
        true => Some("dj"),
        false => tc.draw(gs::sampled_from(NEW_ROLES)),
    };
    let player = tc.draw(gs::weighted_booleans(0.3));
    let (settings, profile) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let (verify, eject, format) =
        (tc.draw(gs::booleans()), tc.draw(gs::weighted_booleans(0.3)), tc.draw(gs::weighted_booleans(0.1)));
    if profile {
        let values = IndexMap::from([("quantize".to_string(), "off".to_string())]);
        api::settings_set(&store, &api::SetSettings { values, drive: None }).unwrap();
    }
    let req = api::ProvisionRequest {
        target: path(&target),
        like: like.then(|| path(&main)),
        library: library.map(Into::into),
        name: name.map(Into::into),
        format,
        format_options: Default::default(),
        role: role.map(Into::into),
        player_name: player.then(|| "SPARE".into()),
        player_colour: player.then_some(2),
        settings,
        verify,
        eject,
    };
    let ctx = Ctx::silent();
    let plan = api::provision_plan(&store, &req, &ctx);
    if role == Some("dj") {
        assert!(plan.expect_err("refused").to_string().contains("unknown role"));
        return;
    }
    if format {
        assert!(plan.expect_err("refused").to_string().contains("LOCALLIBRARY_VOLUMES"));
        return;
    }
    if !like && library == Some("Radio") {
        assert!(matches!(plan, Err(Error::NotFound(m)) if m.contains("Radio")));
        return;
    }
    let plan = plan.unwrap();
    let it = format!("the drive at {}", target.display());
    let mut want = vec![match name {
        Some(n) => format!("give {it} the name {n:?}"),
        None => format!("give {it} an identity"),
    }];
    want.push(match like {
        true => "make it another copy of \"Main\", from Main".to_string(),
        false => plan.plan.steps[1].clone(),
    });
    if !like {
        let step = &want[1];
        assert!(
            step.starts_with("write \"Main\"'s latest snapshot (") && step.ends_with(", 2 tracks) onto it"),
            "{step}"
        );
    }
    if settings && profile {
        want.push("write the My Settings profile".into());
    }
    match role {
        Some("backup") => want.push("mark it as a backup drive".into()),
        Some(_) => want.push("mark it as an archive drive".into()),
        None => {}
    }
    if player {
        want.push("set the name and colour players show".into());
    }
    if verify {
        want.push("read every file back and check it".into());
    }
    if eject {
        want.push("eject it".into());
    }
    assert_eq!(plan.plan.steps, want);
    let warned = over.then(|| "New already holds a rekordbox library; it will be replaced".to_string());
    assert_eq!((plan.destructive, plan.warnings.first()), (over, warned.as_ref()), "{:?}", plan.warnings);

    let report = api::provision_apply(&store, &req, None, &ctx).unwrap();
    let stopped = match (player, eject) {
        (true, _) => Some("set the player name"),
        (false, true) => Some("eject it"),
        (false, false) => None,
    };
    assert_eq!(report.stopped.as_ref().map(|s| s.step.as_str()), stopped, "{:?}", report.stopped);
    assert!(report.copy.is_some());
    assert_eq!(report.settings.is_some(), settings && profile);
    assert_eq!(report.verify.as_ref().map(|v| v.is_clean()), (verify && !player).then_some(true));
    let named = name.unwrap_or("New");
    assert_eq!(report.drive.as_ref().map(|d| d.name.as_deref() == Some(named)), (!player).then_some(true));
    let d = locallibrary::device::Device::open(&target).unwrap();
    let rec = store.drives().unwrap()[&d.id().expect("adopted")].clone();
    let role = role.map(|r| Role::parse(r).unwrap());
    assert_eq!((rec.name.as_str(), rec.role, rec.library_id), (named, role, d.state().unwrap().library_id));
    let titles = |root: &Path| -> Vec<String> {
        locallibrary::device::Device::open(root)
            .unwrap()
            .library()
            .unwrap()
            .tracks
            .into_iter()
            .map(|t| t.title)
            .collect()
    };
    assert_eq!(titles(&target), titles(&main));
}

/// What goes wrong part way through setting up a drive.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mishap {
    /// The new drive is pulled out (moved out of the volumes folder).
    Unplugged,
    /// The user cancels.
    Cancelled,
}

/// Makes `mishap` happen as the step `step` begins, once.
struct MishapAt<'a> {
    step: &'static str,
    mishap: Mishap,
    root: &'a Path,
    away: &'a Path,
    cancel: CancelToken,
    happened: AtomicBool,
}

impl Progress for MishapAt<'_> {
    fn on_event(&self, _: &Event) {}
    fn on_step(&self, scope: &Scope) {
        if scope.step.is_none_or(|s| s.id != self.step) || self.happened.swap(true, Ordering::SeqCst) {
            return;
        }
        match self.mishap {
            Mishap::Unplugged => std::fs::rename(self.root, self.away).unwrap(),
            Mishap::Cancelled => self.cancel.cancel(),
        }
    }
}

/// Setting up a drive (its library pushed from the store or copied from Main, with My Settings,
/// a role, a check and an eject each asked for or not) that loses the drive as one of its steps
/// begins stops at that step, saying which, with the steps before it done and reported and none
/// after: naming it, writing or copying the library, the settings, the role, the check, the eject.
/// Cancelled as a step begins, it ends cancelled when that step or one after it copies or checks
/// files, and finishes otherwise (stopping at the eject, as a folder can't be ejected). Pushed
/// from a library the store hasn't got, or one with nothing saved, it's refused before it starts.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(like = false, settings = true, role = true, verify = true, eject = true, at = 5usize, mishap = Mishap::Unplugged)]
#[hegel::explicit_test_case(like = true, settings = true, role = true, verify = true, eject = false, at = 1usize, mishap = Mishap::Unplugged)]
fn setting_up_stops_at_the_step_the_drive_went_away_in(tc: TestCase) {
    let like = tc.draw(gs::booleans());
    let settings = tc.draw(gs::booleans());
    let role = tc.draw(gs::booleans());
    let verify = tc.draw(gs::booleans());
    let eject = tc.draw(gs::booleans());
    let running: Vec<&'static str> = [
        ("name", true),
        ("library", true),
        ("settings", settings),
        ("role", role),
        ("verify", verify),
        ("eject", eject),
    ]
    .into_iter()
    .filter_map(|(id, runs)| runs.then_some(id))
    .collect();
    let at = tc.draw(gs::integers::<usize>().max_value(running.len() - 1));
    let mishap = tc.draw(gs::sampled_from(vec![Mishap::Unplugged, Mishap::Cancelled]));
    let step = running[at];

    let dir = tempfile::tempdir().unwrap();
    let (store, main) = saved_copy(dir.path());
    let target = dunce_canonicalize(&common::blank(main.parent().unwrap(), "New"));
    if settings {
        let values = IndexMap::from([("quantize".to_string(), "off".to_string())]);
        api::settings_set(&store, &api::SetSettings { values, drive: None }).unwrap();
    }
    let req = api::ProvisionRequest {
        target: path(&target),
        like: like.then(|| path(&main)),
        // Named: the store holds another library, Empty, too.
        library: Some("Main".into()),
        name: Some("Spare".into()),
        format: false,
        format_options: Default::default(),
        role: role.then(|| "backup".into()),
        player_name: None,
        player_colour: None,
        settings,
        verify,
        eject,
    };
    // Pushed from a library the store hasn't got, or one with nothing saved yet, it's refused
    // before it starts.
    store.create_library("Empty").unwrap();
    for (library, why) in [("Radio", "library \"Radio\""), ("Empty", "library \"Empty\" has no snapshots")] {
        let other = api::ProvisionRequest { like: None, library: Some(library.into()), ..req.clone() };
        let refused = api::provision_plan(&store, &other, &Ctx::silent()).map(|p| p.noop);
        assert!(matches!(&refused, Err(Error::NotFound(m)) if m == why), "{refused:?}");
    }
    let away = dir.path().join("away");
    let progress = MishapAt {
        step,
        mishap,
        root: &target,
        away: &away,
        cancel: CancelToken::new(),
        happened: AtomicBool::new(false),
    };
    let ctx = Ctx::new(&progress, progress.cancel.clone());
    let result = api::provision_apply(&store, &req, None, &ctx);
    assert!(progress.happened.load(Ordering::SeqCst), "{step} began");
    let past = |s: &str| running.iter().position(|r| *r == s).is_some_and(|i| i < at);

    if mishap == Mishap::Cancelled {
        // Copying the library and checking it look at the cancel token between files.
        let checked = running[at..].iter().any(|s| ["library", "verify"].contains(s));
        match checked {
            true => assert!(matches!(result, Err(Error::Cancelled)), "{step}: {result:?}"),
            // A folder can't be ejected, cancelled or not.
            false => {
                let stopped = result.unwrap().stopped.map(|s| s.step);
                assert_eq!(stopped.as_deref(), eject.then_some("eject it"), "{step}")
            }
        }
        return;
    }
    let report = result.unwrap();
    let stopped = report.stopped.as_ref().expect("stopped");
    let want = match step {
        "name" => "name it",
        "library" if like => "copy the library",
        "library" => "write the library",
        "settings" => "write settings",
        "role" => "set the role",
        "verify" => "check it",
        _ => "eject it",
    };
    assert_eq!(stopped.step, want, "{stopped:?}");
    assert_ne!(stopped.kind, "cancelled");
    assert_eq!(report.copy.is_some(), past("library"), "{step}");
    assert_eq!(report.settings.is_some(), past("settings"), "{step}");
    assert_eq!(report.verify.as_ref().map(|v| v.is_clean()), past("verify").then_some(true), "{step}");
    assert_eq!(report.drive.is_some(), step == "eject", "handed over only once checked");
    assert!(report.eject.is_none());
    // What was done before stays done: named and registered once the name step is past.
    let registered: Vec<Option<Role>> =
        store.drives().unwrap().values().filter(|r| r.name == "Spare").map(|r| r.role).collect();
    let want_role = (role && past("role")).then_some(Role::Backup);
    match past("name") {
        true => assert_eq!(registered, [want_role]),
        false => assert!(registered.is_empty(), "{registered:?}"),
    }
}
