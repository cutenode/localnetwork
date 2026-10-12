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

/// However many swaps are under way, an exit at once returns only once every one has finished,
/// and says it's finishing them only when there are some.
#[hegel::test(test_cases = 20)]
fn an_exit_at_once_returns_once_every_swap_has_finished(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    let swaps = tc.draw(gs::integers::<usize>().max_value(3));
    let finished: Arc<Vec<AtomicBool>> = Arc::new((0..swaps).map(|_| AtomicBool::new(false)).collect());
    let (entered, all_in) = mpsc::channel();
    // Each swap goes on until the exit says it's waiting.
    let (releases, threads): (Vec<mpsc::Sender<()>>, Vec<_>) = (0..swaps)
        .map(|i| {
            let (release, released) = mpsc::channel::<()>();
            let (finished, entered) = (Arc::clone(&finished), entered.clone());
            let t = std::thread::spawn(move || {
                let swapping = Critical::enter();
                entered.send(()).unwrap();
                released.recv().unwrap();
                finished[i].store(true, Ordering::SeqCst);
                drop(swapping);
            });
            (release, t)
        })
        .unzip();
    for _ in 0..swaps {
        all_in.recv().unwrap();
    }
    let mut said = None;
    let guard = exclusive(|swapping| {
        said = Some(swapping);
        for r in &releases {
            r.send(()).unwrap();
        }
    });
    assert!(finished.iter().all(|f| f.load(Ordering::SeqCst)), "returned during a swap");
    assert_eq!(SWAPS.load(Ordering::SeqCst), 0);
    assert_eq!(said, (swaps > 0).then_some(true), "with {swaps} swaps under way");
    drop(guard);
    drop(wait_for_critical());
    for t in threads {
        t.join().unwrap();
    }
}

/// An exit at once has the lock as soon as no swap holds it: at once with none under way, once a
/// swap under way finishes, and even after an exit before it panicked holding it. A swap asked
/// for meanwhile starts only once the exit lets go.
#[hegel::test(test_cases = 10)]
fn an_exit_at_once_holds_off_the_next_swap(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            // A swap under way: the exit waits for it.
            0 => {
                let (entered, inside) = mpsc::channel();
                let (release, released) = mpsc::channel::<()>();
                let finished = Arc::new(AtomicBool::new(false));
                let done = Arc::clone(&finished);
                let swap = std::thread::spawn(move || {
                    let _swapping = Critical::enter();
                    entered.send(()).unwrap();
                    released.recv().unwrap();
                    done.store(true, Ordering::SeqCst);
                });
                inside.recv().unwrap();
                let exiting = std::thread::spawn(move || drop(wait_for_critical()));
                std::thread::sleep(Duration::from_millis(20));
                release.send(()).unwrap();
                exiting.join().unwrap();
                assert!(finished.load(Ordering::SeqCst), "the exit returned before the swap finished");
                swap.join().unwrap();
            }
            // An exit that panicked holding the lock leaves it to the next.
            1 => {
                let panicked = std::thread::spawn(|| {
                    let _exit = wait_for_critical();
                    panic!("exiting went wrong");
                });
                assert!(panicked.join().is_err());
            }
            _ => {}
        }
        let exit = wait_for_critical();
        let started = mpsc::channel();
        let swap = std::thread::spawn(move || {
            let _swapping = Critical::enter();
            started.0.send(()).unwrap();
        });
        assert!(started.1.recv_timeout(Duration::from_millis(20)).is_err(), "a swap started while exiting");
        drop(exit);
        started.1.recv().unwrap();
        swap.join().unwrap();
    }
}
