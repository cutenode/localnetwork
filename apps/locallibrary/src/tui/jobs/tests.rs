use super::super::app::View;
use super::super::app::testing::*;
use super::super::ui::screen;
use super::*;

fn selected(app: &App) -> Option<usize> {
    let Some(Modal::Jobs(list)) = &app.modal else { panic!("the job list isn't open") };
    list.selected()
}

/// `&` opens it from anywhere; with nothing running it says so, and esc closes it.
#[test]
fn it_opens_and_closes_with_nothing_running() {
    for view in View::ALL {
        let (mut app, _dir) = blank(view);
        press(&mut app, "&");
        let s = screen(&mut app, 80, 24);
        assert!(s.contains("Nothing's running.") && !s.contains("enter cancel"), "{view:?}");
        press(&mut app, "\u{1b}");
        assert!(app.modal.is_none());
    }
}

/// A plan stops at once, from the list as from esc.
#[test]
fn a_cancel_stops_a_plan() {
    let (mut app, _dir) = app_with(library());
    app.job = Some(job(false, false));
    press(&mut app, "&");
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("Planning") && s.contains("enter cancels"), "{s}");
    press(&mut app, "\n");
    assert!(app.job.is_none());
    assert_eq!(last_message(&app), "cancelled");
    assert!(screen(&mut app, 80, 24).contains("Nothing's running."), "it stays open");
}

/// A write stops only until its databases are being written: asked in time it's stopping, and
/// past that the row says so and a cancel changes nothing.
#[test]
fn a_write_says_when_it_cannot_stop() {
    let (mut app, _dir) = app_with(library());
    app.job = Some(job(true, false));
    press(&mut app, "&\n");
    assert!(app.job.as_ref().unwrap().cancel.is_cancelled());
    assert!(screen(&mut app, 80, 24).contains("stopping…"));

    app.job = Some(job(true, true));
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("can't stop now") && !s.contains("enter cancels"), "{s}");
    press(&mut app, "\n");
    assert!(!app.job.as_ref().unwrap().cancel.is_cancelled());
    assert_eq!(last_message(&app), "Writing TEST: too late to stop");
}

/// The status line says the same with the list closed, whatever the hints are set to.
#[test]
fn the_status_line_says_it_too() {
    let (mut app, _dir) = app_with(library());
    app.hints = crate::tui::choices::Hints::Off;
    app.job = Some(job(true, false));
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("Writing TEST") && s.contains("esc cancel"), "{s}");
    app.job = Some(job(true, true));
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("Writing TEST") && s.contains("can't stop now") && !s.contains("esc cancel"), "{s}");
}

/// A read of a drive or of the drives' status has nothing to stop: it isn't offered.
#[test]
fn reads_finish_on_their_own() {
    let (mut app, _dir) = app_with(library());
    app.loading = Some("Reading DNA…".into());
    app.status_loading = true;
    press(&mut app, "&");
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("Reading DNA…") && s.contains("Reading the drives' status"), "{s}");
    assert!(s.contains("finishes on its own") && !s.contains("enter cancels"), "{s}");
    press(&mut app, "\n");
    assert_eq!(last_message(&app), "Reading DNA…: it finishes on its own");
    assert_eq!(selected(&app), Some(0));
    press(&mut app, "j");
    assert_eq!(selected(&app), Some(1));
    press(&mut app, "j");
    assert_eq!(selected(&app), Some(1), "the list stops at its end");
    press(&mut app, "k\n");
    assert_eq!(app.loading.as_deref(), Some("Reading DNA…"), "still running");
}

/// A profile build stops, and what it sends back after is dropped.
#[test]
fn a_profile_build_stops() {
    let (mut app, _dir) = blank(View::Profile);
    super::super::profile::refresh(&mut app);
    assert!(app.profile.loading);
    press(&mut app, "&");
    assert!(screen(&mut app, 80, 24).contains("enter cancels"));
    press(&mut app, "\n");
    assert!(!app.profile.loading);
    assert!(rows(&app).is_empty());
}

/// A click outside closes it, as with the other dialogs; one inside leaves it.
#[test]
fn a_click_outside_closes_it() {
    let (mut app, _dir) = app_with(library());
    app.job = Some(job(false, false));
    press(&mut app, "&");
    screen(&mut app, 80, 24);
    point(&mut app, LEFT, "Planning");
    assert!(matches!(app.modal, Some(Modal::Jobs(_))));
    mouse(&mut app, LEFT, (0, 0));
    assert!(app.modal.is_none());
}

/// Whatever runs, the status line names the job list's first row: one list, not two.
#[test]
fn the_status_line_names_what_the_job_list_holds() {
    let (mut app, _dir) = blank(View::Library);
    app.status_loading = true;
    let s = screen(&mut app, 120, 40);
    assert!(s.lines().nth(38).unwrap_or_default().contains("Reading the drives' status"), "{s}");
    app.loading = Some("Reading DNA…".into());
    assert_eq!(rows(&app)[0].label, "Reading DNA…");
    let status = screen(&mut app, 120, 40).lines().nth(38).unwrap_or_default().to_string();
    assert!(status.contains("Reading DNA…") && !status.contains("status"), "{status}");
}
