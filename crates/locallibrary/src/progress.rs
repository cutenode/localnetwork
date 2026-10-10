//! Progress reporting and cancellation shared by every long-running operation (all of
//! [`loclib_progress`], re-exported), and the guard that keeps a front end exiting at once from
//! cutting a database swap short ([`Critical`]).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};

pub use loclib_progress::*;

/// Steps under way, in this process, that must finish together (swapping in a drive's
/// databases): each [`Critical`] holds it to read, and a front end exiting at once (a second
/// Ctrl-C or signal) takes it to write first ([`wait_for_critical`]), which waits for them.
static CRITICAL: RwLock<()> = RwLock::new(());

/// How many [`Critical`] steps are under way: the lock can be held by another exit instead.
static SWAPS: AtomicUsize = AtomicUsize::new(0);

/// While one lives, a front end exiting at once waits (see [`wait_for_critical`]).
pub struct Critical {
    _held: RwLockReadGuard<'static, ()>,
}

impl Critical {
    pub fn enter() -> Critical {
        let held = CRITICAL.read().unwrap_or_else(PoisonError::into_inner);
        SWAPS.fetch_add(1, Ordering::SeqCst);
        Critical { _held: held }
    }
}

impl Drop for Critical {
    fn drop(&mut self) {
        // Before the lock is let go: an exit that gets it next sees none under way.
        SWAPS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// For a front end about to exit at once: waits for the [`Critical`] steps under way to finish
/// (saying so on stderr when there are any), and keeps new ones from starting while the guard it
/// returns lives.
pub fn wait_for_critical() -> RwLockWriteGuard<'static, ()> {
    exclusive(|swapping| {
        if swapping {
            eprintln!("finishing the database swap under way first...");
        }
    })
}

/// [`wait_for_critical`], telling `waiting` whether a swap is under way when it has to wait (or
/// another exit holds the lock).
fn exclusive(waiting: impl FnOnce(bool)) -> RwLockWriteGuard<'static, ()> {
    match CRITICAL.try_write() {
        Ok(held) => held,
        Err(TryLockError::Poisoned(e)) => e.into_inner(),
        Err(TryLockError::WouldBlock) => {
            waiting(SWAPS.load(Ordering::SeqCst) > 0);
            CRITICAL.write().unwrap_or_else(PoisonError::into_inner)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// An exit at once waits for the swap under way, saying so, and no swap starts while it's
    /// exiting. A second exit waits for the first without saying it's finishing a swap.
    #[test]
    fn exiting_at_once_waits_for_the_swap_and_holds_off_the_next() {
        let swapping = Critical::enter();
        let (said, heard) = mpsc::channel();
        let (held, release) = (mpsc::channel(), mpsc::channel::<()>());
        let exiting = std::thread::spawn(move || {
            let _exit = exclusive(|swap| said.send(swap).unwrap());
            held.0.send(()).unwrap();
            release.1.recv().unwrap();
        });
        assert!(heard.recv().unwrap(), "a swap is under way");
        assert!(held.1.recv_timeout(Duration::from_millis(100)).is_err(), "exited during the swap");
        drop(swapping);
        held.1.recv().unwrap();

        let (said, heard) = mpsc::channel();
        let again = std::thread::spawn(move || drop(exclusive(|swap| said.send(swap).unwrap())));
        assert!(!heard.recv().unwrap(), "no swap is under way, only the other exit");
        let started = mpsc::channel();
        let next = std::thread::spawn(move || {
            let _swapping = Critical::enter();
            started.0.send(()).unwrap();
        });
        assert!(started.1.recv_timeout(Duration::from_millis(100)).is_err(), "a swap started while exiting");
        release.0.send(()).unwrap();
        started.1.recv().unwrap();
        for t in [exiting, again, next] {
            t.join().unwrap();
        }
    }
}
