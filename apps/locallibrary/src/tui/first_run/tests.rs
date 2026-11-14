use locallibrary::{DeviceInfo, api};
use ratatui::crossterm::event::KeyCode;

use super::super::app::testing::*;
use super::super::app::{DrivePane, Msg, Vis};
use super::super::edits::PromptFor;
use super::super::state::Saved;
use super::super::ui::screen;
use super::*;

/// The drive at `root`, as the drive list reports it.
fn listed(root: &std::path::Path) -> DeviceInfo {
    api::drive_info(&api::DriveRef { drive: root.display().to_string() }).unwrap()
}

/// Until the first read of the drives answers, nothing says there's no drive, in any pane or view:
/// that read is what a first frame is waiting for. It then says so, and the loop stays awake for it.
#[test]
fn nothing_says_no_drive_until_the_first_read_answers() {
    let missing = ["No rekordbox drive open.", "no drive open", "no rekordbox drives", "No rekordbox drives mounted."];
    for view in [View::Library, View::Drives] {
        let (mut app, _dir) = blank(view);
        reading_drives(&mut app);
        assert!(app.animating(), "the slow timer would hold its answer back");
        let s = screen(&mut app, 120, 40);
        assert!(missing.iter().all(|m| !s.contains(m)), "{view:?}: {s}");

        app.handle_msg(Msg::Drives(Vec::new()));
        assert!(!app.scanning() && !app.animating());
        let s = screen(&mut app, 120, 40);
        assert!(missing.iter().any(|m| s.contains(m)), "{view:?}: {s}");
    }
}

/// A read that failed answers too.
#[test]
fn a_failed_first_read_still_ends_the_wait() {
    let (mut app, _dir) = blank(View::Library);
    reading_drives(&mut app);
    app.handle_msg(Msg::Failed(super::super::app::Work::Drives, "no".into()));
    assert!(screen(&mut app, 120, 40).contains("No rekordbox drive open."));
}

/// With no drive, the library waits for one and offers the steps there are, the first selected,
/// and enter takes the one selected: never more than three.
#[test]
fn no_drive_offers_at_most_three_next_steps() {
    let (mut app, _dir) = blank(View::Library);
    let s = screen(&mut app, 120, 40);
    assert!(s.contains("It opens as soon as it's here.") && s.contains("open a folder"), "{s}");
    assert!(!s.contains("browse snapshots"), "the store has none: {s}");
    assert_eq!(s.contains("sign in"), cfg!(feature = "account"), "{s}");

    app.library_names.insert(uuid::Uuid::new_v4(), "Main".into());
    let s = screen(&mut app, 120, 40);
    assert!(s.contains("browse snapshots"), "{s}");
    assert_eq!(app.next_steps().len(), if cfg!(feature = "account") { 3 } else { 2 });

    press(&mut app, "\n");
    assert!(matches!(&app.modal, Some(Modal::Prompt(p)) if matches!(p.purpose, PromptFor::OpenFolder)));
    press(&mut app, "\u{1b}");
    press(&mut app, "j\n");
    assert_eq!((app.view, app.drive_pane), (View::Drives, DrivePane::Snapshots));
}

/// The last step starts signing in, in the localnetwork view.
#[cfg(feature = "account")]
#[test]
fn the_last_step_signs_in() {
    let (mut app, _dir) = blank(View::Library);
    press(&mut app, "G\n");
    assert_eq!((app.view, app.profile.account.typing()), (View::Profile, true));
}

#[cfg(feature = "account")]
#[test]
fn signed_in_leaves_a_step_out() {
    let (mut app, _dir) = blank(View::Library);
    let account = serde_json::from_value(serde_json::json!({
        "did": "did:plc:abc",
        "handle": "dj.example.com",
        "pds": "https://pds.example",
        "auth_server": "https://pds.example",
        "scopes": ["atproto"],
        "signed_in_at": "2026-01-05T12:00:00Z",
        "sign_in_again_by": "2099-01-01T00:00:00Z",
        "access_expires_at": null,
        "client": { "kind": "hosted", "client_id": "https://example.com/client.json" },
        "storage": "file"
    }));
    app.profile.account.info = Some(account.unwrap());
    assert!(!app.next_steps().iter().any(|(a, _)| *a == Act::SignIn));
    assert!(!screen(&mut app, 120, 40).contains("sign in"));
}

/// A drive that's plugged in with no export is said, not left to look like nothing happened.
#[test]
fn a_drive_with_no_export_is_said() {
    let (mut app, dir) = blank(View::Library);
    let root = dir.path().join("EMPTY");
    std::fs::create_dir_all(&root).unwrap();
    app.handle_msg(Msg::Drives(vec![listed(&root)]));
    let s = screen(&mut app, 120, 40);
    assert!(s.contains("1 plugged in, with no rekordbox export"), "{s}");
}

/// The footer says what keys do here, and nothing about tracks there aren't.
#[test]
fn the_footer_speaks_of_steps() {
    let (mut app, _dir) = blank(View::Library);
    let footer = screen(&mut app, 120, 40).lines().last().unwrap().to_string();
    assert!(footer.contains("enter run") && footer.contains("? help"), "{footer}");
    assert!(!footer.contains("play") && !footer.contains("to crate"), "{footer}");
    assert!(!screen(&mut app, 120, 40).contains("Pick a track"), "there's no track to pick");
}

/// Clicking a step takes it, at every size that has a track list.
#[test]
fn a_step_is_clicked() {
    for (w, h) in [(60, 20), (80, 24), (120, 40), (160, 45)] {
        let (mut app, _dir) = blank(View::Library);
        screen(&mut app, w, h);
        point(&mut app, LEFT, "open a folder");
        assert!(matches!(app.modal, Some(Modal::Prompt(_))), "{w}x{h}");
    }
}

/// A folder typed opens as a drive; one that won't says why and lets the typing be fixed.
#[test]
fn a_typed_folder_opens() {
    let (_drive, root) = demo_drive();
    let (mut app, _dir) = blank(View::Library);
    press(&mut app, "\n/no/such/folder\n");
    assert!(matches!(app.modal, Some(Modal::Prompt(_))), "it stays");
    assert_eq!(app.toast.as_ref().map(|t| t.1), Some(super::super::app::Level::Error));
    press(&mut app, "\n");
    let Some(Modal::Prompt(p)) = &mut app.modal else { panic!("closed") };
    p.text = root.display().to_string();
    press(&mut app, "\n");
    assert!(app.modal.is_none());
    wait(&mut app, |a| a.loaded.is_some());
    let real = locallibrary::device::without_verbatim_prefix(root.canonicalize().unwrap());
    assert_eq!(app.loaded.as_ref().unwrap().root(), real);
}

/// A saved player mode stays up through the first read, with no flash of the panes, and with
/// nothing to show then opens Browse over itself: the empty state shows and the layout stays saved.
#[test]
fn the_deck_layout_gives_way_to_the_empty_state() {
    let (mut app, _dir) = blank(View::Library);
    app.deck_layout = true;
    reading_drives(&mut app);
    assert!(app.deck_showing());
    app.handle_msg(Msg::Drives(Vec::new()));
    assert!(!app.deck_showing());
    assert!(screen(&mut app, 120, 40).contains("No rekordbox drive open."));
    assert!(app.saved_state().deck_layout);
}

/// Plugged in with nothing open, a drive opens by itself, and the first library this version
/// names the keys to start with, until a key.
#[test]
fn a_first_library_names_its_keys_until_a_key() {
    let (_drive, root) = demo_drive();
    let (mut app, _dir) = blank(View::Library);
    app.intro_seen = None;
    app.handle_msg(Msg::Drives(vec![listed(&root)]));
    wait(&mut app, |a| a.loaded.is_some());
    assert!(app.intro);
    let footer = screen(&mut app, 80, 24).lines().last().unwrap().to_string();
    for k in ["enter play", "e actions", ": commands", "? help"] {
        assert!(footer.contains(k), "{k}: {footer}");
    }
    press(&mut app, "x");
    assert!(!app.intro);
    assert!(!screen(&mut app, 80, 24).contains("new here?"));
}

#[test]
fn a_click_ends_it_too() {
    let (mut app, _dir) = app_with(library());
    app.intro = true;
    mouse(&mut app, LEFT, (1, 1));
    assert!(!app.intro);
}

/// Seen is kept, once per version, and with the hints off it's seen without showing.
#[test]
fn it_shows_once_per_version() {
    let (mut app, _dir) = app_with(library());
    app.apply_saved(&Saved::default(), false);
    app.first_open();
    assert!(app.intro);
    assert_eq!(app.saved_state().intro_seen.as_deref(), Some(VERSION));

    let (mut again, _dir) = app_with(library());
    again.apply_saved(&app.saved_state(), false);
    again.first_open();
    assert!(!again.intro, "seen in the run before");

    let older = Saved { intro_seen: Some("0.0.1".into()), ..Saved::default() };
    again.apply_saved(&older, false);
    again.first_open();
    assert!(again.intro, "a new version shows it again");

    let quiet = Saved { hints: Some("off".into()), ..Saved::default() };
    let (mut off, _dir) = app_with(library());
    off.apply_saved(&quiet, false);
    off.first_open();
    assert!(!off.intro);
    assert_eq!(off.saved_state().intro_seen.as_deref(), Some(VERSION));
}

/// What's saved of a first run isn't a preference: it stays out of `settings export`.
#[test]
fn seen_stays_out_of_the_exported_settings() {
    let (app, _dir) = app_with(library());
    let saved = serde_json::to_value(app.saved_state()).unwrap();
    assert!(saved.get("intro_seen").is_some());
    let prefs: locallibrary::settings::transfer::PlayerPrefs = serde_json::from_value(saved).unwrap();
    assert!(!serde_json::to_string(&prefs).unwrap().contains("intro_seen"));
}

/// Starting with a saved visualiser and no drive still reaches the empty state: nothing about the
/// empty state depends on a first run.
#[test]
fn the_empty_state_is_every_run_with_no_drive() {
    let (mut app, _dir) = blank(View::Library);
    app.vis = Vis::Scope;
    assert!(screen(&mut app, 80, 24).contains("open a folder"));
    key(&mut app, KeyCode::Up);
    assert_eq!(app.steps.selected(), Some(0), "the list stops at its top");
    key(&mut app, KeyCode::End);
    assert_eq!(app.steps.selected(), Some(app.next_steps().len() - 1));
}
