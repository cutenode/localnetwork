//! Plugins' questions in the player, a list to choose from (any of its items, marked, when it
//! takes several), and what providers find for the tracks selected, asked about track by track.

use std::path::{Path, PathBuf};

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::Store;
use loclib_plugin::ProviderKind;
use ratatui::crossterm::event::KeyCode;
use serde_json::{Value, json};

use super::super::super::app::App;
use super::super::super::app::testing::{
    app_on, demo_drive_in, echo_plugin, key, last_message, mounted, press, say_yes, settle, wait,
};
use super::super::super::edits::Modal;
use super::super::super::ui::screen;
use super::super::{Cli, Context, Run};
use super::{panel, picker};

/// The folder the drives of this process's tests are in, with the echo plugin's manifest beside
/// them, its playlist's name optional (a param a hook asks for isn't one the host requires) and
/// a metadata provider, `found`, answering as its `answer` setting says.
fn asking_once() -> &'static Path {
    static DIR: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        echo_plugin(dir.path());
        let file = dir.path().join("echo.toml");
        let required = r#"x-from = "selection.tracks" } }, required = ["name"] }"#;
        let manifest = std::fs::read_to_string(&file).unwrap();
        assert!(manifest.contains(required));
        let more = "\n[[contributes.providers]]\nname = \"found\"\ntitle = \"Found\"\nkind = \"metadata\"\n\n\
                    [[contributes.settings]]\nkey = \"answer\"\ntitle = \"Answer\"\ntype = \"string\"\n";
        std::fs::write(&file, manifest.replace(required, r#"x-from = "selection.tracks" } } }"#) + more).unwrap();
        mounted(dir.path());
        dir
    })
    .path()
}

/// A demo drive of its own for a test (or a case), open.
fn opened() -> (PathBuf, App) {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let root = demo_drive_in(asking_once(), &format!("ASK{n}"));
    let app = app_on(&root);
    (root, app)
}

/// The echo plugin's command or operation `name`, run on the drive at `root`.
fn hook(app: &App, root: &Path, name: &str) -> Run {
    let c = &crate::plugins::host().unwrap().manifest("echo").unwrap().contributes;
    let command = c.commands.iter().find(|x| x.name == name).map(Cli::Command);
    let hook = command.or_else(|| c.operations.iter().find(|x| x.name == name).map(Cli::Operation)).unwrap();
    let context = Context { drive: app.plugin_drive(root), ..Context::default() };
    Run { plugin: "echo", hook, root: Some(root.to_path_buf()), context }
}

/// A question about `param`, its items labelled after their values.
fn question(param: &str, multi: bool, values: &[Value]) -> Value {
    let items: Vec<Value> = values.iter().map(|v| json!({ "label": format!("item {v}"), "value": v })).collect();
    json!({ "title": "Which one?", "param": param, "multi": multi, "items": items })
}

/// A command's question is a list to choose from, at any width; chosen, it runs again with the
/// answer, which it shows.
#[test]
fn a_commands_question_is_chosen_from_and_it_runs_again() {
    let (root, mut app) = opened();
    let run = hook(&app, &root, "titles");
    app.run_plugin(run, json!({ "ask": question("pick", false, &[json!(1), json!("two")]) }));
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(picker(&app).title, "Which one?");
    for (w, h) in [(40, 16), (120, 40)] {
        let s = screen(&mut app, w, h);
        for needle in ["Which one?", "item 1", "item \"two\"", "enter"] {
            assert!(s.contains(needle), "{needle:?} isn't in\n{s}");
        }
        assert!(!s.contains("space mark"), "one is chosen\n{s}");
    }
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(panel(&app), "\"two\"");
    // Esc asks nothing more, and runs nothing.
    let run = hook(&app, &root, "titles");
    app.modal = None;
    app.run_plugin(run, json!({ "ask": question("pick", false, &[json!(1)]) }));
    wait(&mut app, |a| a.job.is_none());
    press(&mut app, "\u{1b}");
    assert!(app.modal.is_none() && app.job.is_none());
}

/// A question taking several is answered with those marked (space, or a click), none too, the
/// marks shown as the tracks' are.
#[test]
fn a_question_taking_several_takes_those_marked() {
    let (root, mut app) = opened();
    let values = [json!(1), json!(2), json!(3)];
    app.run_plugin(hook(&app, &root, "titles"), json!({ "ask": question("picks", true, &values) }));
    wait(&mut app, |a| a.job.is_none());
    press(&mut app, " ");
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Down);
    press(&mut app, " ");
    assert_eq!(picker(&app).query, "", "space marks, rather than typing");
    for (w, h) in [(40, 16), (120, 40)] {
        let s = screen(&mut app, w, h);
        assert_eq!(s.matches('✓').count(), 2, "{s}");
        assert!(w < 72 || s.contains("space mark"), "{s}");
    }
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(panel(&app), "[1,3]");
    app.modal = None;
    app.run_plugin(hook(&app, &root, "titles"), json!({ "ask": question("picks", true, &values) }));
    wait(&mut app, |a| a.job.is_none());
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(panel(&app), "[]", "none marked");
}

/// An operation's question is asked as it's planned; answered, it's planned again, and the plan
/// confirmed and applied as any is.
#[test]
fn an_operations_question_is_asked_then_it_is_planned() {
    let (root, mut app) = opened();
    let names = [json!("Warm up"), json!("Peak")];
    app.run_plugin(hook(&app, &root, "playlist"), json!({ "ask": question("name", false, &names) }));
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Planning"));
    wait(&mut app, |a| a.job.is_none());
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    say_yes(&mut app);
    let lib = locallibrary::Device::open_export(&root).unwrap().library().unwrap();
    assert!(lib.playlists.iter().any(|p| p.name == "Peak"), "{}", last_message(&app));
    // Planned without a drive that's still there: said, and not asked.
    app.loaded.as_mut().unwrap().stale = true;
    app.run_plugin(hook(&app, &root, "playlist"), json!({ "ask": question("name", false, &names) }));
    assert!(app.job.is_none());
    assert_eq!(last_message(&app), "the drive was unplugged");
}

/// The echo plugin's providers answer `answer`.
fn answering(app: &App, answer: Value) {
    let store = Store::open_or_init(&app.store_root).unwrap();
    let plugins = crate::plugins::host().unwrap();
    plugins.set_setting("echo", &store, "answer", Some(answer.to_string().into())).unwrap();
}

/// A candidate with these tags.
fn titled(track: u32, score: f64, title: &str) -> Value {
    json!({ "track": track, "score": score, "source": "Tags", "label": title, "metadata": { "title": title } })
}

/// Fetching track details for both tracks from the `found` provider, over what they have: the
/// form, then the task.
fn fetch_details(app: &mut App, root: &Path) {
    press(app, "mjm");
    let target = app.target();
    app.fetch_form(ProviderKind::Metadata, Some(root.to_path_buf()), target.as_ref());
    let s = screen(app, 120, 40);
    for needle in ["Fetch track details", "from", "replace", "Only from this provider", "‹ none ›"] {
        assert!(s.contains(needle), "{needle:?} isn't in\n{s}\n{}", last_message(app));
    }
    // The third provider (after none, coverart's, which would go online, and echo's own), and
    // over what they have.
    key(app, KeyCode::Right);
    key(app, KeyCode::Right);
    key(app, KeyCode::Right);
    let s = screen(app, 120, 40);
    assert!(s.contains("‹ echo.found ›"), "not echo's found:\n{s}");
    key(app, KeyCode::Tab);
    press(app, " \n");
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Fetching track details"));
    wait(app, |a| a.job.is_none());
}

/// The track details providers find are offered track by track (one alone sure enough picked
/// already), their warnings in the log, and what's chosen planned and confirmed.
#[test]
fn what_providers_find_is_asked_about_then_planned() {
    let (root, mut app) = opened();
    let tracks = app.loaded.as_ref().unwrap().library.tracks.clone();
    let (sure, unsure) = (&tracks[0], &tracks[1]);
    let candidates =
        [titled(sure.id, 0.95, "Sure"), titled(unsure.id, 0.5, "Maybe A"), titled(unsure.id, 0.4, "Maybe B")];
    answering(&app, json!({ "candidates": candidates, "warnings": ["one more"] }));
    fetch_details(&mut app, &root);
    assert!(app.log.iter().any(|(m, ..)| m == "Found: one more"), "{:?}", app.log);
    let p = picker(&app);
    assert_eq!(p.title, unsure.display_name());
    let labels: Vec<&str> = p.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["Maybe A", "Maybe B", "none of these"]);
    assert_eq!(p.items[0].detail, "Tags, 50%: Maybe A");
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    settle(&mut app);
    let s = screen(&mut app, 120, 40);
    // What the host planned from them said first, as a plugin's own plan's summary is.
    let planned = "Plugins: fill in the details of 2 track(s) from Tags";
    for needle in [planned, "set title \"Sure\" on 1 track(s)", "set title \"Maybe B\" on 1 track(s)"] {
        assert!(s.contains(needle), "{needle:?} isn't in\n{s}");
    }
    say_yes(&mut app);
    let lib = locallibrary::Device::open_export(&root).unwrap().library().unwrap();
    let title = |id| lib.tracks.iter().find(|t| t.id == id).unwrap().title.clone();
    assert_eq!((title(sure.id), title(unsure.id)), ("Sure".to_string(), "Maybe B".to_string()));
}

/// Nothing found is said; the providers' menu entries need a track selected.
#[test]
fn nothing_found_is_said() {
    let (root, mut app) = opened();
    answering(&app, json!({ "warnings": ["not there"] }));
    fetch_details(&mut app, &root);
    assert!(app.modal.is_none());
    assert_eq!(last_message(&app), "no track details found for 2 tracks (! says why)");
    app.fetch_form(ProviderKind::Metadata, Some(root.clone()), None);
    assert_eq!((last_message(&app), app.modal.is_none()), ("no track selected", true));
}

/// The providers' settings in a form, from the palette.
fn provider_settings(app: &mut App) -> Vec<String> {
    press(app, ":provider settings\n");
    let Some(Modal::Form(f)) = &app.modal else { panic!("no form: {}", last_message(app)) };
    f.fields.iter().map(|x| x.text.clone()).collect()
}

/// The score a candidate alone at or above is picked with is a setting in the palette, as it's set
/// now, kept where `plugin providers` keeps it (left empty, back to its default); fetching then
/// picks by it.
#[test]
fn the_auto_pick_is_set_from_the_palette() {
    let (root, mut app) = opened();
    assert_eq!(provider_settings(&mut app), ["0.9"]);
    let s = screen(&mut app, 120, 40);
    for needle in ["Provider settings", "auto pick", "(empty for 0.9)", "enter save"] {
        assert!(s.contains(needle), "{needle:?} isn't in\n{s}");
    }
    let auto_pick = |app: &App| locallibrary::plugin::auto_pick(Store::open(&app.store_root).ok().as_ref());
    let kept = |app: &App| {
        let file = app.store_root.join("plugins/settings.json");
        serde_json::from_str::<Value>(&std::fs::read_to_string(file).unwrap()).unwrap()["providers.auto_pick"].clone()
    };
    for _ in 0.."0.9".len() {
        key(&mut app, KeyCode::Backspace);
    }
    press(&mut app, "0.5\n");
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Saving settings"));
    wait(&mut app, |a| a.job.is_none());
    assert_eq!((last_message(&app), auto_pick(&app), kept(&app)), ("provider settings saved", 0.5, json!(0.5)));
    // Open again: as set now. Nothing changed: nothing written.
    assert_eq!(provider_settings(&mut app), ["0.5"]);
    press(&mut app, "\n");
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "nothing changed");
    // A candidate alone at 0.5 is picked now: nothing's asked.
    let one = json!({ "score": 0.6, "source": "Tags", "label": "Picked", "metadata": { "title": "Picked" } });
    answering(&app, json!({ "each": [one] }));
    fetch_details(&mut app, &root);
    settle(&mut app);
    let Some(Modal::Confirm(c)) = &app.modal else { panic!("a confirmation: {}", last_message(&app)) };
    assert_eq!(c.plan.summary()[0], "fill in the details of 2 track(s) from Tags");
    press(&mut app, "\u{1b}");
    // A score below 0 isn't one; left empty, it's the default again.
    provider_settings(&mut app);
    for _ in 0.."0.5".len() {
        key(&mut app, KeyCode::Backspace);
    }
    press(&mut app, "-1\n");
    let Some(Modal::Form(f)) = &app.modal else { panic!("closed") };
    assert!(f.error.as_deref().is_some_and(|e| e.starts_with("auto pick: ")), "{:?}", f.error);
    assert!(app.job.is_none());
    key(&mut app, KeyCode::Backspace);
    key(&mut app, KeyCode::Backspace);
    press(&mut app, "\n");
    wait(&mut app, |a| a.job.is_none());
    assert_eq!((last_message(&app), auto_pick(&app), kept(&app)), ("provider settings saved", 0.9, Value::Null));
}

/// Any score typed in the providers' settings: a number of 0 or more is kept as typed (left empty,
/// the default), anything else refused on its field with nothing kept.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn any_auto_pick_typed_is_kept_or_refused(tc: TestCase) {
    let (_root, mut app) = opened();
    let typed = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => String::new(),
        1 => tc.draw(gs::sampled_from(vec!["high", "0.5.1", "-"])).to_string(),
        _ => format!("{:.2}", tc.draw(gs::integers::<i16>().min_value(-200).max_value(300)) as f64 / 100.0),
    };
    let score: Option<f64> = typed.parse().ok();
    provider_settings(&mut app);
    for _ in 0.."0.9".len() {
        key(&mut app, KeyCode::Backspace);
    }
    press(&mut app, &format!("{typed}\n"));
    let auto_pick = |app: &App| locallibrary::plugin::auto_pick(Store::open(&app.store_root).ok().as_ref());
    match score {
        Some(s) if s < 0.0 => {}
        Some(s) => {
            wait(&mut app, |a| a.job.is_none());
            let said = if s == 0.9 { "nothing changed" } else { "provider settings saved" };
            assert_eq!((last_message(&app), auto_pick(&app)), (said, s), "{typed:?}");
            return;
        }
        None if typed.is_empty() => {
            wait(&mut app, |a| a.job.is_none());
            assert_eq!((last_message(&app), auto_pick(&app)), ("nothing changed", 0.9));
            return;
        }
        None => {}
    }
    let Some(Modal::Form(f)) = &app.modal else { panic!("{typed:?} saved: {}", last_message(&app)) };
    assert!(f.error.as_deref().is_some_and(|e| e.starts_with("auto pick")), "{typed:?}: {:?}", f.error);
    assert!(app.job.is_none() && !app.store_root.join("plugins/settings.json").exists(), "{typed:?}");
}

/// The values a case answers a question with, as the keys choosing them: one item, or (taking
/// several) any of them marked.
#[hegel::test_helper]
fn chosen(tc: &TestCase, n: usize, multi: bool) -> Vec<usize> {
    match multi {
        true => tc.draw(gs::subsequences((0..n).collect::<Vec<_>>())),
        false => vec![tc.draw(gs::integers::<usize>().max_value(n - 1))],
    }
}

/// Moves the list open to its item `i`, marking it with space when `mark`.
fn to_item(app: &mut App, i: usize, mark: bool) {
    while picker(app).state.selected().unwrap_or(0) < i {
        key(app, KeyCode::Down);
    }
    if mark {
        press(app, " ");
    }
}

/// Any question, of one or several of any number of items, answered with any of them by the keys:
/// the command runs again with exactly those values, in the question's order.
#[hegel::test(test_cases = 15, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn any_question_runs_again_with_exactly_what_was_chosen(tc: TestCase) {
    let (root, mut app) = opened();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(6));
    let multi = tc.draw(gs::booleans());
    let values: Vec<Value> = (0..n as i64).map(|i| json!(i * 7)).collect();
    app.run_plugin(hook(&app, &root, "titles"), json!({ "ask": question("pick", multi, &values) }));
    wait(&mut app, |a| a.job.is_none());
    let w = tc.draw(gs::integers::<u16>().min_value(20).max_value(160));
    let h = tc.draw(gs::integers::<u16>().min_value(5).max_value(50));
    assert_eq!(screen(&mut app, w, h).lines().count(), h as usize);
    let picks = chosen(&tc, n, multi);
    for &i in &picks {
        to_item(&mut app, i, multi);
    }
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| a.job.is_none());
    let answer: Vec<Value> = picks.iter().map(|&i| values[i].clone()).collect();
    let expected = match multi {
        true => Value::Array(answer),
        false => answer[0].clone(),
    };
    assert_eq!(panel(&app), expected.to_string());
}

/// Questions asked together are asked one after another, each saying how far along it is; esc on
/// any of them cancels them all, and once the last is answered the command runs once with every
/// answer.
#[test]
fn questions_asked_together_are_asked_in_turn_then_it_runs_once() {
    let (root, mut app) = opened();
    let mut asked = question("pick", false, &[json!(1), json!(2)]);
    asked["more"] = json!([question("picks", true, &[json!("a"), json!("b")])]);
    app.run_plugin(hook(&app, &root, "titles"), json!({ "ask": asked }));
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(picker(&app).title, "Which one? (1 of 2)");
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert!(app.job.is_none(), "nothing runs until the last is answered");
    for (w, h) in [(40, 16), (120, 40)] {
        let s = screen(&mut app, w, h);
        for needle in ["(2 of 2)", "item \"a\"", "item \"b\""] {
            assert!(s.contains(needle), "{needle:?} isn't in\n{s}");
        }
    }
    press(&mut app, " ");
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(panel(&app), "2 [\"a\"]");
    // Esc on the second asks nothing more, and runs nothing.
    app.modal = None;
    app.run_plugin(hook(&app, &root, "titles"), json!({ "ask": asked }));
    wait(&mut app, |a| a.job.is_none());
    key(&mut app, KeyCode::Enter);
    assert_eq!(picker(&app).title, "Which one? (2 of 2)");
    press(&mut app, "\u{1b}");
    assert!(app.modal.is_none() && app.job.is_none());
}

/// Any questions asked together, each of one or several of any number of items, answered with
/// any of them by the keys: each says how far along it is, and the command runs once, with
/// exactly the values chosen for each.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn any_questions_together_run_once_with_exactly_what_was_chosen(tc: TestCase) {
    let (root, mut app) = opened();
    let k = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let asked: Vec<(bool, Vec<Value>)> = (0..k)
        .map(|q| {
            let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
            (tc.draw(gs::booleans()), (0..n).map(|i| json!(q * 10 + i)).collect())
        })
        .collect();
    let questions: Vec<Value> =
        (asked.iter().enumerate()).map(|(q, (multi, values))| question(&format!("p{q}"), *multi, values)).collect();
    let mut first = questions[0].clone();
    first["more"] = json!(questions[1..]);
    app.run_plugin(hook(&app, &root, "titles"), json!({ "ask": first }));
    wait(&mut app, |a| a.job.is_none());
    let mut answers = Vec::new();
    for (q, (multi, values)) in asked.iter().enumerate() {
        let title = match k {
            1 => "Which one?".to_string(),
            _ => format!("Which one? ({} of {k})", q + 1),
        };
        assert_eq!(picker(&app).title, title);
        let picks = chosen(&tc, values.len(), *multi);
        for &i in &picks {
            to_item(&mut app, i, *multi);
        }
        key(&mut app, KeyCode::Enter);
        let answer: Vec<Value> = picks.iter().map(|&i| values[i].clone()).collect();
        answers.push(match multi {
            true => Value::Array(answer).to_string(),
            false => answer[0].to_string(),
        });
    }
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(panel(&app), answers.join(" "));
}

/// Whatever the providers find for the tracks, scored anyhow: the player asks about exactly the
/// tracks without a candidate alone sure enough, in the library's order, and plans what's
/// chosen and picked (or says none was).
#[hegel::test(test_cases = 15, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_tracks_asked_about_are_those_not_picked(tc: TestCase) {
    let (root, mut app) = opened();
    let tracks = app.loaded.as_ref().unwrap().library.tracks.clone();
    let mut candidates = Vec::new();
    let mut asked = Vec::new();
    for t in &tracks {
        let scores: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(100)).max_size(3));
        let sure = scores.iter().filter(|s| **s >= 90).count();
        if !scores.is_empty() && sure != 1 {
            asked.push(t.display_name());
        }
        candidates
            .extend(scores.iter().enumerate().map(|(i, s)| titled(t.id, *s as f64 / 100.0, &format!("T{i} {s}"))));
    }
    answering(&app, json!({ "candidates": candidates }));
    fetch_details(&mut app, &root);
    let mut seen = Vec::new();
    let mut chose = 0;
    while let Some(Modal::Pick(p)) = &app.modal {
        seen.push(p.title.clone());
        let n = p.items.len();
        let i = tc.draw(gs::integers::<usize>().max_value(n - 1));
        chose += usize::from(i < n - 1);
        to_item(&mut app, i, false);
        key(&mut app, KeyCode::Enter);
    }
    assert_eq!(seen, asked);
    let found = candidates.iter().map(|c| c["track"].clone()).collect::<rustc_hash::FxHashSet<_>>().len();
    let picks = (found - asked.len()) + chose;
    match picks {
        0 if found == 0 => assert_eq!(last_message(&app), "no track details found for 2 tracks"),
        0 => assert_eq!(last_message(&app), "no track details chosen"),
        _ => {
            settle(&mut app);
            // New titles, over what the tracks have: something to confirm.
            let Some(Modal::Confirm(c)) = &app.modal else { panic!("a confirmation: {}", last_message(&app)) };
            assert_eq!(c.plan.operation(), "plugins.provide");
            let said = format!("Plugins: fill in the details of {picks} track(s) from Tags");
            let s = screen(&mut app, 120, 40);
            assert!(s.contains(&said), "{said:?} isn't in\n{s}");
            press(&mut app, "\u{1b}");
        }
    }
}
