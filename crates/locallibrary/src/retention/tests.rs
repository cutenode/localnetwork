use chrono::{DateTime, Duration, TimeZone, Utc};

use super::*;
use crate::model::LibrarySummary;

fn candidate(n: u8, created: DateTime<Utc>) -> Candidate {
    let id = Hash::of(&[n]);
    Candidate {
        info: SnapshotInfo {
            id,
            short_id: n.to_string(),
            library_id: uuid::Uuid::nil(),
            parents: vec![],
            created,
            message: None,
            source: None,
            fingerprint: Hash::of(&[n, n]),
            summary: LibrarySummary::default(),
            files: 0,
            bytes: 0,
            labels: vec![],
            beside: false,
        },
        reasons: vec![],
        drives: vec![],
        history: vec![],
    }
}

/// Kept snapshots by number, newest first.
fn kept(snaps: &[Candidate]) -> Vec<&str> {
    snaps.iter().filter(|c| !c.reasons.is_empty()).map(|c| c.info.short_id.as_str()).collect()
}

/// Two snapshots a day, at 10:00 and 14:00 local time, for 21 days: newest first.
fn three_weeks() -> Vec<Candidate> {
    let start = Local.with_ymd_and_hms(2026, 3, 2, 10, 0, 0).unwrap().with_timezone(&Utc); // a Monday
    let mut v: Vec<Candidate> = (0..42u8)
        .map(|i| candidate(i, start + Duration::days(i64::from(i / 2)) + Duration::hours(4 * i64::from(i % 2))))
        .collect();
    v.reverse();
    v
}

#[test]
fn last_daily_and_weekly_keep_the_newest_of_each() {
    let none = FxHashMap::default();
    let mut snaps = three_weeks();
    by_policy(&mut snaps, None, &none, &PruneOptions { keep_last: Some(3), ..Default::default() });
    assert_eq!(kept(&snaps), ["41", "40", "39"]);

    let mut snaps = three_weeks();
    by_policy(&mut snaps, None, &none, &PruneOptions { keep_daily: Some(3), ..Default::default() });
    assert_eq!(kept(&snaps), ["41", "39", "37"], "each day's 14:00");

    let mut snaps = three_weeks();
    by_policy(&mut snaps, None, &none, &PruneOptions { keep_weekly: Some(2), ..Default::default() });
    // Sunday 22 March, then Sunday 15 March.
    assert_eq!(kept(&snaps), ["41", "27"]);

    // Together, with the head (an older one), a label and a drive's state.
    let mut snaps = three_weeks();
    snaps[30].info.labels = vec!["before NYE".into()];
    let drives = FxHashMap::from_iter([(snaps[35].info.fingerprint, vec!["Backup".to_string()])]);
    let head = Some(snaps[1].info.id);
    let opts = PruneOptions { keep_last: Some(1), keep_daily: Some(2), keep_weekly: Some(3), ..Default::default() };
    by_policy(&mut snaps, head, &drives, &opts);
    assert_eq!(kept(&snaps), ["41", "40", "39", "27", "13", "11", "6"]);
    assert_eq!(snaps[0].reasons, [KeepReason::Last, KeepReason::Daily, KeepReason::Weekly]);
    assert_eq!(snaps[1].reasons, [KeepReason::Latest]);
    assert_eq!(snaps[30].reasons, [KeepReason::Label]);
    assert_eq!(
        (snaps[35].reasons.as_slice(), snaps[35].drives.as_slice()),
        (&[KeepReason::Drive][..], &["Backup".to_string()][..])
    );
}

/// Whatever the policy, the snapshots kept are exactly: the head, labelled ones, drives'
/// states, the newest N, and the newest of each of the newest N days and weeks that have
/// snapshots, each kept for each reason that applies.
#[hegel::test(test_cases = 300)]
fn the_policy_keeps_exactly_what_it_names(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<u8>().max_value(30));
    let start = Local.with_ymd_and_hms(2026, 3, 2, 0, 0, 0).unwrap().with_timezone(&Utc);
    let mut snaps: Vec<Candidate> = (0..n)
        .map(|i| candidate(i, start + Duration::hours(i64::from(tc.draw(gs::integers::<u16>().max_value(24 * 40))))))
        .collect();
    snaps.sort_by(|a, b| b.info.created.cmp(&a.info.created).then(b.info.id.cmp(&a.info.id)));
    let pick = |tc: &hegel::TestCase| (n > 0).then(|| tc.draw(gs::integers::<usize>().max_value(n as usize - 1)));
    let head = pick(&tc).map(|i| snaps[i].info.id);
    let labelled: Vec<usize> = (0..2).filter_map(|_| pick(&tc)).collect();
    for i in &labelled {
        snaps[*i].info.labels = vec!["kept".into()];
    }
    let on_drives: Vec<usize> = (0..2).filter_map(|_| pick(&tc)).collect();
    let drives: FxHashMap<Hash, Vec<String>> =
        on_drives.iter().map(|i| (snaps[*i].info.fingerprint, vec![format!("Drive {i}")])).collect();
    let size = || gs::optional(gs::integers::<usize>().max_value(6));
    let opts = PruneOptions {
        keep_last: tc.draw(size()),
        keep_daily: tc.draw(size()),
        keep_weekly: tc.draw(size()),
        ..Default::default()
    };
    let before: Vec<SnapshotInfo> = snaps.iter().map(|c| c.info.clone()).collect();
    by_policy(&mut snaps, head, &drives, &opts);

    // Each period's newest snapshot, for the newest `n` periods.
    let newest_per = |n: Option<usize>, period: &dyn Fn(&SnapshotInfo) -> (i32, u32)| -> FxHashSet<Hash> {
        let mut periods: Vec<(i32, u32)> = before.iter().map(period).collect();
        periods.sort_unstable_by(|a, b| b.cmp(a));
        periods.dedup();
        periods
            .into_iter()
            .take(n.unwrap_or(0))
            .filter_map(|p| before.iter().find(|s| period(s) == p).map(|s| s.id))
            .collect()
    };
    let day = |s: &SnapshotInfo| {
        let d = s.created.with_timezone(&Local).date_naive();
        (d.year(), d.ordinal())
    };
    let week = |s: &SnapshotInfo| {
        let w = s.created.with_timezone(&Local).date_naive().iso_week();
        (w.year(), w.week())
    };
    let daily = newest_per(opts.keep_daily, &day);
    let weekly = newest_per(opts.keep_weekly, &week);
    for (i, c) in snaps.iter().enumerate() {
        let mut want = Vec::new();
        if Some(c.info.id) == head {
            want.push(KeepReason::Latest);
        }
        if labelled.contains(&i) {
            want.push(KeepReason::Label);
        }
        if on_drives.contains(&i) {
            want.push(KeepReason::Drive);
        }
        if i < opts.keep_last.unwrap_or(0) {
            want.push(KeepReason::Last);
        }
        if daily.contains(&c.info.id) {
            want.push(KeepReason::Daily);
        }
        if weekly.contains(&c.info.id) {
            want.push(KeepReason::Weekly);
        }
        assert_eq!(c.reasons, want, "snapshot {i} of {}", snaps.len());
        assert_eq!(!c.drives.is_empty(), on_drives.contains(&i));
    }
}
