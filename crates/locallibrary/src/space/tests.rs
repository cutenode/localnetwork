use super::*;

fn at(days: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z").unwrap().with_timezone(&Utc) + Duration::days(days)
}

#[test]
fn fits_a_growth_line() {
    let gb = 1_000_000_000u64;
    let points = [(at(0), 50 * gb), (at(30), 53 * gb), (at(60), 56 * gb)];
    let f = fit("Main", 56 * gb, &points, Some(9 * gb + PLAYER_HEADROOM), at(60)).unwrap();
    assert_eq!(f.per_month, 3 * gb as i64);
    // 9 GB of room at 0.1 GB a day.
    assert_eq!(f.full_around, Some(at(150).date_naive()));
    assert!(fit("Main", 0, &points[..1], None, at(0)).is_none());
    assert!(fit("Main", 0, &[(at(0), 1), (at(2), 2)], None, at(2)).is_none());
}

#[test]
fn shrinking_libraries_never_fill() {
    let points = [(at(0), 100), (at(30), 50)];
    let f = fit("Main", 50, &points, Some(1_000_000_000), at(30)).unwrap();
    assert!(f.per_month < 0);
    assert!(f.full_around.is_none());
}

#[test]
fn classifies_files() {
    let referenced: FxHashSet<String> = ["music/a.mp3".to_string()].into_iter().collect();
    assert_eq!(classify("Contents/x.mp3", "x.mp3", &referenced), SpaceKind::Audio);
    assert_eq!(classify("Music/A.mp3", "A.mp3", &referenced), SpaceKind::Audio);
    assert_eq!(classify("Music/b.mp3", "b.mp3", &referenced), SpaceKind::LooseAudio);
    assert_eq!(classify("Contents/._x.mp3", "._x.mp3", &referenced), SpaceKind::OsClutter);
    assert_eq!(
        classify(".locallibrary/backups/1/export.pdb", "export.pdb", &referenced),
        SpaceKind::LocallibraryBackups
    );
    assert_eq!(classify("PIONEER/rekordbox/export.pdb", "export.pdb", &referenced), SpaceKind::Databases);
    assert_eq!(classify("PIONEER/MYSETTING.DAT", "MYSETTING.DAT", &referenced), SpaceKind::Settings);
    assert_eq!(classify("notes.txt", "notes.txt", &referenced), SpaceKind::Other);
}

#[test]
fn classifies_what_rekordbox_keeps_besides_the_library() {
    let none = FxHashSet::default();
    assert_eq!(classify("PIONEER/Artwork/00001/a1.jpg", "a1.jpg", &none), SpaceKind::Artwork);
    assert_eq!(classify("PIONEER/rekordbox/exportLibrary.db-shm", "exportLibrary.db-shm", &none), SpaceKind::Databases);
    let backup = "PIONEER/DeviceLibBackup/rbDevLibBaInfo_815904346.json";
    assert_eq!(classify(backup, "rbDevLibBaInfo_815904346.json", &none), SpaceKind::RekordboxOther);
    assert_eq!(classify(".locallibrary/state.json", "state.json", &none), SpaceKind::Locallibrary);
}

/// Whole folders the OS keeps are measured as one kind, wherever they are; links aren't
/// followed or counted.
#[test]
fn system_folders_are_measured_whole() {
    let dir = tempfile::tempdir().unwrap();
    let put = |key: &str, len: usize| {
        let p = dir.path().join(key);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![0u8; len]).unwrap();
    };
    put("System Volume Information/IndexerVolumeGuid", 76);
    put(".Spotlight-V100/Store-V2/store.db", 4096);
    put("Music/.fseventsd/fseventsd-uuid", 36);
    put("PIONEER/Artwork/00001/a1.jpg", 900);
    put("notes.txt", 24);
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("notes.txt"), dir.path().join("link.txt")).unwrap();
    let r = breakdown(&Device::open(dir.path()).unwrap(), &Ctx::silent()).unwrap();
    assert_eq!(r.bytes(SpaceKind::Other), 76 + 24);
    assert_eq!(r.bytes(SpaceKind::OsClutter), 4096 + 36);
    assert_eq!(r.bytes(SpaceKind::Artwork), 900);
    let files: usize = r.categories.iter().map(|c| c.files).sum();
    assert_eq!((files, r.unreadable), (5, 0));
    assert_eq!(r.categories[0].kind, SpaceKind::OsClutter, "largest first");
    assert_eq!(dir_size(&dir.path().join("Music")), (1, 36, 0));
}

#[test]
fn warns_when_little_room_is_left() {
    assert!(headroom_warning(Some(1_000_000_000), 900_000_000, 0).is_some());
    assert!(headroom_warning(Some(1_000_000_000), 100_000_000, 0).is_none());
    assert!(headroom_warning(Some(1_000_000_000), 900_000_000, 500_000_000).is_none());
    assert!(headroom_warning(None, 1, 0).is_none());
}

/// Snapshots on a line of growth: the fit finds its slope, says when the room left runs out
/// at that rate, and says nothing from fewer than two snapshots or less than a week.
#[hegel::test(test_cases = 300)]
fn a_line_of_growth_is_found_again(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let base = tc.draw(gs::integers::<u64>().max_value(1 << 40));
    let per_day = tc.draw(gs::integers::<i64>().min_value(-1_000_000).max_value(50_000_000));
    let mut days: Vec<i64> = tc.draw(gs::vecs(gs::integers::<i64>().min_value(0).max_value(400)).max_size(8));
    days.sort_unstable();
    days.dedup();
    let points: Vec<(DateTime<Utc>, u64)> =
        days.iter().map(|d| (at(*d), (base as i64 + per_day * d).max(0) as u64)).collect();
    let free = tc.draw(gs::optional(gs::integers::<u64>().max_value(1 << 41)));
    let now = at(days.last().copied().unwrap_or(0));
    let f = fit("Main", base, &points, free, now);
    let span = days.last().zip(days.first()).map_or(0, |(l, f)| l - f);
    if points.len() < 2 || span < 7 {
        assert!(f.is_none());
        return;
    }
    // Clamped at zero, the points may not lie on the line.
    if points.iter().zip(&days).any(|(p, d)| p.1 as i64 != base as i64 + per_day * d) {
        return;
    }
    let f = f.unwrap();
    assert_eq!((f.snapshots, f.span_days), (points.len(), span));
    assert!((f.per_month - per_day * 30).abs() <= 1, "{} vs {}", f.per_month, per_day * 30);
    match (free, per_day > 0) {
        (Some(free), true) => {
            let days = (free.saturating_sub(PLAYER_HEADROOM) as f64 / per_day as f64).ceil();
            let want = (days < 365.0 * 20.0).then(|| (now + Duration::days(days as i64)).date_naive());
            assert_eq!(f.full_around, want);
        }
        _ => assert!(f.full_around.is_none()),
    }
}

/// The warning comes exactly when what's left after the plan is under the players' headroom.
#[hegel::test(test_cases = 300)]
fn headroom_is_warned_about_exactly_when_short(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let free = tc.draw(gs::optional(gs::integers::<u64>().max_value(1 << 34)));
    let needed = tc.draw(gs::integers::<u64>().max_value(1 << 34));
    let freed = tc.draw(gs::integers::<u64>().max_value(1 << 33));
    let left = free.and_then(|f| (f + freed).checked_sub(needed));
    let warned = headroom_warning(free, needed, freed);
    assert_eq!(warned.is_some(), left.is_some_and(|l| l < PLAYER_HEADROOM));
    if let (Some(w), Some(l)) = (warned, left) {
        assert!(w.contains(&format!("{} MB", l / 1_000_000)), "{w}");
    }
}
