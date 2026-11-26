use std::time::Duration;

use super::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn what_is_heard_lags_by_what_is_still_to_play() {
    // A 20 ms buffer to be heard 80 ms on: just handed over, the last of it is 100 ms off.
    assert_eq!(lag_at(ms(100), ms(0)), ms(100));
    // The device plays on until the next buffer is handed over.
    assert_eq!(lag_at(ms(100), ms(30)), ms(70));
    // Nothing is left to hear: what's heard is what was handed over.
    assert_eq!(lag_at(ms(100), ms(150)), ms(0));
}

#[test]
fn what_is_heard_never_goes_back_past_a_jump() {
    // Played on to 10 s: heard 100 ms behind.
    assert_eq!(heard(ms(10_000), ms(100), ms(0)), ms(9_900));
    // Just sought 30 s: what's coming is from there, so it shows there, not before it.
    assert_eq!(heard(ms(30_020), ms(100), ms(30_000)), ms(30_000));
    // Once the sound has caught up it moves on as before.
    assert_eq!(heard(ms(30_200), ms(100), ms(30_000)), ms(30_100));
    // A floor ahead of the count (a seek back past it) never pushes it on.
    assert_eq!(heard(ms(5_000), ms(0), ms(30_000)), ms(5_000));
}

#[test]
fn a_clock_with_no_buffer_handed_over_lags_by_nothing() {
    let clock = OutputClock::default();
    assert_eq!(clock.lag(), Duration::ZERO);
    clock.handed(960, 48_000, ms(80));
    let lag = clock.lag();
    assert!(lag <= ms(100) && lag > ms(50), "{lag:?}");
}

#[hegel::test(test_cases = 300)]
fn prop_what_is_heard_is_never_ahead_of_the_count_nor_before_the_floor(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let counted = ms(tc.draw(gs::integers::<u64>().max_value(600_000)));
    let lag = ms(tc.draw(gs::integers::<u64>().max_value(500)));
    let floor = ms(tc.draw(gs::integers::<u64>().max_value(600_000)));
    let h = heard(counted, lag, floor);
    assert!(h <= counted, "{h:?} ahead of {counted:?}");
    assert!(h >= floor.min(counted), "{h:?} before {floor:?}");
    assert!(counted - h <= lag, "{h:?} more than {lag:?} behind {counted:?}");
}

#[test]
fn what_is_heard_holds_still_rather_than_going_back_a_little() {
    // A new buffer's estimate lands a few ms behind what was shown: it holds.
    assert_eq!(steady(ms(345), Some(ms(347))), ms(347));
    // Ahead, it moves on.
    assert_eq!(steady(ms(350), Some(ms(347))), ms(350));
    // After a jump (no last), or a real step back, it's taken as it is.
    assert_eq!(steady(ms(345), None), ms(345));
    assert_eq!(steady(ms(1_000), Some(ms(9_000))), ms(1_000));
}
