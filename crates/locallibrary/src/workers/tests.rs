use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Whatever the items and the bound, each item's result comes back in its place, a panic in one
/// doesn't stop the others, and no more than `workers` run at once.
#[hegel::test(test_cases = 50)]
fn each_item_runs_once_in_order_and_within_the_bound(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let items: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().max_value(20)).max_size(12));
    let workers = tc.draw(gs::integers::<usize>().max_value(5));
    let (running, most) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let results = each(items.clone(), workers, |i| {
        let now = running.fetch_add(1, Ordering::SeqCst) + 1;
        most.fetch_max(now, Ordering::SeqCst);
        std::thread::yield_now();
        running.fetch_sub(1, Ordering::SeqCst);
        if i == 13 {
            std::panic::panic_any("unlucky");
        }
        i * 2
    });
    assert_eq!(results.len(), items.len());
    for (i, r) in items.iter().zip(results) {
        match r {
            Ok(v) => assert_eq!(v, i * 2),
            Err(p) => assert!(*i == 13 && p.downcast_ref::<&str>() == Some(&"unlucky")),
        }
    }
    assert!(most.load(Ordering::SeqCst) <= workers.max(1), "{} at once", most.load(Ordering::SeqCst));
}
