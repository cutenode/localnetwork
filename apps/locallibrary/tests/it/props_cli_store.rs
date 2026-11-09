//! Property tests of the store's own upkeep: what `store prune` keeps and why, and a copy of the
//! store brought up to date, repaired and mirrored, each printed as its `--json` counts it.

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::Value;

use crate::support::{Env, path};

/// Drive Main pulled into the library Main, then changed and pulled again `more` times: a
/// snapshot each.
fn snapshots(fx: &Env, more: usize) {
    fx.pulled("Main");
    for i in 0..more {
        fx.ok(&["--yes", "playlist", "new", "Main", &format!("Pull {i}")]);
        fx.ok(&["pull", "Main"]);
    }
}

/// Snapshots pulled one after another, pruned as a dry run keeping the last few, the newest of
/// each day or the newest of each week (drawn): the plan printed is its JSON's, each snapshot
/// kept with every reason its JSON gives and each one removed marked so, or it says there's
/// nothing to prune. Applied, it removes the snapshots its plan removes.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_prune_says_why_it_keeps_each_snapshot(tc: TestCase) {
    let fx = Env::new();
    snapshots(&fx, tc.draw(gs::integers::<usize>().max_value(2)));
    let mut args = vec!["store", "prune"];
    let keep = ["--keep-last", "--keep-daily", "--keep-weekly"];
    for flag in tc.draw(gs::subsequences(keep.to_vec()).min_size(1)) {
        args.extend([flag, tc.draw(gs::sampled_from(&["1", "2"][..]))]);
    }
    let dry = [&args[..], &["--dry-run"]].concat();
    let (planned, printed) = (fx.json(&dry), fx.ok(&dry));
    let said = format!("{dry:?}\n{printed}\n{planned}");
    let p = &planned["plan"];
    if planned["outcome"] == "nothing_to_do" {
        assert!(printed.contains("nothing to prune: every snapshot is kept"), "{said}");
        return;
    }
    assert_eq!(planned["outcome"], "dry_run", "{said}");
    let line = |id: &Value| {
        let id = id.as_str().unwrap();
        printed.lines().find(|l| l.contains(id)).unwrap_or_else(|| panic!("no line for {id}: {said}"))
    };
    for lib in p["libraries"].as_array().unwrap() {
        for k in lib["kept"].as_array().unwrap() {
            let l = line(&k["snapshot"]["short_id"]);
            assert!(l.trim_start().starts_with("+ "), "{l:?}: {said}");
            for reason in k["reasons"].as_array().unwrap() {
                let why = match reason.as_str().unwrap() {
                    "latest" => "latest",
                    "label" => "labelled ",
                    "drive" => " last seen holding it",
                    "history" => "holds ",
                    "unreadable" => "couldn't be read",
                    "last" => "among the newest",
                    "daily" => "newest that day",
                    "weekly" => "newest that week",
                    other => panic!("no reason {other}: {said}"),
                };
                assert!(l.contains(why), "{why:?} for {reason} in {l:?}: {said}");
            }
        }
        for s in lib["removed"].as_array().unwrap() {
            let l = line(&s["short_id"]);
            assert!(l.trim_start().starts_with("- "), "{l:?}: {said}");
        }
    }
    let applied = fx.json(&[&["--yes"][..], &args].concat());
    assert_eq!(applied["report"]["removed_snapshots"], p["removed_snapshots"], "{applied}\n{said}");
}

/// The store copied to a folder; then (drawn) more snapshots pulled and all but the last pruned
/// from the store; a file in the copy that the store still has damaged; then copied again,
/// verified or not and mirrored or not, printed or as JSON: the copy is repaired only when
/// verified, loses the snapshots pruned only when mirrored, and says so as its JSON counts it.
/// Mostly verified and printed, the case that says the most.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn a_copy_of_the_store_says_what_it_repaired_and_removed(tc: TestCase) {
    let fx = Env::new();
    let more = tc.draw(gs::integers::<usize>().max_value(2));
    snapshots(&fx, more);
    let copy = fx.outside("NAS");
    fx.ok(&["--yes", "store", "copy", path(&copy)]);
    let pruned = more > 0 && tc.draw(gs::booleans());
    if pruned {
        fx.ok(&["--yes", "store", "prune", "--keep-last", "1"]);
    }
    {
        let in_store: Vec<std::path::PathBuf> = Env::files_in(&fx.store.join("objects"))
            .into_iter()
            .map(|(p, _)| copy.join(p.strip_prefix(&fx.store).unwrap()))
            .collect();
        let file = tc.draw(gs::sampled_from(in_store));
        let mut bytes = std::fs::read(&file).unwrap();
        bytes[0] ^= 0xff;
        std::fs::write(&file, bytes).unwrap();
    }
    let (verify, mirror) = (!tc.draw(gs::weighted_booleans(0.3)), tc.draw(gs::booleans()));
    let json = tc.draw(gs::weighted_booleans(0.25));
    let mut args = vec!["--yes", "store", "copy", path(&copy)];
    if verify {
        args.push("--verify");
    }
    if mirror {
        args.push("--mirror");
    }
    let repaired = usize::from(verify);
    let removed = if pruned && mirror { more } else { 0 };
    if json {
        let v = fx.json(&args);
        let r = &v["report"];
        assert_eq!(
            (r["objects_repaired"].as_u64(), r["removed_snapshots"].as_u64()),
            (Some(repaired as u64), Some(removed as u64)),
            "{v}"
        );
        return;
    }
    let out = fx.ok(&args);
    let repairs = format!("{repaired} file(s) in the copy had changed and were copied again");
    assert_eq!(out.contains(&repairs), repaired > 0, "{repairs:?}: {out}");
    let removes = format!("removed {removed} snapshot(s) and ");
    assert_eq!(out.contains(&removes), removed > 0, "{removes:?}: {out}");
}
