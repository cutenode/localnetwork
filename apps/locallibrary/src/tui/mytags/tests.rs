use locallibrary::smart::SmartPlaylist;
use locallibrary::{Ctx, Device};

use super::super::app::View;
use super::super::app::testing::{
    app_on, app_with, blank, confirming, demo_drive, last_message, library, press, say_yes, wait,
};
use super::super::keymap::Act;
use super::*;

/// A tag needs tracks on a drive that's there.
#[test]
fn tagging_needs_a_drive_and_a_track() {
    let (mut app, _dir) = app_with(library());
    let (mut no_drive, _store) = blank(View::Library);
    press(&mut no_drive, "T");
    assert_eq!(last_message(&no_drive), "open a drive first");
    press(&mut app, "/nothing like this\n");
    press(&mut app, "T");
    assert_eq!(last_message(&app), "no track selected");
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    app.handle_msg(super::super::app::Msg::Volumes(Vec::new(), Some((root, false))));
    press(&mut app, "T");
    assert_eq!(last_message(&app), "the drive was unplugged");
    assert!(app.job.is_none());
}

/// A tag on the track already is offered to take off, and taken off once confirmed; on
/// another track it's offered to give.
#[test]
fn takes_a_tag_off_and_offers_it_for_another_track() {
    let (_dir, root) = demo_drive();
    let mut device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let edit = locallibrary::tags::Edit::Add { tag: "Genre/Techno".into(), tracks: vec![lib.tracks[0].id] };
    let plan = locallibrary::tags::plan(&device, &edit).unwrap();
    locallibrary::tags::apply(&plan, &mut device, &Ctx::silent()).unwrap();
    let mut app = app_on(&root);
    // (Through the actions menu: T there does what T does.)
    let choices = |app: &mut App| {
        press(app, "eT");
        wait(app, |a| a.job.is_none());
        match &app.modal {
            Some(Modal::Pick(p)) => p.items.iter().map(|i| i.label.clone()).collect::<Vec<_>>(),
            _ => panic!("no tags: {}", last_message(app)),
        }
    };
    app.track_state.select(Some(1));
    let offered = choices(&mut app);
    assert!(offered.iter().any(|c| c == "Genre/Techno") && !offered.iter().any(|c| c.starts_with("take")));
    press(&mut app, "genre/techno\n");
    assert_eq!(confirming(&mut app).0, "tags", "giving it is planned");
    press(&mut app, "\u{1b}");
    app.track_state.select(Some(0));
    assert!(choices(&mut app).iter().any(|c| c == "take Genre/Techno off"));
    press(&mut app, "take techno\n");
    assert_eq!(confirming(&mut app).0, "tags");
    say_yes(&mut app);
    assert!(last_message(&app).starts_with("My Tags changed on 1 track"), "{}", last_message(&app));
    let tags = locallibrary::api::tags_list(&DriveRef { drive: root.display().to_string() }).unwrap();
    let techno = tags.categories.iter().flat_map(|c| &c.tags).find(|t| t.path == "Genre/Techno").unwrap();
    assert!(techno.tracks.is_empty(), "{techno:?}");
}

/// The rules panel: how to make one when there are none; each rule's filter, sort, limit and
/// drives when there are.
#[test]
fn lists_the_rules_in_the_store() {
    let (mut app, dir) = app_with(library());
    let rules = |app: &mut App| {
        app.perform(Act::SmartList, None);
        wait(app, |a| a.job.is_none());
        match &app.modal {
            Some(Modal::Report(r)) => r.lines.iter().map(|(_, l)| l.clone()).collect::<Vec<_>>(),
            _ => panic!("no rules: {}", last_message(app)),
        }
    };
    app.perform(Act::SmartList, None);
    wait(&mut app, |a| a.job.is_none());
    assert!(last_message(&app).contains("a store: pull a drive to make one"), "{}", last_message(&app));

    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    assert_eq!(rules(&mut app)[0], "No rules yet.");
    press(&mut app, "x");
    let mut all = store.smart_playlists().unwrap();
    let rule = SmartPlaylist {
        path: "Smart/Peak".into(),
        filter: "bpm:>=128".into(),
        sort: Some("-bpm".into()),
        limit: Some(20),
        drives: vec![uuid::Uuid::new_v4()],
    };
    all.set(rule).unwrap();
    store.save_smart_playlists(&all).unwrap();
    let lines = rules(&mut app);
    assert_eq!(lines[..2], ["Smart/Peak", "bpm:>=128  ·  sorted by -bpm  ·  at most 20  ·  some libraries only"]);
}

/// Filling the rules needs a drive; a rule needs a search that reads as a filter.
#[test]
fn smart_playlists_need_a_drive_and_a_filter() {
    let (mut app, _dir) = app_with(library());
    let (mut no_drive, _store) = blank(View::Library);
    no_drive.perform(Act::SmartApply, None);
    assert_eq!(last_message(&no_drive), "open a drive first");
    press(&mut app, "/bpm:>\n");
    app.perform(Act::SmartNew, None);
    assert!(app.modal.is_none());
    assert!(last_message(&app).starts_with("the search isn't a filter a rule can keep"), "{}", last_message(&app));
}
