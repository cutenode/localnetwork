//! What plugins tell the user in the player: a message, and what they suggest, waiting in the
//! palette to be reviewed, planned and confirmed as any plan.

use locallibrary::plugin::PluginNotice;
use loclib_plugin::Suggested;
use serde_json::json;

use super::super::super::app::Msg;
use super::super::super::app::View as Screen;
use super::super::super::app::testing::{
    app_on, blank, confirming, demo_drive, echo_plugin, last_message, press, say_yes, wait,
};
use super::super::super::ui::screen;
use super::{entry, picker, settings_form};

/// What a plugin says is a message; what it suggests waits in the palette, newest first, and
/// reviewed it's planned and confirmed as any plan, then no longer offered. Only the newest few
/// wait.
#[test]
fn what_a_plugin_suggests_is_reviewed_from_the_palette() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let mut app = app_on(&root);
    let suggests = |message: &str, name: &str| PluginNotice {
        plugin: "echo".into(),
        message: message.into(),
        operation: Some(Suggested { name: "playlist".into(), params: json!({ "name": name }) }),
    };
    let said = PluginNotice { plugin: "echo".into(), message: "just saying".into(), operation: None };
    app.handle_msg(Msg::Notice(vec![suggests("older", "Old"), suggests("make Suggested", "Suggested"), said]));
    assert_eq!(last_message(&app), "Echo: just saying");
    assert!(app.log.iter().any(|(m, ..)| m == "Echo: make Suggested (: to review)"));
    press(&mut app, ":");
    let reviews: Vec<&str> =
        picker(&app).items.iter().filter(|i| i.label.starts_with("review: ")).map(|i| i.label.as_str()).collect();
    assert_eq!(reviews, ["review: make Suggested", "review: older"]);
    assert_eq!(entry(&app, "review: make Suggested").0.detail, "Echo");
    press(&mut app, "review");
    for (w, h) in [(40, 16), (120, 40)] {
        let s = screen(&mut app, w, h);
        assert!(s.contains("review: make Sugg") && s.contains("review: older"));
    }
    press(&mut app, ": make\n");
    assert_eq!(confirming(&mut app), ("echo.playlist".into(), "make the playlist Suggested".into()));
    say_yes(&mut app);
    let library = locallibrary::Device::open(&root).unwrap().library().unwrap();
    assert!(library.playlists.iter().any(|p| p.name == "Suggested"));
    assert_eq!(app.suggested.iter().map(|n| n.message.as_str()).collect::<Vec<_>>(), ["older"]);

    let many: Vec<PluginNotice> =
        (0..locallibrary::plugin::MAX_SUGGESTED + 3).map(|n| suggests(&n.to_string(), "N")).collect();
    app.handle_msg(Msg::Notice(many));
    assert_eq!(app.suggested.len(), locallibrary::plugin::MAX_SUGGESTED);
    assert_eq!(app.suggested.front().map(|n| n.message.as_str()), Some("3"), "the oldest go");
}

/// A plugin's service is a switch in its settings form: turned on and saved, it runs; turned
/// off, it stops.
#[test]
fn a_service_is_turned_on_in_the_settings_form() {
    let (dir, _root) = demo_drive();
    echo_plugin(dir.path());
    let file = dir.path().join("echo.toml");
    let service = "\n[[contributes.services]]\nname = \"beat\"\ntitle = \"The beat\"\n";
    std::fs::write(&file, std::fs::read_to_string(&file).unwrap() + service).unwrap();
    let (mut app, _store) = blank(Screen::Library);
    let last = settings_form(&mut app).fields.last().map(|x| (x.name.clone(), x.text.clone()));
    assert_eq!(last, Some(("services.beat".into(), "false".into())));
    press(&mut app, "\t\t\t\t");
    let s = screen(&mut app, 120, 40);
    assert!(s.contains("services.beat") && s.contains("The beat: keep it running"));
    press(&mut app, "\u{1b}");
    let plugins = crate::plugins::host().unwrap();
    for on in [true, false] {
        settings_form(&mut app);
        press(&mut app, "\t\t\t\t \n");
        wait(&mut app, |a| a.job.is_none());
        assert_eq!(last_message(&app), "Echo settings saved");
        assert_eq!(plugins.service_running("echo", "beat"), on);
    }
}
