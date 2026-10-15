use locallibrary::plugin::Cli;
use loclib_plugin::{Command, Context, Manifest};
use serde_json::json;

use super::super::app::View;
use super::super::app::testing::{LEFT, blank, mouse, press, spot};
use super::*;

/// A form for a command whose params are `params` (a JSON Schema, as TOML).
fn form(params: &str) -> Form {
    form_in(params, Context::default())
}

/// [`form`], where `context` says what's selected.
fn form_in(params: &str, context: Context) -> Form {
    let manifest = format!(
        "id = \"t\"\nname = \"T\"\nversion = \"0.1.0\"\nprotocol = \"^0.1\"\n\
         [[contributes.commands]]\nname = \"c\"\ntitle = \"Do it\"\nmode = \"headless\"\n{params}"
    );
    let manifest = Manifest::parse(&manifest).unwrap();
    let command: &'static Command = Box::leak(Box::new(manifest.contributes.commands[0].clone()));
    Form::new(Run { plugin: "t", hook: Cli::Command(command), root: None, context })
}

fn filled(f: &mut Form, texts: &[&str]) -> Result<Value, (usize, String)> {
    for (field, text) in f.fields.iter_mut().zip(texts) {
        field.text = text.to_string();
    }
    f.params()
}

/// No params, or none named, is a form of no fields: the command runs without one.
#[test]
fn a_command_without_params_has_no_form() {
    assert!(form("").fields.is_empty());
    assert!(form("params = { type = \"object\" }").fields.is_empty());
}

/// Fields start at their defaults, a choice that can be left out at none and a switch at off.
#[test]
fn fields_start_at_their_defaults() {
    let f = form(
        r#"params = { type = "object", required = ["kind"], properties = { n = { type = "integer", default = 4 }, kind = { type = "string", enum = ["a", "b"] }, maybe = { type = "string", enum = ["a", "b"] }, on = { type = "boolean" }, skip_it = { type = "boolean", default = true } } }"#,
    );
    let texts: Vec<&str> = f.fields.iter().map(|x| x.text.as_str()).collect();
    assert_eq!(texts, ["4", "a", "", "false", "true"]);
    assert_eq!(f.fields[4].label(), "skip it");
    assert_eq!(f.fields[2].choices(), ["", "a", "b"]);
    assert_eq!(f.fields[1].choices(), ["a", "b"], "a choice that's needed can't be none");
    assert!(f.fields[0].choices().is_empty());
}

/// A list of track ids called `tracks` starts as the tracks selected, which it then takes; one
/// called otherwise, of another kind, with a default or with none selected starts empty.
#[test]
fn tracks_start_as_the_ones_selected() {
    let selected = Context { tracks: vec![3, 812], ..Context::default() };
    let ids = r#"type = "array", items = { type = "integer" }"#;
    let schema = format!(
        r#"params = {{ type = "object", properties = {{ tracks = {{ {ids} }}, others = {{ {ids} }}, also = {{ {ids}, default = [1] }} }} }}"#
    );
    let f = form_in(&schema, selected.clone());
    let texts: Vec<&str> = f.fields.iter().map(|x| x.text.as_str()).collect();
    assert_eq!(texts, ["3, 812", "", "1"]);
    assert_eq!(f.params().unwrap()["tracks"], json!([3, 812]));
    assert_eq!(form_in(&schema, Context::default()).fields[0].text, "");
    let names =
        r#"params = { type = "object", properties = { tracks = { type = "array", items = { type = "string" } } } }"#;
    assert_eq!(form_in(names, selected).fields[0].text, "");
}

/// What's typed is read as the command line reads it: numbers, lists with commas between them,
/// pairs, paths made absolute (`~` for home); what's left empty is left out.
#[test]
fn what_is_typed_becomes_the_params() {
    let mut f = form(
        r#"params = { type = "object", properties = { bpm = { type = "number" }, ids = { type = "array", items = { type = "integer" } }, pick = { type = "object", additionalProperties = { type = "integer" } }, file = { type = "string", x-path = true }, note = { type = "string" } } }"#,
    );
    let home = std::env::var("HOME").unwrap_or_default();
    let params = filled(&mut f, &["120.5", "1, #2,3,", "1=4, 2=5", "~/list.m3u", ""]).unwrap();
    assert_eq!(params["bpm"], json!(120.5));
    assert_eq!(params["ids"], json!([1, 2, 3]));
    assert_eq!(params["pick"], json!({ "1": 4, "2": 5 }));
    assert_eq!(params["file"], json!(format!("{home}/list.m3u")));
    assert!(params.get("note").is_none());
    assert_eq!(filled(&mut f, &["fast"]).unwrap_err(), (0, "bpm takes a number, not \"fast\"".into()));
    assert_eq!(filled(&mut f, &["", "1", "4"]).unwrap_err(), (2, "pick takes KEY=VALUE, not \"4\"".into()));
}

/// A click on a field selects it, the wheel leaves it be, and a click outside closes the form;
/// up and shift-tab go round to the last field, and ← picks the choice before.
#[test]
fn fields_are_clicked_and_gone_round() {
    let (mut app, _dir) = blank(View::Library);
    let f = form(
        r#"params = { type = "object", properties = { first = { type = "string" }, second = { type = "string", enum = ["a", "b"] } } }"#,
    );
    app.modal = Some(Modal::Form(f));
    let selected = |app: &App| match &app.modal {
        Some(Modal::Form(f)) => (f.selected, f.fields[1].text.clone()),
        _ => panic!("closed"),
    };
    let second = spot(&mut app, "second");
    mouse(&mut app, ratatui::crossterm::event::MouseEventKind::ScrollDown, second);
    assert_eq!(selected(&app).0, 0);
    mouse(&mut app, LEFT, second);
    assert_eq!(selected(&app), (1, String::new()));
    app.on_key(KeyEvent::new(KeyCode::Left, ratatui::crossterm::event::KeyModifiers::NONE));
    assert_eq!(selected(&app), (1, "b".into()), "round to the last choice");
    app.on_key(KeyEvent::new(KeyCode::BackTab, ratatui::crossterm::event::KeyModifiers::SHIFT));
    app.on_key(KeyEvent::new(KeyCode::Up, ratatui::crossterm::event::KeyModifiers::NONE));
    assert_eq!(selected(&app).0, 1, "round to the last field");
    press(&mut app, " ");
    assert_eq!(selected(&app).1, "", "space picks the next");
    mouse(&mut app, LEFT, (0, 0));
    assert!(app.modal.is_none());
}

/// What the schema refuses is said on the field it's about.
#[test]
fn the_schema_is_checked_on_the_field() {
    let mut f = form(
        r#"params = { type = "object", properties = { a = { type = "string" }, n = { type = "integer", minimum = 1 } } }"#,
    );
    assert_eq!(filled(&mut f, &["x", "0"]).unwrap_err(), (1, "n: 0 is less than the minimum of 1".into()));
    assert_eq!(filled(&mut f, &["x", "1"]).unwrap(), json!({ "a": "x", "n": 1 }));
}

/// A whole number typed in is a param exactly when it reads as one; left empty, the form is
/// short of it only when it's needed.
#[hegel::test(test_cases = 200)]
fn a_whole_number_is_taken_when_it_reads_as_one(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let required = tc.draw(gs::booleans());
    let schema = format!(
        r#"params = {{ type = "object", required = [{}], properties = {{ n = {{ type = "integer" }} }} }}"#,
        if required { "\"n\"" } else { "" }
    );
    let mut f = form(&schema);
    let typed = tc.draw(gs::sampled_from(&["", " ", "7", " 42 ", "#9", "-3", "4.5", "x", "1e3", "99999999999"][..]));
    let read = typed.trim().strip_prefix('#').unwrap_or(typed.trim()).parse::<i64>();
    match (filled(&mut f, &[typed]), typed.trim().is_empty()) {
        (Ok(p), true) => assert!(!required && p == json!({}), "{typed:?}"),
        (Err((0, why)), true) => assert!(required && why == "n is needed", "{typed:?}: {why}"),
        (Ok(p), false) => assert_eq!(p["n"], json!(read.unwrap()), "{typed:?}"),
        (Err((0, why)), false) => assert!(read.is_err(), "{typed:?}: {why}"),
        (Err(e), _) => panic!("{e:?}"),
    }
}

/// Keys in any order keep the form whole: a field is always selected, a switch is on or off and
/// a choice is one of its choices, and what's typed goes to the field selected alone.
#[hegel::test(test_cases = 200)]
fn keys_keep_the_form_whole(tc: hegel::TestCase) {
    use hegel::generators::{self as gs, Generator};
    let (mut app, _dir) = blank(View::Library);
    let f = form(
        r#"params = { type = "object", properties = { a = { type = "string" }, on = { type = "boolean" }, pick = { type = "string", enum = ["x", "y"] }, b = { type = "string" } } }"#,
    );
    app.modal = Some(Modal::Form(f));
    let moves = [KeyCode::Up, KeyCode::Down, KeyCode::Tab, KeyCode::BackTab, KeyCode::Left, KeyCode::Right];
    // What the two text fields should hold.
    let mut typed = [String::new(), String::new()];
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(30)) {
        let Some(Modal::Form(f)) = &app.modal else { panic!("closed") };
        let text = [0, 3].iter().position(|&i| i == f.selected);
        match tc.draw(gs::booleans()) {
            true => {
                let c = tc.draw(gs::sampled_from(&[' ', 'k', 'j', 'q', '7'][..]));
                press(&mut app, &c.to_string());
                if let Some(t) = text {
                    typed[t].push(c);
                }
            }
            false => app.on_key(KeyEvent::new(
                tc.draw(gs::sampled_from(&moves[..]).print_as_debug()),
                ratatui::crossterm::event::KeyModifiers::NONE,
            )),
        }
        let Some(Modal::Form(f)) = &app.modal else { panic!("closed") };
        assert!(f.selected < 4);
        assert!(["true", "false"].contains(&f.fields[1].text.as_str()), "{:?}", f.fields[1].text);
        assert!(["", "x", "y"].contains(&f.fields[2].text.as_str()), "{:?}", f.fields[2].text);
        assert_eq!([f.fields[0].text.as_str(), f.fields[3].text.as_str()], [typed[0].as_str(), typed[1].as_str()]);
    }
}
