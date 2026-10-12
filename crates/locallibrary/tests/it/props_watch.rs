//! Properties of noticing drives come and go and change (`watch`), in a folder of pretend
//! volumes, against a model of what's plugged in.

use indexmap::IndexMap;
use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use uuid::Uuid;

use locallibrary::device::dunce_canonicalize;
use locallibrary::paths;
use locallibrary::watch::{self, DriveChange, FileStamp, LibraryStamp, MountedDrive, VolumeWatcher, WatchEvent};

const NAMES: [&str; 4] = ["A", "B", "Club", "Main"];
const LABELS: [&str; 2] = ["Gig", "Home"];
/// The store's files a change is reported by, and their names.
const STORE_FILES: [(&str, &str); 7] = [
    ("libraries", "libraries.json"),
    ("labels", "labels.json"),
    ("pruned", "pruned.json"),
    ("drives", "drives.json"),
    ("settings", "settings.json"),
    ("smart", "smart.json"),
    ("sets", locallibrary::sets::SETS_FILE),
];

#[derive(Debug, Clone)]
enum Step {
    /// A drive plugged in (with an export or blank), unless one is there.
    Plug {
        name: usize,
        export: bool,
    },
    Unplug(usize),
    /// An export (or a player's history) rewrites its database.
    Export(usize),
    /// locallibrary gives it an identity, or a new name.
    Adopt {
        name: usize,
        label: usize,
    },
    /// The store's libraries, labels... are written.
    Store(usize),
    /// A snapshot is saved in the store, or the oldest pruned.
    Snapshot {
        add: bool,
    },
    /// Every drive unplugged at once.
    UnplugAll,
    Nothing,
}

fn step() -> impl Generator<Step> {
    let name = || gs::integers::<usize>().max_value(NAMES.len() - 1);
    hegel::one_of!(
        gs::tuples!(name(), gs::booleans()).map(|(name, export)| Step::Plug { name, export }),
        name().map(Step::Unplug),
        name().map(Step::Export),
        gs::tuples!(name(), gs::integers::<usize>().max_value(LABELS.len() - 1))
            .map(|(name, label)| Step::Adopt { name, label }),
        gs::integers::<usize>().max_value(STORE_FILES.len() - 1).map(Step::Store),
        gs::booleans().map(|add| Step::Snapshot { add }),
        gs::just(Step::UnplugAll),
        gs::just(Step::Nothing),
    )
}

/// A plugged-in drive as the model has it.
#[derive(Debug, Clone)]
struct Plugged {
    id: Option<Uuid>,
    label: Option<&'static str>,
    export: bool,
    /// Bytes in its export: each rewrite is longer.
    len: usize,
}

/// Replace `path` with `bytes` the way locallibrary and rekordbox do, through a file renamed into
/// place: a new file id even when the clock hasn't moved on.
fn replace(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

struct Desk {
    _dir: tempfile::TempDir,
    volumes: PathBuf,
    store: PathBuf,
    plugged: IndexMap<&'static str, Plugged>,
    /// How many times each store file was written.
    writes: [usize; STORE_FILES.len()],
    /// The snapshots saved, oldest first, and how many have been.
    snapshots: Vec<PathBuf>,
    saved: u64,
}

impl Desk {
    fn root(&self, name: &str) -> PathBuf {
        dunce_canonicalize(&self.volumes.join(name))
    }

    fn drive(&self, name: &'static str) -> MountedDrive {
        let p = &self.plugged[name];
        MountedDrive {
            root: self.root(name),
            id: p.id,
            name: p.label.unwrap_or(name).to_string(),
            has_export: p.export,
        }
    }

    /// The drives the model has plugged in, by mount path.
    fn drives(&self) -> Vec<MountedDrive> {
        let mut names: Vec<&'static str> = self.plugged.keys().copied().collect();
        names.sort_by_key(|n| self.root(n));
        names.into_iter().map(|n| self.drive(n)).collect()
    }

    fn name_at(&self, root: &Path) -> &'static str {
        self.plugged.keys().copied().find(|n| self.root(n) == root).unwrap()
    }

    fn plug(&mut self, name: &'static str, export: bool) {
        let root = self.volumes.join(name);
        std::fs::create_dir(&root).unwrap();
        if export {
            replace(&root.join(paths::EXPORT_PDB), b"x");
        }
        self.plugged.insert(name, Plugged { id: None, label: None, export, len: usize::from(export) });
    }

    fn export(&mut self, name: &'static str) {
        let p = self.plugged.get_mut(name).unwrap();
        p.len += 1;
        p.export = true;
        replace(&self.volumes.join(name).join(paths::EXPORT_PDB), &vec![b'x'; p.len]);
    }

    fn adopt(&mut self, name: &'static str, label: &'static str) {
        let p = self.plugged.get_mut(name).unwrap();
        let id = *p.id.get_or_insert_with(Uuid::new_v4);
        p.label = Some(label);
        let state = serde_json::json!({ "format": 1, "id": id, "name": label, "created": "2026-10-05T00:00:00Z" });
        replace(&self.volumes.join(name).join(paths::STATE_DIR).join("state.json"), state.to_string().as_bytes());
    }

    fn unplug(&mut self, name: &'static str) -> MountedDrive {
        let d = self.drive(name);
        std::fs::remove_dir_all(self.volumes.join(name)).unwrap();
        self.plugged.shift_remove(name);
        d
    }

    /// Takes `step`, and says what a poll should report of it.
    fn take(&mut self, step: &Step) -> Vec<WatchEvent> {
        match *step {
            Step::Plug { name, export } if !self.plugged.contains_key(NAMES[name]) => {
                self.plug(NAMES[name], export);
                vec![WatchEvent::Attached(self.drive(NAMES[name]))]
            }
            Step::Unplug(name) if self.plugged.contains_key(NAMES[name]) => {
                vec![WatchEvent::Detached(self.unplug(NAMES[name]))]
            }
            Step::Export(name) if self.plugged.contains_key(NAMES[name]) => {
                self.export(NAMES[name]);
                vec![WatchEvent::Changed { drive: self.drive(NAMES[name]), changed: vec![DriveChange::Databases] }]
            }
            Step::Adopt { name, label } if self.plugged.contains_key(NAMES[name]) => {
                self.adopt(NAMES[name], LABELS[label]);
                vec![WatchEvent::Changed { drive: self.drive(NAMES[name]), changed: vec![DriveChange::State] }]
            }
            Step::Store(i) => {
                self.writes[i] += 1;
                replace(&self.store.join(STORE_FILES[i].1), format!("[{}]", "0,".repeat(self.writes[i])).as_bytes());
                vec![WatchEvent::StoreChanged { root: self.store.clone(), changed: vec![STORE_FILES[i].0] }]
            }
            Step::Snapshot { add } if add || !self.snapshots.is_empty() => {
                let dir = self.store.join("snapshots");
                match add {
                    true => {
                        let path = dir.join(format!("{}.json", self.saved));
                        std::fs::write(&path, b"{}").unwrap();
                        self.snapshots.push(path);
                    }
                    false => std::fs::remove_file(self.snapshots.remove(0)).unwrap(),
                }
                // A folder's time can stay the same within a file system's resolution: each change
                // gets a time of its own, as it would a while later.
                self.saved += 1;
                let at = filetime::FileTime::from_unix_time(1_700_000_000 + self.saved as i64, 0);
                filetime::set_file_mtime(&dir, at).unwrap();
                vec![WatchEvent::StoreChanged { root: self.store.clone(), changed: vec!["snapshots"] }]
            }
            Step::UnplugAll => {
                let names: Vec<&'static str> = self.drives().iter().map(|d| self.name_at(&d.root)).collect();
                names.into_iter().map(|n| WatchEvent::Detached(self.unplug(n))).collect()
            }
            _ => Vec::new(),
        }
    }
}

/// Driven by random drives plugged in and out, exports, identities and store writes, each poll
/// reports exactly what changed since the last: each drive attached once, with what it holds,
/// changed when its databases or identity are rewritten, detached as it was last seen, the store
/// by the file written; nothing when nothing changed. What it says is mounted is what is, as a
/// new watcher and `mounted_drives` (looking in the same folder) see it.
#[hegel::test(test_cases = 20)]
fn polls_report_what_changed_since_the_last(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let volumes = dir.path().join("volumes");
    let store = dir.path().join("store");
    std::fs::create_dir_all(&volumes).unwrap();
    std::fs::create_dir_all(store.join("snapshots")).unwrap();
    let mut desk = Desk {
        _dir: dir,
        volumes: volumes.clone(),
        store: store.clone(),
        plugged: IndexMap::new(),
        writes: [0; 7],
        snapshots: Vec::new(),
        saved: 0,
    };
    // Drives already plugged in when watching starts are known, and not reported.
    for (name, export) in tc.draw(gs::vecs(gs::tuples!(gs::sampled_from(NAMES.to_vec()), gs::booleans())).max_size(3)) {
        if !desk.plugged.contains_key(name) {
            desk.plug(name, export);
        }
    }
    let mut w = VolumeWatcher::in_folders(vec![volumes.clone()]).with_store(&store);
    assert_eq!(w.drives(), desk.drives());
    assert_eq!(w.poll(), []);

    locallibrary::testing::volumes_on_this_thread(Some(vec![volumes.clone()]));
    for step in tc.draw(gs::vecs(step().print_as_debug()).max_size(12)) {
        let want = desk.take(&step);
        let got = w.poll();
        assert_eq!(got, want, "after {step:?}");
        for e in &got {
            assert_eq!(serde_json::to_value(e).unwrap()["event"], e.kind());
            assert_eq!(e.drive().is_none(), matches!(e, WatchEvent::StoreChanged { .. }));
        }
        assert_eq!(w.poll(), [], "nothing changed since");
        assert_eq!(w.drives(), desk.drives());
        for d in desk.drives() {
            assert_eq!(LibraryStamp::of(&d.root).has_export(), d.has_export);
            assert_eq!(LibraryStamp::of(&d.root), LibraryStamp::of(&d.root), "a stamp is the same until a rewrite");
        }
    }
    assert_eq!(watch::mounted_drives(), desk.drives());
    assert_eq!(VolumeWatcher::new().drives(), desk.drives());
    assert_eq!(VolumeWatcher::default().poll(), [], "a new watcher starts from what's there");
    assert_eq!(
        watch::mounts().iter().map(|m| m.path.clone()).collect::<Vec<_>>(),
        desk.drives().into_iter().map(|d| d.root).collect::<Vec<_>>()
    );
    locallibrary::testing::volumes_on_this_thread(None);
    assert!(FileStamp::of(&volumes).is_none(), "a folder isn't a file");
}
