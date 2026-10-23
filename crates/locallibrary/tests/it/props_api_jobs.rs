//! Model-based tests of the service layer's jobs: which drive a command means when it's given
//! none (the default drive, a library's drive group, its lead clone, what reads read); the steps
//! getting drives ready for a set and refreshing one after an export plan and run; syncing a
//! drive group; and a write to one clone of a library reaching the others.

use crate::common::{self, path};
use crate::props_api_drives::{saved_copy, saved_demo};

use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, StepKind};
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::edit::Edit;
use locallibrary::error::Reason;
use locallibrary::fleet::{self, Role};
use locallibrary::query::TrackSelector;
use locallibrary::store::Store;
use locallibrary::{Error, paths};
use uuid::Uuid;

/// A step's id parses back to it in any case and with blanks around it; anything else is
/// refused, naming every step.
#[hegel::test(test_cases = 300)]
fn steps_parse_from_their_ids_in_any_case(tc: TestCase) {
    let kind = StepKind::ALL[tc.draw(gs::integers::<usize>().max_value(StepKind::ALL.len() - 1))];
    let id: String =
        kind.id().chars().map(|c| if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c }).collect();
    let pad = tc.draw(gs::sampled_from(vec!["", " ", "\t", "  "]));
    assert_eq!(StepKind::parse(&format!("{pad}{id}{pad}")).unwrap(), kind);
    let other = tc.draw(gs::text().max_size(8));
    match StepKind::ALL.iter().find(|k| k.id().eq_ignore_ascii_case(other.trim())) {
        Some(k) => assert_eq!(StepKind::parse(&other).unwrap(), *k),
        None => {
            let e = StepKind::parse(&other).unwrap_err().to_string();
            assert!(e.contains("clean, replay, smart, player, settings, pull, sync, check, eject"), "{e}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Which drive
// ---------------------------------------------------------------------------------------------

/// What a mounted drive is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    /// An export in no library, never adopted.
    Loose,
    /// A clone of the store's library Main.
    Main,
    /// A drive of Club, a library with no snapshots yet.
    Club,
}

#[derive(Debug)]
struct Mounted {
    root: PathBuf,
    kind: Kind,
    name: String,
    role: Option<Role>,
    retired: bool,
}

impl Mounted {
    fn library(&self) -> Option<&'static str> {
        match self.kind {
            Kind::Loose => None,
            Kind::Main => Some("Main"),
            Kind::Club => Some("Club"),
        }
    }

    /// Archive and retired drives are left out of their library's drive group.
    fn excluded(&self) -> bool {
        self.retired || self.role == Some(Role::Archive)
    }
}

/// A library's mounted drives, as the model has them: its primary first, then the others in
/// mount order, but archive and retired ones.
struct Group {
    library: &'static str,
    drives: Vec<PathBuf>,
}

fn groups(drives: &[Mounted]) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for d in drives {
        let Some(library) = d.library() else { continue };
        let i = match out.iter().position(|g| g.library == library) {
            Some(i) => i,
            None => {
                out.push(Group { library, drives: Vec::new() });
                out.len() - 1
            }
        };
        if d.excluded() {
            continue;
        }
        match d.role == Some(Role::Primary) {
            true => out[i].drives.insert(0, d.root.clone()),
            false => out[i].drives.push(d.root.clone()),
        }
    }
    out
}

/// Give a copy of a drive an identity of its own, as `drive reidentify` does, registered in the
/// store: without asking the OS which disk it's on, which is slow here.
pub(crate) fn own_identity(store: &Store, d: &mut Device, name: &str) {
    let state = d.state_mut().unwrap();
    state.id = Uuid::new_v4();
    state.name = name.into();
    d.save_state().unwrap();
    locallibrary::sync::register_drive(store, d, None).unwrap();
}

/// The export of the demo drive alone (no audio or analysis files): a cheap drive whose
/// library reads.
pub(crate) fn bare_export(root: &Path) {
    let pdb = root.join(paths::EXPORT_PDB);
    std::fs::create_dir_all(pdb.parent().unwrap()).unwrap();
    std::fs::copy(common::fixtures().join("rekordcrate/demo_tracks").join(paths::EXPORT_PDB), pdb).unwrap();
}

/// Up to three mounted drives (exports in no library, clones of Main, drives of Club), with
/// roles and retired ones: the drive group a command given no drive means, the library's lead
/// clone (the primary, or the first mounted, as the clones are identical; a library's name means
/// it), the default drive, what a read with no drive reads, the candidates listed, a write to
/// every drive taken once per library and the group a group sync takes, all agree with a model of
/// the mounted drives and their libraries.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn the_drive_meant_agrees_with_a_model(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let n = tc.draw(gs::integers::<usize>().max_value(3));
    let mut drives: Vec<Mounted> = Vec::new();
    let mut store: Option<Store> = None;
    // Until the first drive brings the saved demo's store (and its own volumes), lookups find no
    // drive and no store.
    let mut volumes = common::isolate(dir.path(), None);
    let mut club: Option<Uuid> = None;
    for i in 0..n {
        let kind = [Kind::Loose, Kind::Main, Kind::Club][tc.draw(gs::integers::<usize>().max_value(2))];
        if store.is_none() {
            // The store holding the saved demo, with any drive: its own folder goes, each clone is
            // written where it's mounted.
            let (s, main) = saved_copy(dir.path());
            volumes = main.parent().unwrap().to_path_buf();
            std::fs::remove_dir_all(&main).unwrap();
            store = Some(s);
        }
        let store = store.as_ref().unwrap();
        let root = volumes.join(format!("d{}", i + 1));
        let name = match kind {
            Kind::Loose => {
                bare_export(&root);
                format!("d{}", i + 1)
            }
            Kind::Main => {
                saved_demo().1.write(&root);
                let mut d = Device::open(&root).unwrap();
                // The first clone keeps the saved drive's identity; the others are copies of it.
                let name = format!("Main {}", i + 1);
                if drives.iter().any(|d| d.kind == Kind::Main) {
                    own_identity(store, &mut d, &name);
                } else {
                    fleet::rename(Some(store), &mut d, &name, false).unwrap();
                }
                name
            }
            Kind::Club => {
                bare_export(&root);
                let id = *club.get_or_insert_with(|| store.create_library("Club").unwrap().id);
                let mut d = Device::open(&root).unwrap();
                let name = format!("Club {}", i + 1);
                d.adopt(Some(&name)).unwrap();
                d.state_mut().unwrap().library_id = Some(id);
                d.save_state().unwrap();
                locallibrary::sync::register_drive(store, &d, None).unwrap();
                name
            }
        };
        let root = dunce_canonicalize(&root);
        let (mut role, mut retired) = (None, false);
        if kind != Kind::Loose {
            let id = Device::open(&root).unwrap().id().unwrap();
            let roles = [None, Some(Role::Primary), Some(Role::Backup), Some(Role::Archive)];
            role = tc.draw(gs::sampled_from(&roles[..]));
            fleet::set_role(store, id, role).unwrap();
            if role == Some(Role::Primary) {
                for d in drives.iter_mut().filter(|d| d.kind == kind && d.role == Some(Role::Primary)) {
                    d.role = Some(Role::Backup);
                }
            }
            retired = tc.draw(gs::weighted_booleans(0.2));
            if retired {
                fleet::retire(store, id, false).unwrap();
            }
        }
        drives.push(Mounted { root, kind, name, role, retired });
    }
    let store = store.as_ref();
    let groups = groups(&drives);
    let with_drives: Vec<&Group> = groups.iter().filter(|g| !g.drives.is_empty()).collect();

    // The drive group a command given no drive takes.
    let group = api::drive_group(store, None);
    match (with_drives.as_slice(), drives.len()) {
        (_, 0) => assert!(matches!(group, Err(Error::NotFound(_))), "{group:?}"),
        ([g], _) => {
            let group = group.unwrap();
            assert_eq!((group.library.as_deref(), &group.drives), (Some(g.library), &g.drives));
            let excluded: Vec<&str> = drives
                .iter()
                .filter(|d| d.library() == Some(g.library) && d.excluded())
                .map(|d| d.name.as_str())
                .collect();
            assert_eq!(group.excluded, excluded);
        }
        ([], 1) => {
            let group = group.unwrap();
            assert_eq!((group.library, &group.drives), (None, &vec![drives[0].root.clone()]));
        }
        _ => assert!(matches!(group, Err(Error::Invalid(_))), "{group:?}"),
    }
    if let Some(store) = store {
        let main = api::drive_group(Some(store), Some("main"));
        match groups.iter().find(|g| g.library == "Main") {
            Some(g) => assert_eq!(main.unwrap().drives, g.drives),
            None => assert!(matches!(main, Err(Error::NotFound(_))), "{main:?}"),
        }
        assert!(matches!(api::drive_group(Some(store), Some("Radio")), Err(Error::NotFound(_))));
    }

    // The drive a command given none means.
    let lead = |g: &Group| g.drives[0].clone();
    let libraries: Vec<&str> = {
        let mut l: Vec<&str> = drives.iter().filter_map(Mounted::library).collect();
        l.sort_unstable();
        l.dedup();
        l
    };
    let meant = match (drives.len(), libraries.len(), with_drives.as_slice()) {
        (0, _, _) => None,
        (1, _, _) => Some(drives[0].root.clone()),
        (_, 1, [g]) => Some(lead(g)),
        _ => None,
    };
    for got in [api::default_drive(store, "refresh"), api::refresh_drive(store)] {
        match (&meant, got) {
            (Some(root), Ok(d)) => assert_eq!(d.root(), root),
            (None, Err(e)) if drives.is_empty() => assert!(matches!(e, Error::NotFound(_)), "{e}"),
            (None, Err(e)) => {
                assert_eq!(e.reason(), Some(Reason::ChooseDrive), "{e}");
                for d in &drives {
                    assert!(e.to_string().contains(&d.root.display().to_string()), "{e} lists {}", d.name);
                }
            }
            (want, got) => panic!("the model means {want:?}, the API {:?}", got.map(|d| d.root().to_path_buf())),
        }
    }

    // The candidates, and what a read given no drive reads: each library once, from its lead.
    let candidates = api::drive_candidates(store);
    let listed: Vec<(PathBuf, Option<Role>, Option<&str>)> =
        candidates.iter().map(|c| (c.root.clone(), c.role, c.library.as_deref())).collect();
    let want: Vec<(PathBuf, Option<Role>, Option<&str>)> =
        drives.iter().map(|d| (d.root.clone(), d.role, d.library())).collect();
    assert_eq!(listed, want);
    let mut want: Vec<(String, PathBuf)> = Vec::new();
    for d in &drives {
        match d.library() {
            None => want.push((d.name.clone(), d.root.clone())),
            Some(l) if want.iter().any(|w| w.0 == l) => {}
            Some(l) => {
                let from = groups.iter().find(|g| g.library == l && !g.drives.is_empty()).map_or(d.root.clone(), lead);
                want.push((l.to_string(), from));
            }
        }
    }
    let reads: Vec<(String, PathBuf)> = api::library_reads(store).into_iter().map(|c| (c.name, c.root)).collect();
    assert_eq!(reads, want);

    // A write to every drive reaches each library's clones from its lead, once.
    let specs: Vec<String> = drives.iter().map(|d| path(&d.root)).collect();
    let mut want: Vec<String> = Vec::new();
    let mut taken: Vec<&str> = Vec::new();
    for d in &drives {
        let Some(g) = groups.iter().find(|g| g.drives.contains(&d.root)) else {
            want.push(path(&d.root));
            continue;
        };
        if taken.contains(&g.library) {
            continue;
        }
        taken.push(g.library);
        want.push(path(&if g.drives.len() == 1 { d.root.clone() } else { lead(g) }));
    }
    assert_eq!(api::libraries_once(specs).unwrap(), want);

    let e = api::resolve_drive("Nobody").expect_err("no such drive").to_string();
    assert!(e.contains("no mounted drive matches \"Nobody\""), "{e}");

    // A library's name means its lead clone, of which a library whose mounted drives are all
    // archive or retired ones has none.
    let got = api::resolve_drive("main");
    match groups.iter().find(|g| g.library == "Main") {
        Some(g) if !g.drives.is_empty() => assert_eq!(got.unwrap().root(), lead(g)),
        Some(_) => assert!(matches!(&got, Err(Error::NotFound(m)) if m.contains("archive and retired")), "{got:?}"),
        None => assert!(matches!(got, Err(Error::NotFound(_))), "{got:?}"),
    }
    // A group sync given no library syncs the one group of two drives or more.
    if let Some(store) = store {
        let resolved = api::SyncSetRequest::default().resolve(store);
        match groups.iter().filter(|g| g.drives.len() >= 2).collect::<Vec<_>>().as_slice() {
            [] => assert!(matches!(resolved, Err(Error::NotFound(_))), "{resolved:?}"),
            [g] => assert_eq!(resolved.unwrap().drives, g.drives.iter().map(|d| path(d)).collect::<Vec<_>>()),
            _ => assert_eq!(resolved.expect_err("refused").reason(), Some(Reason::ChooseLibrary)),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Set and refresh
// ---------------------------------------------------------------------------------------------

pub(crate) const SAMPLE: &str = "/Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES/House1.wav";

/// rekordbox's demo sample, as an export brings it back.
pub(crate) fn add_demo_sample(root: &Path) {
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
    let track =
        rekordbox_pdb::NewTrack { artist: Some("rekordbox".into()), ..rekordbox_pdb::NewTrack::new("House 1", SAMPLE) };
    ed.add_track(&track).unwrap();
    ed.save(&pdb).unwrap();
    let audio = paths::to_fs(root, &paths::key(SAMPLE));
    std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
    std::fs::write(audio, "demo sample").unwrap();
}

fn titles(root: &Path) -> Vec<String> {
    let mut t: Vec<String> =
        Device::open(root).unwrap().library().unwrap().tracks.into_iter().map(|t| t.title).collect();
    t.sort();
    t
}

fn running(plan: &api::JobPlan) -> Vec<StepKind> {
    plan.running().map(|s| s.step).collect()
}

fn reason(plan: &api::JobPlan, kind: StepKind) -> Option<&str> {
    plan.steps.iter().find(|s| s.step == kind).and_then(|s| s.reason.as_deref())
}

/// Main and Backup, clones of the saved demo in the library Main, and which of them rekordbox
/// exported a new track to since.
struct Pair {
    _dir: tempfile::TempDir,
    store: Store,
    main: PathBuf,
    backup: PathBuf,
    main_changed: bool,
    backup_changed: bool,
}

impl Pair {
    fn new(tc: &TestCase) -> Pair {
        let dir = tempfile::tempdir().unwrap();
        let (store, main) = saved_copy(dir.path());
        let backup = main.with_file_name("Backup");
        saved_demo().1.write(&backup);
        own_identity(&store, &mut Device::open(&backup).unwrap(), "Backup");
        let (main_changed, backup_changed) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
        if main_changed {
            common::export_new_track(&main, "Artist", "Fresh at home");
        }
        if backup_changed {
            common::export_new_track(&backup, "Artist", "Fresh at the club");
        }
        Pair { _dir: dir, store, main, backup, main_changed, backup_changed }
    }

    fn name(&self, root: &Path) -> &'static str {
        if root == self.main { "Main" } else { "Backup" }
    }

    fn changed(&self, root: &Path) -> bool {
        if root == self.main { self.main_changed } else { self.backup_changed }
    }
}

/// Getting Main and Backup (or one of them, by path or name; with no drives named, the library's
/// drive group; a drive named twice, or a player model nobody knows, is refused) ready for a set,
/// for the player models given or none: the plan saves the drives the store doesn't hold yet, copies the one that
/// changed onto the other (not when both did, or there's one), checks them and ejects them only
/// when asked; applied, it runs those steps, stops at an eject that can't be done here, and
/// leaves both drives holding the newest library, with nothing left to do but check them.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_set_job_runs_the_steps_the_drives_need(tc: TestCase) {
    let p = &Pair::new(&tc);
    let ctx = locallibrary::progress::Ctx::silent();
    // No drives (the library's drive group), a name (Main's, which is its library's too: the
    // drive is meant), or drives by path, one of them twice.
    let (specs, drives): (Vec<String>, Vec<PathBuf>) = match tc.draw(gs::integers::<u8>().max_value(6)) {
        0 => (Vec::new(), vec![p.backup.clone(), p.main.clone()]),
        1 => (vec!["main".into()], vec![p.main.clone()]),
        2 => (vec![path(&p.main), path(&p.backup)], vec![p.main.clone(), p.backup.clone()]),
        3 => (vec![path(&p.backup), path(&p.main)], vec![p.backup.clone(), p.main.clone()]),
        4 => (vec![path(&p.main)], vec![p.main.clone()]),
        5 => (vec![path(&p.backup)], vec![p.backup.clone()]),
        _ => (vec![path(&p.main), path(&p.main)], Vec::new()),
    };
    let players = tc.draw(gs::sampled_from(&[&[][..], &["cdj-3000"][..], &["CDJ-9000X"][..]][..]));
    let eject = tc.draw(gs::booleans());
    let players: Vec<String> = players.iter().map(|m| m.to_string()).collect();
    let req = api::SetJobRequest { drives: specs, eject, players: players.clone(), ..Default::default() };
    let plan = api::set_job_plan(&p.store, &req, &ctx);
    if drives.is_empty() {
        assert!(matches!(&plan, Err(e) if e.to_string().contains("listed twice")), "a drive named twice");
        return;
    }
    if players == ["CDJ-9000X"] {
        assert!(matches!(plan, Err(Error::Invalid(_))), "a model `players set` doesn't know");
        return;
    }
    let plan = plan.unwrap();
    let changed: Vec<&PathBuf> = drives.iter().filter(|d| p.changed(d)).collect();
    let check = &plan.steps.iter().find(|s| s.step == StepKind::Check).unwrap().summary[0];
    match players.is_empty() {
        true => assert!(check.ends_with("(no player models remembered: `players set` adds them)"), "{check}"),
        false => assert!(check.ends_with("for the CDJ-3000"), "{check}"),
    }
    let targets: Vec<&Path> = plan.targets.iter().map(|t| t.root.as_path()).collect();
    assert_eq!(targets, drives.iter().map(PathBuf::as_path).collect::<Vec<_>>());
    let mut want = Vec::new();
    if !changed.is_empty() {
        want.push(StepKind::Pull);
    }
    let synced = drives.len() == 2 && changed.len() == 1;
    match (drives.len(), changed.len()) {
        (1, _) => assert_eq!(reason(&plan, StepKind::Sync), Some("one drive: nothing to copy")),
        (_, 0) => assert_eq!(reason(&plan, StepKind::Sync), Some("the drives hold the same library already")),
        (_, 1) => {
            want.push(StepKind::Sync);
            let sync = plan.steps.iter().find(|s| s.step == StepKind::Sync).unwrap();
            assert_eq!(sync.summary[0], format!("{} holds the newest library", p.name(changed[0])));
        }
        _ => assert_eq!(reason(&plan, StepKind::Sync), Some("no drive is newer than all the others")),
    }
    want.push(StepKind::Check);
    match eject {
        true => want.push(StepKind::Eject),
        false => assert_eq!(reason(&plan, StepKind::Eject), Some("kept mounted, as asked")),
    }
    assert_eq!(running(&plan), want);
    let pull = &plan.steps[0];
    for d in &drives {
        let line = match p.changed(d) {
            true => format!("save {} in Main", p.name(d)),
            false => format!("the store holds {} already (Main)", p.name(d)),
        };
        assert!(pull.summary.contains(&line), "{line:?} in {:?}", pull.summary);
    }

    let report = plan.apply(Some(&p.store), &ctx).unwrap();
    let done: Vec<StepKind> = report.steps.iter().map(|s| s.step).collect();
    let ran: Vec<StepKind> = want.iter().copied().filter(|k| *k != StepKind::Eject).collect();
    assert_eq!(done, ran, "{:?}", report.stopped);
    if changed.len() < 2 {
        assert!(!report.problems, "{:?}", report.check.as_ref().map(|c| &c.items));
    }
    let stopped = report.stopped.as_ref().map(|s| (s.step.as_str(), s.message.contains("nothing to eject")));
    assert_eq!(stopped, (eject && !report.problems).then_some(("eject", true)));
    if synced {
        assert_eq!(titles(&p.main), titles(&p.backup), "both hold the newest library");
    }
    if changed.len() < 2 {
        let again = api::set_job_plan(&p.store, &api::SetJobRequest { eject: false, ..req }, &ctx).unwrap();
        let rest = if drives.len() == 2 && !synced && !changed.is_empty() { 2 } else { 1 };
        assert_eq!(running(&again).len(), rest, "{:?}", running(&again));
        assert_eq!(running(&again).last(), Some(&StepKind::Check));
        if drives.len() == 2 {
            let group = api::pull_group(&p.store, &api::PullGroupRequest::default(), &ctx).unwrap();
            assert!(group.steps.iter().all(|s| s.reports.is_empty()), "nothing to save");
        }
    }
}

/// Refreshing Main after an export (named, or as the drive a refresh given none means), some
/// steps skipped, and copying it onto Backup when asked: only the steps with something to do run
/// (making a recorded edit the export replaced again, writing My Settings, saving the drive,
/// copying it onto Backup when Main is the newer), every other says why it doesn't; applied, they
/// run in order and do what they said. Without the demos plugin (the app's), rekordbox's demo
/// sample stays.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn a_refresh_runs_the_steps_with_something_to_do(tc: TestCase) {
    let p = &Pair::new(&tc);
    let ctx = locallibrary::progress::Ctx::silent();
    let sample = tc.draw(gs::booleans());
    if sample {
        add_demo_sample(&p.main);
    }
    // A playlist made on Main, then rekordbox exporting over it a minute later: its own database
    // back, with a new track.
    let edited = tc.draw(gs::weighted_booleans(0.4));
    if edited {
        let pdb = p.main.join(paths::EXPORT_PDB);
        let exported = std::fs::read(&pdb).unwrap();
        common::apply(&p.main, &[Edit::CreatePlaylist { path: "Mine".into(), folder: false }]);
        std::fs::write(&pdb, exported).unwrap();
        common::export_new_track(&p.main, "Artist", "Exported over it");
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        std::fs::File::options().write(true).open(&pdb).unwrap().set_modified(later).unwrap();
        assert_eq!(api::exported_over_edits(&Device::open(&p.main).unwrap()), 1, "one recorded edit exported over");
    }
    let profile = tc.draw(gs::booleans());
    if profile {
        let values = indexmap::IndexMap::from([("quantize".to_string(), "off".to_string())]);
        api::settings_set(&p.store, &api::SetSettings { values, drive: None }).unwrap();
    }
    let mut skip = Vec::new();
    for k in [StepKind::Clean, StepKind::Replay, StepKind::Smart, StepKind::Settings, StepKind::Pull, StepKind::Sync] {
        if tc.draw(gs::weighted_booleans(0.25)) {
            skip.push(k);
        }
    }
    let sync = tc.draw(gs::weighted_booleans(0.7));
    let dirty = p.main_changed || sample || edited;
    // A refresh given no drive means the one rekordbox exported over recorded edits on; else,
    // with Main the primary, Main, unless Backup is the newer.
    // (An edit exported over makes Main dirty.)
    let main_meant = dirty || !p.backup_changed;
    let unnamed = tc.draw(gs::booleans()) && main_meant;
    if unnamed {
        if !edited {
            fleet::set_role(&p.store, Device::open(&p.main).unwrap().id().unwrap(), Some(Role::Primary)).unwrap();
        }
        assert_eq!(api::refresh_drive(Some(&p.store)).unwrap().root(), p.main);
    }
    let drive = (!unnamed).then(|| path(&p.main));
    let req = api::RefreshRequest { drive, skip: skip.clone(), sync };
    let plan = api::refresh_plan(&p.store, &req, &ctx).unwrap();
    let skipped = |k: StepKind| skip.contains(&k);
    let mut want = Vec::new();
    if edited && !skipped(StepKind::Replay) {
        want.push(StepKind::Replay);
    }
    // The demo's settings aren't the profile's.
    if profile && !skipped(StepKind::Settings) {
        want.push(StepKind::Settings);
    }
    let idle = [
        (StepKind::Clean, "the demos plugin isn't there"),
        (StepKind::Replay, "no recorded edits"),
        (StepKind::Smart, "no rule-based playlists for this drive"),
        (StepKind::Player, "no name or colour remembered for it (setting the name players show remembers them)"),
        (StepKind::Settings, "the My Settings profile is empty (`settings import` starts one)"),
    ];
    for (k, why) in idle {
        if !(k == StepKind::Replay && edited || k == StepKind::Settings && profile) {
            assert_eq!(reason(&plan, k), Some(if skipped(k) { "skipped" } else { why }), "{k:?}");
        }
    }
    let pulls = (dirty || !want.is_empty()) && !skipped(StepKind::Pull);
    match (skipped(StepKind::Pull), pulls) {
        (true, _) => assert_eq!(reason(&plan, StepKind::Pull), Some("skipped")),
        (false, true) => want.push(StepKind::Pull),
        (false, false) => assert_eq!(reason(&plan, StepKind::Pull), Some("the store holds it already")),
    }
    let copies = sync && !skipped(StepKind::Sync) && !p.backup_changed;
    // A step before writes to Main: then Backup gets a copy even if it matches Main now.
    let after = !want.is_empty();
    match () {
        _ if !sync => assert!(plan.steps.iter().all(|s| s.step != StepKind::Sync)),
        _ if skipped(StepKind::Sync) => assert_eq!(reason(&plan, StepKind::Sync), Some("skipped")),
        _ if p.backup_changed && !dirty => assert_eq!(reason(&plan, StepKind::Sync), Some("Backup is newer than Main")),
        _ if p.backup_changed => assert!(reason(&plan, StepKind::Sync).is_some(), "the drives diverged"),
        _ if !dirty && !after => {
            assert_eq!(reason(&plan, StepKind::Sync), Some("the drives hold the same library already"))
        }
        _ => want.push(StepKind::Sync),
    }
    assert_eq!(running(&plan), want, "{:?}", plan.steps.iter().map(|s| (s.step, &s.reason)).collect::<Vec<_>>());
    assert_eq!(plan.noop, want.is_empty());

    let report = plan.apply(Some(&p.store), &ctx).unwrap();
    assert!(report.stopped.is_none(), "{:?}", report.stopped);
    assert_eq!(report.steps.iter().map(|s| s.step).collect::<Vec<_>>(), want);
    assert_eq!(report.did_anything(), !want.is_empty());
    let has_sample = titles(&p.main).contains(&"House 1".to_string());
    assert_eq!(has_sample, sample);
    let lists: Vec<String> =
        Device::open(&p.main).unwrap().library().unwrap().playlists.into_iter().map(|l| l.name).collect();
    assert_eq!(lists.contains(&"Mine".to_string()), edited && !skipped(StepKind::Replay), "{lists:?}");
    if copies && want.contains(&StepKind::Sync) {
        assert_eq!(titles(&p.main), titles(&p.backup), "Backup holds Main's library");
    }
}

/// Syncing the library's drive group (the only one with two drives mounted, or the one named):
/// the fleet view and the group list its primary first, then the others; the plan is refused when both drives changed
/// (neither is the newest), and otherwise copies the newest onto the other (when they differ),
/// after which they hold the same library.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_group_sync_copies_the_newest_onto_the_rest(tc: TestCase) {
    let p = &Pair::new(&tc);
    let ctx = locallibrary::progress::Ctx::silent();
    let primary = tc.draw(gs::booleans());
    if primary {
        fleet::set_role(&p.store, Device::open(&p.main).unwrap().id().unwrap(), Some(Role::Primary)).unwrap();
    }
    let order = match primary {
        true => [p.main.clone(), p.backup.clone()],
        false => [p.backup.clone(), p.main.clone()],
    };
    // The fleet view's library: both clones, the primary first, identical while neither changed,
    // and holding changes the store doesn't once either did.
    let st = api::fleet_status(Some(&p.store), &api::FleetRequest::default(), &ctx).unwrap();
    let [lib] = st.libraries.as_slice() else { panic!("{:?}", st.libraries) };
    let changed = p.main_changed || p.backup_changed;
    assert_eq!((lib.name.as_str(), &lib.mounted[..]), ("Main", &order[..]));
    assert_eq!((lib.identical, lib.unsaved, lib.offline.len()), (Some(!changed), changed, 0));
    let library = tc.draw(gs::sampled_from(&[None, Some("main"), Some("Radio")][..]));
    let req = api::SyncSetRequest { library: library.map(Into::into), ..Default::default() };
    let resolved = req.resolve(&p.store);
    if library == Some("Radio") {
        assert!(matches!(resolved, Err(Error::NotFound(_))), "{resolved:?}");
        return;
    }
    assert_eq!(resolved.unwrap().drives, order.map(|d| path(&d)), "the primary first");
    let plan = api::sync_set_plan(&p.store, &req, &ctx);
    if p.main_changed && p.backup_changed {
        assert!(plan.is_err(), "neither drive is the newest");
        return;
    }
    let plan = plan.unwrap();
    let newest = if p.backup_changed { &p.backup } else { &p.main };
    if p.main_changed || p.backup_changed {
        assert_eq!(&plan.plan.source, newest);
    }
    let results = api::sync_set_apply(&p.store, &req, &ctx).unwrap();
    // Identical drives need no copy.
    assert_eq!(results.len(), usize::from(p.main_changed || p.backup_changed), "{results:?}");
    assert!(results.iter().all(|r| r.error.is_none()), "{results:?}");
    assert_eq!(titles(&p.main), titles(&p.backup));
}

/// A write to one clone of a library reaches the others: deleting rekordbox's demo sample from
/// Main plans Backup as a clone it's copied onto, unless Backup changed too (then it's left
/// alone, which the plan says); applied, Main loses the sample and Backup ends up holding Main's
/// library, or as it was.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_write_to_a_clone_reaches_the_others(tc: TestCase) {
    let p = &Pair::new(&tc);
    let ctx = locallibrary::progress::Ctx::silent();
    add_demo_sample(&p.main);
    let before = titles(&p.backup);
    let edits = vec![Edit::DeleteTracks { tracks: TrackSelector::paths([SAMPLE.into()]), keep_files: false }];
    let plan =
        api::edit_plan(&api::EditRequest { drive: path(&p.main), edits, options: Default::default() }, &ctx).unwrap();
    let clones: Vec<(&Path, bool)> = plan.clones.iter().map(|c| (c.root.as_path(), c.left_alone.is_some())).collect();
    assert_eq!(clones, [(p.backup.as_path(), p.backup_changed)]);
    assert_eq!(plan.warnings.len(), usize::from(p.backup_changed), "{:?}", plan.warnings);
    plan.apply(Some(&p.store), &ctx).unwrap();
    assert!(!titles(&p.main).contains(&"House 1".to_string()), "the sample is gone");
    match p.backup_changed {
        true => assert_eq!(titles(&p.backup), before, "left alone"),
        false => assert_eq!(titles(&p.backup), titles(&p.main), "copied onto"),
    }
}
