//! Checks: what plugins' checks find on a drive, named after them and with the operation that
//! fixes it; what guards say about an edit plan, among its warnings, keeping it from being applied
//! once the user makes them blocking; a check that fails, answers what it may not or takes too
//! long a warning, never a failure; and which run, as the user says.

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::api::EditRequest;
use locallibrary::check::{CheckItem, Finding, Level};
use locallibrary::edit::{Edit, EditOptions, EditPlan};
use locallibrary::op::Planned;
use locallibrary::plugin::{CheckChange, CheckSettings, Fix, PluginFix, TIMEOUT, check_timeout, set_check_timeout};
use locallibrary::progress::NoProgress;
use locallibrary::{CancelToken, Device};

use super::*;

/// The echo manifest (as plugin `id`) with a check `found` and a guard `guard` besides its own,
/// each answering as its `findings` setting says (see the test plugin's `checked`).
fn checking(id: &str) -> String {
    let more = "[[contributes.checks]]\nname = \"found\"\ntitle = \"Found\"\n\n\
                [[contributes.checks]]\nname = \"guard\"\ntitle = \"Guard\"\nwhen = [\"apply\"]\n\n\
                [[contributes.settings]]\nkey = \"findings\"\ntitle = \"Findings\"\ntype = \"string\"\n\n\
                [[contributes.settings]]";
    ECHO.replace("id = \"echo\"", &format!("id = \"{id}\"")).replacen("[[contributes.settings]]", more, 1)
}

impl Fixture {
    /// The plugins `ids`, each with [`checking`]'s checks, stopped soon once asked to.
    fn checks(ids: &[&str]) -> Fixture {
        let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        let options = Options { grace: Duration::from_millis(300), ..Options::default() };
        Fixture::new(|dir| ids.iter().map(|id| spec(dir, &checking(id), &[])).collect(), options)
    }

    /// `plugin`'s checks and guards answer `answer`.
    fn find(&self, plugin: &str, answer: Value) {
        self.plugins.set_setting(plugin, &self.store, "findings", Some(answer.to_string().into())).unwrap();
    }

    fn device(&self) -> Device {
        Device::open(&self.drive).unwrap()
    }

    /// What the plugins' checks find on the drive.
    fn run_checks(&self) -> Vec<CheckItem> {
        self.plugins.run_checks(&self.device(), Some(&self.store), &Ctx::silent()).unwrap()
    }

    /// Each item as `(check, level, message)`.
    fn found(&self) -> Vec<(String, Level, String)> {
        self.run_checks().into_iter().map(|i| (i.finding.check, i.finding.level, i.finding.message)).collect()
    }

    /// A plan making the playlist `name` on the drive, with what the guards make of it.
    fn guarded(&self, name: &str) -> Planned<EditPlan> {
        self.guarded_as(name, EditOptions::default())
    }

    /// [`Fixture::guarded`], planned with `options`.
    fn guarded_as(&self, name: &str, options: EditOptions) -> Planned<EditPlan> {
        let edits = vec![Edit::CreatePlaylist { path: name.into(), folder: false }];
        let req = EditRequest { drive: "USB".into(), edits, options };
        let mut planned = api::edit_plan(&req, &Ctx::silent()).unwrap();
        self.plugins.guard(&mut planned, |p| p.recorded.clone(), Some(&self.store), &Ctx::silent()).unwrap();
        planned
    }
}

/// The drive's tracks without a title, as echo's own check finds them.
fn untitled(f: &Fixture) -> Vec<u32> {
    common::library(&f.drive).tracks.iter().filter(|t| t.title.is_empty()).map(|t| t.id).collect()
}

/// What echo's own check says about the drive, if anything.
fn echo_said(f: &Fixture, id: &str) -> Vec<(String, Level, String)> {
    let n = untitled(f).len();
    let said = (n > 0).then(|| (format!("{id}.untitled"), Level::Warning, format!("{n} track(s) have no title")));
    said.into_iter().collect()
}

/// What a check finds is on the drive it ran on, named `<plugin>.<check>` whatever the plugin
/// called it, with the files, playlist and tracks it's about and the plugin's operation fixing it;
/// each plugin's checks in the order they're declared.
#[test]
fn a_checks_findings_are_named_after_it_with_what_fixes_them() {
    let f = Fixture::checks(&["echo", "second"]);
    let problem = json!({
        "level": "problem", "check": "anything", "message": "two copies", "tracks": [1, 2],
        "playlist": "Sets/Friday", "paths": ["Contents/a.mp3"],
        "fix": { "operation": "playlist", "params": { "name": "Copies" } },
    });
    f.find("echo", json!({ "findings": [problem, { "level": "info", "message": "fine" }] }));
    let items = f.run_checks();
    let mine: Vec<&CheckItem> = items.iter().filter(|i| i.check == "echo.found").collect();
    let finding = Finding {
        tracks: vec![1, 2],
        playlist: Some("Sets/Friday".into()),
        paths: vec!["Contents/a.mp3".into()],
        ..Finding::new(Level::Problem, "echo.found", "two copies")
    };
    let fix = Fix::Plugin(PluginFix {
        plugin: "echo".into(),
        operation: "playlist".into(),
        params: json!({ "name": "Copies" }),
    });
    assert_eq!(mine[0].finding, finding);
    assert_eq!(mine[0].fix, Some(fix));
    assert_eq!((mine[1].level, mine[1].message.as_str(), &mine[1].fix), (Level::Info, "fine", &None));
    assert!(items.iter().all(|i| i.drive == "USB"));
    let checks: Vec<&str> = items.iter().map(|i| i.check.as_str()).collect();
    let mut expected: Vec<String> = echo_said(&f, "echo").into_iter().map(|(c, ..)| c).collect();
    expected.extend(["echo.found".into(), "echo.found".into()]);
    expected.extend(echo_said(&f, "second").into_iter().map(|(c, ..)| c));
    assert_eq!(checks, expected, "each plugin's, in order; second's check finds nothing");
}

/// Echo's own check, on a drive with a track without a title: found, with its fix.
#[test]
fn echos_check_finds_tracks_without_a_title() {
    let f = Fixture::checks(&["echo"]);
    let first = common::library(&f.drive).tracks[0].id;
    let edits = vec![Edit::EditTracks {
        tracks: locallibrary::query::TrackSelector { ids: vec![first], ..Default::default() },
        fields: locallibrary::edit::TrackFields { title: Some(String::new()), ..Default::default() },
    }];
    let req = EditRequest { drive: "USB".into(), edits, options: EditOptions::default() };
    api::edit_plan(&req, &Ctx::silent()).unwrap().apply(None, &Ctx::silent()).unwrap();
    let items = f.run_checks();
    let echo = items.iter().find(|i| i.check == "echo.untitled").expect("found");
    assert_eq!((echo.level, &echo.tracks), (Level::Warning, &untitled(&f)));
    assert!(matches!(&echo.fix, Some(Fix::Plugin(f)) if f.operation == "playlist"), "{:?}", echo.fix);
}

/// A check that fails, answers what it may not, or takes longer than the setting gives it is a
/// warning on the drive saying so, and what the others find is there all the same. Cancelled, it
/// cancels.
#[test]
fn a_check_that_fails_or_takes_too_long_is_a_warning() {
    let f = Fixture::checks(&["echo"]);
    // Long enough for the checks that answer, however busy the machine; those that don't get 0.2 s.
    set_check_timeout(&f.store, Some(60.0)).unwrap();
    let warned = |answer: Value| {
        f.find("echo", answer);
        let found = f.found();
        let warning = found.iter().find(|(c, ..)| c == "echo.found").cloned().expect("a warning");
        assert_eq!(warning.1, Level::Warning, "{warning:?}");
        assert_eq!(found.iter().filter(|(c, ..)| c == "echo.found").count(), 1);
        warning.2
    };
    assert_eq!(warned(json!({ "fail": "invalid" })), "it failed (the Echo plugin)");
    let fails = "the Echo plugin failed:";
    let said = warned(json!({ "findings": [{ "level": "info", "message": " " }] }));
    assert_eq!(said, format!("{fails} its finding 0 says nothing"));
    let said = warned(json!({ "findings": [{ "level": "info", "message": "m", "paths": ["../out"] }] }));
    assert_eq!(said, format!("{fails} its finding 0 names \"../out\", which isn't a file on the drive"));
    let fix = json!({ "operation": "nope" });
    let said = warned(json!({ "findings": [{ "level": "info", "message": "m", "fix": fix }] }));
    assert_eq!(said, format!("{fails} its finding 0 is fixed by \"nope\", which isn't its operation"));
    // Asked to stop, it stops; one that doesn't is killed.
    set_check_timeout(&f.store, Some(0.2)).unwrap();
    for answer in [json!({ "wait": true }), json!({ "stuck": true })] {
        assert_eq!(warned(answer), "took longer than 0.2s, so it was stopped");
    }
    assert!(f.logs.lock().unwrap().iter().any(|(id, line)| id == "echo" && line.starts_with("its check found: ")));
    let cancel = CancelToken::new();
    cancel.cancel();
    let cancelled = f.plugins.run_checks(&f.device(), Some(&f.store), &Ctx::new(&NoProgress, cancel));
    assert!(matches!(cancelled, Err(Error::Cancelled)), "{cancelled:?}");
    // Cancelled while one waits, long before its time is up.
    set_check_timeout(&f.store, Some(60.0)).unwrap();
    f.find("echo", json!({ "wait": true }));
    let (cancel, started) = (CancelToken::new(), Instant::now());
    let later = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        later.cancel();
    });
    let cancelled = f.plugins.run_checks(&f.device(), Some(&f.store), &Ctx::new(&NoProgress, cancel));
    assert!(matches!(cancelled, Err(Error::Cancelled)), "{cancelled:?}");
    assert!(started.elapsed() < Duration::from_secs(30));
}

/// Of a check that finds too much, what's kept is bounded, a note saying the rest are left out.
#[test]
fn what_a_check_finds_is_bounded() {
    let f = Fixture::checks(&["echo"]);
    let many: Vec<Value> = (0..1001).map(|i| json!({ "level": "info", "message": format!("{i}") })).collect();
    f.find("echo", json!({ "findings": many }));
    let found: Vec<_> = f.found().into_iter().filter(|(c, ..)| c == "echo.found").collect();
    assert_eq!(found.len(), 1001);
    let last = &found[1000];
    assert_eq!((last.1, last.2.as_str()), (Level::Info, "more than 1000 found, the rest left out"));
}

/// Each check runs unless it's turned off (one off to start with, once it's turned on), and only a
/// guard can keep a plan from being applied; how long a check has is a setting too.
#[test]
fn checks_run_as_the_user_says() {
    let f = Fixture::checks(&["echo"]);
    let settings = |check: &str| f.plugins.check_settings("echo", check, Some(&f.store)).unwrap();
    let on = CheckSettings { on: true, blocking: false };
    assert_eq!(settings("found"), on);
    assert_eq!(settings("edits"), CheckSettings { on: false, blocking: false }, "off to start with");
    f.find("echo", json!({ "findings": [{ "level": "info", "message": "here" }] }));
    let set = |check: &str, on, blocking, reset| {
        f.plugins.set_check(&f.store, "echo", check, CheckChange { on, blocking, reset })
    };
    assert_eq!(set("found", Some(false), None, false).unwrap(), CheckSettings { on: false, blocking: false });
    assert!(f.found().iter().all(|(c, ..)| c != "echo.found"));
    assert!(!set("found", None, None, false).unwrap().on, "left as it is");
    assert_eq!(set("found", None, None, true).unwrap(), on, "back to its default");
    assert!(f.found().iter().any(|(c, ..)| c == "echo.found"));
    let e = set("found", None, Some(true), false).unwrap_err();
    assert_eq!(e.to_string(), "echo.found isn't run before a plan is applied, so it can't keep one from being applied");
    assert_eq!(set("guard", None, Some(true), false).unwrap(), CheckSettings { on: true, blocking: true });
    assert_eq!(set("guard", Some(false), None, false).unwrap(), CheckSettings { on: false, blocking: true });
    assert_eq!(set("guard", Some(true), None, true).unwrap(), CheckSettings { on: true, blocking: false });
    set("guard", None, Some(true), false).unwrap();
    assert_eq!(f.plugins.check_settings("echo", "nope", None).unwrap_err().code(), "not_found");
    assert_eq!(f.plugins.check_settings("nope", "found", None).unwrap_err().code(), "not_found");
    assert_eq!(check_timeout(Some(&f.store)), TIMEOUT);
    assert_eq!(set_check_timeout(&f.store, Some(2.5)).unwrap(), Duration::from_millis(2500));
    assert_eq!(set_check_timeout(&f.store, None).unwrap(), TIMEOUT);
    for wrong in [-1.0, 0.0, 0.0001, 3601.0, 1e300, f64::NAN, f64::INFINITY] {
        let e = set_check_timeout(&f.store, Some(wrong)).unwrap_err();
        assert_eq!(e.to_string(), format!("a check has 0.001 to 3600 seconds to answer, not {wrong}"));
    }
    let checks: Vec<(&str, &str, bool)> =
        f.plugins.checks(Some(&f.store)).into_iter().map(|(p, c, s)| (p, c.name.as_str(), s.blocking)).collect();
    let all =
        [("echo", "untitled", false), ("echo", "edits", false), ("echo", "found", false), ("echo", "guard", true)];
    assert_eq!(checks, all);
}

/// A guard is asked about a plan with its operation and edits, and what it says is among the
/// plan's warnings; once it's blocking, a warning or a problem it finds keeps the plan from being
/// applied, and an `info` doesn't.
#[test]
fn guards_warn_about_a_plan_and_block_it_once_told_to() {
    let f = Fixture::checks(&["echo"]);
    f.plugins.set_check(&f.store, "echo", "edits", CheckChange { on: Some(true), ..CheckChange::default() }).unwrap();
    f.find("echo", json!({ "findings": [{ "level": "warning", "message": "on a Friday" }] }));
    let planned = f.guarded("Warmup");
    let guarded = |p: &Planned<EditPlan>| -> Vec<String> {
        p.warnings.iter().filter(|w| w.starts_with("Count") || w.starts_with("Guard")).cloned().collect()
    };
    assert_eq!(guarded(&planned), ["Count a plan's edits: edit makes 1 edit(s)", "Guard: on a Friday"]);
    assert_eq!(planned.blocked, None);
    f.plugins
        .set_check(&f.store, "echo", "guard", CheckChange { blocking: Some(true), ..CheckChange::default() })
        .unwrap();
    let planned = f.guarded("Warmup");
    assert_eq!(planned.blocked.as_deref(), Some("echo.guard"));
    let e = planned.apply(None, &Ctx::silent()).unwrap_err();
    assert_eq!((e.code(), e.detail().fields["check"].as_str()), ("conflict", Some("echo.guard")));
    assert!(common::library(&f.drive).find_playlist("Warmup").is_none(), "not applied");
    f.find("echo", json!({ "findings": [{ "level": "info", "message": "fine" }] }));
    let planned = f.guarded("Warmup");
    assert_eq!(planned.blocked, None, "an info doesn't block");
    planned.apply(None, &Ctx::silent()).unwrap();
    // A guard that fails is a warning, and never blocks.
    f.find("echo", json!({ "fail": "conflict" }));
    let planned = f.guarded("Cooldown");
    assert_eq!(planned.blocked, None);
    assert!(planned.warnings.contains(&"Guard: it failed (the Echo plugin)".to_string()));
    // A plan that changes nothing isn't guarded.
    let planned = f.guarded("Warmup");
    assert!(planned.noop && planned.warnings.iter().all(|w| !w.starts_with("Guard")), "{:?}", planned.warnings);
}

/// A guard is given the edits of a plan made without the journal too (a replay's, say), and
/// blocks it as any; unblocked, it's made, and still not recorded.
#[test]
fn a_guard_sees_the_edits_of_a_plan_kept_out_of_the_journal() {
    let f = Fixture::checks(&["echo"]);
    let change = |check: &str, change| f.plugins.set_check(&f.store, "echo", check, change).unwrap();
    change("edits", CheckChange { on: Some(true), ..CheckChange::default() });
    change("guard", CheckChange { blocking: Some(true), ..CheckChange::default() });
    let about_edits = json!({ "level": "problem", "message": "not before a set" });
    f.find("echo", json!({ "about_edits": true, "findings": [about_edits] }));
    let unjournaled = EditOptions { no_journal: true, ..EditOptions::default() };
    let journal = || locallibrary::edit::journal::list(&f.device()).unwrap().len();
    let before = journal();
    let planned = f.guarded_as("Warmup", unjournaled.clone());
    let counted = "Count a plan's edits: edit makes 1 edit(s)".to_string();
    assert!(planned.warnings.contains(&counted), "{:?}", planned.warnings);
    assert_eq!(planned.blocked.as_deref(), Some("echo.guard"));
    assert!(planned.apply(None, &Ctx::silent()).is_err());
    assert!(common::library(&f.drive).find_playlist("Warmup").is_none(), "not applied");
    change("guard", CheckChange { blocking: Some(false), ..CheckChange::default() });
    f.guarded_as("Warmup", unjournaled).apply(None, &Ctx::silent()).unwrap();
    assert!(common::library(&f.drive).find_playlist("Warmup").is_some());
    assert_eq!(journal(), before, "not recorded");
}

/// What a check says is shown as it's safe to: without control characters or secrets, and cut
/// short past a line; a file it names with a control character in it is the file it names as it
/// prints.
#[test]
fn what_a_check_says_is_printable_and_bounded() {
    let f = Fixture::checks(&["echo"]);
    let long = "a".repeat(600);
    let shown =
        json!({ "level": "warning", "message": "\u{1b}[31mred\u{7}\n", "playlist": "Sets\u{1b}]0;x\u{7}/Friday" });
    f.find("echo", json!({ "findings": [shown, { "level": "info", "message": long }] }));
    let items = f.run_checks();
    let mine: Vec<&CheckItem> = items.iter().filter(|i| i.check == "echo.found").collect();
    assert_eq!(
        (mine[0].message.as_str(), mine[0].playlist.as_deref()),
        ("red ", Some("Sets/Friday")),
        "a newline is a space"
    );
    assert_eq!(mine[1].message, format!("{}...", "a".repeat(500)));
    let path = json!({ "level": "info", "message": "m", "paths": ["Contents/\u{1b}[2Ja.mp3"] });
    f.find("echo", json!({ "findings": [path] }));
    let items = f.run_checks();
    let named = items.iter().find(|i| i.check == "echo.found").expect("a finding");
    assert_eq!((named.level, named.paths.clone()), (Level::Info, vec!["Contents/a.mp3".to_string()]));
}

/// A finding a check might answer, valid or not: its JSON, and whether the host keeps it.
#[hegel::test_helper]
fn finding(tc: &TestCase) -> (Value, bool) {
    let level = tc.draw(gs::sampled_from(vec!["info", "warning", "problem"]));
    let (message, said) = match tc.draw(gs::weighted_booleans(0.9)) {
        true => (tc.draw(gs::from_regex("[a-z][a-z ]{0,11}")), true),
        false => (tc.draw(gs::sampled_from(vec!["", " ", "\t"])).to_string(), false),
    };
    let (path, on_the_drive) = match tc.draw(gs::integers::<u8>().max_value(9)) {
        0 => (Some(tc.draw(gs::sampled_from(vec!["../x", "/abs", "", "Contents/../../x"])).to_string()), false),
        1..=3 => (Some(format!("Contents/{}.mp3", tc.draw(gs::from_regex("[a-z]{1,6}")))), true),
        _ => (None, true),
    };
    let (fix, fixes) = match tc.draw(gs::integers::<u8>().max_value(9)) {
        0 => (Some(json!({ "operation": "nope" })), false),
        1..=3 => (Some(json!({ "operation": "playlist", "params": { "name": "x" } })), true),
        _ => (None, true),
    };
    let tracks: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>()).max_size(3));
    let mut f = json!({ "level": level, "message": message, "tracks": tracks });
    if let Some(path) = path {
        f["paths"] = json!([path]);
    }
    if let Some(fix) = fix {
        f["fix"] = fix;
    }
    (f, said && on_the_drive && fixes)
}

/// Whatever a check answers, what's on the drive is what it found, as it said it, named after its
/// plugin, or (when it answered something it may not, failed or took too long) one warning saying
/// so; and only a problem it found counts as one.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn what_a_check_finds_is_what_it_answered(tc: TestCase) {
    let f = Fixture::checks(&["echo"]);
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let drawn: Vec<(Value, bool)> =
            (0..tc.draw(gs::integers::<usize>().max_value(4))).map(|_| finding(&tc)).collect();
        let waits = json!({ "wait": true });
        let fails = tc.draw(gs::sampled_from(vec![None, Some(json!({ "fail": "io" })), Some(waits.clone())]));
        // A check that waits never answers, so it times out however short its time; one that
        // answers gets long enough to on a busy machine, so its time running out isn't the warning.
        let timeout = if fails.as_ref() == Some(&waits) { 0.3 } else { 60.0 };
        set_check_timeout(&f.store, Some(timeout)).unwrap();
        let answer =
            fails.clone().unwrap_or_else(|| json!({ "findings": drawn.iter().map(|(f, _)| f).collect::<Vec<_>>() }));
        f.find("echo", answer);
        let items = f.run_checks();
        let mine: Vec<&CheckItem> = items.iter().filter(|i| i.check == "echo.found").collect();
        let others: Vec<(String, Level, String)> = items
            .iter()
            .filter(|i| i.check != "echo.found")
            .map(|i| (i.check.clone(), i.level, i.message.clone()))
            .collect();
        assert_eq!(others, echo_said(&f, "echo"), "the other checks' findings are there all the same");
        if fails.is_some() || drawn.iter().any(|(_, kept)| !kept) {
            assert_eq!(mine.len(), 1);
            assert_eq!((mine[0].level, &mine[0].fix), (Level::Warning, &None));
            continue;
        }
        assert_eq!(mine.len(), drawn.len());
        for (item, (json, _)) in mine.iter().zip(&drawn) {
            assert_eq!(serde_json::to_value(item.level).unwrap(), json["level"]);
            assert_eq!(item.message, json["message"].as_str().unwrap());
            assert_eq!(serde_json::to_value(&item.tracks).unwrap(), json["tracks"]);
            assert_eq!(item.paths.first().map(String::as_str), json["paths"][0].as_str());
            let operation = match &item.fix {
                Some(Fix::Plugin(f)) => Some(f.operation.as_str()),
                _ => None,
            };
            assert_eq!(operation, json["fix"]["operation"].as_str());
            assert_eq!(item.drive, "USB");
        }
    }
}

/// Whatever the guards say, a plan's warnings have each of their findings, and it's kept from
/// being applied exactly when a blocking guard found a warning or a problem; one that fails or
/// takes too long is a warning, and never keeps a plan from being applied.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_plan_is_blocked_exactly_when_a_blocking_guard_finds_something(tc: TestCase) {
    let f = Fixture::checks(&["echo", "second"]);
    // Every guard answers: a time that runs out on a busy machine would be another warning.
    set_check_timeout(&f.store, Some(60.0)).unwrap();
    let mut blocks = None;
    let mut said: Vec<String> = Vec::new();
    for plugin in ["echo", "second"] {
        let on = tc.draw(gs::booleans());
        let blocking = tc.draw(gs::booleans());
        let change = CheckChange { on: Some(on), blocking: Some(blocking), reset: false };
        f.plugins.set_check(&f.store, plugin, "guard", change).unwrap();
        let levels: Vec<&str> = tc.draw(gs::vecs(gs::sampled_from(vec!["info", "warning", "problem"])).max_size(3));
        let fails = tc.draw(gs::weighted_booleans(0.2));
        let findings: Vec<Value> =
            levels.iter().enumerate().map(|(i, l)| json!({ "level": l, "message": format!("{plugin} {i}") })).collect();
        f.find(plugin, if fails { json!({ "fail": "io" }) } else { json!({ "findings": findings }) });
        if !on {
            continue;
        }
        if fails {
            said.push("Guard: the Echo plugin failed: it failed".into());
            continue;
        }
        said.extend((0..levels.len()).map(|i| format!("Guard: {plugin} {i}")));
        if blocking && levels.iter().any(|l| *l != "info") {
            blocks.get_or_insert(format!("{plugin}.guard"));
        }
    }
    let planned = f.guarded("Warmup");
    let guarded: Vec<&String> = planned.warnings.iter().filter(|w| w.starts_with("Guard: ")).collect();
    let said: Vec<&String> = said.iter().fold(Vec::new(), |mut once, w| {
        if !once.contains(&w) {
            once.push(w);
        }
        once
    });
    assert_eq!(guarded, said);
    assert_eq!(planned.blocked, blocks);
    let applied = planned.apply(None, &Ctx::silent());
    assert_eq!(applied.is_ok(), blocks.is_none(), "{applied:?}");
}

/// What a check answers, or what becomes of it, as [`prop_a_check_is_what_it_found_or_a_warning`]
/// draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Answers {
    /// Findings past what's kept, the rest left out.
    Many,
    /// A finding saying more than a line, in a playlist.
    Long,
    /// A finding naming a file that isn't on the drive.
    Outside,
    /// It fails.
    Fails,
    /// It waits past its time, until it's stopped.
    Waits,
    /// It's cancelled before it starts.
    Cancelled,
    /// It's cancelled while it waits.
    CancelledWaiting,
    /// The drive it's run on is gone by then.
    Gone,
    /// A finding that says nothing.
    Empty,
    /// A finding fixed by what isn't its operation.
    OtherFix,
}

/// For anything a check answers, what's found on the drive is what it found, bounded (past what
/// a check may find the rest left out, a message past a line cut short), or one warning saying
/// why it found nothing (it named a file that isn't on the drive, failed, took too long or its
/// drive was gone); cancelled, before or while it runs, running them is. Its parts of a view,
/// sent as any request's may be, change nothing.
#[hegel::test(test_cases = 1)]
fn prop_a_check_is_what_it_found_or_a_warning(tc: TestCase) {
    for answers in [
        Answers::Many,
        Answers::Long,
        Answers::Outside,
        Answers::Fails,
        Answers::Waits,
        Answers::Cancelled,
        Answers::CancelledWaiting,
        Answers::Gone,
        Answers::Empty,
        Answers::OtherFix,
    ] {
        prop_a_check_is_what_it_found_or_a_warning_case(&tc, answers);
    }
}

/// [`prop_a_check_is_what_it_found_or_a_warning`] for one `answers`.
#[hegel::test_helper]
fn prop_a_check_is_what_it_found_or_a_warning_case(tc: &TestCase, answers: Answers) {
    let options = Options { grace: Duration::from_millis(300), ..Options::default() };
    let f = Fixture::new(|dir| vec![spec(dir, &checking("echo"), &["--partials"])], options);
    let timeout = if answers == Answers::Waits { 0.2 } else { 60.0 };
    set_check_timeout(&f.store, Some(timeout)).unwrap();
    let answer = match answers {
        Answers::Many => {
            let n = tc.draw(gs::integers::<usize>().min_value(1001).max_value(1003));
            json!({ "findings": (0..n).map(|i| json!({ "level": "info", "message": i.to_string() })).collect::<Vec<_>>() })
        }
        Answers::Long => {
            json!({ "findings": [{ "level": "info", "message": "a".repeat(501), "playlist": "Sets/Friday" }] })
        }
        Answers::Outside => json!({ "findings": [{ "level": "info", "message": "m", "paths": ["../out"] }] }),
        Answers::Fails => json!({ "fail": "io" }),
        Answers::Waits | Answers::CancelledWaiting => json!({ "wait": true }),
        Answers::Cancelled | Answers::Gone => json!({ "findings": [] }),
        Answers::Empty => json!({ "findings": [{ "level": "info", "message": " " }] }),
        Answers::OtherFix => {
            json!({ "findings": [{ "level": "info", "message": "m", "fix": { "operation": "nope" } }] })
        }
    };
    f.find("echo", answer);
    let device = f.device();
    let cancel = CancelToken::new();
    match answers {
        Answers::Cancelled => cancel.cancel(),
        Answers::CancelledWaiting => {
            let later = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                later.cancel();
            });
        }
        Answers::Gone => std::fs::rename(&f.drive, f.drive.with_file_name("Elsewhere")).unwrap(),
        _ => {}
    }
    let ran = f.plugins.run_checks(&device, Some(&f.store), &Ctx::new(&NoProgress, cancel));
    if matches!(answers, Answers::Cancelled | Answers::CancelledWaiting) {
        assert!(matches!(ran, Err(Error::Cancelled)), "{ran:?}");
        return;
    }
    let items = ran.unwrap();
    let mine: Vec<(Level, String, Option<String>)> = items
        .iter()
        .filter(|i| i.check == "echo.found")
        .map(|i| (i.level, i.message.clone(), i.playlist.clone()))
        .collect();
    let fails = "the Echo plugin failed:";
    let warned = |why: &str| (Level::Warning, why.to_string(), None);
    match answers {
        Answers::Many => {
            assert_eq!(mine.len(), 1001);
            assert_eq!(mine[1000], (Level::Info, "more than 1000 found, the rest left out".into(), None));
        }
        Answers::Long => {
            assert_eq!(mine, [(Level::Info, format!("{}...", "a".repeat(500)), Some("Sets/Friday".into()))]);
        }
        Answers::Outside => {
            let why = format!("{fails} its finding 0 names \"../out\", which isn't a file on the drive");
            assert_eq!(mine, [warned(&why)]);
        }
        Answers::Fails => assert_eq!(mine, [warned(&format!("{fails} it failed"))]),
        Answers::Waits => assert_eq!(mine, [warned("took longer than 0.2s, so it was stopped")]),
        Answers::Empty => assert_eq!(mine, [warned(&format!("{fails} its finding 0 says nothing"))]),
        Answers::OtherFix => {
            let why = format!("{fails} its finding 0 is fixed by \"nope\", which isn't its operation");
            assert_eq!(mine, [warned(&why)]);
        }
        _ => {
            let (level, said) = (mine[0].0, &mine[0].1);
            assert_eq!((mine.len(), level), (1, Level::Warning));
            assert!(said.starts_with("not found: no mounted drive matches "), "{said}");
        }
    }
}

/// For any change to a check's settings where the store can't keep them (on Unix), or to how long
/// checks have, it fails and they stay as they were; a check the plugin doesn't have has none, and
/// one that isn't a guard can't keep a plan from being applied.
#[hegel::test(test_cases = 10)]
fn prop_check_settings_that_cant_be_kept_stay_as_they_were(tc: TestCase) {
    let f = Fixture::checks(&["echo"]);
    let unknown = tc.draw(gs::sampled_from(vec![("echo", "nope"), ("nope", "found")]));
    assert_eq!(f.plugins.check_settings(unknown.0, unknown.1, None).unwrap_err().code(), "not_found");
    let e = f.plugins.check_change("echo", "found", CheckChange { blocking: Some(true), ..CheckChange::default() });
    assert_eq!(e.unwrap_err().code(), "invalid");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let settings = |check: &str| f.plugins.check_settings("echo", check, Some(&f.store));
        let on = tc.draw(gs::booleans());
        let reset = CheckChange { reset: true, ..CheckChange::default() };
        assert_eq!(
            f.plugins.set_check(&f.store, "echo", "guard", reset).unwrap(),
            CheckSettings { on: true, blocking: false }
        );
        f.plugins.set_check(&f.store, "echo", "guard", CheckChange { on: Some(on), ..CheckChange::default() }).unwrap();
        set_check_timeout(&f.store, Some(30.0)).unwrap();
        let before = (settings("guard").unwrap(), check_timeout(Some(&f.store)));
        let plugins = f.store.root().join("plugins");
        for change in [
            CheckChange { reset: true, ..CheckChange::default() },
            CheckChange { on: Some(!on), ..CheckChange::default() },
            CheckChange { blocking: Some(true), ..CheckChange::default() },
        ] {
            std::fs::set_permissions(&plugins, std::fs::Permissions::from_mode(0o500)).unwrap();
            let set = f.plugins.set_check(&f.store, "echo", "guard", change);
            let timed = set_check_timeout(&f.store, Some(10.0));
            std::fs::set_permissions(&plugins, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!((set.unwrap_err().code(), timed.unwrap_err().code()), ("io", "io"));
            assert_eq!((settings("guard").unwrap(), check_timeout(Some(&f.store))), before);
        }
    }
}

/// How a plan comes to the guards.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Guarding {
    /// Two plugins' guards say the same.
    Same,
    /// It makes no edit the guards are given.
    NoEdits,
    /// Cancelled first.
    Cancelled,
    /// The drive it writes is gone by then.
    Gone,
    /// Planned as the app plans it, with the plugins hosted, cancelled while a guard waits.
    HostedCancelled,
}

/// For any plan the guards see, what two of them say alike is one warning; a plan with no edits
/// for them isn't guarded; a guard whose drive is gone is a warning; and cancelled, guarding a plan
/// is (as planning one with the plugins hosted). Their parts of a view, sent as any request's may
/// be, change nothing.
#[hegel::test(test_cases = 1)]
fn prop_a_plan_is_guarded_once_for_what_guards_say_alike(tc: TestCase) {
    for guarding in [Guarding::Same, Guarding::NoEdits, Guarding::Cancelled, Guarding::Gone, Guarding::HostedCancelled]
    {
        prop_a_plan_is_guarded_once_for_what_guards_say_alike_case(&tc, guarding);
    }
}

/// [`prop_a_plan_is_guarded_once_for_what_guards_say_alike`] for one `guarding`.
#[hegel::test_helper]
fn prop_a_plan_is_guarded_once_for_what_guards_say_alike_case(tc: &TestCase, guarding: Guarding) {
    let options = Options { grace: Duration::from_millis(300), ..Options::default() };
    let f = Fixture::new(
        |dir| ["echo", "second"].iter().map(|id| spec(dir, &checking(id), &["--partials"])).collect(),
        options,
    );
    set_check_timeout(&f.store, Some(60.0)).unwrap();
    let level = tc.draw(gs::sampled_from(vec!["info", "warning"]));
    for plugin in ["echo", "second"] {
        f.find(plugin, json!({ "findings": [{ "level": level, "message": "the same" }] }));
    }
    let edits = vec![Edit::CreatePlaylist { path: "Warmup".into(), folder: false }];
    let req = EditRequest { drive: "USB".into(), edits, options: EditOptions::default() };
    let cancel = CancelToken::new();
    if guarding == Guarding::HostedCancelled {
        f.find("echo", json!({ "wait": true }));
        let f = Hosted::new(f);
        f.plugins
            .set_check(&f.store, "echo", "guard", CheckChange { on: Some(true), ..CheckChange::default() })
            .unwrap();
        let later = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            later.cancel();
        });
        let planned = api::edit_plan(&req, &Ctx::new(&NoProgress, cancel));
        assert!(matches!(planned, Err(Error::Cancelled)), "{:?}", planned.map(|p| p.warnings));
        return;
    }
    let mut planned = api::edit_plan(&req, &Ctx::silent()).unwrap();
    match guarding {
        Guarding::Cancelled => cancel.cancel(),
        Guarding::Gone => std::fs::rename(&f.drive, f.drive.with_file_name("Elsewhere")).unwrap(),
        _ => {}
    }
    let edits = |p: &EditPlan| if guarding == Guarding::NoEdits { Vec::new() } else { p.recorded.clone() };
    let guarded = f.plugins.guard(&mut planned, edits, Some(&f.store), &Ctx::new(&NoProgress, cancel));
    if guarding == Guarding::Cancelled {
        assert!(matches!(guarded, Err(Error::Cancelled)), "{guarded:?}");
        return;
    }
    guarded.unwrap();
    let said: Vec<&String> = planned.warnings.iter().filter(|w| w.starts_with("Guard: ")).collect();
    match guarding {
        Guarding::Same => assert_eq!(said, ["Guard: the same"]),
        Guarding::NoEdits => assert_eq!(said, Vec::<&String>::new()),
        _ => assert!(said.len() == 1 && said[0].starts_with("Guard: not found: no mounted drive matches "), "{said:?}"),
    }
}
