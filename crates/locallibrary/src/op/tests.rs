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
