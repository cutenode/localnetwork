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
