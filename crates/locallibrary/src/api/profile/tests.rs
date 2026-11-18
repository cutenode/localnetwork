use super::*;

#[test]
fn days_cover_years_and_months() {
    assert_eq!(parse_day("2026", false).unwrap(), NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
    assert_eq!(parse_day("2026", true).unwrap(), NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
    assert_eq!(parse_day("2026-02", true).unwrap(), NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
    assert_eq!(parse_day("2026-09-30", false).unwrap(), NaiveDate::from_ymd_opt(2026, 9, 30).unwrap());
    assert!(parse_day("last week", false).is_err());
    assert!(parse_day("2026-13", false).is_err());
}

#[test]
fn a_month_starts_on_its_first_and_a_day_has_three_parts_at_most() {
    assert_eq!(parse_day("2026-09", false).unwrap(), NaiveDate::from_ymd_opt(2026, 9, 1).unwrap());
    assert!(parse_day("2026-09-30-12", false).is_err());
    assert!(parse_day("1900", false).is_err(), "too long ago to be a set");
}

/// Copies the folder `from` into `to`, as a drive's files.
fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        match e.file_type().unwrap().is_dir() {
            true => copy_dir(&e.path(), &to.join(e.file_name())),
            false => drop(std::fs::copy(e.path(), to.join(e.file_name())).unwrap()),
        }
    }
}

/// rekordbox's collection on this computer is the one a profile counts, as it is now: a drive's
/// export (a copy from some earlier day, with other tracks) still reads, for its history, but its
/// tracks aren't the collection. Naming a drive, or `no_collection`, leaves the collection out;
/// with nothing else to read that's refused.
#[test]
fn the_collection_on_this_computer_is_the_one_counted() {
    let dir = tempfile::tempdir().unwrap();
    let rb = dir.path().join("rekordbox");
    let f = crate::masterdb::tests::sample();
    std::fs::create_dir_all(&rb).unwrap();
    std::fs::write(rb.join("master.db"), f.encrypted(crate::masterdb::DEFAULT_KEY)).unwrap();
    // One More Time's analysis, where rekordbox keeps it.
    let anlz = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/rekordcrate/demo_tracks/PIONEER/USBANLZ/P016/0000875E");
    copy_dir(&anlz, &rb.join("share/PIONEER/USBANLZ/P016/0000875E"));
    let volumes = dir.path().join("volumes");
    std::fs::create_dir_all(&volumes).unwrap();
    crate::testing::volumes_on_this_thread(Some(vec![volumes.clone()]));
    // SAFETY: nextest runs each test in a process of its own.
    unsafe { std::env::set_var(crate::masterdb::PATH_ENV, rb.join("master.db")) };

    let read = |req: &ProfileRequest| profile(None, req, &Ctx::silent());
    let kinds = |v: &ProfileView| v.report.sources.iter().map(|s| s.kind).collect::<Vec<_>>();
    let v = read(&ProfileRequest::default()).unwrap();
    assert_eq!(kinds(&v), ["collection"]);
    assert_eq!(v.report.collection.tracks, 3, "the deleted track isn't counted");
    assert_eq!(v.report.collection.playlists, 2);
    assert_eq!(v.report.energy.as_ref().map(|e| e.tracks), Some(1), "analysis read from rekordbox's share folder");
    let no_analysis = read(&ProfileRequest { no_analysis: true, ..Default::default() }).unwrap();
    assert!(no_analysis.report.energy.is_none());

    let drive = volumes.join("USB");
    copy_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rekordcrate/demo_tracks"), &drive);
    let on_drive = crate::pdb::read_library(&drive.join(crate::paths::EXPORT_PDB)).unwrap().tracks.len();
    let both = read(&ProfileRequest::default()).unwrap();
    assert_eq!(kinds(&both), ["collection", "drive"]);
    assert_eq!(both.report.sources[1].tracks, on_drive);
    assert_eq!(both.report.collection.tracks, 3, "the drive's tracks aren't the collection");

    let named = read(&ProfileRequest { drives: vec![drive.display().to_string()], ..Default::default() }).unwrap();
    assert_eq!(kinds(&named), ["drive"]);
    assert_eq!(named.report.collection.tracks, on_drive);

    std::fs::remove_dir_all(&drive).unwrap();
    let err = read(&ProfileRequest { no_collection: true, ..Default::default() }).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::NoStore), "{err}");
    unsafe { std::env::remove_var(crate::masterdb::PATH_ENV) };
}
