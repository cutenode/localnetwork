//! Commands planned first (`plan = true`): planned with a dry run, confirmed, then run as they were
//! planned; long-running commands, given longer to stop; the plugin's folder for what it keeps;
//! and a plugin suggesting one of its commands.

use hegel::TestCase;
use hegel::generators::{self as gs, Generator};
use locallibrary::op::Planned;
use locallibrary::plugin::{CommandPlanned, PluginCommand};
use loclib_plugin::{CommandAction, HostAction, HostNotify, RunCommand, Suggestion};

use locallibrary::plugin::Event;
use loclib_plugin::EventName;

use super::events::{listening, told};
use super::*;

/// The echo manifest with `keep`, a headless command about the collection planned first and
/// running long, and `slow`, one running long that isn't planned first.
fn planning_manifest(manifest: &str) -> String {
    format!(
        "{manifest}
[[contributes.commands]]
name = \"keep\"
title = \"Keep a copy\"
mode = \"headless\"
where = [\"collection\"]
plan = true
long_running = true
params = {{ type = \"object\" }}

[[contributes.commands]]
name = \"slow\"
title = \"Take a while\"
mode = \"headless\"
long_running = true
params = {{ type = \"object\" }}
"
    )
}

impl Fixture {
    /// The echo plugin with `keep` and `slow`, hosted as `options` say.
    fn planning(options: Options) -> Fixture {
        Fixture::new(|dir| vec![spec(dir, &planning_manifest(ECHO), &[])], options)
    }

    /// `keep` planned with `params`, about the collection: no drive.
    fn plan_keep(&self, params: Value) -> Result<CommandPlanned> {
        self.plugins.plan_command("echo", "keep", params, Context::default(), Some(&self.store), &Ctx::silent())
    }

    /// A file in the fixture's folder.
    fn file(&self, name: &str) -> PathBuf {
        self._dir.path().join(name)
    }
}

/// `params` planning to write each of `files`.
fn writing(files: &[&Path]) -> Value {
    let files: Vec<Value> = files.iter().map(|f| json!({ "path": f, "what": "a copy", "bytes": 7 })).collect();
    json!({ "plan": { "files": files } })
}

/// The request a dry run was, as the test plugin says it in its plan's summary.
fn dry_run_of(plan: &Planned<PluginCommand>) -> RunCommand {
    serde_json::from_str(&plan.summary[0]).expect("the test plugin's summary is the request")
}

/// The request a planned command ran with, as the test plugin answers it.
fn ran(output: CommandOutput) -> RunCommand {
    let CommandOutput::Headless(answer) = output else { panic!("a headless command: {output:?}") };
    serde_json::from_value(answer["request"].clone()).unwrap()
}

/// `run` without its scratch folder, which each request has one of its own.
fn unscratched(mut run: RunCommand) -> RunCommand {
    run.context.scratch = None;
    run
}

/// A command planned first is planned with a dry run: its plan leads with the plugin's summary
/// and says the files it would write. Applied, it runs the request its dry run was, without
/// `dry_run`, and writes them.
#[test]
fn a_planned_command_is_planned_then_run_as_it_was_planned() {
    let f = Fixture::planning(Options::default());
    let out = f.file("copy.txt");
    let plan = f.plan_keep(writing(&[&out])).unwrap().plan().unwrap();
    assert_eq!(plan.operation, "echo.command.keep");
    let dry = dry_run_of(&plan);
    assert!(dry.dry_run, "{dry:?}");
    assert_eq!(dry.context.drive, None, "the collection's: no drive");
    assert_eq!(plan.summary[1..], ["write 1 file(s) (7 B)".to_string()]);
    assert_eq!((plan.destructive, plan.files.len(), plan.files[0].exists), (false, 1, false));
    assert!(!out.exists(), "a dry run writes nothing");
    let run = ran(plan.apply(Some(&f.store), &Ctx::silent()).unwrap());
    assert_eq!(unscratched(run), RunCommand { dry_run: false, ..unscratched(dry) });
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "written");
}

/// A command about the collection alone runs on no drive, whichever the caller names: planned or
/// run, its context has none. One offered on a drive too keeps it.
#[test]
fn a_command_about_the_collection_alone_runs_on_no_drive() {
    let manifest = planning_manifest(ECHO).replace(
        "name = \"slow\"\ntitle = \"Take a while\"\nmode = \"headless\"\n",
        "name = \"slow\"\ntitle = \"Take a while\"\nmode = \"headless\"\nwhere = [\"collection\"]\n",
    );
    assert_ne!(manifest, planning_manifest(ECHO));
    let f = Fixture::new(|dir| vec![spec(dir, &manifest, &[])], Options::default());
    let planned = f.plugins.plan_command("echo", "keep", json!({}), f.context(), None, &Ctx::silent());
    assert_eq!(dry_run_of(&planned.unwrap().plan().unwrap()).context.drive, None);
    let context_of = |command: &str| {
        let params = json!({ "context": true });
        let ran = f.plugins.run_command("echo", command, params, f.context(), None, &Ctx::silent(), &|_| {});
        let CommandOutput::Headless(context) = ran.unwrap() else { panic!("headless") };
        context
    };
    assert_eq!(context_of("slow").get("drive"), None);
    assert!(context_of("echo")["drive"].is_string(), "on a drive, it keeps it");
}

/// A command planned first isn't run unplanned, and one that isn't isn't planned.
#[test]
fn a_planned_command_runs_only_once_planned() {
    let f = Fixture::planning(Options::default());
    let e = f.plugins.run_command("echo", "keep", json!({}), Context::default(), None, &Ctx::silent(), &|_| {});
    assert_eq!(e.unwrap_err().to_string(), "keep is planned first: plan it, then apply the plan");
    let e = f.plugins.plan_command("echo", "echo", json!({}), f.context(), None, &Ctx::silent()).unwrap_err();
    assert_eq!(e.to_string(), "echo isn't planned first: run it");
}

/// A plan overwriting a file there already is destructive, saying how many; and once a file it
/// writes changes (or one it would make is there), applying it is refused as stale, running
/// nothing.
#[test]
fn overwriting_is_destructive_and_a_changed_file_makes_the_plan_stale() {
    let f = Fixture::planning(Options::default());
    let (there, new) = (f.file("there.txt"), f.file("new.txt"));
    std::fs::write(&there, "old").unwrap();
    let plan = f.plan_keep(writing(&[&there, &new])).unwrap().plan().unwrap();
    assert!(plan.destructive);
    assert_eq!(plan.summary[1..], ["write 2 file(s) (14 B)".to_string(), "overwrite 1 of them".to_string()]);
    std::fs::write(&there, "changed since").unwrap();
    let e = plan.apply(Some(&f.store), &Ctx::silent()).unwrap_err();
    assert_eq!(e.code(), "stale_plan", "{e}");
    assert!(!new.exists(), "nothing ran");
    let plan = f.plan_keep(writing(&[&new])).unwrap().plan().unwrap();
    std::fs::write(&new, "made since").unwrap();
    assert_eq!(plan.apply(Some(&f.store), &Ctx::silent()).unwrap_err().code(), "stale_plan");
}

/// A dry run that fails, or answers what isn't a plan (one writing a relative path, say), is
/// refused, and nothing is run or written.
#[test]
fn a_plan_that_fails_or_isnt_one_is_refused() {
    let f = Fixture::planning(Options::default());
    let out = f.file("copy.txt");
    let mut failing = writing(&[&out]);
    failing["fail"] = json!("conflict");
    assert_eq!(f.plan_keep(failing).unwrap_err().code(), "conflict");
    let relative = failure(f.plan_keep(json!({ "plan": { "files": [{ "path": "copy.txt" }] } })).unwrap_err());
    assert!(relative.contains("it would write copy.txt, which isn't an absolute path"), "{relative}");
    let not_one = failure(f.plan_keep(json!({ "plan": { "summary": 3 } })).unwrap_err());
    assert!(not_one.contains("its plan for keep isn't one"), "{not_one}");
    assert!(!out.exists());
}

/// A planned command may ask a question first, in its dry run: answered, it's planned with the
/// answer, and runs with it.
#[test]
fn a_planned_command_asks_first_in_its_dry_run() {
    let f = Fixture::planning(Options::default());
    let item = json!({ "label": "Monday", "value": "monday" });
    let params =
        json!({ "plan": { "choose": { "title": "Which backup?", "param": "backup", "items": [item.clone()] } } });
    let CommandPlanned::Choose { choose } = f.plan_keep(params.clone()).unwrap() else { panic!("a question") };
    assert_eq!(choose.title, "Which backup?");
    let answered = choose.answered(params, vec![vec![json!("monday")]]);
    let plan = f.plan_keep(answered).unwrap().plan().unwrap();
    assert_eq!(ran(plan.apply(Some(&f.store), &Ctx::silent()).unwrap()).params["backup"], "monday");
    // A caller that can't ask says how to answer it instead.
    let e = f.plan_keep(json!({ "plan": { "choose": { "title": "Which?", "param": "b", "items": [item] } } }));
    assert_eq!(e.unwrap().plan().unwrap_err().to_string(), "Which?: it asks which first, as `b` in its params");
}

/// Cancelled, a long-running command has `long_grace` to stop before the plugin is killed, and any
/// other `grace`; a planned one stopping as asked while it runs leaves the plugin running.
#[test]
fn a_long_running_command_has_longer_to_stop() {
    let options =
        Options { grace: Duration::from_millis(100), long_grace: Duration::from_millis(1500), ..Options::default() };
    let f = Fixture::planning(options);
    let stuck_for = |command: &str| {
        let cancel = locallibrary::CancelToken::new();
        let cancelling = cancel.clone();
        let asked = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            cancelling.cancel();
            Instant::now()
        });
        let ctx = Ctx::new(&locallibrary::progress::NoProgress, cancel);
        let e = f.plugins.run_command("echo", command, json!({ "stuck": true }), f.context(), None, &ctx, &|_| {});
        let waited = asked.join().unwrap().elapsed();
        assert!(matches!(e, Err(Error::Cancelled)), "{e:?}");
        assert!(!f.plugins.is_running("echo"), "killed");
        waited
    };
    let long = stuck_for("slow");
    assert!(long >= Duration::from_millis(1500), "{long:?}");
    let other = stuck_for("echo");
    assert!(other < Duration::from_millis(1500), "{other:?}");
    assert!(f.logs.lock().unwrap().iter().any(|(_, line)| line.starts_with("killed: it didn't stop within 1.5s")));
    let plan = f.plan_keep(json!({ "wait": true })).unwrap().plan().unwrap();
    let cancel = locallibrary::CancelToken::new();
    let cancelling = cancel.clone();
    let asked = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        cancelling.cancel();
    });
    let e = plan.apply(Some(&f.store), &Ctx::new(&locallibrary::progress::NoProgress, cancel));
    asked.join().unwrap();
    assert!(matches!(e, Err(Error::Cancelled)), "{e:?}");
    assert!(f.plugins.is_running("echo"), "it stopped as asked");
}

/// With a store, each hook is given the plugin's folder for what it keeps, made first and kept
/// once the plugin stops; without one, none.
#[test]
fn a_hook_has_the_plugins_folder_with_a_store() {
    let f = Fixture::echo();
    let context = |store: Option<&Store>| {
        let ran = f.plugins.run_command(
            "echo",
            "echo",
            json!({ "context": true }),
            f.context(),
            store,
            &Ctx::silent(),
            &|_| {},
        );
        let CommandOutput::Headless(context) = ran.unwrap() else { panic!("headless") };
        context
    };
    let data = f.store.root().join("plugins").join("echo").join("data");
    assert!(!data.exists());
    assert_eq!(context(Some(&f.store))["data_dir"], json!(data));
    assert!(data.is_dir(), "made first");
    std::fs::write(data.join("kept"), "x").unwrap();
    f.plugins.shutdown();
    assert_eq!(context(Some(&f.store))["data_dir"], json!(data));
    assert!(data.join("kept").exists(), "kept once it stopped");
    f.plugins.shutdown();
    assert_eq!(context(None).get("data_dir"), None);
}

/// A plugin without the `plans` feature whose manifest has a command planned first is refused: it
/// would run one as asked.
#[test]
fn a_plugin_that_doesnt_plan_first_is_refused() {
    let f = Fixture::new(|dir| vec![spec(dir, &planning_manifest(ECHO), &["--without", "plans"])], Options::default());
    let e = failure(f.plan_keep(json!({})).unwrap_err());
    assert!(e.contains("it says it doesn't have the plans feature, which its manifest needs"), "{e}");
}

/// A plugin suggesting one of its own commands is told to the user with it; one it doesn't have,
/// or with params it doesn't take, is refused, and the line told alone.
#[test]
fn a_plugin_suggests_one_of_its_commands() {
    let manifest = planning_manifest(&listening(&["plan.applied"]));
    let (f, notices) = told(|dir| vec![spec(dir, &manifest, &[])], Options::default());
    let keep = HostAction::Command(CommandAction { command: "keep".into(), params: Default::default() });
    let nope = HostAction::Command(CommandAction { command: "backup".into(), params: Default::default() });
    let wrong =
        json!({ "message": "wrong", "action": { "name": "command", "params": { "command": "keep", "params": 1 } } });
    let tells = [
        serde_json::to_value(HostNotify::action("back up?", &keep)).unwrap(),
        serde_json::to_value(HostNotify::action("nope", &nope)).unwrap(),
        wrong,
    ];
    for tell in tells {
        let event = Event { name: EventName::PlanApplied, drive: None, payload: json!({ "tell": tell }) };
        f.plugins.deliver(&event, Some(&f.store));
    }
    eventually("it tells the user", || notices.lock().unwrap().len() == 3);
    let said: Vec<(String, Option<Suggestion>)> =
        notices.lock().unwrap().iter().map(|n| (n.message.clone(), n.suggests.clone())).collect();
    let expected =
        [("back up?".to_string(), Some(Suggestion::Action(keep))), ("nope".into(), None), ("wrong".into(), None)];
    assert_eq!(said, expected);
    let logs = f.logs.lock().unwrap().clone();
    assert!(logs.iter().any(|(_, l)| l.contains("no command \"backup\"")), "{logs:?}");
}

/// For any params a planned command is planned with, the plan shown is the one the plugin
/// answered, and applying it runs the request its dry run was, but for `dry_run` (each request
/// has a scratch folder of its own), with the same params.
#[hegel::test(test_cases = 15, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_planned_command_runs_with_what_it_was_planned_with(tc: TestCase) {
    let f = Fixture::planning(Options::default());
    // Named apart from what the test plugin reads in its params (`plan`, `fail`, `exit`...).
    let key = || gs::from_regex("x_[a-z]{1,6}").fullmatch(true);
    // Printable: the host makes what a plugin tells it so.
    let value = || {
        gs::one_of(vec![
            gs::integers::<i64>().map(Value::from).boxed(),
            gs::from_regex("[ -~é]{0,10}").fullmatch(true).map(Value::from).boxed(),
            gs::booleans().map(Value::from).boxed(),
        ])
    };
    let mut params = serde_json::Map::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        params.insert(tc.draw(key()), tc.draw(value()));
    }
    let warnings: Vec<String> = tc.draw(gs::vecs(gs::from_regex("[a-z ]{1,12}").fullmatch(true)).max_size(3));
    params.insert("plan".into(), json!({ "warnings": warnings }));
    let params = Value::Object(params);
    let plan = f.plan_keep(params.clone()).unwrap().plan().unwrap();
    for w in &warnings {
        assert!(plan.warnings.contains(w), "{w:?} in {:?}", plan.warnings);
    }
    let dry = dry_run_of(&plan);
    assert_eq!(dry.params, params);
    let run = ran(plan.apply(Some(&f.store), &Ctx::silent()).unwrap());
    assert_eq!(unscratched(run), RunCommand { dry_run: false, ..unscratched(dry) });
}
