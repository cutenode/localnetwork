use std::fmt::Display;

use chrono::{DateTime, Local, TimeZone};

/// `t` in this computer's time zone, formatted with `format` (strftime). Times are kept and
/// serialised in UTC; anything a person reads shows them local, so a date matches their calendar.
pub fn local_time<Tz: TimeZone>(t: DateTime<Tz>, format: &str) -> impl Display + '_ {
    t.with_timezone(&Local).format(format)
}
