//! Commands planned first (`plan = true`): a dry run answers the plan, and the host runs the same
//! request once the user confirms; commands that run long; the collection as a place; what a
//! command's context says of the plugin's own folder; and suggesting a command (`host/notify`).

use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};

use serde_json::Map;

use super::*;
use crate::{
    Choice, ChoiceItem, CommandAction, CommandPlan, CommandPlanAnswer, Context, HostAction, HostNotify, Suggested,
    Suggestion, Where,
};

/// The echo plugin's manifest, with `keep`: a headless command about the collection, planned first
/// and running long, writing a file it's given.
fn with_keep() -> String {
    let keep = r#"
[[contributes.commands]]
name = "keep"
title = "Keep a copy"
mode = "headless"
where = ["collection"]
plan = true
long_running = true
params = { type = "object", properties = { to = { type = "string", "x-path" = true } }, required = ["to"] }
"#;
    format!("{}{keep}", echo::MANIFEST)
}

/// Echo's hooks, `keep` planning to write its `to` (the plan `plan` makes of the request), then
/// answering the request it ran with; each request it's sent goes to `seen`.
fn keeping(seen: Sender<RunCommand>, plan: fn(&RunCommand) -> CommandPlan) -> Hooks {
    let mut hooks = echo::hooks();
    hooks.hooks.remove(method::RUN_COMMAND);
    hooks.on(move |req: RunCommand, host: &Host| -> Result<Value, RpcError> {
        seen.send(req.clone()).unwrap();
        match (req.command.as_str(), req.dry_run) {
            ("keep", true) => Ok(plan(&req).into()),
            ("keep", false) => Ok(json!({ "ran": req })),
            _ => echo::run_command(req, host),
        }
    })
}

/// What `keep` plans: writing its `to`.
fn writes_to(req: &RunCommand) -> CommandPlan {
    let to = PathBuf::from(req.params["to"].as_str().unwrap_or_default());
    let file = ExportedFile { path: to.clone(), what: "a copy".into(), bytes: 12 };
    CommandPlan { summary: format!("keep a copy at {}", to.display()), warnings: Vec::new(), files: vec![file] }
}

/// Started on `manifest` with `hooks`, past a `hello` naming `features`.
fn greeted_with(manifest: String, hooks: Hooks, features: &[&str]) -> Session {
    let s = start_with(manifest, hooks);
    s.host.request::<_, HelloReply>(method::HELLO, &hello(PROTOCOL_VERSION, features)).unwrap();
    s
}

fn keep(to: &str, dry_run: bool) -> Value {
    json!({ "command": "keep", "params": { "to": to }, "dry_run": dry_run, "context": {} })
}

/// A host that plans commands first gets `keep`'s plan from its dry run, the files it would write
/// and all, and then runs the same request without `dry_run`.
#[test]
fn a_planned_command_answers_its_plan_then_runs_as_asked() {
    let (seen, requests) = channel();
    let s = greeted_with(with_keep(), keeping(seen, writes_to), &["plans"]);
    let planned: CommandPlanAnswer = serde_json::from_value(s.call(method::RUN_COMMAND, keep("/k", true)).unwrap())
        .expect("a dry run answers a plan");
    let file = ExportedFile { path: "/k".into(), what: "a copy".into(), bytes: 12 };
    let plan = CommandPlan { summary: "keep a copy at /k".into(), warnings: Vec::new(), files: vec![file] };
    assert_eq!(planned, CommandPlanAnswer::Plan(plan));
    let ran = s.call(method::RUN_COMMAND, keep("/k", false)).unwrap();
    let dry: RunCommand = requests.recv().unwrap();
    let run: RunCommand = requests.recv().unwrap();
    assert_eq!(serde_json::from_value::<RunCommand>(ran["ran"].clone()).unwrap(), run);
    assert_eq!(run, RunCommand { dry_run: false, ..dry });
    s.stop();
}

/// A host that doesn't say it plans commands first would run `keep` unasked: the SDK refuses it
/// there, as incompatible, before the hook runs. Echo's other commands run as they did.
#[test]
fn a_host_that_doesnt_plan_first_cant_run_a_planned_command() {
    let (seen, requests) = channel();
    let s = greeted_with(with_keep(), keeping(seen, writes_to), &["blobs"]);
    for dry_run in [false, true] {
        let e = s.call(method::RUN_COMMAND, keep("/k", dry_run)).unwrap_err();
        assert_eq!(e.kind(), Some("incompatible"), "{e:?}");
        assert!(e.message.contains("planned and confirmed first"), "{}", e.message);
    }
    assert!(requests.try_recv().is_err(), "the hook never ran");
    assert_eq!(s.call(method::RUN_COMMAND, run("echo", json!({ "n": 1 }), None)).unwrap(), json!({ "n": 1 }));
    s.stop();
}

/// A dry run is only for a command planned first: one of echo's own is refused as bad params.
#[test]
fn a_dry_run_of_a_command_not_planned_first_is_refused() {
    let (seen, _requests) = channel();
    let s = greeted_with(with_keep(), keeping(seen, writes_to), &["plans"]);
    let e = s.call(method::RUN_COMMAND, json!({ "command": "echo", "dry_run": true })).unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (code::INVALID_PARAMS, "invalid params: echo isn't planned first (plan = true)")
    );
    s.stop();
}

/// A plan's answer is read by its `choose` key: a question, or else the plan, whose own mistakes
/// are said as the plan's.
#[test]
fn a_plans_answer_is_a_question_or_the_plan() {
    let item = ChoiceItem { label: "Monday".into(), detail: None, value: json!("monday") };
    let choice =
        Choice { title: "Which backup?".into(), param: "backup".into(), multi: false, items: vec![item], more: vec![] };
    let asked = Value::from(CommandPlanAnswer::from(choice.clone()));
    assert_eq!(asked["choose"]["param"], "backup");
    assert_eq!(
        serde_json::from_value::<CommandPlanAnswer>(asked).unwrap(),
        CommandPlanAnswer::Choose { choose: choice }
    );
    let plan: CommandPlanAnswer = serde_json::from_value(json!({ "summary": "back up" })).unwrap();
    assert_eq!(plan, CommandPlanAnswer::Plan(CommandPlan { summary: "back up".into(), ..CommandPlan::default() }));
    let e = serde_json::from_value::<CommandPlanAnswer>(json!({ "files": [] })).unwrap_err();
    assert!(e.to_string().contains("summary"), "{e}");
}

/// A command says whether it's planned first, runs long and is the collection's; each left out is
/// false (and isn't written back), and an operation is never the collection's.
#[test]
fn commands_say_whether_they_plan_first_run_long_and_are_the_collections() {
    let m = Manifest::parse(&with_keep()).unwrap();
    m.validate(false).unwrap();
    let keep = m.contributes.commands.iter().find(|c| c.name == "keep").unwrap();
    assert_eq!((keep.plan, keep.long_running, keep.offered.as_slice()), (true, true, &[Where::Collection][..]));
    let echo = &m.contributes.commands[0];
    assert_eq!((echo.plan, echo.long_running), (false, false));
    let written = serde_json::to_value(echo).unwrap();
    assert_eq!((written.get("plan"), written.get("long_running")), (None, None));
    let collection = echo::MANIFEST.replace(r#"where = ["track", "playlist", "drive"]"#, r#"where = ["collection"]"#);
    assert_ne!(collection, echo::MANIFEST);
    let e = Manifest::parse(&collection).unwrap().validate(false).unwrap_err();
    assert_eq!(
        e.to_string(),
        "contributes.operations[0].where: an operation edits a drive, so it isn't the collection's"
    );
}

/// The plugin's own folder is in a context only when the host gives one, and reads back as it was.
#[test]
fn a_context_has_the_plugins_folder_when_the_host_gives_one() {
    let none: Context = serde_json::from_value(json!({ "drive": "USB" })).unwrap();
    assert_eq!(none.data_dir, None);
    assert_eq!(serde_json::to_value(&none).unwrap().get("data_dir"), None);
    let given: Context = serde_json::from_value(json!({ "data_dir": "/store/plugins/echo/data" })).unwrap();
    assert_eq!(given.data_dir, Some(PathBuf::from("/store/plugins/echo/data")));
}

/// `host/notify` suggesting one of the command's own, as the action `command`.
fn suggesting(command: &str, params: Value) -> HostNotify {
    let action = Suggested { name: "command".into(), params: json!({ "command": command, "params": params }) };
    HostNotify { message: "back up?".into(), operation: None, action: Some(action) }
}

/// A plugin may suggest one of its own commands, with params its schema takes: another plugin's
/// (or none), params it doesn't take and keys the action doesn't have are refused, saying why.
#[test]
fn a_plugin_suggests_one_of_its_own_commands() {
    let m = Manifest::parse(&with_keep()).unwrap();
    let params = Map::from_iter([("to".to_string(), json!("/k"))]);
    let keep = HostAction::Command(CommandAction { command: "keep".into(), params });
    assert_eq!(HostNotify::action("back up?", &keep).suggestion(&m), Ok(Some(Suggestion::Action(keep.clone()))));
    assert_eq!(suggesting("keep", json!({ "to": "/k" })).suggestion(&m), Ok(Some(Suggestion::Action(keep))));
    let refused = |notice: HostNotify| notice.suggestion(&m).unwrap_err();
    let which = "it suggests the action \"command\", which it can't: ";
    assert_eq!(refused(suggesting("backup", json!({}))), format!("{which}no command \"backup\""));
    assert!(refused(suggesting("keep", json!({}))).starts_with(&format!("{which}params: ")));
    assert!(refused(suggesting("keep", json!({ "to": 1 }))).starts_with(&format!("{which}params: ")));
    let extra = HostNotify {
        action: Some(Suggested { name: "command".into(), params: json!({ "command": "keep", "drive": "USB" }) }),
        ..suggesting("keep", json!({}))
    };
    assert!(refused(extra).contains("unknown field `drive`"));
    let action = HostAction::Command(CommandAction { command: "keep".into(), params: Map::new() });
    assert_eq!((action.drive(), action.fetch().is_none()), (None, true));
}

/// Any plan: a summary, a few warnings and up to three files it would write.
fn plan(tc: &TestCase) -> CommandPlan {
    let file = || {
        let path = tc.draw(gs::from_regex("(/[a-z0-9 ._-]{1,8}){1,3}").fullmatch(true));
        let what = tc.draw(gs::text().max_size(12));
        ExportedFile { path: path.into(), what, bytes: tc.draw(gs::integers::<u64>()) }
    };
    let n = tc.draw(gs::integers::<usize>().max_value(3));
    CommandPlan {
        summary: tc.draw(gs::text().max_size(40)),
        warnings: tc.draw(gs::vecs(gs::text().max_size(20)).max_size(3)),
        files: (0..n).map(|_| file()).collect(),
    }
}

/// Any plan is the answer it makes, and as JSON reads back as that answer.
#[hegel::test(test_cases = 50)]
fn prop_a_plan_is_its_own_answer_and_reads_back_as_one(tc: TestCase) {
    let plan = plan(&tc);
    assert_eq!(CommandPlanAnswer::from(plan.clone()), CommandPlanAnswer::Plan(plan.clone()));
    let read: CommandPlanAnswer = serde_json::from_value(Value::from(plan.clone())).unwrap();
    assert_eq!(read, CommandPlanAnswer::Plan(plan));
}

/// For any plan a planned command answers its dry run, the host reads back that plan, and the
/// run it then asks for is the dry run's request but for `dry_run`.
#[hegel::test(test_cases = 25)]
fn prop_any_plan_reads_back_and_its_run_differs_only_in_dry_run(tc: TestCase) {
    let plan = plan(&tc);
    let to = tc.draw(gs::from_regex("/[a-z]{1,6}").fullmatch(true));
    static PLANNED: std::sync::Mutex<Option<CommandPlan>> = std::sync::Mutex::new(None);
    *PLANNED.lock().unwrap() = Some(plan.clone());
    let (seen, requests) = channel();
    let s = greeted_with(with_keep(), keeping(seen, |_| PLANNED.lock().unwrap().clone().unwrap()), &["plans"]);
    let read: CommandPlanAnswer =
        serde_json::from_value(s.call(method::RUN_COMMAND, keep(&to, true)).unwrap()).unwrap();
    assert_eq!(read, CommandPlanAnswer::Plan(plan));
    s.call(method::RUN_COMMAND, keep(&to, false)).unwrap();
    let (dry, run): (RunCommand, RunCommand) = (requests.recv().unwrap(), requests.recv().unwrap());
    assert!(dry.dry_run && !run.dry_run);
    assert_eq!(run, RunCommand { dry_run: false, ..dry });
    s.stop();
}

/// For any command (planned first or not), dry run or not, and host (planning first or not), the
/// hook runs exactly when it's a dry run of a planned command or a run of any, and a planned one
/// only for a host that plans first.
#[hegel::test(test_cases = 50)]
fn prop_a_planned_command_runs_only_where_its_planned_first(tc: TestCase) {
    let planned = tc.draw(gs::booleans());
    let dry_run = tc.draw(gs::booleans());
    let plans_first = tc.draw(gs::booleans());
    let manifest = match planned {
        true => with_keep(),
        false => with_keep().replace("plan = true\n", ""),
    };
    let (seen, requests) = channel();
    let s = greeted_with(manifest, keeping(seen, writes_to), if plans_first { &["plans"] } else { &[] });
    let answered = s.call(method::RUN_COMMAND, keep("/k", dry_run));
    let runs = (!dry_run || planned) && (!planned || plans_first);
    assert_eq!(answered.is_ok(), runs, "{answered:?}");
    assert_eq!(requests.try_recv().is_ok(), runs);
    s.stop();
}

/// For any command named (echo's own or not) and params (fitting its schema or not), a suggestion
/// of it is taken exactly when it's one of the plugin's commands taking those params, and then as
/// it was sent.
#[hegel::test(test_cases = 100)]
fn prop_a_suggested_command_is_taken_only_when_its_one_the_plugin_has(tc: TestCase) {
    let m = Manifest::parse(&with_keep()).unwrap();
    let command = tc.draw(gs::sampled_from(vec!["keep", "echo", "titles", "backup", ""]));
    let to = tc.draw(gs::one_of(vec![
        gs::just(Value::Null).boxed(),
        gs::from_regex("/[a-z]{1,6}").fullmatch(true).map(Value::from).boxed(),
        gs::integers::<i64>().map(Value::from).boxed(),
    ]));
    let params: Map<String, Value> = match &to {
        Value::Null => Map::new(),
        to => Map::from_iter([("to".to_string(), to.clone())]),
    };
    let taken = suggesting(command, Value::Object(params.clone())).suggestion(&m);
    let fits = match command {
        "keep" => to.is_string(),
        // Neither says which params it takes: any object fits.
        "titles" | "echo" => true,
        _ => false,
    };
    match fits {
        true => {
            let action = HostAction::Command(CommandAction { command: command.into(), params });
            assert_eq!(taken, Ok(Some(Suggestion::Action(action))));
        }
        false => assert!(taken.is_err(), "{taken:?}"),
    }
}
