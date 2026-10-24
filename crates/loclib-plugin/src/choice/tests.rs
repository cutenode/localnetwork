use hegel::TestCase;
use hegel::generators as gs;
use serde_json::json;

use super::*;
use crate::check_params;

fn item(value: Value) -> ChoiceItem {
    ChoiceItem { label: format!("{value}"), detail: None, value }
}

fn asking(param: &str, multi: bool, values: Vec<Value>) -> Choice {
    Choice { title: "Which one?".into(), param: param.into(), multi, items: values.into_iter().map(item).collect() }
}

/// A question names one of the hook's params, has items, labelled, and each a value it takes.
#[test]
fn a_choice_is_checked_against_the_hooks_params() {
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": { "track": { "type": "integer" }, "tags": { "type": "array", "items": { "type": "string" } } }
    });
    let schema = Some(&schema);
    assert_eq!(asking("track", false, vec![json!(1), json!(2)]).check(schema), Ok(()));
    assert_eq!(asking("tags", true, vec![json!("a")]).check(schema), Ok(()));
    let wrong = |c: Choice| c.check(schema).unwrap_err();
    let unfit =
        |i: usize, param: &str| format!("\"Which one?\"'s item {i} isn't a value of a param {param:?} the hook takes");
    assert_eq!(wrong(asking("track", false, vec![json!(1), json!("x")])), unfit(1, "track"));
    assert_eq!(wrong(asking("tags", true, vec![json!(1)])), unfit(0, "tags"));
    assert_eq!(wrong(asking("other", false, vec![json!(1)])), unfit(0, "other"));
    assert_eq!(wrong(asking("track", false, vec![])), "\"Which one?\" has nothing to choose from");
    assert_eq!(
        wrong(Choice { title: " ".into(), ..asking("track", false, vec![json!(1)]) }),
        "its question has no title"
    );
    let unlabelled = ChoiceItem { label: String::new(), detail: None, value: json!(1) };
    assert_eq!(
        wrong(Choice { items: vec![unlabelled], ..asking("track", false, vec![]) }),
        "\"Which one?\"'s item 0 has no label"
    );
    // A param its schema takes by a pattern, whatever else it requires.
    let entries = json!({
        "type": "object",
        "required": ["file"],
        "additionalProperties": false,
        "patternProperties": { "^[0-9]+$": { "type": ["integer", "null"] } }
    });
    assert_eq!(asking("3", false, vec![json!(42), Value::Null]).check(Some(&entries)), Ok(()));
    assert_eq!(asking("x", false, vec![json!(42)]).check(Some(&entries)).unwrap_err(), unfit(0, "x"));
    // Any param of a hook without a schema, or one open to others.
    assert_eq!(asking("other", false, vec![json!(1)]).check(None), Ok(()));
    assert_eq!(asking("other", false, vec![json!(1)]).check(Some(&json!({ "type": "object" }))), Ok(()));
}

/// What's chosen goes in the param, over what it had, a list of them with `multi`.
#[test]
fn an_answer_sets_the_param() {
    let one = asking("track", false, vec![json!(1)]);
    assert_eq!(one.answered(json!({ "track": 9, "keep": true }), vec![json!(1)]), json!({ "track": 1, "keep": true }));
    assert_eq!(one.answered(Value::Null, vec![json!(1)]), json!({ "track": 1 }));
    let many = asking("tags", true, vec![json!("a"), json!("b")]);
    assert_eq!(many.answered(json!({}), vec![json!("a"), json!("b")]), json!({ "tags": ["a", "b"] }));
    assert_eq!(many.answered(json!({}), vec![]), json!({ "tags": [] }));
}

/// A param's schema and a value it takes, or (`fits` false) one it doesn't.
#[hegel::test_helper]
fn drawn_value(tc: &TestCase, kind: &str, fits: bool) -> Value {
    match (kind, fits) {
        ("integer", true) => json!(tc.draw(gs::integers::<i64>())),
        ("string", true) => json!(tc.draw(gs::text().max_size(8))),
        ("boolean", true) => json!(tc.draw(gs::booleans())),
        ("integer" | "boolean", false) => json!(tc.draw(gs::text().max_size(8))),
        _ => json!(tc.draw(gs::integers::<i64>())),
    }
}

/// Any question about a param of any type is fine for a hook exactly when every item's value fits
/// the param (an item of its list, with `multi`); and answered with any of its items, the params
/// it gives the hook again fit the hook's schema.
#[hegel::test(test_cases = 60)]
fn a_choice_that_checks_is_answered_with_params_that_fit(tc: TestCase) {
    let kind = tc.draw(gs::sampled_from(vec!["integer", "string", "boolean"]));
    let multi = tc.draw(gs::booleans());
    let property = match multi {
        true => json!({ "type": "array", "items": { "type": kind } }),
        false => json!({ "type": kind }),
    };
    let schema = json!({ "type": "object", "additionalProperties": false, "properties": { "pick": property } });
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(5));
    let fitting: Vec<bool> = (0..n).map(|_| tc.draw(gs::weighted_booleans(0.8))).collect();
    let values: Vec<Value> = fitting.iter().map(|&fits| drawn_value(&tc, kind, fits)).collect();
    let choice = asking("pick", multi, values.clone());
    let checked = choice.check(Some(&schema));
    assert_eq!(checked.is_ok(), fitting.iter().all(|&f| f), "{checked:?}");
    if checked.is_err() {
        return;
    }
    let chosen: Vec<Value> = tc.draw(gs::subsequences(values));
    let chosen = match (multi, chosen.is_empty()) {
        (false, true) => vec![choice.items[0].value.clone()],
        _ => chosen,
    };
    let params = choice.answered(json!({}), chosen);
    assert_eq!(check_params(Some(&schema), &params), Ok(()), "{params}");
}
