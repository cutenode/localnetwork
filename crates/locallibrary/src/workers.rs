//! Work spread over a bounded number of threads.

use std::panic::AssertUnwindSafe;
use std::sync::{Mutex, PoisonError};

/// How many threads to run CPU or disk work on at once: one per core.
pub(crate) fn per_core() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get())
}

/// Run `work` on each of `items`, on at most `workers` threads at once, each taking the next item
/// when it's free. Results come back in `items`' order; an item whose work panicked has the
/// panic's payload instead, and the other items still run.
pub(crate) fn each<T: Send, R: Send>(
    items: Vec<T>,
    workers: usize,
    work: impl Fn(T) -> R + Sync,
) -> Vec<std::thread::Result<R>> {
    let n = items.len();
    let queue = Mutex::new(items.into_iter().enumerate());
    let done: Mutex<Vec<Option<std::thread::Result<R>>>> = Mutex::new((0..n).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..workers.clamp(1, n.max(1)) {
            s.spawn(|| {
                loop {
                    let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
                    let Some((i, item)) = next else { return };
                    let r = std::panic::catch_unwind(AssertUnwindSafe(|| work(item)));
                    done.lock().unwrap_or_else(PoisonError::into_inner)[i] = Some(r);
                }
            });
        }
    });
    done.into_inner().unwrap_or_else(PoisonError::into_inner).into_iter().map(|r| r.expect("every item ran")).collect()
}

#[cfg(test)]
mod tests;
