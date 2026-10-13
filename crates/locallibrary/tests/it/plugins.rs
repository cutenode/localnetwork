//! The plugin host, running the test plugin (`examples/test-plugin`: loclib-plugin's echo
//! example, and what these tests need besides) as a child process, on a fixture drive.

mod callbacks;
mod hooks;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use locallibrary::api::RpcFailure;
use locallibrary::api::{self, LibraryCache};
use locallibrary::plugin::{CommandOutput, Launch, Options, PluginSpec, Plugins, Reads};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::{Error, Result};
use loclib_plugin::{Context, View};
use serde_json::{Value, json};

use crate::common;

/// The echo plugin's manifest: the test plugin serves it unless told otherwise.
const ECHO: &str = include_str!("../../../loclib-plugin/examples/echo/locallibrary-plugin.toml");

/// The test plugin, as cargo builds it with the tests.
fn program() -> PathBuf {
    let deps = std::env::current_exe().unwrap();
    let examples = deps.parent().unwrap().parent().unwrap().join("examples");
    let exe = examples.join(format!("test-plugin{}", std::env::consts::EXE_SUFFIX));
    assert!(exe.exists(), "cargo builds the examples with the tests: {}", exe.display());
    exe
}

/// The test plugin with `args`, serving `manifest` (the echo plugin's unless it's another,
/// written to `dir` for it to read).
fn spec(dir: &Path, manifest: &str, args: &[&str]) -> PluginSpec {
    let mut args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    if manifest != ECHO {
        static WRITTEN: AtomicUsize = AtomicUsize::new(0);
        let file = dir.join(format!("manifest-{}.toml", WRITTEN.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&file, manifest).unwrap();
        args.extend(["--manifest".into(), file.display().to_string()]);
    }
    PluginSpec::first_party(manifest, Launch { program: program(), args: args.into_iter().map(Into::into).collect() })
        .unwrap()
}

/// The echo manifest, granting `capabilities` besides its own.
fn granting(capabilities: &[&str]) -> String {
    let more: String = capabilities.iter().map(|c| format!(", {c:?}")).collect();
    ECHO.replace(
        r#"capabilities = ["library:read", "library:plan""#,
        &format!(r#"capabilities = ["library:read", "library:plan"{more}"#),
    )
}

/// What the app reads for the plugins here: tracks from the drive, a failure for
/// `track.matches`, a wait for `track.show` until the test says, a panic for `playlists.tree`, and
/// for any other read what the host asked (the method, and the params as it passed them on).
struct TestReads {
    volumes: PathBuf,
    store: PathBuf,
    entered: Mutex<Sender<()>>,
    release: Mutex<Receiver<()>>,
}

impl TestReads {
    /// Drive lookups on this (the callback's) thread see the test's drives and store.
    fn isolate(&self) {
        locallibrary::testing::volumes_on_this_thread(Some(vec![self.volumes.clone()]));
        locallibrary::testing::store_root_on_this_thread(Some(self.store.clone()));
    }
}

impl Reads for TestReads {
    fn read(&self, method: &str, params: Value, _: &Ctx<'_>) -> Result<Value, RpcFailure> {
        self.isolate();
        match method {
            "tracks.query" => {
                let req = serde_json::from_value(params).unwrap();
                Ok(serde_json::to_value(api::tracks_query_cached(LibraryCache::none(), &req)?).unwrap())
            }
            "track.matches" => {
                let path = self.volumes.join("USB/PIONEER/rekordbox/export.pdb");
                let e = Error::Io { path, source: std::io::Error::other("unreadable") };
                Err(e.action(locallibrary::error::Action::elevate("Run it as an administrator")).into())
            }
            "track.show" => {
                self.entered.lock().unwrap().send(()).unwrap();
                // A call let through that the test meant to be refused fails, rather than waits.
                match self.release.lock().unwrap().recv_timeout(Duration::from_secs(5)) {
                    Ok(()) => Ok(json!("shown")),
                    Err(_) => Err(Error::Invalid("never let through".into()).into()),
                }
            }
            "playlists.tree" => panic!("a read that panics"),
            _ => Ok(json!({ "method": method, "params": params })),
        }
    }

    fn drive(&self, spec: &str) -> Result<PathBuf> {
        self.isolate();
        Ok(api::resolve_known_drive(spec)?.root().to_path_buf())
    }
}

/// A store, a fixture drive (`USB`, adopted) and the plugins `specs` makes.
struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
    drive: PathBuf,
    plugins: Plugins,
    /// What the plugins logged, with their ids.
    logs: Arc<Mutex<Vec<(String, String)>>>,
    /// A `track.show` callback has started.
    entered: Mutex<Receiver<()>>,
    /// Lets a `track.show` callback finish.
    release: Sender<()>,
}

impl Fixture {
    fn new(specs: impl FnOnce(&Path) -> Vec<PluginSpec>, options: Options) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let store_root = dir.path().join("store");
        let store = Store::open_or_init(&store_root).unwrap();
        let volumes = common::isolate(dir.path(), Some(&store_root));
        let drive = common::demo_drive(&volumes, "USB");
        common::adopt(Some(&store), &drive, "USB");
        let ((entered_tx, entered), (release, release_rx)) = (mpsc::channel(), mpsc::channel());
        let reads =
            TestReads { volumes, store: store_root, entered: Mutex::new(entered_tx), release: Mutex::new(release_rx) };
        let logs = Arc::new(Mutex::new(Vec::new()));
        let logged = logs.clone();
        let log: locallibrary::plugin::Log =
            Arc::new(move |id, line| logged.lock().unwrap().push((id.into(), line.into())));
        let plugins = Plugins::new(specs(dir.path()), Arc::new(reads), Options { log, ..options }).unwrap();
        Fixture { _dir: dir, store, drive, plugins, logs, entered: Mutex::new(entered), release }
    }

    /// The echo plugin, as the test plugin.
    fn echo() -> Fixture {
        Fixture::new(|dir| vec![spec(dir, ECHO, &[])], Options::default())
    }

    /// The user is on the drive.
    fn context(&self) -> Context {
        Context { drive: Some("USB".into()), ..Context::default() }
    }

    /// Runs the echo plugin's command `command`.
    fn command(&self, command: &str, params: Value, ctx: &Ctx<'_>) -> Result<CommandOutput> {
        self.plugins.run_command("echo", command, params, self.context(), Some(&self.store), ctx)
    }

    /// What the echo command answers with `params`.
    fn echo_params(&self, params: Value) -> Result<Value> {
        match self.command("echo", params, &Ctx::silent())? {
            CommandOutput::Headless(v) => Ok(v),
            view => panic!("a headless command answers JSON: {view:?}"),
        }
    }
}

/// Waits (up to ten seconds) until `done`.
fn eventually(what: &str, done: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < until, "{what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The message of an error a plugin caused.
fn failure(e: Error) -> String {
    assert_eq!(e.code(), "external", "{e}");
    e.to_string()
}

#[test]
fn a_plugin_starts_when_first_used_and_answers() {
    let f = Fixture::echo();
    assert!(!f.plugins.is_running("echo"));
    assert_eq!(f.echo_params(json!({ "n": 1 })).unwrap(), json!({ "n": 1 }));
    assert!(f.plugins.is_running("echo"));
    // A view command calls back for the drive's tracks, by the drive's id.
    let titles: Vec<Vec<Value>> = common::library(&f.drive).tracks.iter().map(|t| vec![json!(t.title)]).collect();
    let CommandOutput::View(View::Table { title, columns, rows }) =
        f.command("titles", Value::Null, &Ctx::silent()).unwrap()
    else {
        panic!("a table")
    };
    let id = locallibrary::Device::open(&f.drive).unwrap().id().unwrap().to_string();
    assert_eq!((title, columns, rows), (Some(id), vec!["Title".to_string()], titles));
    let e = f.command("echo", json!([1]), &Ctx::silent()).unwrap_err();
    assert_eq!(e.code(), "invalid", "params are checked against the command's schema: {e}");
    let e = f.command("nope", Value::Null, &Ctx::silent()).unwrap_err();
    assert_eq!(e.code(), "not_found");
    assert_eq!(
        f.plugins
            .run_command("nope", "echo", Value::Null, Context::default(), None, &Ctx::silent())
            .unwrap_err()
            .code(),
        "not_found"
    );
}

#[test]
fn only_plugins_locallibrary_ships_are_hosted() {
    let dir = tempfile::tempdir().unwrap();
    let new = |specs: Vec<PluginSpec>| match Plugins::new(specs, Arc::new(NoReads), Options::default()) {
        Ok(_) => String::new(),
        Err(e) => e.to_string(),
    };
    let echo = spec(dir.path(), ECHO, &[]);
    assert_eq!(new(vec![echo.clone()]), "");
    let third = PluginSpec { first_party: false, ..echo.clone() };
    assert_eq!(new(vec![third]), "echo: third-party plugins aren't supported yet");
    assert_eq!(new(vec![echo.clone(), echo.clone()]), "two plugins are called \"echo\"");
    let cli = |id: &str| {
        let toml = ECHO.replace("id = \"echo\"", &format!("id = \"{id}\"")).replace(
            "name = \"echo\"\ntitle = \"Answer the params\"",
            "name = \"echo\"\ntitle = \"Answer the params\"\ncli = \"say\"",
        );
        spec(dir.path(), &toml, &[])
    };
    assert_eq!(new(vec![cli("one"), cli("two")]), "the two plugin's command \"say\" is another plugin's too");
    let hosted = Plugins::new(vec![cli("one")], Arc::new(NoReads), Options::default()).unwrap();
    assert_eq!(hosted.cli_command("say").map(|(id, c)| (id, c.name.as_str())), Some(("one", "echo")));
    assert!(hosted.cli_command("echo").is_none());
    let secret = ECHO.replace("type = \"string\"\ndefault = \"hello\"", "type = \"secret\"");
    let secret = PluginSpec { manifest: loclib_plugin::Manifest::parse(&secret).unwrap(), ..echo.clone() };
    assert_eq!(
        new(vec![secret]),
        "the echo plugin's setting \"greeting\" is a secret, which locallibrary can't keep yet"
    );
    let mut invalid = echo;
    invalid.manifest.name = " ".into();
    assert_eq!(new(vec![invalid]), "the echo plugin's manifest: name: is empty");
    let e = PluginSpec::first_party("id = 1", launch_nothing()).unwrap_err().to_string();
    assert!(e.starts_with("a plugin's manifest: "), "{e}");
    let e = PluginSpec::first_party(&ECHO.replace("name = \"Echo\"", "name = \"\""), launch_nothing()).unwrap_err();
    assert_eq!(e.to_string(), "the echo plugin's manifest: name: is empty");
}

fn launch_nothing() -> Launch {
    Launch { program: "nothing".into(), args: Vec::new() }
}

/// Reads for plugins that never call back.
struct NoReads;

impl Reads for NoReads {
    fn read(&self, method: &str, _: Value, _: &Ctx<'_>) -> Result<Value, RpcFailure> {
        panic!("nothing calls back here: {method}")
    }
}

#[test]
fn a_plugin_must_speak_the_protocol_and_be_what_its_manifest_says() {
    let f = Fixture::new(
        |dir| {
            let other =
                ECHO.replace("id = \"echo\"", "id = \"otherwise\"").replace("name = \"Echo\"", "name = \"Echoes\"");
            let mut described_otherwise = spec(dir, &other, &[]);
            described_otherwise.manifest.name = "Echo".into();
            let mut another = spec(dir, ECHO, &["--protocol", "9.0.0"]);
            another.manifest.id = "old".into();
            let mut stranger = spec(dir, ECHO, &[]);
            stranger.manifest.id = "stranger".into();
            let mut missing = spec(dir, ECHO, &[]);
            (missing.manifest.id, missing.launch.program) = ("missing".into(), dir.join("no-such-program"));
            vec![described_otherwise, another, stranger, missing]
        },
        Options::default(),
    );
    let run = |id: &str| {
        let e = f.plugins.run_command(id, "echo", json!({}), Context::default(), None, &Ctx::silent()).unwrap_err();
        failure(e)
    };
    assert_eq!(run("old"), "the Echo plugin failed: protocol \"9.0.0\" isn't compatible with 0.1.0");
    assert_eq!(run("stranger"), "the Echo plugin failed: it says it's echo 0.1.0, not stranger 0.1.0");
    assert_eq!(
        run("otherwise"),
        "the Echo plugin failed: it describes itself otherwise than the manifest locallibrary has for it"
    );
    assert!(run("missing").starts_with("the Echo plugin failed: it couldn't be started: "));
    assert!(!f.plugins.is_running("otherwise"), "a plugin refused isn't kept running");
}

#[test]
fn an_elevated_process_starts_no_plugin() {
    let f = Fixture::echo();
    locallibrary::testing::elevated_on_this_thread(Some(true));
    let e = f.echo_params(json!({})).unwrap_err();
    locallibrary::testing::elevated_on_this_thread(None);
    assert_eq!(e.code(), "invalid");
    assert!(e.to_string().contains("isn't started from a process running as an administrator (or root)"), "{e}");
    assert!(!f.plugins.is_running("echo"));
    assert!(f.echo_params(json!({})).is_ok());
}

#[test]
fn a_plugin_crashing_starts_again_on_next_use() {
    let f = Fixture::echo();
    assert_eq!(
        failure(f.echo_params(json!({ "exit": 3 })).unwrap_err()),
        "the Echo plugin failed: it stopped unexpectedly"
    );
    assert!(!f.plugins.is_running("echo"));
    assert_eq!(f.echo_params(json!({ "again": true })).unwrap(), json!({ "again": true }));
}

#[test]
fn an_idle_plugin_is_stopped() {
    let options = Options { idle: Duration::from_secs(1), ..Options::default() };
    let f = Fixture::new(|dir| vec![spec(dir, ECHO, &[])], options);
    f.echo_params(json!({})).unwrap();
    assert!(f.plugins.is_running("echo"));
    eventually("an idle plugin is stopped", || !f.plugins.is_running("echo"));
    f.echo_params(json!({})).unwrap();
    assert!(f.plugins.is_running("echo"), "and started again when it's used");
}

#[test]
fn a_plugins_log_reaches_the_host() {
    let f = Fixture::echo();
    // What would draw on a terminal is left out.
    f.echo_params(json!({ "log": "\u{1b}[1;31mtuning\u{1b}[0m \u{1b}]0;title\u{7}up\u{7}" })).unwrap();
    eventually("its line is logged", || f.logs.lock().unwrap().contains(&("echo".into(), "tuning up".into())));
}

/// Progress events, with their drives.
#[derive(Default)]
struct Recorded(Mutex<Vec<(Option<String>, locallibrary::Event)>>);

impl locallibrary::Progress for Recorded {
    fn on_event(&self, event: &locallibrary::Event) {
        self.0.lock().unwrap().push((None, event.clone()));
    }

    fn on_scoped_event(&self, scope: &locallibrary::progress::Scope, event: &locallibrary::Event) {
        self.0.lock().unwrap().push((scope.target.clone(), event.clone()));
    }

    fn on_step(&self, scope: &locallibrary::progress::Scope) {
        panic!("a plugin's steps aren't passed on: {scope:?}");
    }
}

#[test]
fn a_plugins_progress_reaches_the_caller() {
    let f = Fixture::echo();
    let recorded = Recorded::default();
    f.command("steps", Value::Null, &Ctx::new(&recorded, Default::default())).unwrap();
    let items: Vec<Option<String>> = (recorded.0.lock().unwrap().iter())
        .map(|(target, e)| match e {
            locallibrary::Event::Item { .. } => target.clone(),
            other => panic!("only items: {other:?}"),
        })
        .collect();
    // Coalesced by the plugin (into one item standing for three), each about its drive.
    assert_eq!(items, vec![Some("USB".to_string()); 3]);
}

#[test]
fn cancelling_stops_a_plugins_work() {
    let f = Fixture::echo();
    let cancel = locallibrary::CancelToken::new();
    let started = cancel.clone();
    let on_event = move |_: &locallibrary::Event| started.cancel();
    let e = f.command("wait", Value::Null, &Ctx::new(&on_event, cancel)).unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    assert!(f.plugins.is_running("echo"), "it stopped as asked: it's still running");
}

#[test]
fn a_plugin_that_doesnt_stop_when_asked_is_killed() {
    let options = Options { grace: Duration::from_millis(200), ..Options::default() };
    let f = Fixture::new(|dir| vec![spec(dir, ECHO, &[])], options);
    let cancel = locallibrary::CancelToken::new();
    let cancelling = cancel.clone();
    let asked = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        cancelling.cancel();
    });
    let e = f.plugins.run_command(
        "echo",
        "echo",
        json!({ "stuck": true }),
        f.context(),
        None,
        &Ctx::new(&locallibrary::progress::NoProgress, cancel),
    );
    asked.join().unwrap();
    assert!(matches!(e, Err(Error::Cancelled)), "{e:?}");
    assert!(!f.plugins.is_running("echo"));
    assert!(f.logs.lock().unwrap().iter().any(|(_, line)| line.starts_with("killed: it didn't stop within 200ms")));
    assert_eq!(f.echo_params(json!({ "after": 1 })).unwrap(), json!({ "after": 1 }));
    // Cancelled before it started, nothing is asked.
    let cancelled = locallibrary::CancelToken::new();
    cancelled.cancel();
    let e = f.command("echo", json!({}), &Ctx::new(&locallibrary::progress::NoProgress, cancelled)).unwrap_err();
    assert!(matches!(e, Error::Cancelled));
}

#[test]
fn a_plugin_that_doesnt_shake_hands_in_time_is_stopped() {
    let options = Options { handshake: Duration::from_millis(500), ..Options::default() };
    let f = Fixture::new(|dir| vec![spec(dir, ECHO, &["--silent"])], options);
    let asked = Instant::now();
    let e = failure(f.echo_params(json!({})).unwrap_err());
    assert!(asked.elapsed() < Duration::from_secs(4), "its own time, not the grace: {:?}", asked.elapsed());
    assert_eq!(e, "the Echo plugin failed: it didn't answer hello in time");
    assert!(!f.plugins.is_running("echo"));
}

#[test]
fn shutting_down_stops_every_plugin() {
    let f = Fixture::new(
        |dir| vec![spec(dir, ECHO, &[]), spec(dir, &ECHO.replace("id = \"echo\"", "id = \"other\""), &[])],
        Options::default(),
    );
    for id in ["echo", "other"] {
        f.plugins.run_command(id, "echo", json!({}), Context::default(), None, &Ctx::silent()).unwrap();
    }
    assert!(f.plugins.is_running("echo") && f.plugins.is_running("other"));
    f.plugins.shutdown();
    assert!(!f.plugins.is_running("echo") && !f.plugins.is_running("other"));
    assert_eq!(f.echo_params(json!({ "after": 1 })).unwrap(), json!({ "after": 1 }), "used again, it starts");
}
