//! Questions a plugin asks the user first (the `choose` feature): a view command's
//! `View::Choose`, an operation's `{"choose": ...}`, which the host takes only from a plugin with
//! the feature and only about a param the hook takes; answered, the hook runs again with it.

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::plugin::OpPlanned;
use loclib_plugin::{Choice, ChoiceItem};

use super::*;

fn choice(param: &str, values: Vec<Value>) -> Choice {
    let items = values.into_iter().map(|value| ChoiceItem { label: value.to_string(), detail: None, value }).collect();
    Choice { title: "Which one?".into(), param: param.into(), multi: false, items }
}

/// The echo manifest, its `playlist` operation's name not required: a param a hook asks for is
/// one its schema doesn't require, or the host would refuse its params before asking it.
fn optional_name() -> String {
    let required = ", required = [\"name\"] }";
    assert!(ECHO.contains(required));
    ECHO.replace(required, " }")
}

impl Fixture {
    /// The echo plugin, its playlist's name optional.
    fn asking() -> Fixture {
        Fixture::new(|dir| vec![spec(dir, &optional_name(), &[])], Options::default())
    }

    /// Plans the echo plugin's `playlist` operation with `params`.
    fn plan_asking(&self, params: Value) -> Result<OpPlanned> {
        self.plugins.plan_op("echo", "playlist", params, self.context(), Some(&self.store), &Ctx::silent())
    }
}

#[test]
fn a_view_command_asks_and_is_run_again_answered() {
    let f = Fixture::echo();
    let asked = choice("pick", vec![json!(1), json!("two")]);
    let params = json!({ "ask": asked });
    let CommandOutput::View(View::Choose(got)) = f.command("titles", params.clone(), &Ctx::silent()).unwrap() else {
        panic!("a question")
    };
    assert_eq!(got, asked);
    let answered = got.answered(params, vec![got.items[1].value.clone()]);
    let CommandOutput::View(View::Text { text, .. }) = f.command("titles", answered, &Ctx::silent()).unwrap() else {
        panic!("its answer")
    };
    assert_eq!(text, "\"two\"");
    // A headless command's answer is any JSON, a question's shape too.
    let headless = f.echo_params(json!({ "kind": "choose", "title": "x" })).unwrap();
    assert_eq!(headless["kind"], "choose");
}

#[test]
fn an_operation_asks_and_is_planned_again_answered() {
    let f = Fixture::asking();
    let params = json!({ "ask": choice("name", vec![json!("Warm up"), json!("Peak")]) });
    let OpPlanned::Choose { choose } = f.plan_asking(params.clone()).unwrap() else { panic!("a question") };
    let e = f.plan_asking(params.clone()).and_then(OpPlanned::plan).unwrap_err();
    assert_eq!(e.to_string(), "Which one?: it asks which first, as `name` in its params");
    let answered = choose.answered(params, vec![json!("Peak")]);
    let plan = f.plan_asking(answered).unwrap().plan().unwrap();
    assert_eq!(plan.playlists_created, ["Peak"]);
}

/// A question is taken only from a plugin with the `choose` feature, and only about a param the
/// hook takes, with something to choose.
#[test]
fn a_question_needs_the_feature_and_a_param_the_hook_takes() {
    let f = Fixture::asking();
    let refused = |c: Choice| failure(f.plan_asking(json!({ "ask": c })).unwrap_err());
    assert_eq!(
        refused(choice("name", vec![json!(5)])),
        "the Echo plugin failed: its question won't do: \"Which one?\"'s item 0 isn't a value of a param \"name\" the hook takes"
    );
    assert_eq!(
        refused(choice("name", vec![])),
        "the Echo plugin failed: its question won't do: \"Which one?\" has nothing to choose from"
    );
    // Answered, it may not ask the same again: the user would answer it forever.
    let again = json!({ "ask": choice("name", vec![json!("x")]), "name": "x", "ask_again": true });
    assert_eq!(
        failure(f.plan_asking(again).unwrap_err()),
        "the Echo plugin failed: it asks again for name, which it was given"
    );
    let again = json!({ "ask": choice("pick", vec![json!(1)]), "pick": 1, "ask_again": true });
    let e = f.command("titles", again, &Ctx::silent()).unwrap_err();
    assert_eq!(failure(e), "the Echo plugin failed: it asks again for pick, which it was given");
    let without = Fixture::new(|dir| vec![spec(dir, &optional_name(), &["--without", "choose"])], Options::default());
    let e = without.plan_asking(json!({ "ask": choice("name", vec![json!("x")]) })).unwrap_err();
    assert_eq!(failure(e), "the Echo plugin failed: it asks a question without the choose feature");
    let e = without.command("titles", json!({ "ask": choice("pick", vec![json!(1)]) }), &Ctx::silent()).unwrap_err();
    assert_eq!(failure(e), "the Echo plugin failed: it asks a question without the choose feature");
}

/// Any question an operation asks, of its `name` or another param, of values of any kind: the
/// host passes it on exactly when the operation could be planned again with each of its answers,
/// and planned with any of them, the plan is the operation's with that param.
#[hegel::test(test_cases = 15)]
fn any_question_passed_on_is_one_its_answers_plan(tc: TestCase) {
    let f = Fixture::asking();
    let param = tc.draw(gs::sampled_from(vec!["name", "other"]));
    let n = tc.draw(gs::integers::<usize>().max_value(3));
    let values: Vec<Value> = (0..n)
        .map(|_| match tc.draw(gs::weighted_booleans(0.8)) {
            true => json!(tc.draw(gs::from_regex("[A-Za-z]([A-Za-z ]{0,8}[A-Za-z])?").fullmatch(true))),
            false => json!(tc.draw(gs::integers::<i32>())),
        })
        .collect();
    let asked = choice(param, values.clone());
    // The playlist operation takes any param besides its name, a string.
    let fits = !values.is_empty() && (param == "other" || values.iter().all(Value::is_string));
    let params = match param {
        "name" => json!({ "ask": asked }),
        _ => json!({ "ask": asked, "name": "Kept" }),
    };
    match f.plan_asking(params.clone()) {
        Ok(OpPlanned::Choose { choose }) => {
            assert!(fits, "{asked:?}");
            assert_eq!(choose, asked);
            let pick = tc.draw(gs::sampled_from(values));
            let named = pick.as_str().filter(|_| param == "name").unwrap_or("Kept").to_string();
            let plan = f.plan_asking(choose.answered(params, vec![pick])).unwrap().plan().unwrap();
            assert_eq!(plan.playlists_created, [named]);
        }
        Ok(OpPlanned::Plan(_)) => panic!("asked, not planned"),
        Err(e) => {
            assert!(!fits, "{asked:?}: {e}");
            assert!(failure(e).starts_with("the Echo plugin failed: its question won't do: "));
        }
    }
}
