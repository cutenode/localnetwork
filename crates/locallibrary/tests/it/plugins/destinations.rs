//! Plugins' destinations, through the test plugin (echo's: a store kept in `destination/` where it
//! runs, and sets published nowhere): copying the store there in a folder copy's order, restoring
//! from it, checking what comes back, repairing a drive from it, and publishing a set.

use std::path::Path;

use chrono::{NaiveDate, Utc};
use locallibrary::api::{self, RepairRequest, VerifyRequest};
use locallibrary::device::Device;
use locallibrary::hash::Hash;
use locallibrary::offsite::{self, CopyOptions, RestoreOptions};
use locallibrary::op::AnyPlanned;
use locallibrary::paths;
use locallibrary::peers::LibraryOutcome;
use locallibrary::scrub::{self, RepairSource};
use locallibrary::sets::{Recording, Set, SetBook, SetEntry};
use locallibrary::sync::{self, PullOptions};

use super::*;

impl Fixture {
    /// Pulls the drive into the store: its latest snapshot.
    fn pulled(&self) -> Hash {
        common::age(&self.drive);
        let mut d = Device::open(&self.drive).unwrap();
        let opts = PullOptions { library: None, message: Some("pulled".into()), force: false };
        sync::pull(&self.store, &mut d, &opts, &Ctx::silent()).unwrap();
        self.store.libraries().unwrap()[0].head.unwrap()
    }

    /// What the test plugin was asked, in order, as `<method> <what>`: noted before each answer.
    fn asked(&self) -> Vec<String> {
        let asked = std::fs::read_to_string(Fixture::kept(&self.store).with_file_name("asked")).unwrap();
        asked.lines().map(String::from).collect()
    }

    /// Where the plugin started in `store` keeps its store destination.
    fn kept(store: &Store) -> PathBuf {
        store.root().join("plugins/echo/work/destination")
    }
}

/// Copies the folder `from` to `to`.
fn copy_dir(from: &Path, to: &Path) {
    for e in walkdir::WalkDir::new(from).into_iter().flatten() {
        let there = to.join(e.path().strip_prefix(from).unwrap());
        match e.file_type().is_dir() {
            true => std::fs::create_dir_all(there).unwrap(),
            false => drop(std::fs::copy(e.path(), there).unwrap()),
        }
    }
}

/// Every object `store` holds, by its path there.
fn objects(store: &Store) -> Vec<String> {
    let all =
        walkdir::WalkDir::new(store.root().join("objects")).into_iter().flatten().filter(|e| e.file_type().is_file());
    let mut names: Vec<String> =
        all.map(|e| e.path().strip_prefix(store.root()).unwrap().to_string_lossy().replace('\\', "/")).collect();
    names.retain(|n| !n.starts_with("objects/tmp/"));
    names.sort();
    names
}

#[test]
fn a_copy_at_a_destination_goes_in_a_folder_copys_order_and_comes_back_whole() {
    let f = Fixture::echo();
    let head = f.pulled();
    f.store.add_label(&head, "gig").unwrap();
    let dest = f.plugins.store_destination("echo:folder", Some(&f.store)).unwrap();
    let dry = offsite::copy_to(&f.store, &dest, &CopyOptions { dry_run: true, ..Default::default() }, &Ctx::silent())
        .unwrap();
    assert!(dry.objects_copied > 0 && dry.snapshots_copied == 1 && dry.dry_run, "{dry:?}");
    assert!(f.asked().iter().all(|a| !a.starts_with("blobs/put") && !a.starts_with("manifest/put")));
    let r = offsite::copy_to(&f.store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!((r.objects_copied, r.snapshots_copied, r.complete), (dry.objects_copied, 1, true), "{r:?}");
    assert_eq!(r.path, Path::new("echo:folder"));

    // Stored files, then snapshots, then the index of them, then libraries.json, last.
    let asked = f.asked();
    let at = |what: &str| asked.iter().position(|a| a == what).unwrap_or_else(|| panic!("never asked {what}"));
    let puts: Vec<usize> = (0..asked.len()).filter(|&i| asked[i].starts_with("blobs/put ")).collect();
    let snapshot = at(&format!("manifest/put snapshots/{}.json", head.to_hex()));
    assert!(puts.iter().all(|&i| i < snapshot), "{asked:?}");
    assert!(snapshot < at("manifest/put snapshots.json") && at("manifest/put snapshots.json") < asked.len() - 1);
    assert_eq!(at("manifest/put libraries.json"), asked.len() - 1, "given once, last: {asked:?}");
    // The store's records, but the account session and the rest that stays here.
    let records: Vec<&str> = asked.iter().filter_map(|a| a.strip_prefix("manifest/put ")).collect();
    for name in &records {
        let allowed = ["store.json", "drives.json", "labels.json", "snapshots.json", "libraries.json"];
        assert!(allowed.contains(name) || name.starts_with("snapshots/"), "{name} went to the destination");
    }
    let kept = Fixture::kept(&f.store);
    let stored = walkdir::WalkDir::new(kept.join("objects")).into_iter().flatten().filter(|e| e.file_type().is_file());
    assert_eq!(stored.count(), objects(&f.store).len());

    // Nothing new: nothing copied.
    let seen = f.asked().len();
    let again = offsite::copy_to(&f.store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!((again.objects_copied, again.snapshots_copied, again.objects_present), (0, 0, objects(&f.store).len()));
    assert!(f.asked()[seen..].iter().all(|a| !a.starts_with("blobs/put") && !a.starts_with("manifest/put snapshots/")));

    // Brought back into a new store, from the same destination (its folder, as a service would
    // keep it).
    f.plugins.shutdown();
    let other = Store::open_or_init(f.store.root().with_file_name("other")).unwrap();
    copy_dir(&kept, &Fixture::kept(&other));
    let dest = f.plugins.store_destination("echo:folder", Some(&other)).unwrap();
    let dry = offsite::restore(&other, &dest, &RestoreOptions { dry_run: true, ..Default::default() }, &Ctx::silent())
        .unwrap();
    assert_eq!((dry.snapshots, dry.libraries[0].outcome, dry.peer.as_str()), (1, LibraryOutcome::Added, "echo:folder"));
    assert!(other.libraries().unwrap().is_empty() && objects(&other).is_empty(), "a dry run fetches nothing");
    let r = offsite::restore(&other, &dest, &RestoreOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!((r.snapshots, r.files, r.present), (1, objects(&f.store).len(), 0), "{r:?}");
    assert_eq!(other.libraries().unwrap()[0].head, Some(head));
    assert_eq!(objects(&other), objects(&f.store));
    assert_eq!(other.find_label("gig").unwrap().map(|l| l.snapshot), Some(head));
    let drives = |s: &Store| std::fs::read(s.root().join("drives.json")).unwrap();
    assert_eq!(drives(&other), drives(&f.store), "the store's records come back where it has none");
    let again = offsite::restore(&other, &dest, &RestoreOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!((again.snapshots, again.files, again.libraries[0].outcome), (0, 0, LibraryOutcome::UpToDate));
}

#[test]
fn what_a_destination_answers_damaged_never_goes_in() {
    let f = Fixture::echo();
    let head = f.pulled();
    let dest = f.plugins.store_destination("echo:folder", Some(&f.store)).unwrap();
    offsite::copy_to(&f.store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap();
    f.plugins.shutdown();
    let kept = Fixture::kept(&f.store);
    let other = Store::open_or_init(f.store.root().with_file_name("other")).unwrap();
    copy_dir(&kept, &Fixture::kept(&other));
    let rotten = walkdir::WalkDir::new(Fixture::kept(&other).join("objects")).into_iter().flatten();
    let rotten = rotten.filter(|e| e.file_type().is_file()).find(|e| e.metadata().unwrap().len() > 1).unwrap();
    common::rot(rotten.path());
    let name = rotten.file_name().to_string_lossy().into_owned();
    let dest = f.plugins.store_destination("echo:folder", Some(&other)).unwrap();
    let e = offsite::restore(&other, &dest, &RestoreOptions::default(), &Ctx::silent()).unwrap_err().to_string();
    assert_eq!(e, format!("the echo:folder destination failed: a stored file came back damaged ({})", &name[..12]));
    assert!(objects(&other).iter().all(|o| !o.ends_with(&name[2..])), "the damaged file isn't in the store");
    assert!(other.libraries().unwrap().is_empty(), "nor the library its snapshot is of");

    // A snapshot's record that isn't the snapshot it's named after is refused before anything.
    std::fs::copy(kept.join("objects").join(&name), rotten.path()).unwrap();
    let record = Fixture::kept(&other).join(format!("manifests/snapshots/{}.json", head.to_hex()));
    let mut snapshot: Value = serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
    snapshot["message"] = "rewritten".into();
    std::fs::write(&record, snapshot.to_string()).unwrap();
    let e = offsite::restore(&other, &dest, &RestoreOptions::default(), &Ctx::silent()).unwrap_err().to_string();
    assert_eq!(e, format!("the echo:folder destination failed: snapshot {} came back damaged", head.short()));
}

#[test]
fn a_destination_names_a_store_of_its_own_and_keeps_what_its_given() {
    let f = Fixture::echo();
    f.pulled();
    for (spec, said) in [
        ("echo", "\"echo\" names no destination: <plugin>:<destination>"),
        ("echo:nope", "not found: the echo plugin's destination \"nope\""),
        ("nope:folder", "not found: plugin \"nope\""),
        ("echo:nowhere", "echo:nowhere (Nowhere, saying where) isn't a destination for a store"),
    ] {
        let e = f.plugins.store_destination(spec, Some(&f.store)).err().unwrap();
        assert_eq!(e.to_string(), said, "{spec}");
    }
    let dest = f.plugins.store_destination("echo:folder", Some(&f.store)).unwrap();
    let e = offsite::restore(&f.store, &dest, &RestoreOptions::default(), &Ctx::silent()).unwrap_err();
    assert_eq!(e.to_string(), "not found: a copy of a store at echo:folder");
    for opts in [CopyOptions { mirror: true, ..Default::default() }, CopyOptions { verify: true, ..Default::default() }]
    {
        let e = offsite::copy_to(&f.store, &dest, &opts, &Ctx::silent()).unwrap_err();
        assert_eq!(e.to_string(), "echo:folder keeps what it's given: a copy there can't be mirrored or verified");
    }
    offsite::copy_to(&f.store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap();
    // Another store copying there finds this one's.
    let other = Store::open_or_init(f.store.root().with_file_name("other")).unwrap();
    let e = offsite::copy_to(&other, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap_err();
    assert_eq!(e.code(), "conflict", "{e}");
    assert_eq!(e.to_string(), "echo:folder holds a different store's copy; force the copy to replace it");
    // Forced, it's this store's copy now: the other's snapshots it has stay, unreached.
    let forced = CopyOptions { force: true, ..Default::default() };
    assert!(offsite::copy_to(&other, &dest, &forced, &Ctx::silent()).unwrap().complete);
    offsite::copy_to(&other, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap();
}

#[test]
fn a_drive_is_repaired_from_a_destination_once_the_store_has_lost_its_copy() {
    let f = Fixture::echo();
    f.pulled();
    let dest = f.plugins.store_destination("echo:folder", Some(&f.store)).unwrap();
    offsite::copy_to(&f.store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap();
    let d = Device::open(&f.drive).unwrap();
    let audio = paths::key(&d.library().unwrap().tracks[0].file_path);
    common::rot(&paths::to_fs(d.root(), &audio));
    let verify = VerifyRequest { drive: f.drive.display().to_string(), snapshot: None, against: Vec::new() };
    let found = api::drive_verify(Some(&f.store), &verify, &Ctx::silent()).unwrap();
    let expected = found.damaged[0].expected.unwrap();
    std::fs::remove_file(f.store.object_path(&expected)).unwrap();
    let mut found = api::drive_verify(Some(&f.store), &verify, &Ctx::silent()).unwrap();
    assert_eq!((found.damaged[0].path.as_str(), &found.damaged[0].repair), (audio.as_str(), &None));
    scrub::offer(&mut found, &dest, &Ctx::silent()).unwrap();
    let from = RepairSource::Destination { plugin: "echo".into(), destination: "folder".into() };
    assert_eq!(found.damaged[0].repair, Some(from));
    let problems: Vec<_> = found.problems().cloned().collect();
    scrub::fetch(&f.store, &dest, &problems, &Ctx::silent()).unwrap();
    assert!(f.store.has_object(&expected), "fetched into the store, checked");
    let req = RepairRequest { drive: verify.drive.clone(), problems, against: Vec::new() };
    let repaired = api::drive_repair(Some(&f.store), &req, &Ctx::silent()).unwrap();
    assert_eq!(repaired.repaired, [audio]);
    assert!(api::drive_verify(Some(&f.store), &verify, &Ctx::silent()).unwrap().is_clean());
}

/// A set with a recording in `dir` and two tracks, the second published without naming it.
fn set(store: &Store, dir: &Path) -> Set {
    let recording = dir.join("friday.wav");
    std::fs::write(&recording, b"RIFF").unwrap();
    let track = |title: &str, unnamed| SetEntry { unnamed, start_seconds: Some(60), ..SetEntry::added(title) };
    let set = Set {
        id: "3m4nset0000000".into(),
        title: Some("Friday".into()),
        date: NaiveDate::from_ymd_opt(2026, 10, 2).unwrap(),
        recording: Some(Recording { path: recording, report: None }),
        notes: Some("never published".into()),
        tracks: vec![track("Burial - Archangel", false), track("Floorplan - Never Grow Old", true)],
        created_at: Utc::now(),
        updated_at: Utc::now(),
        ..Set::default()
    };
    let mut book = SetBook { sets: vec![set.clone()], ..SetBook::default() };
    book.save(store).unwrap();
    set
}

#[test]
fn a_set_is_published_after_its_dry_run_and_keeps_its_link() {
    let f = Fixture::echo();
    let s = set(&f.store, f.store.root());
    let plan = f.plugins.publish_plan("echo:nowhere", "last", &f.store, &Ctx::silent()).unwrap();
    assert_eq!((plan.operation(), plan.tracks, plan.destructive), ("echo.publish.nowhere", 2, false));
    assert_eq!(plan.summary, ["publish Friday (2 tracks) and its recording at Nowhere, saying where"]);
    assert_eq!(plan.warnings, ["2 tracks would be published"]);
    let asked = f.asked();
    let sent: Value = serde_json::from_str(asked[0].strip_prefix("set/publish ").unwrap()).unwrap();
    assert_eq!(sent["dry_run"], true);
    assert_eq!(
        sent["tracklist"],
        json!([{ "artist": "Burial", "title": "Archangel", "start_seconds": 60 }, { "start_seconds": 60 }])
    );
    assert!(!asked[0].contains("never published"), "a set's notes stay here");
    assert_eq!(SetBook::load(&f.store).unwrap().sets[0].link, None, "a dry run publishes nothing");
    let done = plan.apply(Some(&f.store), &Ctx::silent()).unwrap();
    let url = format!("https://sets.example/{}", s.id);
    assert_eq!(done.url.as_deref(), Some(url.as_str()));
    assert_eq!(SetBook::load(&f.store).unwrap().sets[0].link.as_deref(), Some(url.as_str()), "the host keeps the link");
    // Published again, the link it has is replaced: said so, and destructive.
    let again = f.plugins.publish_plan("echo:nowhere", &s.id, &f.store, &Ctx::silent()).unwrap();
    assert!(again.destructive && again.summary.contains(&format!("replace its link {url}")), "{:?}", again.summary);
    // Changed since it was planned: planned again.
    let mut book = SetBook::load(&f.store).unwrap();
    book.sets[0].updated_at = Utc::now() + chrono::Duration::seconds(5);
    book.save(&f.store).unwrap();
    let e = again.apply(Some(&f.store), &Ctx::silent()).unwrap_err();
    assert_eq!(e.code(), "stale_plan", "{e}");
}

#[test]
fn a_set_is_published_only_with_its_recording_at_a_set_destination() {
    let f = Fixture::new(
        |dir| {
            vec![spec(
                dir,
                &format!("{ECHO}\n[[contributes.settings]]\nkey = \"answer\"\ntitle = \"Answer\"\ntype = \"string\"\n"),
                &[],
            )]
        },
        Options::default(),
    );
    let s = set(&f.store, f.store.root());
    let e = f.plugins.publish_plan("echo:folder", "last", &f.store, &Ctx::silent()).err().unwrap();
    assert_eq!(e.to_string(), "echo:folder (A folder of its own) isn't a destination for a set");
    // A link that isn't one is refused, and the set keeps none.
    let answer = json!({ "url": "javascript:alert(1)" }).to_string();
    f.plugins.set_setting("echo", &f.store, "answer", Some(answer.into())).unwrap();
    let plan = f.plugins.publish_plan("echo:nowhere", "last", &f.store, &Ctx::silent()).unwrap();
    let e = plan.apply(Some(&f.store), &Ctx::silent()).unwrap_err().to_string();
    assert_eq!(
        e,
        "the Echo plugin failed: it says the set is at \"javascript:alert(1)\", which isn't an https:// address"
    );
    assert_eq!(SetBook::load(&f.store).unwrap().sets[0].link, None);
    std::fs::remove_file(&s.recording.as_ref().unwrap().path).unwrap();
    let e = f.plugins.publish_plan("echo:nowhere", "last", &f.store, &Ctx::silent()).err().unwrap();
    assert!(e.to_string().ends_with("friday.wav, the set's recording"), "{e}");
    let mut book = SetBook::load(&f.store).unwrap();
    book.sets[0].recording = None;
    book.save(&f.store).unwrap();
    let e = f.plugins.publish_plan("echo:nowhere", "last", &f.store, &Ctx::silent()).err().unwrap();
    assert_eq!(e.to_string(), "Friday has no recording to publish: a set destination publishes one");
}
