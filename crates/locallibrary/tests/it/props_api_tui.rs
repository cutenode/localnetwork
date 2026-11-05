//! Properties of the plans the terminal player added to the service layer (`api::tui`): saving a
//! drive in the store, planned before it's made.

use crate::common::{self, path};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, PullRequest};
use locallibrary::compare::Relation;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;

/// A drive's pull, planned against no store or one that saved it once or twice (the drive changed
/// since or not), into the library its last pull linked it to, the one named, or one the store
/// lacks: a library the store lacks is refused unless it's to be started; otherwise the plan starts
/// a library exactly when there's none to pull into, and has nothing to do exactly when the drive
/// is as the store last saved it. Applied, with the store, another or none, the library has the
/// drive as it is, so planning again has nothing to do.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_pull_plan_says_what_pulling_would_do(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let store_root = dir.path().join("store");
    let ctx = Ctx::silent();
    let pulls = tc.draw(gs::integers::<usize>().max_value(2));
    for i in 0..pulls {
        if i > 0 {
            common::export_new_track(&root, "New", &format!("Track {i}"));
        }
        let store = Store::open_or_init(&store_root).unwrap();
        api::pull(&store, &PullRequest { drive: path(&root), ..Default::default() }, &ctx).unwrap();
    }
    let changed = pulls > 0 && tc.draw(gs::booleans());
    if changed {
        common::export_new_track(&root, "New", "Since");
    }
    let named = tc.draw(gs::sampled_from(vec![None, Some("USB"), Some("Elsewhere")]));
    let new_library = tc.draw(gs::booleans());
    let req = PullRequest { drive: path(&root), library: named.map(Into::into), new_library, ..Default::default() };

    let planned = api::pull_plan(&store_root, &req, &ctx);
    let existing = pulls > 0 && named != Some("Elsewhere");
    if named.is_some() && !existing && !new_library {
        assert_eq!(planned.unwrap_err().code(), "not_found");
        return;
    }
    let plan = planned.unwrap();
    assert_eq!(plan.new_library, !existing);
    assert_eq!(plan.library, named.unwrap_or("USB"));
    let relation = existing.then_some(if changed { Relation::RightNewer } else { Relation::Identical });
    assert_eq!(plan.relation, relation);
    assert_eq!(plan.noop, relation == Some(Relation::Identical));
    assert_eq!(plan.head.is_some(), existing);

    let other = Store::open_or_init(dir.path().join("other")).unwrap();
    let mine = Store::open(&store_root).ok();
    let with = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => None,
        1 => mine.as_ref(),
        _ => Some(&other),
    };
    plan.apply(with, &ctx).unwrap();
    let store = Store::open(&store_root).unwrap();
    assert!(store.find_library(&plan.library).unwrap().is_some_and(|l| l.head.is_some()));
    assert!(other.libraries().unwrap().is_empty(), "pulled into the store the plan was for");
    let again = api::pull_plan(&store_root, &req, &ctx).unwrap();
    assert!(again.noop && !again.new_library, "{:?}", again.summary);
}
