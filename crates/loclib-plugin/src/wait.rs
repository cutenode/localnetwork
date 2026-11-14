//! Waiting before asking a server again: as long as it asks (`Retry-After`), noticing the hook
//! being cancelled.

use std::time::{Duration, Instant, SystemTime};

use loclib_progress::Ctx;

use crate::{Fetched, RpcError};

/// How often a wait notices the hook being cancelled.
const CHECK_EVERY: Duration = Duration::from_millis(50);

impl Fetched {
    /// How many seconds after `now` the answer asks to wait (`Retry-After`): a number of them, or
    /// until an HTTP-date (rounded up). One too big to count is longer than any wait; none, one
    /// past, or one that can't be read says nothing.
    pub fn retry_after(&self, now: SystemTime) -> Option<u64> {
        let said = self.headers.iter().find(|(name, _)| name.eq_ignore_ascii_case("retry-after"))?.1.trim();
        if !said.is_empty() && said.bytes().all(|b| b.is_ascii_digit()) {
            return Some(said.parse().unwrap_or(u64::MAX));
        }
        let until = httpdate::parse_http_date(said).ok()?.duration_since(now).ok()?;
        Some(until.as_secs() + u64::from(until.subsec_nanos() > 0))
    }
}

/// Waits `wait`, failing with `cancelled` once `ctx` is.
pub fn pause(ctx: &Ctx<'_>, wait: Duration) -> Result<(), RpcError> {
    let until = Instant::now() + wait;
    loop {
        ctx.check()?;
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(());
        }
        std::thread::sleep(left.min(CHECK_EVERY));
    }
}

#[cfg(test)]
mod tests;
