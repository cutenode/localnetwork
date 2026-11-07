use super::super::app::View;
use super::super::app::testing::*;
use super::super::ui::screen;
use super::*;

/// The list is the palette's own jobs, named as it names them, with the drive they'd run on.
#[test]
fn the_jobs_are_listed_with_what_they_do() {
    let (mut app, _dir) = blank(View::Prepare);
    let s = screen(&mut app, 160, 45);
    assert!(s.contains("▸ 1 Prepare") && s.contains("no drive open"), "{s}");
    for act in JOBS {
        assert!(s.contains(keymap::name(act)), "{act:?}: {s}");
    }
    assert!(s.contains("check the drive against the player models"), "{s}");
    let (mut app, _dir) = app_with(library());
    press(&mut app, "gp");
    let root = app.loaded.as_ref().unwrap().root().to_path_buf();
    let drive = app.drive_label(&root);
    let s = screen(&mut app, 120, 40);
    assert!(s.contains(&format!("on {drive}")), "{s}");
}

/// Enter runs the job selected, as the palette would: with no drive, it says so as the palette's
/// does. The list stops at its ends.
#[test]
fn enter_runs_the_job_selected() {
    let (mut app, _dir) = blank(View::Prepare);
    press(&mut app, "\n");
    assert_eq!(last_message(&app), "no drive: plug one in, or open one in the library");
    press(&mut app, "jj");
    assert_eq!(app.prepare.selected(), Some(2));
    press(&mut app, "G");
    assert_eq!(app.prepare.selected(), Some(JOBS.len() - 1));
    press(&mut app, "j");
    assert_eq!(app.prepare.selected(), Some(JOBS.len() - 1));
    press(&mut app, "gg");
    assert_eq!(app.prepare.selected(), Some(0));
    app.prepare.select(None);
    app.toast = None;
    run(&mut app);
    assert!(app.toast.is_none(), "nothing selected, nothing run");
}

/// The last job reads the drive again.
#[test]
fn refresh_reads_the_drive_from_there() {
    let (mut app, _dir) = app_with(library());
    press(&mut app, "gpG\n");
    assert!(app.loading.is_some(), "reading the drive");
    wait(&mut app, |a| a.loading.is_none());
    press(&mut app, "R");
    assert!(app.loading.is_some(), "R does the same");
}

/// A click selects a row, and a second on it runs the job.
#[test]
fn a_job_is_clicked() {
    let (mut app, _dir) = blank(View::Prepare);
    screen(&mut app, 120, 40);
    let r = app.areas.prepare;
    let row = (r.x + 4, r.y + 3);
    mouse(&mut app, LEFT, row);
    assert_eq!(app.prepare.selected(), Some(2));
    assert!(app.toast.is_none());
    quick_again(&mut app);
    mouse(&mut app, LEFT, row);
    assert!(app.toast.is_some(), "run");
    mouse(&mut app, LEFT, (r.x + 4, r.y + 30));
    assert_eq!(app.prepare.selected(), Some(2), "under the last, nothing");
}
