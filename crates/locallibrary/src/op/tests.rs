use super::*;

#[derive(Serialize)]
struct Plan {
    tracks: usize,
    warnings: Vec<String>,
}

impl Operation for Plan {
    type Request = ();
    type Report = usize;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, _: &Ctx<'_>) -> Result<usize> {
        Ok(planned.plan.tracks)
    }
}

fn write(root: &Path, key: &str, bytes: &[u8]) {
    let p = paths::to_fs(root, key);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

#[test]
fn a_plan_is_refused_once_what_it_touches_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, paths::EXPORT_PDB, b"one");
    write(root, "Contents/a.mp3", b"audio");
    let d = Device::open(root).unwrap();
    let plan = Plan { tracks: 3, warnings: vec!["careful".into()] };
    let planned = Planned::new("test", plan, ()).touches(Touched::drive(
        &d,
        true,
        vec![Probe::Databases, Probe::Listing("Contents".into())],
    ));
    assert_eq!(planned.apply(None, &Ctx::silent()).unwrap(), 3);

    // Rewritten with the same bytes: still the state that was planned.
    write(root, paths::EXPORT_PDB, b"one");
    assert!(planned.check(None).is_ok());
    write(root, paths::EXPORT_PDB, b"two");
    let err = planned.apply(None, &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::StalePlan(_)), "{err}");
    assert_eq!(err.code(), "stale_plan");
    write(root, paths::EXPORT_PDB, b"one");
    assert!(planned.check(None).is_ok());

    // A new file in a listed folder.
    write(root, "Contents/b.mp3", b"more");
    assert!(matches!(planned.check(None), Err(Error::StalePlan(_))));
}

/// A folder on this computer that wasn't there when planned (one an export makes) is as planned
/// while it still isn't; there now, or gone once it was, the plan is stale.
#[test]
fn a_folder_missing_when_planned_is_unchanged_while_it_stays_missing() {
    let dir = tempfile::tempdir().unwrap();
    let new = dir.path().join("new/dir");
    let planned = Planned::new("test", Plan { tracks: 1, warnings: Vec::new() }, ()).touches(Touched::path(
        &new,
        true,
        vec![Probe::Stamp(vec!["list.m3u".into()])],
    ));
    assert!(planned.check(None).is_ok());
    std::fs::create_dir_all(&new).unwrap();
    let said = planned.check(None).unwrap_err().to_string();
    assert!(said.contains("is there now since the plan was made"), "{said}");
    let there = Planned::new("test", Plan { tracks: 1, warnings: Vec::new() }, ()).touches(Touched::path(
        &new,
        true,
        vec![Probe::Stamp(vec!["list.m3u".into()])],
    ));
    std::fs::remove_dir(&new).unwrap();
    assert!(there.check(None).unwrap_err().to_string().contains("is gone since the plan was made"));
}

/// Writes its drive's database before it first locks the drive (`true`: as another write
/// slipping in after the check would), and between two locks (as its own next write does).
struct Writes;

impl Operation for Writes {
    type Request = bool;
    type Report = ();

    fn apply(planned: &Planned<Self, bool>, _: Option<&Store>, _: &Ctx<'_>) -> Result<()> {
        let d = planned.drive(0)?;
        if *planned.request() {
            write(d.root(), paths::EXPORT_PDB, b"slipped in");
        }
        drop(crate::commit::lock(&d, None)?);
        write(d.root(), paths::EXPORT_PDB, b"its own");
        drop(crate::commit::lock(&d, None)?);
        Ok(())
    }
}

#[test]
fn a_write_slipping_in_before_the_lock_is_refused_too() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(dir.path()).unwrap();
    let plan = |early| Planned::new("test", Writes, early).touches(Touched::library(&d, true));
    write(dir.path(), paths::EXPORT_PDB, b"one");
    assert!(matches!(plan(true).apply(None, &Ctx::silent()), Err(Error::StalePlan(_))));
    write(dir.path(), paths::EXPORT_PDB, b"one");
    plan(false).apply(None, &Ctx::silent()).unwrap();
    assert!(planned_databases(d.root()).is_none(), "nothing is noted once it's applied");
}

#[test]
fn stamps_and_identity() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "rec.wav", b"recording");
    let mut d = Device::open(root).unwrap();
    let t = Touched::drive(&d, true, vec![Probe::Identity, Probe::Stamp(vec!["rec.wav".into()])]);
    assert!(!t.changed());
    d.adopt(Some("Main")).unwrap();
    assert!(t.changed(), "a drive given an identity is another state");
    let t = Touched::drive(&d, true, vec![Probe::Identity, Probe::Stamp(vec!["rec.wav".into()])]);
    d.adopt(Some("Renamed")).unwrap();
    assert!(!t.changed(), "a new name changes nothing a plan depends on");
    std::fs::remove_file(root.join("rec.wav")).unwrap();
    assert!(t.changed());
}

struct Allowed;

impl Operation for Allowed {
    type Request = ();
    type Report = bool;

    fn apply(_: &Planned<Self>, _: Option<&Store>, _: &Ctx<'_>) -> Result<bool> {
        Ok(archive_writes_allowed())
    }
}

/// Regression: a plan made to write to archive drives lost that permission in what it ran
/// when applied without it (a provision's format and push).
#[test]
fn what_a_plan_runs_may_write_where_the_plan_may() {
    allow_archive_writes(true);
    let allowed = Planned::new("test", Allowed, ());
    allow_archive_writes(false);
    assert!(allowed.apply(None, &Ctx::silent()).unwrap());
    assert!(!archive_writes_allowed(), "only while it applies");
    assert!(!Planned::new("test", Allowed, ()).apply(None, &Ctx::silent()).unwrap());
}

struct Panics;

impl Operation for Panics {
    type Request = ();
    type Report = ();

    fn apply(_: &Planned<Self>, _: Option<&Store>, _: &Ctx<'_>) -> Result<()> {
        std::panic::resume_unwind(Box::new("stopped part way"))
    }
}

/// Regression: an operation that panicked left writes to archive drives allowed on its thread.
#[test]
fn a_plan_that_panics_gives_the_archive_permission_back() {
    allow_archive_writes(true);
    let plan = Planned::new("test", Panics, ());
    allow_archive_writes(false);
    let applied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plan.apply(None, &Ctx::silent())));
    assert!(applied.is_err());
    assert!(!archive_writes_allowed());
}

/// Regression: two drives without an identity measured the same when mounted at one path in
/// turn, so a plan made for one blank stick was applied to another.
#[test]
fn blank_drives_at_one_path_are_told_apart() {
    let dir = tempfile::tempdir().unwrap();
    let (root, other) = (dir.path().join("NO NAME"), dir.path().join("other"));
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&other).unwrap();
    let t = Touched::drive(&Device::open(&root).unwrap(), true, vec![Probe::Identity]);
    assert!(!t.changed());
    std::fs::rename(&root, dir.path().join("away")).unwrap();
    std::fs::rename(&other, &root).unwrap();
    assert!(t.changed(), "another volume at the same path");
}

#[test]
fn serialises_as_the_plan_with_its_details() {
    let plan = Plan { tracks: 3, warnings: vec!["careful".into()] };
    let planned = Planned::new("test", plan, ())
        .line("3 tracks")
        .warnings(["careful".to_string(), "and another".to_string()])
        .destructive(true);
    let v = serde_json::to_value(&planned).unwrap();
    assert_eq!(v["tracks"], 3);
    assert_eq!(v["operation"], "test");
    assert_eq!(v["plan_id"], planned.id.as_str());
    assert_eq!(v["summary"], serde_json::json!(["3 tracks"]));
    assert_eq!(v["warnings"], serde_json::json!(["careful", "and another"]));
    assert_eq!(v["destructive"], true);
    let any: Box<dyn AnyPlanned> = Box::new(planned);
    assert_eq!(any.to_json(), v);
    assert_eq!(any.apply_json(None, &Ctx::silent()).unwrap(), 3);
}

#[derive(Clone, Serialize, JsonSchema)]
struct Shaped {
    tracks: u32,
    note: String,
}

impl Operation for Shaped {
    type Request = ();
    type Report = ();

    fn apply(_: &Planned<Self>, _: Option<&Store>, _: &Ctx<'_>) -> Result<()> {
        Ok(())
    }
}

impl Operation for () {
    type Request = ();
    type Report = ();

    fn apply(_: &Planned<Self>, _: Option<&Store>, _: &Ctx<'_>) -> Result<()> {
        Ok(())
    }
}

impl Operation for Vec<u32> {
    type Request = ();
    type Report = usize;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, _: &Ctx<'_>) -> Result<usize> {
        Ok(planned.plan.len())
    }
}

/// `planned` as `*.plan` returns it, checked against the schema its type gives.
fn described<P: Serialize + JsonSchema>(planned: &Planned<P>) -> serde_json::Value {
    let json = serde_json::to_value(planned).unwrap();
    let settings = schemars::generate::SchemaSettings::draft2020_12().for_serialize();
    let schema = settings.into_generator().into_root_schema_for::<Planned<P>>().to_value();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(validator.is_valid(&json), "{json:#} isn't described by {schema:#}");
    json
}

/// A plan reads as its own fields with every plan's details beside them, whatever shape it is: an
/// object's fields at the top, nothing for one that's `null`, and anything else under `plan`; its
/// schema describes it either way. Its details read back as they were given, each warning once.
#[hegel::test(test_cases = 50)]
fn a_plan_reads_as_its_own_fields_beside_the_details(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let summary: Vec<String> = tc.draw(gs::vecs(gs::text().max_size(6)).max_size(3));
    let warnings: Vec<String> = tc.draw(gs::vecs(gs::sampled_from(vec!["a".to_string(), "b".into(), "c".into()])));
    let (destructive, noop) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let numbers: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>()).max_size(4));
    let shaped = Shaped { tracks: tc.draw(gs::integers::<u32>()), note: tc.draw(gs::text().max_size(6)) };
    fn dress<P>(planned: Planned<P>, summary: &[String], warnings: &[String], flags: (bool, bool)) -> Planned<P> {
        planned.summary(summary.to_vec()).warnings(warnings.to_vec()).destructive(flags.0).noop(flags.1)
    }
    let flags = (destructive, noop);
    let unit = dress(Planned::new("test", (), ()), &summary, &warnings, flags);
    let list = dress(Planned::new("test", numbers.clone(), ()), &summary, &warnings, flags);
    let object = dress(Planned::new("test", shaped.clone(), ()), &summary, &warnings, flags);
    let mut once: Vec<String> = Vec::new();
    for w in &warnings {
        if !once.contains(w) {
            once.push(w.clone());
        }
    }
    let details = ["plan_id", "operation", "summary", "warnings", "destructive", "noop", "drives", "planned_at"];
    for (json, own) in [
        (described(&unit), serde_json::json!({})),
        (described(&list), serde_json::json!({ "plan": numbers })),
        (described(&object), serde_json::to_value(&shaped).unwrap()),
    ] {
        assert_eq!(json["summary"], serde_json::json!(summary));
        assert_eq!(json["warnings"], serde_json::json!(once));
        assert_eq!((json["destructive"].as_bool(), json["noop"].as_bool()), (Some(destructive), Some(noop)));
        let own = own.as_object().unwrap();
        for (k, v) in own {
            assert_eq!(&json[k], v, "{k}");
        }
        let keys = json.as_object().unwrap().keys();
        assert!(keys.into_iter().all(|k| details.contains(&k.as_str()) || own.contains_key(k)), "{json}");
    }

    let json = serde_json::to_value(&list).unwrap();
    let any: Box<dyn AnyPlanned> = Box::new(list);
    assert_eq!((any.operation(), any.summary(), any.warnings()), ("test", &summary[..], &once[..]));
    assert_eq!((any.destructive(), any.is_noop(), any.drives().len()), (destructive, noop, 0));
    assert_eq!(json["plan_id"], any.id());
    assert!(any.planned_at() <= Utc::now());
    assert_eq!(any.to_json(), json);
    assert_eq!(any.apply_json(None, &Ctx::silent()).unwrap(), numbers.len());
}

/// A plan that reads files by content is stale exactly when one of them changes: rewritten with
/// other bytes, deleted, or a folder (which reads as unreadable) turned into a file. Rewriting it
/// with the same bytes, or changing a file it doesn't read, isn't a change.
#[hegel::test(test_cases = 20)]
fn a_plan_reading_files_by_content_sees_exactly_their_changes(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "a.txt", b"one");
    write(root, "b.txt", b"two");
    std::fs::create_dir(root.join("folder")).unwrap();
    let d = Device::open(root).unwrap();
    let t = Touched::drive(&d, true, vec![Probe::Content(vec!["a.txt".into(), "folder".into()])]);
    assert!(!t.changed(), "nothing changed yet");
    let stale = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => {
            write(root, "b.txt", b"changed");
            false
        }
        1 => {
            let bytes: Vec<u8> = tc.draw(gs::sampled_from(vec![b"one".to_vec(), b"once".to_vec(), Vec::new()]));
            write(root, "a.txt", &bytes);
            bytes != b"one"
        }
        2 => {
            std::fs::remove_dir(root.join("folder")).unwrap();
            write(root, "folder", b"a file now");
            true
        }
        _ => {
            std::fs::remove_file(root.join("a.txt")).unwrap();
            true
        }
    };
    assert_eq!(t.changed(), stale);
}
