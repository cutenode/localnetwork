use hegel::TestCase;
use hegel::generators as gs;
use serde_json::json;

use super::*;
use crate::check_params;

fn item(value: Value) -> ChoiceItem {
    ChoiceItem { label: format!("{value}"), detail: None, value }
}

fn asking(param: &str, multi: bool, values: Vec<Value>) -> Choice {
    Choice {
        title: "Which one?".into(),
        param: param.into(),
        multi,
        items: values.into_iter().map(item).collect(),
        more: Vec::new(),
    }
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
    assert_eq!(
        one.answered(json!({ "track": 9, "keep": true }), vec![vec![json!(1)]]),
        json!({ "track": 1, "keep": true })
    );
    assert_eq!(one.answered(Value::Null, vec![vec![json!(1)]]), json!({ "track": 1 }));
    let many = asking("tags", true, vec![json!("a"), json!("b")]);
    assert_eq!(many.answered(json!({}), vec![vec![json!("a"), json!("b")]]), json!({ "tags": ["a", "b"] }));
    assert_eq!(many.answered(json!({}), vec![vec![]]), json!({ "tags": [] }));
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
    let params = choice.answered(json!({}), vec![chosen]);
    assert_eq!(check_params(Some(&schema), &params), Ok(()), "{params}");
}

/// Questions about the params `params`, each taking the track id 1: the first with the rest as
/// its `more`.
fn several(params: &[&str]) -> Choice {
    Choice::together(params.iter().map(|p| asking(p, false, vec![json!(1)]))).unwrap()
}

/// Questions asked together come first then the rest in turn, at most as many as a round may
/// ask; each is checked as one alone would be, about a param of its own, asking none of its own,
/// and named by its place when it's wrong.
#[test]
fn questions_asked_together_are_each_checked() {
    let schema = json!({ "type": "object", "patternProperties": { "^[0-9]+$": { "type": "integer" } } });
    let schema = Some(&schema);
    assert_eq!(Choice::together([]), None);
    let three = several(&["1", "2", "3"]);
    assert_eq!(three.questions().map(|q| q.param.as_str()).collect::<Vec<_>>(), ["1", "2", "3"]);
    assert_eq!(three.check(schema), Ok(()));
    let all: Vec<String> = (1..=Choice::MAX_QUESTIONS + 1).map(|n| n.to_string()).collect();
    let most = several(&all.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(most.questions().count(), Choice::MAX_QUESTIONS, "the rest wait for the round after");
    assert_eq!(most.check(schema), Ok(()));
    let mut too_many = most.clone();
    too_many.more.push(asking("x", false, vec![json!(1)]));
    assert_eq!(too_many.check(schema).unwrap_err(), "it asks 65 questions at once, more than 64");
    let twice = Choice {
        more: vec![asking("2", false, vec![json!(1)]), asking("1", false, vec![json!(1)])],
        ..several(&["1"])
    };
    assert_eq!(twice.check(schema).unwrap_err(), "question 3 of 3: \"Which one?\" asks for \"1\" again");
    let mut nested = several(&["1", "2"]);
    nested.more[0].more.push(asking("3", false, vec![json!(1)]));
    assert_eq!(nested.check(schema).unwrap_err(), "question 2 of 2: \"Which one?\" asks more questions of its own");
    let mut unfit = several(&["1", "2"]);
    unfit.more[0].items[0].value = json!("x");
    let unfit = unfit.check(schema).unwrap_err();
    assert_eq!(unfit, "question 2 of 2: \"Which one?\"'s item 0 isn't a value of a param \"2\" the hook takes");
    let mut empty = several(&["1", "2"]);
    empty.more[0].items.clear();
    assert_eq!(empty.check(None).unwrap_err(), "question 2 of 2: \"Which one?\" has nothing to choose from");
}

/// Each question's answer goes in its own param; one without an answer leaves its param be.
#[test]
fn answers_set_each_questions_param() {
    let mut c = several(&["1", "2", "3"]);
    c.more[1].multi = true;
    let all = c.answered(json!({ "keep": 0 }), vec![vec![json!(4)], vec![Value::Null], vec![json!(5), json!(6)]]);
    assert_eq!(all, json!({ "keep": 0, "1": 4, "2": null, "3": [5, 6] }));
    assert_eq!(c.answered(json!({ "2": 9 }), vec![vec![json!(4)]]), json!({ "1": 4, "2": 9 }), "only the first");
}

/// On the wire the further questions are `more`, which a host that doesn't know them reads past:
/// a `Choice` as it was before them takes the first question alone.
#[test]
fn a_host_that_doesnt_know_more_reads_the_first_question() {
    #[derive(Debug, Deserialize)]
    struct Before {
        param: String,
        #[serde(default)]
        multi: bool,
        items: Vec<ChoiceItem>,
    }
    let mut c = several(&["1", "2"]);
    c.more[0].multi = true;
    let wire = serde_json::to_value(&c).unwrap();
    assert_eq!(wire["more"][0]["param"], "2");
    assert_eq!(wire["more"][0]["multi"], true);
    let before: Before = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!((before.param.as_str(), before.multi, before.items), ("1", false, c.items.clone()));
    assert_eq!(serde_json::from_value::<Choice>(wire).unwrap(), c);
    let one = serde_json::to_value(asking("1", false, vec![json!(1)])).unwrap();
    assert!(one.get("more").is_none(), "{one}");
}

/// Any questions asked together, about any params (now and then one twice, one of a type the
/// hook doesn't take, or more than a round may ask): they're fine for a hook exactly when they're
/// at most as many as a round may ask, each about a param of its own whose values it takes; and
/// answered, every param asked about is set, fitting the hook's schema.
#[hegel::test(test_cases = 60)]
fn questions_that_check_are_answered_with_params_that_fit(tc: TestCase) {
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "patternProperties": {
            "^[0-9]+$": { "type": "integer" },
            "^tags[0-9]+$": { "type": "array", "items": { "type": "string" } }
        }
    });
    let too_many = tc.draw(gs::weighted_booleans(0.05));
    let n = match too_many {
        true => Choice::MAX_QUESTIONS + 1,
        false => tc.draw(gs::integers::<usize>().min_value(1).max_value(8)),
    };
    let (mut fine, mut params) = (!too_many, Vec::<String>::new());
    let questions: Vec<Choice> = (0..n)
        .map(|i| {
            let param = match !params.is_empty() && tc.draw(gs::weighted_booleans(0.1)) {
                true => tc.draw(gs::sampled_from(params.clone())),
                false if tc.draw(gs::booleans()) => format!("tags{i}"),
                false => i.to_string(),
            };
            let multi = param.starts_with("tags");
            let fits = tc.draw(gs::weighted_booleans(0.9));
            fine &= fits && !params.contains(&param);
            params.push(param.clone());
            asking(&param, multi, vec![drawn_value(&tc, if multi { "string" } else { "integer" }, fits)])
        })
        .collect();
    let choice = Choice { more: questions[1..].to_vec(), ..questions[0].clone() };
    let checked = choice.check(Some(&schema));
    assert_eq!(checked.is_ok(), fine, "{checked:?}");
    if !fine {
        return;
    }
    let chosen: Vec<Vec<Value>> = choice.questions().map(|q| vec![q.items[0].value.clone()]).collect();
    let params = choice.answered(json!({}), chosen);
    assert_eq!(params.as_object().map(Map::len), Some(n), "{params}");
    assert_eq!(check_params(Some(&schema), &params), Ok(()), "{params}");
}
