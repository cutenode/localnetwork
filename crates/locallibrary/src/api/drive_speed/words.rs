//! What a plan says about how long its write will take, from loclib-drive-speed's numbers.

use loclib_drive_speed::{Limit, Stale, WriteEstimate, link_rate};

use crate::op::bytes;

/// What a plan says about `estimate`: the time, then a line about the test and the port if
/// there's one to say.
pub fn estimate_lines(estimate: &WriteEstimate) -> Vec<String> {
    let (Some(speed), Some(seconds)) = (&estimate.speed, estimate.seconds) else {
        return vec!["time unknown: this drive hasn't been measured".into()];
    };
    let mostly = if estimate.limit == Some(Limit::Files) { ", mostly making files" } else { "" };
    let plus = estimate.work.plus.as_deref().map(|p| format!(", plus {p}")).unwrap_or_default();
    let rate = format!("drive writes ~{:.0} MB/s", speed.bytes_per_sec as f64 / 1e6);
    let create = format!("{:.0} ms a file", speed.create_us as f64 / 1e3);
    let stale = estimate.stale.map(|s| format!(", {}", stale_words(s))).unwrap_or_default();
    let time = time_words(seconds, estimate.best_case);
    let mut lines = vec![format!("{time}{mostly}{plus} ({rate}, {create}{stale})")];
    if estimate.ran_test {
        lines.push("ran a short test on this drive to time it, and removed its files".into());
    }
    if let Some(link) = estimate.port_limit_bps {
        lines.push(format!(
            "this port ({}) tops out near {:.0} MB/s, so a faster one may help",
            link_name(link),
            link_rate(link) / 1e6
        ));
    }
    lines
}

fn stale_words(stale: Stale) -> String {
    match stale {
        Stale::Old { days } => format!("measured {days} days ago"),
        Stale::Worn { bytes: written } => format!("measured before {} of writes", bytes(written)),
        Stale::Moved { then, .. } => format!("measured on a {} port", link_name(then)),
    }
}

/// A USB link by what people call it.
pub(super) fn link_name(bps: u64) -> String {
    match bps {
        0..=12_000_000 => "USB 1".into(),
        480_000_000 => "USB 2.0".into(),
        bps => format!("USB 3, {} Gb/s", bps / 1_000_000_000),
    }
}

/// A time as a plan says it, a best case as the least it could be.
fn time_words(seconds: u64, best_case: bool) -> String {
    match (seconds, best_case) {
        (0..60, false) => "under a minute".into(),
        (0..60, true) => "a minute or more".into(),
        (_, false) => format!("about {}", duration(seconds)),
        (_, true) => format!("at least {}", duration(seconds)),
    }
}

/// `7 min`, `1 h 25 min`: as exact as a guess deserves.
pub(super) fn duration(seconds: u64) -> String {
    match seconds {
        0..3600 => format!("{} min", (seconds + 30) / 60),
        _ => {
            let fives = (seconds + 150) / 300;
            match (fives / 12, fives % 12 * 5) {
                (hours, 0) => format!("{hours} h"),
                (hours, minutes) => format!("{hours} h {minutes} min"),
            }
        }
    }
}
