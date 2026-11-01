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
    let said = say_yes(&mut app);
    assert!(said.starts_with("My Tags changed on 1 track"), "{said}");
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
    all.set(rule, &Default::default()).unwrap();
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

/// The My Tag and rule commands say what they need, or go on: a tag needs a drive that's there
/// and a track to give it; a rule needs a search that's a filter, and its playlist's place asked,
/// starting from the search; the rules in the store are listed with what each keeps (its sort,
/// how many at most, which drives), or said to be none, or that there's no store; filling them
/// needs a drive.
#[hegel::test(test_cases = 40)]
fn the_tag_and_rule_commands_say_what_they_need(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, _dir) = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => blank(View::Library),
        _ => app_with(library()),
    };
    let drive = app.loaded.is_some();
    let stale = drive && tc.draw(gs::booleans());
    if stale {
        app.loaded.as_mut().unwrap().stale = true;
    }
    let query = tc.draw(gs::sampled_from(&["", "nothing like this", "(", "bpm:>120 key:8A/9A"][..]));
    if drive {
        app.query = query.into();
        app.refresh_visible();
    }
    let store = tempfile::tempdir().unwrap();
    app.store_root = store.path().join("store");
    let rules: Vec<SetSmartPlaylist> = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => Vec::new(),
        n => {
            let s = Store::open_or_init(&app.store_root).unwrap();
            let rules: Vec<SetSmartPlaylist> = (0..n)
                .map(|i| SetSmartPlaylist {
                    path: format!("Smart/Rule {i}"),
                    filter: "bpm:>120".into(),
                    sort: tc.draw(gs::booleans()).then(|| "-bpm".into()),
                    limit: tc.draw(gs::optional(gs::integers::<usize>().min_value(1).max_value(50))),
                    drives: Vec::new(),
                })
                .collect();
            for r in &rules {
                api::smart_set(&s, r).unwrap();
            }
            rules
        }
    };
    // A store with no rules in it, or none at all.
    if rules.is_empty() && tc.draw(gs::booleans()) {
        Store::open_or_init(&app.store_root).unwrap();
    }
    let had_store = app.store_root.join("store.json").is_file();
    for act in [Act::Tag, Act::SmartNew, Act::SmartList, Act::SmartApply] {
        wait(&mut app, |a| a.job.is_none());
        (app.modal, app.toast) = (None, None);
        says_what_it_needs(&mut app, act, (drive, stale, query, had_store), &rules);
    }
}

/// [`the_tag_and_rule_commands_say_what_they_need`] for one command.
fn says_what_it_needs(
    app: &mut App,
    act: Act,
    (drive, stale, query, had_store): (bool, bool, &str, bool),
    rules: &[SetSmartPlaylist],
) {
    app.perform(act, None);
    let said = last_message(app).to_string();
    let job = app.job.as_ref().map(|j| j.label.clone());
    match act {
        Act::Tag => match () {
            _ if !drive => assert_eq!(said, "open a drive first"),
            _ if stale => assert_eq!(said, "the drive was unplugged"),
            _ if app.visible.is_empty() => assert_eq!(said, "no track selected"),
            _ => assert_eq!(job.as_deref(), Some("Reading My Tags")),
        },
        Act::SmartNew => {
            let q = if drive { query } else { "" };
            match Filter::parse(q) {
                _ if q.is_empty() => assert!(said.starts_with("search first (/)"), "{said}"),
                Err(_) => assert!(said.starts_with("the search isn't a filter a rule can keep"), "{said}"),
                Ok(_) => {
                    let Some(Modal::Prompt(p)) = &app.modal else { panic!("no prompt: {said}") };
                    assert_eq!(
                        (p.label.as_str(), p.text.clone()),
                        ("Smart playlist at", format!("Smart/{}", q.replace('/', "-")))
                    );
                }
            }
        }
        Act::SmartList => {
            assert_eq!(job.as_deref(), Some("Reading the rules"));
            wait(app, |a| a.job.is_none());
            if !had_store {
                assert!(last_message(app).contains("a store: pull a drive to make one"), "{}", last_message(app));
                return;
            }
            let Some(Modal::Report(r)) = &app.modal else { panic!("no report: {}", last_message(app)) };
            let lines: Vec<&str> = r.lines.iter().map(|(_, l)| l.as_str()).collect();
            if rules.is_empty() {
                assert_eq!(lines[0], "No rules yet.");
            }
            for rule in rules {
                let at = lines.iter().position(|l| *l == rule.path).expect("listed");
                let about = lines[at + 1];
                assert!(about.starts_with(&rule.filter) && about.ends_with("every drive"), "{about}");
                assert_eq!(about.contains("sorted by -bpm"), rule.sort.is_some(), "{about}");
                assert_eq!(
                    rule.limit.is_some_and(|n| about.contains(&format!("at most {n}"))),
                    rule.limit.is_some(),
                    "{about}"
                );
            }
        }
        _ => match drive {
            false => assert_eq!(said, "open a drive first"),
            true if stale => assert_eq!(said, "the drive was unplugged"),
            true => assert_eq!(job.as_deref(), Some("Planning")),
        },
    }
}

/// Whatever the chosen tracks hold, a tag is offered to take off just when every one of them
/// has it (and to give otherwise), and what's picked is what's done: given to all of them, or
/// taken off all of them, the drive's other tracks kept as they were.
#[hegel::test(test_cases = 15, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_tag_is_offered_off_just_when_every_chosen_track_has_it(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use std::collections::BTreeSet;
    const TAGS: [&str; 2] = ["Genre/Techno", "Situation/Peak"];
    let (_dir, root) = demo_drive();
    let mut device = Device::open_export(&root).unwrap();
    let lib = device.library().unwrap();
    let n = lib.tracks.len();
    // Which tracks hold each tag, by index.
    let mut holding: Vec<BTreeSet<usize>> = Vec::new();
    for tag in TAGS {
        let on: BTreeSet<usize> = tc.draw(gs::subsequences((0..n).collect::<Vec<_>>())).into_iter().collect();
        if !on.is_empty() {
            let edit = locallibrary::tags::Edit::Add {
                tag: tag.into(),
                tracks: on.iter().map(|&i| lib.tracks[i].id).collect(),
            };
            let plan = locallibrary::tags::plan(&device, &edit).unwrap();
            locallibrary::tags::apply(&plan, &mut device, &Ctx::silent()).unwrap();
        }
        holding.push(on);
    }
    let mut app = app_on(&root);
    // Marked tracks, or (none marked) the selected one.
    let marked: BTreeSet<usize> = tc.draw(gs::subsequences((0..n).collect::<Vec<_>>())).into_iter().collect();
    let chosen: BTreeSet<usize> = if marked.is_empty() {
        let i = tc.draw(gs::integers::<usize>().max_value(n - 1));
        app.track_state.select(app.visible.iter().position(|&v| v == i));
        [i].into()
    } else {
        app.marks = marked.iter().map(|&i| lib.tracks[i].file_path.clone()).collect();
        marked
    };
    app.perform(Act::Tag, None);
    wait(&mut app, |a| a.job.is_none());
    let Some(Modal::Pick(p)) = &mut app.modal else { panic!("no tags: {}", last_message(&app)) };
    let s = |k: usize| if k == 1 { "" } else { "s" };
    assert_eq!(p.title, format!("My Tag for {} track{}", chosen.len(), s(chosen.len())));
    assert_eq!(p.items[0].label, "a new tag…");
    for (tag, on) in TAGS.iter().zip(&holding) {
        let every = chosen.is_subset(on);
        // (A tag on no track isn't on the drive to list.)
        let Some(item) = p.items.iter().find(|i| i.label == *tag || i.label == format!("take {tag} off")) else {
            assert!(on.is_empty(), "{tag} listed");
            continue;
        };
        assert_eq!(item.label.starts_with("take"), every, "{tag} on {on:?}, chosen {chosen:?}");
        assert_eq!(item.detail, format!("{} track{}", on.len(), s(on.len())));
    }
    let pick = tc.draw(gs::integers::<usize>().max_value(TAGS.len()));
    let pick = if pick > 0 && holding[pick - 1].is_empty() { 0 } else { pick };
    let label = if pick == 0 { "a new tag…".to_string() } else { TAGS[pick - 1].to_string() };
    let at = p.items.iter().position(|i| i.label == label || i.label == format!("take {label} off")).unwrap();
    let k = p.shown.iter().position(|&i| i == at).unwrap();
    p.state.select(Some(k));
    press(&mut app, "\n");
    if pick == 0 {
        let Some(Modal::Prompt(p)) = &app.modal else { panic!("no prompt") };
        assert!(
            p.label.starts_with(&format!("New My Tag for {} track{}", chosen.len(), s(chosen.len()))),
            "{}",
            p.label
        );
        assert_eq!(p.text, "Genre/");
        return;
    }
    assert_eq!(confirming(&mut app).0, "tags");
    say_yes(&mut app);
    let was = &holding[pick - 1];
    let now: BTreeSet<usize> = if chosen.is_subset(was) { was - &chosen } else { was | &chosen };
    let tags = locallibrary::api::tags_list(&DriveRef { drive: root.display().to_string() }).unwrap();
    let tag = tags.categories.iter().flat_map(|c| &c.tags).find(|t| t.path == label).unwrap();
    let held: BTreeSet<usize> =
        tag.tracks.iter().map(|r| lib.tracks.iter().position(|t| t.file_path == r.file_path).unwrap()).collect();
    assert_eq!(held, now, "{label}");
}
