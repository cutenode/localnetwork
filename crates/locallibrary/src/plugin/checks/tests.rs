use hegel::TestCase;
use hegel::generators as gs;

use super::*;

/// A store for this process's tests, made once: each case sets what it reads.
fn store() -> &'static Store {
    static STORE: std::sync::OnceLock<(tempfile::TempDir, Store)> = std::sync::OnceLock::new();
    &STORE
        .get_or_init(|| {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::open_or_init(dir.path().join("store")).unwrap();
            (dir, store)
        })
        .1
}

/// Any time a check is given, however long, it runs: a deadline past what the clock can say is
/// none, rather than a panic.
#[hegel::test(test_cases = 50)]
fn a_check_is_run_within_any_time(tc: TestCase) {
    let timeout = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => Duration::MAX,
        1 => Duration::new(tc.draw(gs::integers::<u64>()), tc.draw(gs::integers::<u32>().max_value(999_999_999))),
        _ => Duration::from_millis(tc.draw(gs::integers::<u64>().min_value(1).max_value(1000))),
    };
    let ran = within(timeout, &Ctx::silent(), |ctx| {
        ctx.check()?;
        Ok(7)
    });
    assert!(matches!(ran, Ok(Some(7))), "{timeout:?}: {ran:?}");
}

/// A timeout is set only from [`MIN_TIMEOUT`] to [`MAX_TIMEOUT`]; one kept otherwise (by hand)
/// reads as the nearest of them, and one of 0 or less as [`TIMEOUT`].
#[hegel::test(test_cases = 60)]
fn a_timeout_is_a_millisecond_to_an_hour(tc: TestCase) {
    let store = store();
    let secs = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => tc.draw(gs::sampled_from(vec![
            0.0,
            -0.0,
            -1.0,
            1e-9,
            0.001,
            3600.0,
            3_600.000_001,
            1e300,
            f64::MAX,
            f64::MIN_POSITIVE,
        ])),
        _ => tc.draw(gs::floats::<f64>().min_value(-10_000.0).max_value(10_000.0).allow_nan(false)),
    };
    let fits = (0.001..=3600.0).contains(&secs);
    let set = set_check_timeout(store, Some(secs));
    match set {
        Ok(d) => assert!(fits && d == Duration::from_secs_f64(secs), "{secs}: {d:?}"),
        Err(e) => {
            assert!(!fits, "{secs}");
            assert_eq!(e.to_string(), format!("a check has 0.001 to 3600 seconds to answer, not {secs}"));
        }
    }
    // Kept as it is, whatever it is.
    set_host_setting(store, TIMEOUT_SETTING, Some(Value::from(secs))).unwrap();
    let expected = match secs {
        s if s <= 0.0 => TIMEOUT,
        s if s < 0.001 => MIN_TIMEOUT,
        s if s > 3600.0 => MAX_TIMEOUT,
        s => Duration::from_secs_f64(s),
    };
    assert_eq!(check_timeout(Some(store)), expected, "{secs}");
}
