//! Checks in the player: a fix a check gave, in the palette and planned from it, confirmed as any
//! plugin's operation is; and which checks run, a form kept where `plugin checks` keeps it.

use locallibrary::Store;
use locallibrary::api::{self, CheckRequest};
use locallibrary::check::{CheckItem, Finding, Level};
use locallibrary::plugin::{Fix, PluginFix, TIMEOUT, check_timeout};
use loclib_plugin::{DriveAction, FetchAction, HostAction};
use serde_json::json;

use super::super::super::app::testing::{app_on, blank, last_message, plugged, plugins_once, press, say_yes, wait};
use super::super::super::app::{App, View as Screen};
use super::super::super::edits::Modal;
use super::super::checks::fixes;
use super::{form_open, palette};
use crate::tui::keymap::Act;

/// A fix a check gave is in the palette until the next check, by its plugin's name; chosen, its
/// plugin's operation is planned on the drive it was found on and confirmed, as any is.
#[test]
fn a_checks_fix_is_planned_from_the_palette_and_confirmed() {
    plugins_once();
    let drive = plugged();
    let mut app = app_on(&drive.0);
    let req = CheckRequest { drives: vec![drive.0.display().to_string()], options: Default::default() };
    let mut report = api::check(None, &req, &locallibrary::Ctx::silent()).unwrap();
    let fix = PluginFix { plugin: "echo".into(), operation: "playlist".into(), params: json!({ "name": "Untitled" }) };
    let fix = Fix::Plugin(fix);
    let finding = Finding::new(Level::Warning, "echo.untitled", "1 track(s) have no title");
    report.items.push(CheckItem { drive: report.drives[0].clone(), finding, fix: Some(fix) });
    app.fixes = fixes(std::slice::from_ref(&drive.0), &report);
    press(&mut app, ":");
    let (items, acts) = palette(&app);
    let i = acts.iter().position(|a| *a == Act::Fix(0)).expect("the fix is in the palette");
    let label = format!("fix on {}: 1 track(s) have no title", report.drives[0]);
    assert_eq!((items[i].label.as_str(), items[i].detail.as_str()), (label.as_str(), "Echo"));
    press(&mut app, "\u{1b}");
    app.perform(Act::Fix(0), None);
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Confirm(c)) = &app.modal else { panic!("no plan: {}", last_message(&app)) };
    assert_eq!((c.plan.operation(), c.plan.summary()[0].as_str()), ("echo.playlist", "make the playlist Untitled"));
    say_yes(&mut app);
    let lib = locallibrary::Device::open_export(&drive.0).unwrap().library().unwrap();
    assert!(lib.find_playlist("Untitled").is_some(), "{}", last_message(&app));
}

/// The fixes locallibrary's own checks give are in the palette too, with no plugin's name: each
/// opens the player's own flow on the drive it was found on (saving a snapshot asks first, a
/// sync of a drive alone says why it can't, and fetching covers opens the form for the tracks).
#[test]
fn the_checks_own_fixes_open_the_players_flows() {
    plugins_once();
    let drive = plugged();
    let mut app = app_on(&drive.0);
    let req = CheckRequest { drives: vec![drive.0.display().to_string()], options: Default::default() };
    let mut report = api::check(None, &req, &locallibrary::Ctx::silent()).unwrap();
    let name = report.drives[0].clone();
    let on = |fix: HostAction, message: &str| CheckItem {
        drive: name.clone(),
        finding: Finding::new(Level::Warning, "files", message),
        fix: Some(Fix::Host(fix)),
    };
    let fetch = FetchAction { drive: name.clone(), tracks: vec![1], provider: None };
    report.items.extend([
        on(HostAction::Pull(DriveAction { drive: name.clone() }), "history"),
        on(HostAction::Sync(DriveAction { drive: name.clone() }), "behind"),
        on(HostAction::ArtFetch(fetch), "no artwork"),
    ]);
    app.fixes = fixes(std::slice::from_ref(&drive.0), &report);
    press(&mut app, ":");
    let (items, acts) = palette(&app);
    let i = acts.iter().position(|a| *a == Act::Fix(0)).expect("the fix is in the palette");
    assert_eq!((items[i].label.clone(), items[i].detail.as_str()), (format!("fix on {name}: history"), ""));
    press(&mut app, "\u{1b}");
    app.perform(Act::Fix(0), None);
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Confirm(c)) = &app.modal else { panic!("no plan: {}", last_message(&app)) };
    assert_eq!(c.plan.operation(), "pull");
    app.modal = None;
    app.perform(Act::Fix(1), None);
    let why = ["try again in a moment", "save a snapshot of it first", "to sync them"];
    assert!(app.modal.is_none() && why.iter().any(|w| last_message(&app).contains(w)), "{}", last_message(&app));
    app.perform(Act::Fix(2), None);
    let (title, fields) = form_open(&app);
    assert_eq!(title, format!("Fetch cover art: 1 track on {}", app.drive_label(&drive.0)));
    assert_eq!(fields, ["from", "replace"]);
}

/// "check settings…" is a form of every check (and whether each guard stops changes) and how long
/// one has, as they're set now; saved, what changed is kept where `plugin checks` keeps it, and a
/// timeout left empty goes back to its default.
#[test]
fn which_checks_run_is_a_form_kept_where_plugin_checks_keeps_it() {
    plugins_once();
    let (mut app, _store) = blank(Screen::Library);
    app.perform(Act::CheckSettings, None);
    let fields =
        ["demos.tracks", "echo.untitled", "echo.edits", "echo.edits.blocking", "timeout"].map(String::from).to_vec();
    assert_eq!(form_open(&app), ("Check settings".into(), fields));
    let plugins = crate::plugins::host().unwrap();
    let saved = |app: &mut App, given: serde_json::Value| {
        app.save_check_settings(given);
        wait(app, |a| a.job.is_none());
        last_message(app).to_string()
    };
    let given = json!({ "echo.untitled": false, "echo.edits": true, "echo.edits.blocking": true, "timeout": 4 });
    assert_eq!(saved(&mut app, given.clone()), "check settings saved");
    let store = Store::open(&app.store_root).unwrap();
    let kept: Vec<(bool, bool)> = plugins
        .checks(Some(&store))
        .into_iter()
        .filter(|(p, ..)| *p == "echo")
        .map(|(_, _, s)| (s.on, s.blocking))
        .collect();
    assert_eq!(kept, [(false, false), (true, true)]);
    assert_eq!(check_timeout(Some(&store)).as_secs(), 4);
    assert_eq!(saved(&mut app, given), "nothing changed");
    let given = json!({ "echo.untitled": false, "echo.edits": true, "echo.edits.blocking": true, "timeout": null });
    assert_eq!(saved(&mut app, given), "check settings saved");
    assert_eq!(check_timeout(Some(&store)), TIMEOUT);
}
