//! What plugins tell the user in the player: a message, and what they suggest, waiting in the
//! palette (and the store) to be reviewed, planned and confirmed as any plan, or dismissed.

use std::path::Path;

use locallibrary::plugin::{MAX_SUGGESTED, PluginNotice, suggested};
use loclib_plugin::{AnalyzeAction, ExportAction, FetchAction, HostAction, ProviderKind, Suggested, Suggestion};
use serde_json::json;

use super::super::super::app::testing::{
    app_on, blank, confirming, demo_drive, echo_plugin, last_message, mounted, press, say_yes, wait,
};
use super::super::super::app::{App, View as Screen};
use super::super::super::edits::Modal;
use super::super::super::form::For;
use super::super::super::ui::screen;
use super::{entry, picker, settings_form};

/// Echo's suggestion of its `playlist` operation, making the playlist `name`.
fn suggests(message: &str, name: &str) -> PluginNotice {
    PluginNotice {
        plugin: "echo".into(),
        message: message.into(),
        suggests: Some(Suggestion::Operation(Suggested { name: "playlist".into(), params: json!({ "name": name }) })),
    }
}

/// `notices`, told the player as a plugin tells them, then shown.
fn tell(app: &mut App, notices: Vec<PluginNotice>) {
    let (told, last) = (app.plugin_notices(), notices.last().map(|n| n.message.clone()));
    notices.into_iter().for_each(|n| told(n));
    wait(app, |a| last.as_ref().is_none_or(|l| a.log.iter().any(|(m, ..)| m.starts_with(&format!("Echo: {l}")))));
}

/// The messages of what's suggested now, in the player, oldest first.
fn waiting(app: &App) -> Vec<&str> {
    app.suggested.iter().map(|n| n.message.as_str()).collect()
}

/// The messages of the suggestions kept in the store at `root`.
fn kept(root: &Path) -> Vec<String> {
    let store = locallibrary::Store::open(root).unwrap();
    suggested(&store).unwrap().into_iter().map(|n| n.message).collect()
}

/// What a plugin says is a message; what it suggests waits in the palette, newest first, and
/// reviewed it's planned and confirmed as any plan, then no longer offered. Only the newest few
/// wait.
#[test]
fn what_a_plugin_suggests_is_reviewed_from_the_palette() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let mut app = app_on(&root);
    let said = PluginNotice { plugin: "echo".into(), message: "just saying".into(), suggests: None };
    tell(&mut app, vec![suggests("older", "Old"), suggests("make Suggested", "Suggested"), said]);
    assert_eq!(last_message(&app), "Echo: just saying");
    assert!(app.log.iter().any(|(m, ..)| m == "Echo: make Suggested (: to review)"));
    wait(&mut app, |a| waiting(a) == ["older", "make Suggested"]);
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
    assert_eq!(waiting(&app), ["older"]);

    let many: Vec<PluginNotice> = (0..MAX_SUGGESTED + 3).map(|n| suggests(&n.to_string(), "N")).collect();
    tell(&mut app, many);
    wait(&mut app, |a| a.suggested.len() == MAX_SUGGESTED);
    assert_eq!(waiting(&app)[0], "3", "the oldest go");
}

/// What a plugin suggests is kept in the store until it's reviewed or dismissed: the player
/// opened again offers what it didn't, and one dismissed is gone for good.
#[test]
fn a_suggestion_waits_until_its_reviewed_or_dismissed() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let mut app = app_on(&root);
    tell(&mut app, vec![suggests("make First", "First"), suggests("make Second", "Second")]);
    drop(app);
    let mut app = app_on(&root);
    app.start_plugins();
    wait(&mut app, |a| waiting(a) == ["make First", "make Second"]);
    press(&mut app, ":dismiss: make First\n");
    assert_eq!(last_message(&app), "suggestion dismissed");
    assert_eq!(waiting(&app), ["make Second"]);
    let store = app.store_root.clone();
    wait(&mut app, |_| kept(&store) == ["make Second"]);
    let mut app = app_on(&root);
    app.start_plugins();
    wait(&mut app, |a| waiting(a) == ["make Second"]);
    press(&mut app, ":review: make Second\n");
    assert_eq!(confirming(&mut app), ("echo.playlist".into(), "make the playlist Second".into()));
    wait(&mut app, |_| kept(&store).is_empty());
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

/// A suggestion picked in the palette is the one planned, even when older ones went to make room
/// for newer ones while it was open; one that went itself isn't planned.
#[test]
fn the_suggestion_picked_is_planned_as_others_come() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    let mut app = app_on(&root);
    tell(&mut app, vec![suggests("oldest", "Oldest"), suggests("make Picked", "Picked")]);
    wait(&mut app, |a| a.suggested.len() == 2);
    press(&mut app, ":");
    tell(&mut app, (1..MAX_SUGGESTED).map(|n| suggests(&format!("newer {n}"), "Newer")).collect());
    wait(&mut app, |a| a.suggested.first().is_some_and(|n| n.message == "make Picked"));
    press(&mut app, "review: make Picked\n");
    assert_eq!(confirming(&mut app), ("echo.playlist".into(), "make the playlist Picked".into()));
    press(&mut app, "\u{1b}");
    wait(&mut app, |a| a.modal.is_none());
    press(&mut app, ":");
    tell(&mut app, (0..MAX_SUGGESTED).map(|n| suggests(&format!("late {n}"), "Late")).collect());
    wait(&mut app, |a| a.suggested.first().is_some_and(|n| n.message == "late 0"));
    press(&mut app, "review: newer\n");
    wait(&mut app, |a| a.job.is_none());
    assert!(app.modal.is_none(), "what went isn't planned");
    assert_eq!(app.suggested.len(), MAX_SUGGESTED);
}

/// Something the player's drive poll notices first, before anything else used the plugins, is
/// told to those listening: delivering loads them.
#[test]
fn what_the_player_notices_first_is_told() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    mounted(dir.path());
    let file = dir.path().join("echo.toml");
    let manifest = std::fs::read_to_string(&file).unwrap().replacen(
        r#"capabilities = ["library:read", "library:plan""#,
        r#"capabilities = ["library:read", "library:plan", "events:drives""#,
        1,
    );
    std::fs::write(&file, manifest + "\n[[contributes.events]]\non = [\"drive.mounted\"]\n").unwrap();
    let logs = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let logged = logs.clone();
    crate::plugins::log_to(Some(std::sync::Arc::new(move |_, line| logged.lock().unwrap().push(line.into()))));
    let drive = locallibrary::watch::MountedDrive { root, id: None, name: "DEMO".into(), has_export: true };
    let attached = locallibrary::api::Notice::Watch(locallibrary::watch::WatchEvent::Attached(drive));
    crate::plugins::events::deliver(&attached);
    let heard = || logs.lock().unwrap().iter().any(|l| l.starts_with("event drive.mounted DEMO "));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !heard() && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    crate::plugins::log_to(None);
    assert!(heard(), "the plugin heard it");
}

/// Echo's suggestion of one of the host's actions.
fn suggests_action(message: &str, action: HostAction) -> PluginNotice {
    PluginNotice { plugin: "echo".into(), message: message.into(), suggests: Some(Suggestion::Action(action)) }
}

/// What a plugin suggests of the host's actions is reviewed from the palette as the user would
/// start it, on the drive it names: fetching from its providers opens the fetch form, only from
/// them to start with; its export, the export form with its options filled in and where to as the
/// player picks it (a set's export, its set; a mix's isn't the player's); and its analyzer asks
/// first. Each then goes on as it does when the user starts it. A drive that isn't plugged in is
/// said.
#[test]
fn a_suggested_action_opens_the_players_own_flow() {
    let (dir, root) = demo_drive();
    echo_plugin(dir.path());
    mounted(dir.path());
    let file = dir.path().join("echo.toml");
    let manifest = std::fs::read_to_string(&file)
        .unwrap()
        .replacen(r#"capabilities = ["library:read""#, r#"capabilities = ["sets:read", "library:read""#, 1)
        .replacen(
            "extension = \"m3u\"",
            "extension = \"m3u\"\noptions = { type = \"object\", properties = { loud = { type = \"boolean\" } } }",
            1,
        );
    let lists = "\n[[contributes.exporters]]\nformat = \"echo-set\"\ntitle = \"Set list\"\nextension = \"txt\"\nsource = \"set\"\n\n[[contributes.exporters]]\nformat = \"echo-mix\"\ntitle = \"Mix list\"\nextension = \"txt\"\nsource = \"mix\"\n";
    std::fs::write(&file, manifest + lists).unwrap();
    let mut app = app_on(&root);
    let drive = app.plugin_drive(&root).unwrap();
    let fetch = FetchAction { drive: drive.clone(), tracks: vec![1], provider: None };
    let loud = serde_json::Map::from_iter([("loud".to_string(), json!(true)), ("write_to".to_string(), json!("/tmp"))]);
    let export = ExportAction {
        format: "echo-m3u".into(),
        drive: Some(drive.clone()),
        options: loud,
        ..ExportAction::default()
    };
    let set = ExportAction { format: "echo-set".into(), set: Some("last".into()), ..ExportAction::default() };
    let mix = ExportAction { format: "echo-mix".into(), mix: Some("1".into()), ..ExportAction::default() };
    let analyze = AnalyzeAction { analyzer: "size".into(), drive: drive.clone(), tracks: Vec::new() };
    let elsewhere = AnalyzeAction { drive: "ELSEWHERE".into(), ..analyze.clone() };
    tell(
        &mut app,
        vec![
            suggests_action("fetch the tags", HostAction::TrackFetch(fetch)),
            suggests_action("export it", HostAction::Export(export)),
            suggests_action("export the set", HostAction::Export(set)),
            suggests_action("export the mix", HostAction::Export(mix)),
            suggests_action("analyze it", HostAction::Analyze(analyze)),
            suggests_action("analyze elsewhere", HostAction::Analyze(elsewhere)),
        ],
    );
    wait(&mut app, |a| a.suggested.len() == 6);

    press(&mut app, ":review: fetch the tags\n");
    let Some(Modal::Form(f)) = &app.modal else { panic!("no form: {}", last_message(&app)) };
    let fields: Vec<(&str, &str)> = f.fields.iter().map(|x| (x.name.as_str(), x.text.as_str())).collect();
    assert_eq!((f.title.as_str(), &fields[..]), ("Fetch track details", &[("from", "echo"), ("replace", "false")][..]));
    assert!(matches!(&f.then, For::Fetch { kind: ProviderKind::Metadata, tracks, .. } if tracks == &[1]));
    press(&mut app, "\n");
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Fetching track details"));
    wait(&mut app, |a| a.job.is_none());

    press(&mut app, ":review: export it\n");
    let name = app.drive_label(&root);
    let Some(Modal::Form(f)) = &mut app.modal else { panic!("no form") };
    let fields: Vec<(&str, &str)> = f.fields.iter().map(|x| (x.name.as_str(), x.text.as_str())).collect();
    let to = format!("~/Music/{name}.m3u");
    assert_eq!((f.title.as_str(), &fields[..]), ("M3U playlist", &[("write_to", to.as_str()), ("loud", "true")][..]));
    let out = dir.path().join("list.m3u");
    f.fields[0].text = out.display().to_string();
    press(&mut app, "\n");
    assert_eq!(confirming(&mut app).0, "echo.export.echo-m3u");
    assert_eq!(say_yes(&mut app), "Echo: 1 file written");
    assert!(out.exists());

    press(&mut app, ":review: export the set\n");
    let Some(Modal::Form(f)) = &app.modal else { panic!("no form: {}", last_message(&app)) };
    assert!(matches!(&f.then, For::Export { format: "echo-set", set: Some(set), .. } if set == "last"));
    press(&mut app, "\u{1b}");
    press(&mut app, ":review: export the mix\n");
    assert_eq!((app.modal.is_none(), last_message(&app)), (true, "the player doesn't export mixes"));

    press(&mut app, ":review: analyze elsewhere\n");
    assert_eq!((app.modal.is_none(), last_message(&app)), (true, "ELSEWHERE isn't plugged in"));
    press(&mut app, ":review: analyze it\n");
    let Some(Modal::Ask(question, _)) = &app.modal else { panic!("not asked: {}", last_message(&app)) };
    assert_eq!(question, &format!("Analyze every track on {name} with file size?"));
    assert!(screen(&mut app, 120, 40).contains("Analyze every track on"));
    press(&mut app, "y");
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Analyzing"));
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "file size: 2 tracks analyzed, 0 already");
    assert!(app.suggested.is_empty(), "each reviewed is no longer offered");
}

/// For any of echo's actions a plugin suggests, wherever the user is, reviewing it starts
/// nothing by itself: it opens the player's own form or question for it, its params filled in,
/// or says why it can't (no drive open, one not plugged in), and the drive is as it was.
#[hegel::test(test_cases = 15, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn reviewing_a_suggested_action_starts_nothing_by_itself(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (_, mut app, plugged, _store) = super::somewhere(&tc);
    let named = plugged.as_ref().and_then(|d| app.plugin_drive(&d.0));
    let drive = match named.filter(|_| tc.draw(gs::weighted_booleans(0.8))) {
        Some(named) => named,
        None => "ELSEWHERE".to_string(),
    };
    let tracks: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(3)).unique(true).max_size(2));
    let provider = tc.draw(gs::optional(gs::sampled_from(vec!["tags".to_string()])));
    let action = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => HostAction::TrackFetch(FetchAction { drive: drive.clone(), tracks: tracks.clone(), provider }),
        1 => HostAction::Export(ExportAction {
            format: "echo-m3u".into(),
            drive: Some(drive.clone()),
            ..ExportAction::default()
        }),
        _ => {
            HostAction::Analyze(AnalyzeAction { analyzer: "size".into(), drive: drive.clone(), tracks: tracks.clone() })
        }
    };
    let before = plugged.as_ref().map(|d| std::fs::read(d.0.join("PIONEER/rekordbox/export.pdb")).unwrap());
    tell(&mut app, vec![suggests_action("have a look", action.clone())]);
    wait(&mut app, |a| a.suggested.len() == 1);
    press(&mut app, ":review: have a look\n");
    assert!(app.job.is_none(), "nothing runs by itself");
    match (&app.modal, &action) {
        (Some(Modal::Form(f)), HostAction::TrackFetch(a)) => {
            assert!(matches!(&f.then, For::Fetch { tracks, .. } if *tracks == a.tracks));
            let from = a.provider.as_ref().map_or("echo".to_string(), |p| format!("echo.{p}"));
            assert_eq!(f.fields[0].text, from);
        }
        (Some(Modal::Form(f)), HostAction::Export(_)) => {
            assert!(matches!(f.then, For::Export { format: "echo-m3u", .. }))
        }
        (Some(Modal::Ask(q, _)), HostAction::Analyze(a)) => {
            let what = if a.tracks.is_empty() {
                "every track".to_string()
            } else {
                crate::output::plural(a.tracks.len(), "track")
            };
            assert!(q.starts_with(&format!("Analyze {what} on ")), "{q}");
        }
        (None, _) => {
            let said = last_message(&app);
            let why = ["ELSEWHERE isn't plugged in", "open a drive first", "only on a plugged-in drive"];
            assert!(why.contains(&said), "{said}");
        }
        (_, action) => panic!("{action:?} opened something else"),
    }
    let after = plugged.as_ref().map(|d| std::fs::read(d.0.join("PIONEER/rekordbox/export.pdb")).unwrap());
    assert_eq!(before, after, "the drive is as it was");
}
