//! What a plugin tells the host reaches the player and the command line printable: its text
//! without control characters or the escape sequences that would move the cursor, retitle the
//! window or recolour the screen, whichever way it comes.

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::api::RpcFailure;
use locallibrary::plugin::Event;
use loclib_plugin::EventName;

use super::events::{listening, told};
use super::imports::given;
use super::*;

/// Text with control characters and escape sequences drawn in among letters (never only those:
/// a manifest's title isn't empty), and what of it prints.
#[hegel::test_helper]
fn noisy(tc: &TestCase) -> (String, String) {
    let unprintable = vec![
        ("\u{1b}[31m", ""),
        ("\u{1b}[2J\u{1b}[H", ""),
        ("\u{1b}]0;retitled\u{7}", ""),
        ("\u{1b}]8;;https://x.example\u{1b}\\", ""),
        ("\u{1b}7", ""),
        ("\r", " "),
        ("\n", " "),
        ("\u{0}", ""),
        ("\u{8}", ""),
        ("\u{7f}", ""),
        // C1's control sequence introducer, alone: what follows it is text.
        ("\u{9b}2K", "2K"),
    ];
    let (mut text, mut prints) = ("a".to_string(), "a".to_string());
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let (sequence, kept) = tc.draw(gs::sampled_from(unprintable.clone()));
        let letters: String = tc.draw(gs::from_regex("[a-z ]{0,3}").fullmatch(true));
        text.push_str(&format!("{sequence}{letters}"));
        prints.push_str(&format!("{kept}{letters}"));
    }
    (text, prints.trim().to_string())
}

/// Whether some string in `v` has `text` in it.
fn holds(v: &Value, text: &str) -> bool {
    match v {
        Value::String(s) => s.contains(text),
        Value::Array(list) => list.iter().any(|v| holds(v, text)),
        Value::Object(map) => map.values().any(|v| holds(v, text)),
        _ => false,
    }
}

/// Whether `v` holds a control character in any string or key.
fn unprintable(v: &Value) -> bool {
    let control = |s: &str| s.chars().any(char::is_control);
    match v {
        Value::String(s) => control(s),
        Value::Array(list) => list.iter().any(unprintable),
        Value::Object(map) => map.iter().any(|(k, v)| control(k) || unprintable(v)),
        _ => false,
    }
}

/// How a plugin's text reaches the host.
#[derive(Debug, Clone, Copy)]
enum Answer {
    /// A headless command's answer, any JSON.
    Headless,
    /// A view command's view.
    View,
    /// A view's rows, sent in parts.
    Partial,
    /// Its error's message and fields.
    Failure,
    /// What it tells the user.
    Notice,
    /// An importer's answer, in a file in its scratch folder.
    InAFile,
    /// Its manifest's titles.
    Manifest,
}

/// For any text with control characters and escape sequences in it, in any kind of answer a
/// plugin gives (a command's JSON, view or rows in parts, an error, a notice, an answer in a file,
/// its manifest), none of them reach what the host gives the player and the command line, and
/// the text around them does. Regression: only check findings, notices and the plugin's log were
/// made printable.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn no_control_character_a_plugin_sends_reaches_the_output(tc: TestCase) {
    let (said, prints) = noisy(&tc);
    let kinds = vec![
        Answer::Headless,
        Answer::View,
        Answer::Partial,
        Answer::Failure,
        Answer::Notice,
        Answer::InAFile,
        Answer::Manifest,
    ];
    let kind = tc.draw(gs::sampled_from(kinds));
    let shown: Value = match kind {
        Answer::Headless => Fixture::echo().echo_params(json!({ "said": [said, { said.clone(): said }] })).unwrap(),
        Answer::View => {
            let view = json!({ "kind": "text", "title": said, "text": said });
            let f = Fixture::echo();
            let CommandOutput::View(view) = f.command("titles", json!({ "view": view }), &Ctx::silent()).unwrap()
            else {
                panic!("a view")
            };
            // Its text keeps its lines (it's drawn as them), and nothing else.
            let mut view = serde_json::to_value(view).unwrap();
            let text = view["text"].as_str().unwrap().replace('\n', " ");
            view["text"] = text.into();
            view
        }
        Answer::Partial => {
            let f = Fixture::echo();
            let parts = Mutex::new(Vec::new());
            let shown = |p: loclib_plugin::Partial| parts.lock().unwrap().push(json!(p.rows));
            let params = json!({ "partial": [said] });
            let view =
                f.plugins.run_command("echo", "titles", params, f.context(), None, &Ctx::silent(), &shown).unwrap();
            let CommandOutput::View(view) = view else { panic!("a view") };
            json!([parts.into_inner().unwrap(), view])
        }
        Answer::Failure => {
            let fail = json!({ "fail": "invalid", "message": said, "fields": { "about": said } });
            let failure = RpcFailure::from(Fixture::echo().echo_params(fail).unwrap_err());
            json!([failure.message, failure.data()])
        }
        Answer::Notice => {
            let (f, notices) = told(|dir| vec![spec(dir, &listening(&["plan.applied"]), &[])], Options::default());
            let tell = json!({ "message": said });
            let event = Event { name: EventName::PlanApplied, drive: None, payload: json!({ "tell": tell }) };
            f.plugins.deliver(&event, Some(&f.store));
            eventually("it tells the user", || !notices.lock().unwrap().is_empty());
            serde_json::to_value(&notices.lock().unwrap()[0]).unwrap()
        }
        Answer::InAFile => {
            let f = Fixture::new(|dir| vec![spec(dir, &given("[\"tags\"]"), &[])], Options::default());
            let read = json!({ "tracks": [], "warnings": [said] });
            let planned = f.import(&read, json!({ "blob": "read.json" })).unwrap();
            json!([planned.summary, planned.warnings])
        }
        Answer::Manifest => {
            // As TOML spells it: escaped, as JSON escapes it, and DEL too.
            let title = serde_json::to_string(&said).unwrap().replace('\u{7f}', "\\u007f");
            let manifest = ECHO.replace("title = \"Answer the params\"", &format!("title = {title}"));
            let f = Fixture::new(|dir| vec![spec(dir, &manifest, &[])], Options::default());
            assert_eq!(f.echo_params(json!({ "n": 1 })).unwrap(), json!({ "n": 1 }), "it describes itself as it is");
            serde_json::to_value(f.plugins.manifests().collect::<Vec<_>>()).unwrap()
        }
    };
    assert!(!unprintable(&shown), "{kind:?}: {shown}");
    assert!(holds(&shown, &prints), "{kind:?}: {prints:?}, the text around them, is there: {shown}");
}
