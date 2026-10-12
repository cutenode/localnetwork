use super::*;

/// What reached a front end: each event's scope (`None`: plain `on_event`), and each step
/// announced.
#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<(Option<Scope>, String)>>,
    steps: Mutex<Vec<Scope>>,
}

impl Progress for Recorder {
    fn on_event(&self, event: &Event) {
        self.events.lock().unwrap().push((None, format!("{event:?}")));
    }
    fn on_scoped_event(&self, scope: &Scope, event: &Event) {
        self.events.lock().unwrap().push((Some(scope.clone()), format!("{event:?}")));
    }
    fn on_step(&self, scope: &Scope) {
        self.steps.lock().unwrap().push(scope.clone());
    }
}

/// An event knows both the drive and the step it belongs to, whichever was entered first.
#[test]
fn events_carry_their_drive_and_step_however_scopes_nest() {
    let recorder = Recorder::default();
    let ctx = Ctx::new(&recorder, CancelToken::new());
    let library = Step { index: 2, total: 2, id: "library" };
    let both = Scope { target: Some("Backup".into()), step: Some(library) };

    // A step, then a drive inside it.
    let steps = Steps::new(&ctx, vec!["format", "library"]);
    steps.begin("library");
    steps.ctx().for_target("Backup", |c| c.phase_start(Phase::Copy, 1, 10));
    // A drive, then steps inside it.
    ctx.for_target("Backup", |c| {
        let steps = Steps::new(c, vec!["format", "library"]);
        steps.begin("library");
        steps.ctx().phase_end(Phase::Copy);
        // Steps inside steps: the inner step is the one that's running.
        let inner = Steps::new(&steps.ctx(), vec!["verify"]);
        inner.begin("verify");
    });

    let events = recorder.events.into_inner().unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|(scope, _)| scope.as_ref() == Some(&both)), "{events:?}");
    let steps = recorder.steps.into_inner().unwrap();
    let backup = Some("Backup".to_string());
    assert_eq!(steps[0], Scope { target: None, step: Some(library) });
    assert_eq!(steps[1], both);
    assert_eq!(steps[2], Scope { target: backup, step: Some(Step { index: 1, total: 1, id: "verify" }) });
}

/// Cancelling any clone of the token fails the next check, and the error reads as
/// locallibrary's own cancelled error does.
#[test]
fn cancelling_a_clone_fails_the_next_check() {
    let ctx = Ctx::silent();
    assert_eq!(ctx.check(), Ok(()));
    ctx.cancel.clone().cancel();
    assert_eq!(ctx.check(), Err(Cancelled));
    assert_eq!(Cancelled.to_string(), "operation cancelled");
    assert_eq!(ctx.uncancellable().check(), Ok(()));
}

#[test]
fn repeated_warnings_are_summed_up() {
    let recorder = Recorder::default();
    let ctx = Ctx::new(&recorder, CancelToken::new());
    let mut limiter = WarningLimiter::new(&ctx, 2);
    for n in 1..=4 {
        limiter.warn("unreadable files", format!("ANLZ{n}.DAT is unreadable"));
    }
    limiter.warn("missing files", "a.mp3 is missing");
    limiter.finish();
    let warnings: Vec<String> = recorder.events.into_inner().unwrap().into_iter().map(|(_, e)| e).collect();
    assert_eq!(warnings.len(), 4, "{warnings:?}");
    assert!(warnings[1].contains("ANLZ2.DAT") && warnings[2].contains("a.mp3"), "{warnings:?}");
    assert!(warnings[3].contains("... and 2 more like that (4 unreadable files in total)"), "{warnings:?}");
}
