use hegel::generators as gs;
use serde_json::json;

use super::super::super::app::testing::{app_on, last_message, plugged, plugins_once, press, wait};
use super::super::super::edits::Modal;
use super::*;

/// Every key the player uses, written as a binding would be: none of them can be bound.
#[test]
fn the_players_own_keys_cant_be_bound() {
    let act = Act::Plugin("echo", "titles");
    for spec in ["q", "z", "e", "?", ":", "1", "5", "8", "R", "F5", "f5", "ctrl-c", "ctrl+q", "ctrl-C"] {
        let refused = bind(&[], spec, act, "Titles (Echo)");
        assert!(refused.as_ref().is_err_and(|why| why.contains(" is the player's (")), "{spec}: {refused:?}");
    }
    let why = bind(&[], "z", act, "Titles (Echo)").unwrap_err();
    assert_eq!(why, "Titles (Echo): z is the player's (zoom the set's timeline)");
}

/// A key that's free is bound, once: the second command it's set for is refused.
#[test]
fn a_free_key_is_bound_once() {
    let (titles, steps) = (Act::Plugin("echo", "titles"), Act::Plugin("echo", "steps"));
    let first = bind(&[], "%", titles, "Titles (Echo)").unwrap();
    assert_eq!(first.1, titles);
    let why = bind(&[first], " % ", steps, "Steps (Echo)").unwrap_err();
    assert_eq!(why, "Steps (Echo): % is bound to another command");
    let percent = KeyEvent::new(KeyCode::Char('%'), KeyModifiers::SHIFT);
    assert_eq!(lookup(&[first], &percent), Some(titles));
    let ctrl = KeyEvent::new(KeyCode::Char('%'), KeyModifiers::CONTROL);
    assert_eq!(lookup(&[first], &ctrl), None, "not with control held");
}

/// What isn't a key is refused, saying what one is.
#[test]
fn what_isnt_a_key_is_refused() {
    for spec in ["", "  ", "ab", "ctrl-", "ctrl-ab", "ctrl-1", "f0", "f13", "shift-a", "\t"] {
        let why = parse(spec).unwrap_err();
        assert!(why.ends_with("isn't a key: a character, ctrl- and a letter, or f1 to f12"), "{spec:?}: {why}");
    }
}

/// For any character, ctrl- and a letter, or function key, it reads as that key, and is bound
/// exactly when the player uses it nowhere: then pressing it is what it's bound to.
#[hegel::test(test_cases = 200)]
fn any_key_is_bound_unless_the_player_uses_it(tc: hegel::TestCase) {
    let (spec, press) = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => {
            let c = tc.draw(gs::sampled_from((' '..='~').filter(|c| *c != ' ').chain(['é', 'ß']).collect::<Vec<_>>()));
            let shift = if c.is_uppercase() { KeyModifiers::SHIFT } else { KeyModifiers::NONE };
            (c.to_string(), KeyEvent::new(KeyCode::Char(c), shift))
        }
        1 => {
            let c = tc.draw(gs::sampled_from(('a'..='z').collect::<Vec<_>>()));
            (format!("ctrl-{c}"), KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
        }
        _ => {
            let n = tc.draw(gs::integers::<u8>().min_value(1).max_value(12));
            (format!("F{n}"), KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE))
        }
    };
    let act = Act::Plugin("echo", "titles");
    let players = KEYMAP.iter().any(|b| b.keys.iter().any(|k| k.matches(&press)));
    match bind(&[], &spec, act, "Titles (Echo)") {
        Ok(b) => {
            assert!(!players, "{spec} is the player's");
            assert_eq!(lookup(&[b], &press), Some(act));
        }
        Err(why) => assert!(players && why.contains(" is the player's ("), "{spec}: {why}"),
    }
}

/// Keys set for the echo plugin's commands are bound when the player starts: pressing one runs
/// its command as the palette would. One the player uses, or one already bound, is refused,
/// saying why. Set in its settings form, a refused one is said and left as it was, and the rest
/// are kept and bound at once.
#[test]
fn keys_set_for_plugins_commands_run_them() {
    plugins_once();
    let drive = plugged();
    let mut app = app_on(&drive.0);
    let plugins = crate::plugins::host().unwrap();
    let store = Store::open_or_init(&app.store_root).unwrap();
    for (command, key) in [("titles", "%"), ("steps", "z"), ("wait", "%")] {
        plugins.set_setting("echo", &store, &format!("keys.{command}"), Some(json!(key))).unwrap();
    }
    app.bind_plugin_keys();
    let said: Vec<String> = app.log.iter().map(|(m, ..)| m.clone()).collect();
    assert!(
        said.iter().any(|m| m == "Report progress in steps (Echo): z is the player's (zoom the set's timeline)"),
        "{said:?}"
    );
    assert!(said.iter().any(|m| m == "Wait until cancelled (Echo): % is bound to another command"), "{said:?}");
    assert_eq!(app.plugin_keys.len(), 1);
    app.modal = None;
    press(&mut app, "%");
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Report(r)) = &app.modal else { panic!("no panel: {}", last_message(&app)) };
    assert_eq!(r.title, app.drive_label(&drive.0));
    app.modal = None;
    // From the settings form: `z` is refused and left as it was (unset), `^` kept and bound.
    plugins.set_setting("echo", &store, "keys.wait", None).unwrap();
    app.save_settings("echo", json!({ "keys.titles": "%", "keys.steps": "^", "keys.wait": "z" }));
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "Wait until cancelled (Echo): z is the player's (zoom the set's timeline)");
    let kept = plugins.settings("echo", Some(&store)).unwrap();
    assert_eq!((kept.get("keys.steps"), kept.get("keys.wait")), (Some(&json!("^")), None));
    let bound: Vec<Act> = app.plugin_keys.iter().map(|(_, a)| *a).collect();
    assert_eq!(bound, [Act::Plugin("echo", "titles"), Act::Plugin("echo", "steps")]);
}
