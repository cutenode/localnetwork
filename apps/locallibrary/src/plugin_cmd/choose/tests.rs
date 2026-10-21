use hegel::TestCase;
use hegel::generators as gs;
use loclib_plugin::ChoiceItem;
use serde_json::json;

use super::*;

fn choice(multi: bool, n: usize) -> Choice {
    let items = (0..n)
        .map(|i| ChoiceItem {
            label: format!("item {i}"),
            detail: (i == 0).then(|| "first".into()),
            value: json!(i * 10),
        })
        .collect();
    Choice { title: "Which?".into(), param: "pick".into(), multi, items }
}

/// One number, from 1, picks one item; with `multi`, any with commas, none empty.
#[test]
fn an_answer_is_the_items_numbered() {
    let one = choice(false, 3);
    assert_eq!(picked(&one, "2\n"), Ok(vec![json!(10)]));
    assert_eq!(picked(&one, " 3 "), Ok(vec![json!(20)]));
    assert_eq!(picked(&one, "1,2"), Err("one number, 1 to 3".into()));
    assert_eq!(picked(&one, ""), Err("one number, 1 to 3".into()));
    assert_eq!(picked(&one, "4"), Err("\"4\" isn't a number from 1 to 3".into()));
    assert_eq!(picked(&one, "0"), Err("\"0\" isn't a number from 1 to 3".into()));
    let many = choice(true, 3);
    assert_eq!(picked(&many, "3, 1"), Ok(vec![json!(20), json!(0)]));
    assert_eq!(picked(&many, "\n"), Ok(vec![]));
    assert_eq!(picked(&many, "1,x"), Err("\"x\" isn't a number from 1 to 3".into()));
}

/// Asked, a question takes the first answer that is one, and fails when there's none.
#[test]
fn a_question_is_asked_until_answered() {
    let c = choice(false, 3);
    assert_eq!(answer(&c, &mut "9\nx\n2\n".as_bytes()).unwrap(), [json!(10)]);
    assert_eq!(answer(&choice(true, 3), &mut "1,3\n".as_bytes()).unwrap(), [json!(0), json!(20)]);
    let e = answer(&c, &mut "4\n".as_bytes()).unwrap_err();
    assert_eq!(e.to_string(), "Which?: no answer");
}

/// `--choose`'s values are JSON, else text, by param, the last given winning.
#[test]
fn choose_takes_json_or_text() {
    let answers = ["3=42", "name=Peak Time", "x=null", "3=7", " t = [1]"].map(String::from);
    let got = given(&answers).unwrap();
    assert_eq!(got["3"], json!(7));
    assert_eq!(got["name"], json!("Peak Time"));
    assert_eq!(got["x"], Value::Null);
    assert_eq!(got["t"], json!([1]));
    assert!(given(&["nothing".into()]).unwrap_err().to_string().contains("--choose takes PARAM=VALUE"));
}

/// Unanswered, a question says how to answer it: its param and each value, with what it is.
#[test]
fn an_unanswered_question_says_how_to_answer_it() {
    let said = unanswered(&[&choice(false, 2), &choice(true, 1)]);
    assert_eq!(
        said,
        "Which?: answer with --choose pick=VALUE, VALUE one of:\n  0   item 0 (first)\n  10  item 1\n\
         Which?: answer with --choose pick=VALUE, VALUE a JSON list of any of:\n  0  item 0 (first)"
    );
}

/// Whatever is typed, an answer is only ever the values of items it names, in its order: one,
/// unless the question takes any.
#[hegel::test(test_cases = 100)]
fn any_answer_typed_is_items_named(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(6));
    let c = choice(tc.draw(gs::booleans()), n);
    let parts: Vec<String> = tc
        .draw(gs::vecs(gs::integers::<i64>().min_value(-1).max_value(8)).max_size(4))
        .into_iter()
        .map(|i| i.to_string())
        .collect();
    let line = parts.join(",");
    match picked(&c, &line) {
        Ok(values) => {
            assert!(c.multi || values.len() == 1, "{line}");
            let named: Vec<Value> = parts.iter().map(|p| json!((p.parse::<usize>().unwrap() - 1) * 10)).collect();
            assert_eq!(values, named);
        }
        Err(_) => {
            let fine = parts.iter().all(|p| p.parse::<usize>().is_ok_and(|i| (1..=n).contains(&i)));
            assert!(!fine || (!c.multi && parts.len() != 1), "{line}");
        }
    }
}
