//! `smart apply` given no drive: the drives it writes are those a rule is for.

use hegel::HealthCheck;
use hegel::TestCase;
use hegel::generators as gs;
use serde_json::Value;

use crate::support::Env;

/// Rule-based playlists, each for every library or for library A only: `smart apply` given no
/// drive plans library A's drive (every rule is for it) and the club drive, which is in no
/// library, only when a rule is for every library; a drive no rule is for is left out, not an
/// error. With no rule at all, there's nothing to apply.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(only_a = Vec::<bool>::new())]
#[hegel::explicit_test_case(only_a = vec![true])]
#[hegel::explicit_test_case(only_a = vec![false, true])]
fn smart_apply_writes_the_drives_a_rule_is_for(tc: TestCase) {
    let fx = Env::from_template();
    let only_a: Vec<bool> = tc.draw(gs::vecs(gs::booleans()).max_size(3));
    for (i, &a) in only_a.iter().enumerate() {
        let path = format!("Smart/Rule {i}");
        let mut args = vec!["--yes", "smart", "add", &path, "--filter", "bpm:>=0"];
        if a {
            args.extend(["--drive", "A"]);
        }
        fx.ok(&args);
    }
    let r = fx.run(&["--json", "smart", "apply", "--dry-run"]);
    if only_a.is_empty() {
        assert!(r.code == 1 && r.out.contains("no smart playlists apply"), "{}\n{}", r.out, r.err);
        return;
    }
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    let v: Value = serde_json::from_str(&r.out).unwrap();
    let mut planned: Vec<String> = v["plan"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["drive"].as_str().unwrap().rsplit('/').next().unwrap().to_string())
        .collect();
    planned.sort();
    let mut expected = vec!["A".to_string()];
    if only_a.contains(&false) {
        expected.push("Club".into());
    }
    assert_eq!(planned, expected, "{}", r.out);
}
