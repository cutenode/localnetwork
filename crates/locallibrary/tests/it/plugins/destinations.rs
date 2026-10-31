//! Plugins' destinations, through the test plugin (echo's: a store kept in `destination/` where it
//! runs, and sets published nowhere): copying the store there in a folder copy's order, restoring
//! from it, checking what comes back, repairing a drive from it, and publishing a set.

use std::path::Path;

use chrono::{NaiveDate, Utc};
use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::api::{self, RepairRequest, VerifyRequest};
use locallibrary::catalog::LibraryOutcome;
use locallibrary::device::Device;
use locallibrary::error::Reason;
use locallibrary::hash::Hash;
use locallibrary::offsite::{self, CopyOptions, RestoreOptions};
use locallibrary::op::AnyPlanned;
use locallibrary::paths;
use locallibrary::profile::ProfilePrefs;
use locallibrary::scrub::{self, RepairSource};
use locallibrary::sets::{Recording, Set, SetBook, SetEntry};
use locallibrary::sync::{self, PullOptions};
use loclib_plugin::StrongRef;

use super::shared_accounts::{WITH, asking, mock_for, who};
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

/// Every string in `v` with a window title set after it: what a terminal shows of it is the same.
fn retitled(v: &mut Value) {
    match v {
        Value::String(s) => s.push_str("\u{1b}]0;retitled\u{7}"),
        Value::Array(list) => list.iter_mut().for_each(retitled),
        Value::Object(map) => map.values_mut().for_each(retitled),
        _ => {}
    }
}

/// The store's records a destination gives back are taken in as all a plugin answers is: their
/// text without control characters or escape sequences. Regression: they were written to the
/// store as the plugin gave them.
#[test]
fn a_destinations_records_come_back_printable() {
    let f = Fixture::echo();
    f.pulled();
    let dest = f.plugins.store_destination("echo:folder", Some(&f.store)).unwrap();
    offsite::copy_to(&f.store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap();
    f.plugins.shutdown();
    let other = Store::open_or_init(f.store.root().with_file_name("other")).unwrap();
    copy_dir(&Fixture::kept(&f.store), &Fixture::kept(&other));
    let record = Fixture::kept(&other).join("manifests/drives.json");
    let mut drives: Value = serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
    retitled(&mut drives);
    std::fs::write(&record, drives.to_string()).unwrap();
    let dest = f.plugins.store_destination("echo:folder", Some(&other)).unwrap();
    offsite::restore(&other, &dest, &RestoreOptions::default(), &Ctx::silent()).unwrap();
    let drives = |s: &Store| serde_json::from_slice::<Value>(&std::fs::read(s.root().join("drives.json")).unwrap());
    assert_eq!(drives(&other).unwrap(), drives(&f.store).unwrap(), "as the store wrote them");
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
    let plan = f.plugins.publish_plan("echo:nowhere", "last", &[], &f.store, &Ctx::silent()).unwrap();
    assert_eq!((plan.operation(), plan.tracks, plan.destructive), ("echo.publish.nowhere", 2, false));
    assert_eq!(
        plan.summary,
        [
            "publish Friday (2 tracks) and its recording at Nowhere, saying where",
            "track 2 isn't named: marked unnamed (`sets edit --named`)"
        ]
    );
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
    let kept = SetBook::load(&f.store).unwrap().sets.remove(0);
    assert_eq!(
        (kept.link.as_deref(), kept.link_name.as_deref()),
        (Some(url.as_str()), Some("Nowhere")),
        "the host keeps the link"
    );
    // Published again, the link it has is replaced: said so, and destructive.
    let again = f.plugins.publish_plan("echo:nowhere", &s.id, &[], &f.store, &Ctx::silent()).unwrap();
    assert!(again.destructive && again.summary.contains(&format!("replace its link {url}")), "{:?}", again.summary);
    // Changed since it was planned: planned again.
    let mut book = SetBook::load(&f.store).unwrap();
    book.sets[0].updated_at = Utc::now() + chrono::Duration::seconds(5);
    book.save(&f.store).unwrap();
    let e = again.apply(Some(&f.store), &Ctx::silent()).unwrap_err();
    assert_eq!(e.code(), "stale_plan", "{e}");
}

#[test]
fn tracks_the_profile_hides_or_hide_names_are_published_without_their_names() {
    let f = Fixture::echo();
    set(&f.store, f.store.root());
    let mut book = SetBook::load(&f.store).unwrap();
    book.sets[0].tracks.push(SetEntry { start_seconds: Some(60), ..SetEntry::added("Daft Punk - Around the World") });
    book.save(&f.store).unwrap();
    let prefs = ProfilePrefs { hidden_artists: vec!["burial".into()], ..ProfilePrefs::default() };
    prefs.save(&f.store).unwrap();
    let e = f.plugins.publish_plan("echo:nowhere", "last", &[4], &f.store, &Ctx::silent()).err().unwrap();
    assert_eq!(e.to_string(), "there's no track 4 to hide: the set has 3");
    let plan = f.plugins.publish_plan("echo:nowhere", "last", &[3], &f.store, &Ctx::silent()).unwrap();
    assert_eq!(
        plan.summary,
        [
            "publish Friday (3 tracks) and its recording at Nowhere, saying where",
            "track 2 isn't named: marked unnamed (`sets edit --named`)",
            "track 3 isn't named: hidden as asked",
            "track 1 isn't named: hidden by your profile (`profile unhide`)"
        ]
    );
    let asked = f.asked();
    assert!(!asked[0].contains("Burial") && !asked[0].contains("Daft Punk"), "{asked:?}");
    let sent: Value = serde_json::from_str(asked[0].strip_prefix("set/publish ").unwrap()).unwrap();
    assert_eq!(sent["tracklist"], json!([{ "start_seconds": 60 }, { "start_seconds": 60 }, { "start_seconds": 60 }]));
    // The profile changed since it was planned: planned again.
    ProfilePrefs::default().save(&f.store).unwrap();
    let e = plan.apply(Some(&f.store), &Ctx::silent()).unwrap_err();
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
    let e = f.plugins.publish_plan("echo:folder", "last", &[], &f.store, &Ctx::silent()).err().unwrap();
    assert_eq!(e.to_string(), "echo:folder (A folder of its own) isn't a destination for a set");
    // A link that isn't one is refused, and the set keeps none.
    let answer = json!({ "url": "javascript:alert(1)" }).to_string();
    f.plugins.set_setting("echo", &f.store, "answer", Some(answer.into())).unwrap();
    let plan = f.plugins.publish_plan("echo:nowhere", "last", &[], &f.store, &Ctx::silent()).unwrap();
    let e = plan.apply(Some(&f.store), &Ctx::silent()).unwrap_err().to_string();
    assert_eq!(
        e,
        "the Echo plugin failed: it says the set is at \"javascript:alert(1)\", which isn't an https:// address"
    );
    assert_eq!(SetBook::load(&f.store).unwrap().sets[0].link, None);
    // So is a record that isn't one, or a name too long for the link, and nothing of the answer
    // is kept.
    let refused = [
        (
            json!({ "url": "https://plyr.fm/track/1", "record": { "uri": "https://plyr.fm/track/1", "cid": TRACK.1 } }),
            format!(
                "it says the recording is the record \"https://plyr.fm/track/1\" ({:?}), which isn't an at:// URI and a CID",
                TRACK.1
            ),
        ),
        (
            json!({ "record": { "uri": TRACK.0, "cid": "bafy" } }),
            format!("it says the recording is the record {:?} (\"bafy\"), which isn't an at:// URI and a CID", TRACK.0),
        ),
        (
            json!({ "url": "https://plyr.fm/track/1", "name": "n".repeat(65) }),
            format!("it names the link {:?}, longer than 64 characters", "n".repeat(65)),
        ),
    ];
    for (answer, why) in refused {
        f.plugins.set_setting("echo", &f.store, "answer", Some(answer.to_string().into())).unwrap();
        let plan = f.plugins.publish_plan("echo:nowhere", "last", &[], &f.store, &Ctx::silent()).unwrap();
        let e = plan.apply(Some(&f.store), &Ctx::silent()).unwrap_err().to_string();
        assert_eq!(e, format!("the Echo plugin failed: {why}"));
        let kept = SetBook::load(&f.store).unwrap().sets.remove(0);
        assert_eq!((kept.link, kept.link_name, kept.recording_record), (None, None, None));
    }
    // Saying nowhere, the set keeps no link, and a name without one isn't kept; a record it gives
    // is.
    let track = StrongRef { uri: TRACK.0.into(), cid: TRACK.1.into() };
    for (answer, record) in [(json!({ "name": "plyr.fm" }), None), (json!({ "record": track }), Some(track.clone()))] {
        f.plugins.set_setting("echo", &f.store, "answer", Some(answer.to_string().into())).unwrap();
        let plan = f.plugins.publish_plan("echo:nowhere", "last", &[], &f.store, &Ctx::silent()).unwrap();
        let done = plan.apply(Some(&f.store), &Ctx::silent()).unwrap();
        assert_eq!((done.url, done.name, &done.record), (None, None, &record));
        let kept = SetBook::load(&f.store).unwrap().sets.remove(0);
        assert_eq!((kept.link, kept.link_name, kept.recording_record), (None, None, record));
    }
    std::fs::remove_file(&s.recording.as_ref().unwrap().path).unwrap();
    let e = f.plugins.publish_plan("echo:nowhere", "last", &[], &f.store, &Ctx::silent()).err().unwrap();
    assert!(e.to_string().ends_with("friday.wav, the set's recording"), "{e}");
    let mut book = SetBook::load(&f.store).unwrap();
    book.sets[0].recording = None;
    book.save(&f.store).unwrap();
    let e = f.plugins.publish_plan("echo:nowhere", "last", &[], &f.store, &Ctx::silent()).err().unwrap();
    assert_eq!(e.to_string(), "Friday has no recording to publish: a set destination publishes one");
}

/// What the test plugin answers publishing a set with, as its `answer` setting has it: where it
/// says the set is (or would be), what it names that link, the record it says holds the
/// recording (its URI and CID) and its warnings. None: echo's own answer.
type Answer = Option<(Option<&'static str>, Option<String>, Option<(&'static str, &'static str)>, Vec<String>)>;

/// A plyr.fm track, and its version.
const TRACK: (&str, &str) =
    ("at://did:plc:dj/fm.plyr.track/3m4n5o6p7q8r9", "bafyreie5737gdxlw5i64vzichcalba3z2v5n6icifvx5xytvske7mr3hpm");

/// For any set (titled or not, with any tracks, each named, marked unnamed or by an artist the
/// profile hides, with a link or none), any positions asked to be hidden and whatever the plugin
/// answers, publishing it at a set destination is planned with the plugin's dry run, which
/// publishes nothing: the plan names the set's heading and its tracks, which tracks go unnamed and
/// why, where the plugin says it would be, and the link it replaces (destructive exactly when
/// there's one), warning as the plugin does, and the plugin is sent the set's details and a
/// tracklist naming only the tracks it may, never the set's notes. Applied, the set's link is where
/// the plugin says it's published when that's an https address (named as the plugin names it, up
/// to 64 characters), stays as it was when it says nowhere, and the record it says holds the
/// recording is kept when it's an at:// URI and a CID; an address of another kind, a name too long
/// or a record that isn't one is refused, nothing of the answer kept. A set, recording or profile
/// changed since it was planned is planned again. A destination of another kind, or none, a set
/// without its recording and a position to hide it doesn't have are refused before the plugin's
/// asked.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_set_is_published_as_its_dry_run_said(tc: TestCase) {
    let manifest =
        format!("{ECHO}\n[[contributes.settings]]\nkey = \"answer\"\ntitle = \"Answer\"\ntype = \"string\"\n");
    let f = Fixture::new(|dir| vec![spec(dir, &manifest, &[])], Options::default());
    let titled = tc.draw(gs::booleans());
    // Each named (0), marked unnamed (1) or by an artist the profile hides (2).
    let tracks: Vec<(u8, Option<u32>)> = tc.draw(
        gs::vecs(gs::tuples!(
            gs::sampled_from(vec![0, 0, 0, 1, 2]),
            gs::optional(gs::integers::<u32>().max_value(7200))
        ))
        .max_size(4),
    );
    let hide: Vec<usize> =
        tc.draw(gs::vecs(gs::integers::<usize>().min_value(1).max_value(tracks.len() + 1)).max_size(2));
    let hidden_artists = vec!["Hidden".to_string()];
    ProfilePrefs { hidden_artists, ..ProfilePrefs::default() }.save(&f.store).unwrap();
    let link = tc.draw(gs::optional(gs::just("https://old.example/friday")));
    // 0: on this computer, 1: gone since, 2: none.
    let recording_is = tc.draw(gs::sampled_from(vec![0, 0, 0, 1, 2]));
    let recording = f.store.root().join("friday.wav");
    std::fs::write(&recording, b"RIFF").unwrap();
    let entries = tracks.iter().enumerate().map(|(i, &(how, start_seconds))| SetEntry {
        unnamed: how == 1,
        start_seconds,
        ..SetEntry::added(&format!("{} - Title {i}", if how == 2 { "Hidden".into() } else { format!("Artist {i}") }))
    });
    let date = NaiveDate::from_ymd_opt(2026, 10, 2).unwrap();
    let s = Set {
        id: "3m4nset0000000".into(),
        title: titled.then(|| "Friday".into()),
        date,
        recording: (recording_is < 2).then(|| Recording { path: recording.clone(), report: None }),
        notes: Some("never published".into()),
        link: link.map(String::from),
        tracks: entries.collect(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        ..Set::default()
    };
    SetBook { sets: vec![s.clone()], ..SetBook::default() }.save(&f.store).unwrap();
    if recording_is == 1 {
        std::fs::remove_file(&recording).unwrap();
    }
    let answer: Answer = tc.draw(gs::optional(gs::tuples!(
        gs::sampled_from(vec![None, Some("https://pub.example/friday"), Some("http://pub.example/friday")]),
        gs::optional(hegel::one_of!(
            gs::sampled_from(vec![" plyr.fm ".to_string(), " ".to_string(), "é".repeat(64), "n".repeat(65)]),
            gs::text().max_size(64)
        )),
        gs::optional(gs::sampled_from(vec![TRACK, ("https://plyr.fm/track/1", TRACK.1), (TRACK.0, "bafy"),])),
        gs::vecs(gs::sampled_from(vec!["heard".to_string(), "slow".to_string()])).max_size(2)
    )));
    if let Some((url, name, record, warnings)) = &answer {
        let record = record.map(|(uri, cid)| json!({ "uri": uri, "cid": cid }));
        let said = json!({ "url": url, "name": name, "record": record, "warnings": warnings }).to_string();
        f.plugins.set_setting("echo", &f.store, "answer", Some(said.into())).unwrap();
    }
    let (spec_of, refused) = match tc.draw(gs::integers::<u8>().max_value(9)) {
        0 => ("echo:folder", Some("echo:folder (A folder of its own) isn't a destination for a set")),
        1 => ("echo:nope", Some("not found: the echo plugin's destination \"nope\"")),
        2 => ("echo", Some("\"echo\" names no destination: <plugin>:<destination>")),
        _ => ("echo:nowhere", None),
    };
    let query = tc.draw(gs::sampled_from(vec!["last", "3m4nset", "Friday"]));
    let query = if query == "Friday" && !titled { "last" } else { query };
    let heading = if titled { "Friday".to_string() } else { date.to_string() };
    let planned = f.plugins.publish_plan(spec_of, query, &hide, &f.store, &Ctx::silent());
    let asked = || std::fs::read_to_string(Fixture::kept(&f.store).with_file_name("asked")).unwrap_or_default();
    if let Some(said) = refused {
        assert_eq!(planned.err().unwrap().to_string(), said);
        assert!(asked().is_empty(), "the plugin was asked");
        return;
    }
    if recording_is > 0 {
        let e = planned.err().unwrap();
        match recording_is {
            1 => assert!(e.code() == "not_found" && e.to_string().ends_with("friday.wav, the set's recording"), "{e}"),
            _ => assert_eq!(
                e.to_string(),
                format!("{heading} has no recording to publish: a set destination publishes one")
            ),
        }
        assert!(asked().is_empty(), "the plugin was asked");
        return;
    }
    let n = tracks.len();
    if let Some(bad) = hide.iter().find(|&&p| p > n) {
        assert_eq!(planned.err().unwrap().to_string(), format!("there's no track {bad} to hide: the set has {n}"));
        assert!(asked().is_empty(), "the plugin was asked");
        return;
    }
    let plan = planned.unwrap();
    // Positions (from 1) of the tracks each reason leaves unnamed: marked, asked, the profile.
    let why = |of: &dyn Fn(usize, u8) -> bool| -> Vec<usize> {
        (1..).zip(&tracks).filter(|&(p, &(how, _))| of(p, how)).map(|(p, _)| p).collect()
    };
    let notes = [
        (why(&|_, how| how == 1), "marked unnamed (`sets edit --named`)"),
        (why(&|p, how| how != 1 && hide.contains(&p)), "hidden as asked"),
        (why(&|p, how| how == 2 && !hide.contains(&p)), "hidden by your profile (`profile unhide`)"),
    ];
    let notes = notes.iter().filter(|(at, _)| !at.is_empty()).map(|(at, why)| match at.as_slice() {
        [one] => format!("track {one} isn't named: {why}"),
        _ => format!("tracks {} aren't named: {why}", at.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")),
    });
    let would_be = answer.as_ref().and_then(|(url, ..)| *url);
    let mut summary = vec![format!("publish {heading} ({n} tracks) and its recording at Nowhere, saying where")];
    summary.extend(notes);
    summary.extend(would_be.map(|u| format!("it would be at {u}")));
    summary.extend(link.map(|l| format!("replace its link {l}")));
    assert_eq!(plan.summary, summary);
    // Each once.
    let mut warnings = match &answer {
        Some((.., w)) => w.clone(),
        None => vec![format!("{n} tracks would be published")],
    };
    warnings.dedup();
    assert_eq!(plan.warnings, warnings);
    assert_eq!((plan.operation(), plan.tracks, plan.destructive), ("echo.publish.nowhere", n, link.is_some()));
    assert_eq!((plan.url.as_deref(), plan.replaces.as_deref(), &plan.recording), (would_be, link, &recording));
    let sent = asked();
    assert!(!sent.contains("never published"), "a set's notes stay here");
    let sent: Value = serde_json::from_str(sent.trim_end().strip_prefix("set/publish ").unwrap()).unwrap();
    let tracklist: Vec<Value> = tracks
        .iter()
        .enumerate()
        .map(|(i, &(how, start))| {
            let mut e = json!({});
            if how == 0 && !hide.contains(&(i + 1)) {
                e = json!({ "artist": format!("Artist {i}"), "title": format!("Title {i}") });
            }
            if let Some(start) = start {
                e["start_seconds"] = start.into();
            }
            e
        })
        .collect();
    let mut details = json!({ "id": s.id, "date": date.to_string() });
    if titled {
        details["title"] = "Friday".into();
    }
    assert_eq!((&sent["dry_run"], &sent["set"], &sent["tracklist"]), (&json!(true), &details, &json!(tracklist)));
    assert_eq!(SetBook::load(&f.store).unwrap().sets[0].link.as_deref(), link, "a dry run publishes nothing");

    // Changed since it was planned, or not.
    let changed = tc.draw(gs::sampled_from(vec![None, None, None, Some("set"), Some("recording"), Some("profile")]));
    match changed {
        Some("set") => {
            let mut book = SetBook::load(&f.store).unwrap();
            book.sets[0].updated_at = Utc::now() + chrono::Duration::seconds(5);
            book.save(&f.store).unwrap();
        }
        Some("profile") => ProfilePrefs::default().save(&f.store).unwrap(),
        Some(_) => std::fs::write(&recording, b"RIFF, longer").unwrap(),
        None => {}
    }
    let done = plan.apply(Some(&f.store), &Ctx::silent());
    let now = || {
        let set = SetBook::load(&f.store).unwrap().sets.remove(0);
        (set.link, set.link_name, set.recording_record)
    };
    let before = (link.map(String::from), None, None);
    if changed.is_some() {
        assert_eq!(done.unwrap_err().code(), "stale_plan");
        assert_eq!(now(), before);
        return;
    }
    let echoed = format!("https://sets.example/{}", s.id);
    let (published, name, record) = match &answer {
        Some((url, name, record, _)) => (*url, name.clone(), *record),
        None => (Some(echoed.as_str()), Some("Nowhere".to_string()), None),
    };
    // A name goes with a link, trimmed; a blank one is none.
    let name = published.and(name).map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    let refused = match (published, &name, record) {
        (Some(url), ..) if !url.starts_with("https://") => {
            Some(format!("it says the set is at {url:?}, which isn't an https:// address"))
        }
        (_, Some(name), _) if name.chars().count() > 64 => {
            Some(format!("it names the link {name:?}, longer than 64 characters"))
        }
        (.., Some((uri, cid))) if (uri, cid) != TRACK => {
            Some(format!("it says the recording is the record {uri:?} ({cid:?}), which isn't an at:// URI and a CID"))
        }
        _ => None,
    };
    if let Some(why) = refused {
        assert_eq!(done.unwrap_err().to_string(), format!("the Echo plugin failed: {why}"));
        assert_eq!(now(), before, "nothing of an answer that isn't right is kept");
        return;
    }
    let done = done.unwrap();
    let record = record.map(|(uri, cid)| StrongRef { uri: uri.into(), cid: cid.into() });
    assert_eq!((done.set.as_str(), done.url.as_deref()), (s.id.as_str(), published));
    assert_eq!((&done.name, &done.record), (&name, &record));
    assert_eq!(done.warnings, answer.map(|(.., w)| w).unwrap_or_default());
    let kept = match published {
        Some(url) => (Some(url.to_string()), name),
        None => (before.0, None),
    };
    assert_eq!(now(), (kept.0, kept.1, record), "the host keeps where it's published, and the record");
}

/// How long the mock service's tokens last in
/// [`a_destination_starts_only_when_its_accounts_will_last`].
#[derive(Debug, Clone, Copy)]
enum Lasting {
    /// An hour, then they're refreshed.
    Refreshed,
    /// Two hours, with nothing to refresh them with.
    TwoHours,
    /// Half an hour, with nothing to refresh them with.
    HalfAnHour,
    /// Until they're revoked.
    Ever,
}

/// What's wrong with echo's access to the account in
/// [`a_destination_starts_only_when_its_accounts_will_last`].
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mistake {
    /// The account is signed out.
    SignedOut,
    /// The account's tokens are lost.
    Lost,
    /// The access lacks a scope echo needs now.
    Ungiven,
}

/// For echo needing any of the mock type's `read` and `write`, an account signed in for it or not,
/// the service granting any of what they ask, its tokens refreshed or lasting two hours, half an
/// hour or until they're revoked, and the account then signed out, its tokens lost or echo's access
/// lacking a scope it needs, or none of these: echo's store destination, and publishing a set with
/// a recording small or a gigabyte big (which takes over two hours to send at the slowest), start
/// only when echo has a default access to an account signed in, with every scope it needs, each
/// granted what it asks, and staying signed in for an hour besides sending the recording when it
/// isn't refreshed; otherwise each fails as `account` (`no_access`, `not_signed_in`,
/// `missing_scope`), offering to sign in again as the account once echo has access, and echo is
/// never asked. Publishing checks again before it's applied: a plan whose account was signed out
/// since is refused, and echo isn't asked to publish.
#[hegel::test(test_cases = 16, suppress_health_check = [hegel::HealthCheck::TooSlow])]
#[hegel::explicit_test_case(
    needs = vec!["read"],
    lasting = Lasting::TwoHours,
    grants = vec!["read"],
    given = true,
    mistake = Option::<Mistake>::None,
    publishing = true,
    big = false,
    later = true
)]
fn a_destination_starts_only_when_its_accounts_will_last(tc: TestCase) {
    let some = vec![vec![], vec!["read"], vec!["write"], vec!["read", "write"]];
    let needs: Vec<&str> = tc.draw(gs::sampled_from(some[1..].to_vec()));
    let m = mock_for(3600, "/userinfo", "", &asking(&needs, &[]));
    let lasting =
        tc.draw(gs::sampled_from(vec![Lasting::Refreshed, Lasting::TwoHours, Lasting::HalfAnHour, Lasting::Ever]));
    let grants: Vec<&str> = tc.draw(gs::sampled_from(some));
    {
        let mut s = m.state.lock().unwrap();
        s.refreshless = !matches!(lasting, Lasting::Refreshed);
        s.expires_in = match lasting {
            Lasting::Refreshed => Some(3600),
            Lasting::TwoHours => Some(7200),
            Lasting::HalfAnHour => Some(1800),
            Lasting::Ever => None,
        };
        s.grants = Some(grants.iter().map(|g| g.to_string()).collect());
    }
    let given = tc.draw(gs::booleans());
    m.sign_in_for(given.then_some("echo"), who(json!("a"), None), None, &[]).0.unwrap();
    let mistake =
        tc.draw(gs::sampled_from(vec![None, Some(Mistake::SignedOut), Some(Mistake::Lost), Some(Mistake::Ungiven)]));
    let store = &m.f.store;
    match mistake {
        Some(Mistake::SignedOut) => drop(m.f.plugins.sign_out_account(WITH, "a", store).unwrap()),
        Some(Mistake::Lost) => std::fs::remove_file(store.root().join("secrets/accounts.json")).unwrap(),
        Some(Mistake::Ungiven) if given => {
            let file = store.root().join("accounts.json");
            let mut kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            kept["accesses"][0]["scopes"] = json!(needs[1..]);
            std::fs::write(&file, kept.to_string()).unwrap();
        }
        _ => {}
    }
    let publishing = tc.draw(gs::booleans());
    let big = tc.draw(gs::booleans());
    let big = big && publishing;

    let recording = store.root().join("friday.wav");
    std::fs::File::create(&recording).unwrap().set_len(if big { 1 << 30 } else { 4 }).unwrap();
    let s = Set {
        id: "3m4nset0000000".into(),
        title: Some("Friday".into()),
        date: NaiveDate::from_ymd_opt(2026, 10, 2).unwrap(),
        recording: Some(Recording { path: recording.clone(), report: None }),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        ..Set::default()
    };
    SetBook { sets: vec![s], ..SetBook::default() }.save(store).unwrap();

    let asks = |scope: &str| if scope == "write" { vec!["write", "read"] } else { vec!["read"] };
    let granted = needs.iter().all(|n| asks(n).iter().all(|a| grants.contains(a)));
    let lasts = match lasting {
        Lasting::Refreshed | Lasting::Ever => true,
        Lasting::TwoHours => !big,
        Lasting::HalfAnHour => false,
    };
    let expected = match mistake {
        _ if !given => Some(Reason::NoAccess),
        Some(Mistake::SignedOut | Mistake::Lost) => Some(Reason::NotSignedIn),
        Some(Mistake::Ungiven) => Some(Reason::MissingScope),
        None if !granted => Some(Reason::MissingScope),
        None if !lasts => Some(Reason::NotSignedIn),
        None => None,
    };
    let asked = || std::fs::read_to_string(Fixture::kept(store).with_file_name("asked")).unwrap_or_default();
    let started = match publishing {
        true => m.f.plugins.publish_plan("echo:nowhere", "last", &[], store, &Ctx::silent()).map(Some),
        false => m.f.plugins.store_destination("echo:folder", Some(store)).map(|_| None),
    };
    let said = format!("{lasting:?} {mistake:?} given {given} needs {needs:?} grants {grants:?} big {big}");
    let Some(reason) = expected else {
        let Some(plan) = started.unwrap() else { return };
        assert_eq!(asked().lines().count(), 1, "{said}: the dry run");
        let later = tc.draw(gs::booleans());
        if later {
            m.f.plugins.sign_out_account(WITH, "a", store).unwrap();
        }
        let done = plan.apply(Some(store), &Ctx::silent());
        assert_eq!(done.is_ok(), !later, "{said}");
        if later {
            assert_eq!(done.unwrap_err().reason(), Some(Reason::NotSignedIn), "{said}");
            assert_eq!(asked().lines().count(), 1, "{said}: asked to publish");
        }
        return;
    };
    let e = started.err().unwrap();
    assert_eq!((e.code(), e.reason()), ("account", Some(reason)), "{said}: {e}");
    assert!(asked().is_empty(), "{said}: echo was asked");
    // What resolves it for the account echo has access to comes first: giving its access what it
    // lacks when the account was granted that, else signing in again as the account.
    let actions = e.detail().actions;
    let first = actions.first().and_then(|a| Some((a.method.clone()?, a.params.get("account")?.clone())));
    let givable = matches!(mistake, Some(Mistake::Ungiven)) && asks(needs[0]).iter().all(|a| grants.contains(a));
    let resolving = given.then(|| match givable {
        true => ("accounts.access.give".to_string(), json!({ "plugin": WITH, "subject": "a" })),
        false => ("accounts.sign_in.begin".to_string(), json!("a")),
    });
    assert_eq!(first, resolving, "{said}: {actions:?}");
}

/// A step of [`a_store_copied_through_a_plugin_comes_back_as_it_was_last_copied`].
#[derive(Debug, Clone, Copy)]
enum Step {
    /// A track added to the drive, and the drive pulled.
    Pull,
    /// The store copied to the destination, planned first with a dry run or not.
    Copy { planned: bool },
}

/// For any order of pulls and copies to a plugin's store destination (each planned with a dry
/// run or not), a dry run asks the plugin to keep nothing and says what the copy after it copies,
/// a copy brings the destination up to date with every stored file the store has, and one with
/// nothing new copies nothing; what can't be done there (mirroring, verifying), and a destination
/// that isn't one, are refused. A store that lost everything brings back from it (planned first
/// or not) the libraries and stored files the store had when it was last copied, or is told
/// there's no copy there; and copying from that store to the destination, which holds the first
/// one's copy, is refused unless forced.
#[hegel::test(test_cases = 15, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_store_copied_through_a_plugin_comes_back_as_it_was_last_copied(tc: TestCase) {
    let f = Fixture::echo();
    let spec_of = tc.draw(gs::sampled_from(vec!["echo:folder", "echo:folder", "echo:nowhere", "nope:folder"]));
    match spec_of {
        "echo:nowhere" => {
            let e = f.plugins.store_destination(spec_of, Some(&f.store)).err().unwrap();
            assert_eq!(e.to_string(), "echo:nowhere (Nowhere, saying where) isn't a destination for a store");
        }
        "nope:folder" => {
            let e = f.plugins.store_destination(spec_of, Some(&f.store)).err().unwrap();
            assert_eq!(e.to_string(), "not found: plugin \"nope\"");
        }
        _ => {}
    }
    let dest = f.plugins.store_destination("echo:folder", Some(&f.store)).unwrap();
    let refused = tc.draw(gs::sampled_from(vec![
        None,
        Some(CopyOptions { mirror: true, ..Default::default() }),
        Some(CopyOptions { verify: true, ..Default::default() }),
    ]));
    if let Some(opts) = refused {
        let e = offsite::copy_to(&f.store, &dest, &opts, &Ctx::silent()).unwrap_err();
        assert_eq!(e.to_string(), "echo:folder keeps what it's given: a copy there can't be mirrored or verified");
    }
    let asked = || std::fs::read_to_string(Fixture::kept(&f.store).with_file_name("asked")).unwrap_or_default();
    let steps = tc.draw(
        gs::vecs(gs::sampled_from(vec![Step::Pull, Step::Copy { planned: false }, Step::Copy { planned: true }]))
            .min_size(1)
            .max_size(4),
    );
    // The libraries and stored files the store had when it was last copied.
    let (mut last_copied, mut pulled_since) = (None, false);
    for (i, step) in steps.into_iter().enumerate() {
        match step {
            Step::Pull => {
                if i > 0 {
                    common::export_new_track(&f.drive, "Artist", &format!("Track {i}"));
                }
                f.pulled();
                pulled_since = true;
            }
            Step::Copy { planned } => {
                let dry = planned.then(|| {
                    let seen = asked().len();
                    let opts = CopyOptions { dry_run: true, ..Default::default() };
                    let dry = offsite::copy_to(&f.store, &dest, &opts, &Ctx::silent()).unwrap();
                    let kept =
                        asked()[seen..].lines().any(|a| a.starts_with("blobs/put") || a.starts_with("manifest/put"));
                    assert!(dry.dry_run && !kept, "a dry run had the plugin keep something: {dry:?}");
                    dry
                });
                let r = offsite::copy_to(&f.store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap();
                assert!(r.complete && r.problems.is_empty() && r.path == Path::new("echo:folder"), "{r:?}");
                let all = objects(&f.store).len();
                assert_eq!(r.objects_copied + r.objects_present, all, "{r:?}");
                assert_eq!(r.snapshots, f.store.snapshot_ids().unwrap().len(), "{r:?}");
                if let Some(dry) = dry {
                    let said = (dry.objects_copied, dry.bytes_copied, dry.snapshots_copied, dry.objects_present);
                    assert_eq!(said, (r.objects_copied, r.bytes_copied, r.snapshots_copied, r.objects_present));
                }
                if last_copied.is_some() && !pulled_since {
                    assert_eq!((r.objects_copied, r.snapshots_copied), (0, 0), "nothing new, nothing copied");
                }
                pulled_since = false;
                let libraries: Vec<(String, Option<Hash>)> =
                    f.store.libraries().unwrap().into_iter().map(|l| (l.name, l.head)).collect();
                last_copied = Some((libraries, objects(&f.store)));
            }
        }
    }

    f.plugins.shutdown();
    let other = Store::open_or_init(f.store.root().with_file_name("other")).unwrap();
    if Fixture::kept(&f.store).exists() {
        copy_dir(&Fixture::kept(&f.store), &Fixture::kept(&other));
    }
    let dest = f.plugins.store_destination("echo:folder", Some(&other)).unwrap();
    let Some((libraries, stored)) = last_copied else {
        let e = offsite::restore(&other, &dest, &RestoreOptions::default(), &Ctx::silent()).unwrap_err();
        assert_eq!(e.to_string(), "not found: a copy of a store at echo:folder");
        return;
    };
    let only = tc.draw(gs::sampled_from(vec![vec![], vec!["usb".to_string()], vec!["nope".to_string()]]));
    let known = libraries.iter().any(|(name, _)| only.iter().all(|o| name.eq_ignore_ascii_case(o)));
    let opts = RestoreOptions { libraries: only.clone(), dry_run: false };
    if !only.is_empty() && !known {
        let e = offsite::restore(&other, &dest, &opts, &Ctx::silent()).unwrap_err();
        assert_eq!(e.code(), "not_found", "{e}");
        assert!(other.libraries().unwrap().is_empty() && objects(&other).is_empty());
        return;
    }
    if tc.draw(gs::booleans()) {
        let dry = RestoreOptions { dry_run: true, ..opts.clone() };
        let planned = offsite::restore(&other, &dest, &dry, &Ctx::silent()).unwrap();
        assert_eq!((planned.files, planned.present), (stored.len(), 0), "{planned:?}");
        assert!(other.libraries().unwrap().is_empty() && objects(&other).is_empty(), "a dry run fetches nothing");
    }
    let r = offsite::restore(&other, &dest, &opts, &Ctx::silent()).unwrap();
    assert_eq!((r.files, r.peer.as_str()), (stored.len(), "echo:folder"), "{r:?}");
    let back: Vec<(String, Option<Hash>)> = other.libraries().unwrap().into_iter().map(|l| (l.name, l.head)).collect();
    assert_eq!((back, objects(&other)), (libraries, stored));

    // The destination holds the first store's copy.
    let force = tc.draw(gs::booleans());
    let copied = offsite::copy_to(&other, &dest, &CopyOptions { force, ..Default::default() }, &Ctx::silent());
    match force {
        true => assert!(copied.unwrap().complete),
        false => assert_eq!(copied.unwrap_err().code(), "conflict"),
    }
}

/// The test plugin reaching servers on this computer, uploading as its `upload` setting says.
fn uploading() -> Fixture {
    let upload = "\n[[contributes.settings]]\nkey = \"upload\"\ntitle = \"Upload\"\ntype = \"string\"\n";
    super::net::fixture(&format!("{}{upload}", super::net::talking(100.0)), &[], Options::default())
}

/// For any recording and any way of cutting it into parts, each sent by a request of its own (as
/// the Internet Archive takes them), publishing a set sends each part of the recording itself,
/// named as the plugin was given it, saying how long it is, and the bytes sent are the
/// publishing's progress, between the plugin's start and end of the upload. Its dry run may send
/// none of it, and once it's published, nothing may.
#[hegel::test(test_cases = 5, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_set_s_recording_is_sent_in_parts_as_it_s_published(tc: TestCase) {
    use locallibrary::progress::{CancelToken, Event, Phase};
    let server = super::net::Server::new(|_, _| super::net::ok(""));
    let f = uploading();
    let s = set(&f.store, f.store.root());
    let recording = s.recording.unwrap().path;
    let size = tc.draw(gs::integers::<usize>().max_value(300_000));
    std::fs::write(&recording, super::net::letters(size)).unwrap();
    let mut cuts: Vec<usize> = tc.draw(gs::vecs(gs::integers::<usize>().max_value(size)).max_size(2));
    cuts.sort_unstable();
    let starts: Vec<usize> = std::iter::once(0).chain(cuts.iter().copied()).collect();
    let ends: Vec<usize> = cuts.iter().copied().chain(std::iter::once(size)).collect();
    // The last part to the recording's end, saying no length.
    let fetches: Vec<Value> = (0..starts.len())
        .map(|i| {
            let range = match i + 1 == starts.len() {
                true => json!({ "offset": starts[i] }),
                false => json!({ "offset": starts[i], "length": ends[i] - starts[i] }),
            };
            let url = format!("{}/part/{i}", server.url);
            json!({ "url": url, "method": "PUT", "body_blob": "recording", "body_range": range })
        })
        .collect();
    let upload = serde_json::to_string(&fetches).unwrap();
    f.plugins.set_setting("echo", &f.store, "upload", Some(upload.into())).unwrap();

    let plan = f.plugins.publish_plan("echo:nowhere", "last", &[], &f.store, &Ctx::silent()).unwrap();
    let refused =
        format!("{} isn't in the scratch folder of a request under way, nor a file one was given", recording.display());
    assert!(plan.warnings.iter().any(|w| w.ends_with(&refused)), "{:?}", plan.warnings);
    assert!(server.got().is_empty(), "the dry run sent nothing");

    let events = Mutex::new(Vec::new());
    let progress = |e: &Event| events.lock().unwrap().push(e.clone());
    plan.apply(Some(&f.store), &Ctx::new(&progress, CancelToken::new())).unwrap();
    let got = server.got();
    let all = super::net::letters(size);
    for (i, got) in got.iter().enumerate() {
        assert_eq!(got.target, format!("/part/{i}"));
        assert_eq!(got.body.as_bytes(), &all[starts[i]..ends[i]]);
        assert_eq!(got.headers["content-length"], (ends[i] - starts[i]).to_string());
    }
    assert_eq!(got.len(), fetches.len());
    let events = events.into_inner().unwrap();
    let upload: Vec<&Event> = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::PhaseStart { phase: Phase::Upload, .. }
                    | Event::Bytes { phase: Phase::Upload, .. }
                    | Event::PhaseEnd { phase: Phase::Upload }
            )
        })
        .collect();
    assert!(matches!(upload.first(), Some(Event::PhaseStart { bytes, .. }) if *bytes == size as u64), "{upload:?}");
    assert!(matches!(upload.last(), Some(Event::PhaseEnd { .. })), "{upload:?}");
    let sent: u64 = upload[1..upload.len() - 1]
        .iter()
        .map(|e| match e {
            Event::Bytes { bytes, .. } => *bytes,
            other => panic!("only bytes sent between: {other:?}"),
        })
        .sum();
    assert_eq!(sent, size as u64);

    // Published, its recording is no longer the plugin's to send.
    let fetch = json!({ "url": server.url.clone(), "method": "PUT", "body_blob": recording });
    let e = f.fetch(fetch);
    assert!(e["error"]["message"].as_str().unwrap().ends_with(&refused), "{e}");
    assert_eq!(server.got().len(), fetches.len());
}

/// Publishing cancelled while the recording is being sent stops sending it.
#[test]
fn a_recording_stops_being_sent_once_publishing_is_cancelled() {
    use std::io::Read;
    use std::net::TcpListener;

    use locallibrary::progress::{CancelToken, NoProgress};
    // A server that cancels publishing once the recording is being sent (a timer can go off
    // before the plugin has started, and then nothing is sent to stop), and reads no more of it
    // until publishing has stopped: what the host sends meanwhile waits in the connection's
    // buffers. Then it reads the rest at once, until the upload stops or all of it came,
    // answering how much it read. Publishing stops once the plugin answers, which it does after
    // passing the cancel on to its `net/fetch`.
    let size = 32 << 20;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let cancel = CancelToken::new();
    let cancelling = cancel.clone();
    let (stopped, publishing_stopped) = std::sync::mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let mut stream = listener.incoming().next().unwrap().unwrap();
        let mut buf = [0; 8192];
        let mut read = stream.read(&mut buf).unwrap();
        cancelling.cancel();
        let _ = publishing_stopped.recv();
        while read < size
            && let Ok(n @ 1..) = stream.read(&mut buf)
        {
            read += n;
        }
        read
    });
    let f = uploading();
    let s = set(&f.store, f.store.root());
    std::fs::write(&s.recording.unwrap().path, vec![0; size]).unwrap();
    let upload = json!([{ "url": format!("{url}/mix"), "method": "PUT", "body_blob": "recording" }]).to_string();
    f.plugins.set_setting("echo", &f.store, "upload", Some(upload.into())).unwrap();
    let plan = f.plugins.publish_plan("echo:nowhere", "last", &[], &f.store, &Ctx::silent()).unwrap();
    let e = plan.apply(Some(&f.store), &Ctx::new(&NoProgress, cancel)).unwrap_err();
    assert_eq!(e.code(), "cancelled", "{e}");
    drop(stopped);
    let read = server.join().unwrap();
    assert!(read < size / 2, "{read} of {size} sent");
    assert_eq!(SetBook::load(&f.store).unwrap().sets[0].link, None);
}
