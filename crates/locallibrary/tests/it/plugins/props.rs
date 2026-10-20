//! Properties of the plugin host, running the test plugin as a child process: its lifecycle
//! against a model, its errors, its hooks (operations, exports, analyzers) and its settings and
//! registry. Spawning a process is slow, so each case reuses one host and draws few cases.

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::api::{LibraryRef, PluginFieldsRequest};
use locallibrary::fields::{FieldType, FieldValue};
use locallibrary::op::AnyPlanned;
use locallibrary::paths;
use locallibrary::progress::NoProgress;
use locallibrary::{CancelToken, Event};
use loclib_plugin::ExportSource;

use super::*;

/// What a step of the lifecycle model does.
#[derive(Debug, Clone, Copy)]
enum Life {
    /// Runs a command, which answers.
    Use,
    /// Runs one telling the plugin to write a file once it has stopped on its own.
    UseLeavingWord,
    /// Runs one the plugin exits during.
    Crash,
    /// Stops every plugin.
    Shutdown,
    /// Runs one from a thread that counts as elevated.
    Elevated,
}

/// For any sequence of uses, crashes, shutdowns and uses from an elevated thread, the plugin runs
/// exactly when a model says: it starts on use (but never from an elevated thread), stops when it
/// crashes or the host shuts down (exiting on its own, within its grace, when it can), and starts
/// again when used next.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_plugin_runs_exactly_when_the_model_says(tc: TestCase) {
    let f = Fixture::echo();
    let all = vec![Life::Use, Life::UseLeavingWord, Life::Crash, Life::Shutdown, Life::Elevated];
    let steps: Vec<Life> = tc.draw(gs::vecs(gs::sampled_from(all)).min_size(1).max_size(7));
    // Whether it runs, the file it writes once it stops on its own, and files it must never
    // write (it crashed, or was told another).
    let (mut running, mut word, mut never) = (false, None::<PathBuf>, Vec::new());
    for (i, step) in steps.into_iter().enumerate() {
        match step {
            Life::Use => {
                assert_eq!(f.echo_params(json!({ "n": i })).unwrap(), json!({ "n": i }));
                running = true;
            }
            Life::UseLeavingWord => {
                let file = f.store.root().join(format!("stopped-{i}"));
                f.echo_params(json!({ "stopped": file })).unwrap();
                never.extend(word.replace(file));
                running = true;
            }
            Life::Crash => {
                let e = f.echo_params(json!({ "exit": i % 3 + 1 })).unwrap_err();
                assert_eq!(failure(e), "the Echo plugin failed: it stopped unexpectedly");
                never.extend(word.take());
                running = false;
            }
            Life::Shutdown => {
                f.plugins.shutdown();
                if let Some(file) = word.take() {
                    assert!(file.exists(), "it stopped on its own, as asked, before shutting down returned");
                }
                running = false;
            }
            Life::Elevated => {
                locallibrary::testing::elevated_on_this_thread(Some(true));
                let used = f.echo_params(json!({ "n": i }));
                locallibrary::testing::elevated_on_this_thread(None);
                match running {
                    // Already running: nothing is started.
                    true => assert_eq!(used.unwrap(), json!({ "n": i })),
                    false => {
                        let e = used.unwrap_err();
                        assert_eq!(e.code(), "invalid", "{e}");
                        assert!(e.to_string().contains("isn't started from a process running as an administrator"));
                    }
                }
            }
        }
        assert_eq!(f.plugins.is_running("echo"), running, "after {step:?}");
    }
    f.plugins.shutdown();
    for file in never {
        assert!(!file.exists(), "{} is written only by a plugin that stops on its own", file.display());
    }
}

/// How a plugin fails to stop when asked.
#[derive(Debug, Clone, Copy)]
enum Stubborn {
    /// A hook that never answers, cancelled.
    Stuck,
    /// It takes far longer than its grace to exit once shut down.
    Lingering,
}

/// For any grace and any sequence of plugins that won't stop (a hook never answering once
/// cancelled, a plugin lingering at shutdown), each is killed once its grace is up, saying so for
/// a stuck hook, never finishing what it lingered for, and starts again when used next.
#[hegel::test(test_cases = 4, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_plugin_that_wont_stop_is_killed_after_its_grace(tc: TestCase) {
    let grace = Duration::from_millis(tc.draw(gs::sampled_from(vec![100, 250, 400])));
    let f = Fixture::new(|dir| vec![spec(dir, ECHO, &[])], Options { grace, ..Options::default() });
    let killed = format!("killed: it didn't stop within {grace:?} of being asked to");
    let kills = || f.logs.lock().unwrap().iter().filter(|(id, line)| id == "echo" && *line == killed).count();
    let steps: Vec<Stubborn> =
        tc.draw(gs::vecs(gs::sampled_from(vec![Stubborn::Stuck, Stubborn::Lingering])).min_size(1).max_size(3));
    let mut stuck = 0;
    for (i, step) in steps.into_iter().enumerate() {
        match step {
            Stubborn::Stuck => {
                // Cancelled once the plugin has said it's stuck: the hook is under way.
                let line = format!("stuck {i}");
                let (cancel, logs, said) = (CancelToken::new(), f.logs.clone(), line.clone());
                let cancelling = cancel.clone();
                let asking = std::thread::spawn(move || {
                    eventually("the hook is under way", || logs.lock().unwrap().iter().any(|(_, l)| *l == said));
                    cancelling.cancel();
                });
                let params = json!({ "log": line, "stuck": true });
                let e = f.plugins.run_command(
                    "echo",
                    "echo",
                    params,
                    Context::default(),
                    None,
                    &Ctx::new(&NoProgress, cancel),
                );
                asking.join().unwrap();
                assert!(matches!(e, Err(Error::Cancelled)), "{e:?}");
                stuck += 1;
                assert_eq!(kills(), stuck);
            }
            Stubborn::Lingering => {
                let file = f.store.root().join(format!("stopped-{i}"));
                f.echo_params(json!({ "stopped": file, "linger": 60_000 })).unwrap();
                f.plugins.shutdown();
                assert!(!file.exists(), "killed before it finished lingering");
            }
        }
        assert!(!f.plugins.is_running("echo"));
        assert_eq!(f.echo_params(json!({ "after": i })).unwrap(), json!({ "after": i }));
    }
}

/// The kinds of error a plugin might answer with: locallibrary's own, those only it may give,
/// and others.
const ERROR_KINDS: [&str; 11] = [
    "invalid",
    "not_found",
    "conflict",
    "io",
    "parse",
    "permission",
    "stale_plan",
    "busy",
    "explode",
    "cancelled",
    "closed",
];

/// An error of the kind `kind` a plugin answers with, about a file or not, with fields or not.
#[hegel::test_helper]
fn plugin_error(tc: &TestCase, dir: &Path, kind: &str) -> Value {
    let message = tc.draw(gs::sampled_from(vec!["it went wrong", "日本: no", "x"]));
    let mut fields = serde_json::Map::new();
    if tc.draw(gs::booleans()) {
        fields.insert("path".into(), json!(dir.join("set.txt")));
    }
    if tc.draw(gs::booleans()) {
        fields.insert("playlist".into(), json!(tc.draw(gs::sampled_from(vec!["Warmup", "", "a/b"]))));
    }
    json!({ "fail": kind, "message": message, "fields": fields })
}

/// For any error a plugin answers a hook with, the caller gets locallibrary's error of the kind
/// it says, naming the plugin, with every field it gives, when locallibrary has that kind (`io`
/// and `parse` only about a file); its own failure otherwise, with no actions to offer; a
/// cancellation as one; and a connection said to be closed as the plugin having stopped, which
/// starts again when used next.
#[hegel::test(test_cases = 5, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_plugins_error_reaches_the_caller_as_the_kind_it_says(tc: TestCase) {
    let f = Fixture::echo();
    // Every kind, in any order.
    for kind in tc.draw(gs::permutations(ERROR_KINDS.to_vec())) {
        let params = plugin_error(&tc, f.store.root(), kind);
        let (message, fields) = (params["message"].as_str().unwrap(), params["fields"].as_object().unwrap());
        let e = f.echo_params(params.clone()).unwrap_err();
        let about_a_file = fields.contains_key("path");
        match kind {
            "cancelled" => assert!(matches!(e, Error::Cancelled), "{e}"),
            "closed" => {
                assert_eq!(failure(e), "the Echo plugin failed: it stopped unexpectedly");
                assert!(!f.plugins.is_running("echo"));
            }
            "invalid" | "not_found" | "conflict" => {
                assert_eq!(e.code(), kind);
                assert!(e.to_string().ends_with(&format!("{message} (the Echo plugin)")), "{e}");
                assert_eq!(&e.detail().fields, fields);
            }
            "io" | "parse" if about_a_file => {
                assert_eq!(e.code(), kind);
                // Said as locallibrary's own error about that file is.
                let (path, said) =
                    (PathBuf::from(fields["path"].as_str().unwrap()), format!("{message} (the Echo plugin)"));
                let own = match kind {
                    "io" => Error::Io { path, source: std::io::Error::other(said) },
                    _ => Error::Parse { path, message: said },
                };
                assert_eq!(e.to_string(), own.to_string());
                assert_eq!(&e.detail().fields, fields);
            }
            _ => {
                assert!(e.detail().actions.is_empty());
                assert_eq!(failure(e), format!("the Echo plugin failed: {message}"));
            }
        }
    }
    assert!(f.echo_params(json!({})).is_ok());
}

const FIELD_TYPES: [&str; 5] = ["integer", "number", "string", "boolean", "string[]"];

/// A track's audio file, as the test leaves it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Audio {
    Intact,
    Removed,
    Emptied,
}

/// What analyzers worked out for the drive's tracks, as `track.fields` reads them back: by track,
/// the fields of `analyzer`, in order.
fn read_back(f: &Fixture, analyzer: &str) -> Vec<(u32, Vec<(String, Value)>)> {
    let req = PluginFieldsRequest { library: LibraryRef::from(common::path(&f.drive)), tracks: None, plugin: None };
    let read = api::track_fields_of(LibraryCache::none(), Some(&f.store), &req, &f.plugins.analyzers()).unwrap();
    read.tracks
        .iter()
        .map(|t| {
            let fields = &t.plugins["echo"][analyzer];
            (t.track, fields.iter().map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap())).collect())
        })
        .collect()
}

/// For any fields an analyzer declares, whatever the plugin answers for each track (a track's
/// size, fields in another order, fields of the wrong type, a failure), only answers whose every
/// field the analyzer declares, of its type, are kept, in the order it declares them, and read
/// back by plugin, version and analyzer; every other track is named in a warning, its audio file
/// missing or the plugin wrong about it, without failing the rest; and analysing again leaves
/// what's kept as it is.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_analyzers_answers_are_kept_only_as_it_declares(tc: TestCase) {
    let bytes = tc.draw(gs::sampled_from(FIELD_TYPES.to_vec()));
    let tempo = tc.draw(gs::optional(gs::sampled_from(FIELD_TYPES.to_vec())));
    // The `order` analyzer, which the test plugin answers `a` then `b` (integers) for.
    let names: Vec<&str> = tc.draw(gs::permutations(vec!["b", "a", "c"]));
    let named: Vec<&str> = tc.draw(gs::subsequences(names).min_size(1));
    let order: Option<Vec<(&str, &str)>> = tc
        .draw(gs::booleans())
        .then(|| named.iter().map(|&n| (n, tc.draw(gs::sampled_from(FIELD_TYPES.to_vec())))).collect());
    let wrong_fields = tc.draw(gs::integers::<u8>().max_value(3)) == 0;
    let analyzer = match &order {
        Some(_) if tc.draw(gs::booleans()) => "order",
        _ => "size",
    };
    let toml_fields = |fields: &[(&str, &str)]| {
        let listed: Vec<String> = fields.iter().map(|(n, t)| format!("{n} = {t:?}")).collect();
        format!("fields = {{ {} }}\n", listed.join(", "))
    };
    let mut size: Vec<(&str, &str)> = vec![("bytes", bytes)];
    size.extend(tempo.map(|t| ("tempo", t)));
    let mut analyzers = toml_fields(&size);
    if let Some(order) = &order {
        analyzers.push_str("\n[[contributes.analyzers]]\nname = \"order\"\ntitle = \"Order\"\nversion = \"1\"\n");
        analyzers.push_str(&toml_fields(order));
    }
    let manifest = ECHO.replace("fields = { bytes = \"integer\" }\n", &analyzers);
    let args: &[&str] = if wrong_fields { &["--wrong-fields"] } else { &[] };
    let analyses = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let f = Fixture::new(|dir| vec![spec(dir, &manifest, args)], Options { analyses, ..Options::default() });

    let tracks = common::library(&f.drive).tracks;
    let audio_of = |t: &locallibrary::Track| paths::to_fs(&f.drive, &paths::key(&t.file_path));
    let declared: Vec<(&str, FieldType)> = match analyzer {
        "order" => order.clone().unwrap(),
        _ => size.clone(),
    }
    .into_iter()
    .map(|(n, t)| (n, serde_json::from_value(json!(t)).unwrap()))
    .collect();
    let (mut warnings, mut kept) = (Vec::new(), Vec::new());
    for t in &tracks {
        let state = tc.draw(gs::sampled_from(vec![Audio::Intact, Audio::Intact, Audio::Removed, Audio::Emptied]));
        let audio = audio_of(t);
        match state {
            Audio::Intact => {}
            Audio::Removed => {
                std::fs::remove_file(&audio).unwrap();
                warnings.push((t.id, format!("{}: its audio file isn't on the drive", t.file_path)));
                continue;
            }
            Audio::Emptied => std::fs::write(&audio, "").unwrap(),
        }
        let answer: Vec<(&str, FieldValue)> = match (wrong_fields, analyzer) {
            (true, _) => vec![("bytes", FieldValue::String("many".into()))],
            (false, "order") if state == Audio::Emptied => {
                warnings.push((t.id, format!("{} is empty (the Echo plugin)", t.file_path)));
                continue;
            }
            (false, "order") => vec![("a", FieldValue::Integer(2)), ("b", FieldValue::Integer(1))],
            (false, _) => vec![("bytes", FieldValue::Integer(std::fs::metadata(&audio).unwrap().len() as i64))],
        };
        let wrong = answer.iter().find_map(|(name, v)| match declared.iter().find(|(n, _)| n == name) {
            None => Some(format!("its field {name} isn't one {analyzer} declares")),
            Some((_, kind)) if !kind.accepts(v) => {
                Some(format!("its field {name} isn't the {} {analyzer} declares", json!(kind).as_str().unwrap()))
            }
            Some(_) => None,
        });
        match wrong {
            Some(wrong) => warnings.push((t.id, format!("{}: the Echo plugin failed: {wrong}", t.file_path))),
            None => {
                let mut fields: Vec<(String, Value)> =
                    answer.iter().map(|(n, v)| (n.to_string(), serde_json::to_value(v).unwrap())).collect();
                fields.sort_by_key(|(n, _)| declared.iter().position(|(d, _)| d == n));
                kept.push((t.id, fields));
            }
        }
    }
    // Warnings come as the workers get to the tracks: compared in order.
    let said_of = |ids: &[u32]| {
        let mut said: Vec<String> =
            warnings.iter().filter(|(id, _)| ids.contains(id)).map(|(_, w)| w.clone()).collect();
        said.sort();
        said
    };
    let every: Vec<u32> = tracks.iter().map(|t| t.id).collect();
    let said = common::Warnings::default();
    let done = f.plugins.analyze("echo", analyzer, "USB", None, &f.store, &said.ctx()).unwrap();
    assert_eq!((done.analyzed, done.kept, done.failed), (kept.len(), 0, warnings.len()));
    let mut warned = said.said();
    warned.sort();
    assert_eq!(warned, said_of(&every));
    assert_eq!(read_back(&f, analyzer), kept);

    // Again, for some tracks: what's kept is left as it is, and what failed fails again.
    let some: Vec<u32> = tc.draw(gs::subsequences(every));
    let said = common::Warnings::default();
    let again = f.plugins.analyze("echo", analyzer, "USB", Some(&some), &f.store, &said.ctx()).unwrap();
    let kept_of = kept.iter().filter(|(id, _)| some.contains(id)).count();
    assert_eq!((again.analyzed, again.kept, again.failed), (0, kept_of, said_of(&some).len()));
    let mut warned = said.said();
    warned.sort();
    assert_eq!(warned, said_of(&some));
    assert_eq!(read_back(&f, analyzer), kept);
    let e = f.plugins.analyze("echo", analyzer, "USB", Some(&[9999]), &f.store, &Ctx::silent()).unwrap_err();
    assert_eq!(e.code(), "not_found");
}

/// For any point an analysis is cancelled at, it stops there, failing with the cancellation,
/// having kept only tracks it had finished; analysing again does the rest, leaving those as
/// they are.
#[hegel::test(test_cases = 4, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_analysis_cancelled_part_way_picks_up_where_it_stopped(tc: TestCase) {
    let analyses = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let f = Fixture::new(|dir| vec![spec(dir, ECHO, &[])], Options { analyses, ..Options::default() });
    let n = common::library(&f.drive).tracks.len();
    let at = tc.draw(gs::integers::<usize>().min_value(1).max_value(n));
    let (cancel, seen) = (CancelToken::new(), AtomicUsize::new(0));
    let cancelling = cancel.clone();
    let on_event = move |e: &Event| {
        if matches!(e, Event::Item { .. }) && seen.fetch_add(1, Ordering::SeqCst) + 1 == at {
            cancelling.cancel();
        }
    };
    let e = f.plugins.analyze("echo", "size", "USB", None, &f.store, &Ctx::new(&on_event, cancel)).unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    let finished = read_back(&f, "size").len();
    assert!(finished < at, "{finished} kept, cancelled at item {at}");
    let rest = f.plugins.analyze("echo", "size", "USB", None, &f.store, &Ctx::silent()).unwrap();
    assert_eq!((rest.analyzed, rest.kept, rest.failed), (n - finished, finished, 0));
    assert_eq!(read_back(&f, "size").len(), n);
}

/// An edit a plugin's operation might answer.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PluginEdit {
    Create,
    /// Renames a playlist the drive doesn't have: planned strictly, it fails.
    RenameNothing,
    /// Deletes the playlist `Doomed`: destructive.
    Delete,
    Tag,
    Cues,
    ArtworkBytes,
    ArtworkFile,
    Nonsense,
}

/// For any edits a plugin's operation answers, with any capabilities granted, the host refuses
/// the first edit it can't read, that names an image file, or that its capabilities don't grant,
/// then a file it says it read that isn't one (or isn't named by an absolute path); plans the
/// rest strictly as one edit plan, named for the plugin, with its summary and warnings; and
/// refuses a destructive plan without `library:delete`. A plan let through applies.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_operations_edits_are_planned_as_its_grants_say(tc: TestCase) {
    let grants: Vec<&str> = tc.draw(gs::subsequences(vec!["library:delete", "tags:plan", "cues:plan", "artwork:plan"]));
    let granted = |c: &str| grants.contains(&c);
    let all = vec![
        PluginEdit::Create,
        PluginEdit::RenameNothing,
        PluginEdit::Delete,
        PluginEdit::Tag,
        PluginEdit::Cues,
        PluginEdit::ArtworkBytes,
        PluginEdit::ArtworkFile,
        PluginEdit::Nonsense,
    ];
    let f = Fixture::new(|dir| vec![spec(dir, &granting(&grants), &[])], Options::default());
    let plan =
        |params: Value| f.plugins.plan_op("echo", "playlist", params, f.context(), Some(&f.store), &Ctx::silent());
    let first = common::library(&f.drive).tracks[0].file_path.clone();
    for n in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let playlists = || common::library(&f.drive).playlists.into_iter().map(|p| p.name).collect::<Vec<_>>();
        if !playlists().contains(&"Doomed".to_string()) {
            plan(json!({ "name": "Doomed" })).unwrap().apply(None, &Ctx::silent()).unwrap();
        }
        // Cues and artwork the drive can't take are left out once granted: what's under test is
        // the grant, not the edit.
        let edits: Vec<PluginEdit> = tc.draw(gs::vecs(gs::sampled_from(all.clone())).min_size(1).max_size(4));
        let edits: Vec<PluginEdit> = edits
            .into_iter()
            .filter(|e| match e {
                PluginEdit::Cues => !granted("cues:plan"),
                PluginEdit::ArtworkBytes => !granted("artwork:plan"),
                _ => true,
            })
            .collect();
        let mut deleted = false;
        let answered: Vec<Value> = edits
            .iter()
            .enumerate()
            .map(|(i, e)| match e {
                PluginEdit::Create => json!({ "op": "create_playlist", "path": format!("P{n}-{i}") }),
                PluginEdit::RenameNothing => json!({ "op": "rename_playlist", "playlist": "Nope", "name": "Yes" }),
                // Deleting twice can't apply: the second is another rename of nothing.
                PluginEdit::Delete if !std::mem::replace(&mut deleted, true) => {
                    json!({ "op": "delete_playlist", "playlist": "Doomed" })
                }
                PluginEdit::Delete => json!({ "op": "rename_playlist", "playlist": "Nope", "name": "Yes" }),
                PluginEdit::Tag => json!({ "op": "my_tag", "action": "add", "tag": "Genre/House", "tracks": [first] }),
                PluginEdit::Cues => json!({ "op": "cues", "tracks": [first], "change": { "copy": { "from": first } } }),
                PluginEdit::ArtworkBytes => {
                    json!({ "op": "set_artwork", "tracks": { "ids": [1] }, "image": { "base64": "AAAA" } })
                }
                PluginEdit::ArtworkFile => {
                    json!({ "op": "set_artwork", "tracks": { "ids": [1] }, "image": { "path": "/tmp/cover.jpg" } })
                }
                PluginEdit::Nonsense => json!({ "op": "explode" }),
            })
            .collect();
        let needs = |c: &str, i: usize| Some(format!("its edit {i} needs the {c} capability, which it doesn't have"));
        let refused = edits.iter().enumerate().find_map(|(i, e)| match e {
            PluginEdit::Nonsense => Some(format!("its edit {i} isn't one: ")),
            PluginEdit::ArtworkFile => {
                Some(format!("its edit {i} names an image file: a plugin gives an image's bytes"))
            }
            PluginEdit::Tag if !granted("tags:plan") => needs("tags:plan", i),
            PluginEdit::Cues => needs("cues:plan", i),
            PluginEdit::ArtworkBytes => needs("artwork:plan", i),
            _ => None,
        });
        // What the plugin says it read: a file, or something it can't have read as one.
        let list = f.store.root().parent().unwrap().join("set.txt");
        std::fs::write(&list, "one\n").unwrap();
        let input = tc.draw(gs::sampled_from(vec![None, Some(0), Some(1), Some(2)]));
        let inputs: Vec<PathBuf> = match input {
            None => Vec::new(),
            Some(0) => vec![list.clone()],
            Some(1) => vec![PathBuf::from("set.txt")],
            _ => vec![f.store.root().to_path_buf()],
        };
        let refused = refused.or_else(|| match input {
            Some(1) => Some("it read set.txt, which isn't an absolute path".into()),
            Some(2) => Some(format!("it read {}, which isn't a file", f.store.root().display())),
            _ => None,
        });
        let renames = answered.iter().any(|e| e["op"] == "rename_playlist");
        let planned = plan(json!({ "name": "x", "edits": answered, "inputs": inputs }));
        match refused {
            Some(refused) => {
                let said = failure(planned.unwrap_err());
                assert!(said.starts_with(&format!("the Echo plugin failed: {refused}")), "{said}");
            }
            None if renames => assert_eq!(planned.unwrap_err().code(), "not_found", "every edit must apply"),
            None if deleted && !granted("library:delete") => assert_eq!(
                failure(planned.unwrap_err()),
                "the Echo plugin failed: playlist removes from the library, which needs the library:delete capability"
            ),
            None => {
                let planned = planned.unwrap();
                assert_eq!((&*planned.operation, planned.destructive), ("echo.playlist", deleted));
                assert_eq!((planned.summary[0].as_str(), planned.warnings()), ("as given", &["given".to_string()][..]));
                planned.apply(None, &Ctx::silent()).unwrap();
                let playlists = playlists();
                let created = edits.iter().enumerate().filter(|(_, e)| **e == PluginEdit::Create);
                assert!(created.map(|(i, _)| format!("P{n}-{i}")).all(|p| playlists.contains(&p)), "{playlists:?}");
                assert_eq!(playlists.contains(&"Doomed".to_string()), !deleted);
            }
        }
    }
}

/// Where an export is written: a folder of the test's, a drive, or the store.
#[derive(Debug, Clone, Copy)]
enum Output {
    Here,
    OnTheDrive,
    InTheStore,
}

/// For any outputs, whether a file is there already, and whether it changes before the export is
/// applied: an export planned there says which files it writes, overwriting one only as a
/// destructive plan, which applies as planned unless a file it writes changed since (then it's
/// stale, and the file is left as it is). One not given by an absolute path, onto a drive or into
/// the store is refused, as is a plugin saying it would write outside the output (beside it,
/// above it, or by `..`), and one writing more than it said.
#[hegel::test(test_cases = 5, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_export_overwrites_only_as_planned(tc: TestCase) {
    let f = Fixture::echo();
    let here = f.store.root().parent().unwrap().join("exports");
    let relative = tc.draw(gs::sampled_from(vec!["out.m3u", "exports/out.m3u", "./x", ""]));
    let source = ExportSource { drive: "USB".into(), playlists: Vec::new(), history: Vec::new() };
    let e = f.plugins.export_plan("echo", "m3u", source, Path::new(relative), Value::Null, None, &Ctx::silent());
    assert_eq!(e.err().unwrap().to_string(), format!("the export's output must be an absolute path, not {relative}"));
    let lines: Vec<String> = common::library(&f.drive).tracks.iter().map(|t| t.file_path.clone()).collect();
    let text = format!("#EXTM3U\n{}\n", lines.join("\n"));
    for i in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let where_to =
            tc.draw(gs::sampled_from(vec![Output::Here, Output::Here, Output::OnTheDrive, Output::InTheStore]));
        let root = match where_to {
            Output::Here => here.clone(),
            Output::OnTheDrive => f.drive.join("PIONEER"),
            Output::InTheStore => f.store.root().join("exports"),
        };
        let out = root.join(format!("out-{}.m3u", tc.draw(gs::integers::<u8>().max_value(2))));
        let playlists: Vec<String> = tc.draw(gs::subsequences(vec!["Warmup".to_string(), "Peak".to_string()]));
        let source = ExportSource { drive: "USB".into(), playlists: playlists.clone(), history: Vec::new() };
        // Now and then the plugin writes more than it says.
        let options = match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => json!({ "extra": out.with_file_name(format!("extra-{i}.txt")) }),
            _ => Value::Null,
        };
        let plan = |options: &Value| {
            f.plugins.export_plan("echo", "m3u", source.clone(), &out, options.clone(), Some(&f.store), &Ctx::silent())
        };
        if !matches!(where_to, Output::Here) {
            let what = if matches!(where_to, Output::OnTheDrive) { "the drive at" } else { "the store at" };
            let e = plan(&options).err().unwrap().to_string();
            assert!(e.starts_with(&format!("a plugin's export isn't written to {what}")), "{e}");
            continue;
        }
        // The plugin saying it would write elsewhere: inside the output (a folder) is the
        // output's, beside it, above it or out by `..` isn't.
        let elsewhere = ["x.m3u", "../x.m3u", "out/../../x.m3u", "inner/x.m3u", "inner/../y.m3u"];
        let beside = out.with_file_name(format!("beside-{i}.m3u"));
        for said in elsewhere.iter().map(|e| out.join(e)).chain([beside]) {
            let inside = said.strip_prefix(&out).is_ok_and(|p| !p.components().any(|c| c.as_os_str() == ".."));
            match plan(&json!({ "outside": said })) {
                Ok(planned) => {
                    assert!(inside, "{} is outside {}", said.display(), out.display());
                    assert_eq!(planned.files.iter().map(|f| f.file.path.clone()).collect::<Vec<_>>(), [said]);
                }
                Err(e) => {
                    assert!(!inside, "{} is inside {}", said.display(), out.display());
                    let refused =
                        format!("the Echo plugin failed: it would write {} outside the output", said.display());
                    assert_eq!(failure(e), refused);
                }
            }
        }
        let existed = out.exists();
        let before = std::fs::read(&out).ok();
        let planned = plan(&options).unwrap();
        let [file] = &planned.files[..] else { panic!("one file") };
        assert_eq!((&file.file.path, file.exists, planned.destructive), (&out, existed, existed));
        assert_eq!(planned.summary.iter().any(|s| s == "overwrite 1 of them"), existed);
        let noted = (!playlists.is_empty()).then(|| format!("playlists: {}", playlists.join(", ")));
        assert_eq!(planned.notes.first(), noted.as_ref());
        assert_eq!(before.as_deref(), std::fs::read(&out).ok().as_deref(), "a dry run writes nothing");
        let changed = tc.draw(gs::integers::<u8>().max_value(3)) == 0;
        if changed {
            std::fs::create_dir_all(&here).unwrap();
            std::fs::write(&out, format!("mine {i}")).unwrap();
        }
        let applied = planned.apply(None, &Ctx::silent());
        match (changed, options["extra"].as_str()) {
            (true, _) => {
                assert_eq!(applied.err().unwrap().code(), "stale_plan");
                assert_eq!(std::fs::read_to_string(&out).unwrap(), format!("mine {i}"), "left as it was");
            }
            (false, Some(extra)) => {
                let said = format!("the Echo plugin failed: it wrote what it didn't say it would: {extra}");
                assert_eq!(failure(applied.err().unwrap()), said);
            }
            (false, None) => {
                applied.unwrap();
                assert_eq!(std::fs::read_to_string(&out).unwrap(), text);
            }
        }
    }
}

/// A setting a test plugin declares: its type (with choices for an enum) and default.
#[derive(Debug, Clone)]
struct Declared {
    key: String,
    kind: &'static str,
    choices: Vec<&'static str>,
    default: Option<Value>,
}

impl Declared {
    fn accepts(&self, v: &Value) -> bool {
        match self.kind {
            "string" | "file" | "directory" => v.is_string(),
            "number" => v.is_number(),
            "boolean" => v.is_boolean(),
            _ => v.as_str().is_some_and(|s| self.choices.contains(&s)),
        }
    }

    fn toml(&self) -> String {
        let mut s =
            format!("\n[[contributes.settings]]\nkey = {:?}\ntitle = \"S\"\ntype = {:?}\n", self.key, self.kind);
        if !self.choices.is_empty() {
            s.push_str(&format!("choices = {:?}\n", self.choices));
        }
        if let Some(d) = &self.default {
            s.push_str(&format!("default = {d}\n"));
        }
        s
    }
}

/// A value of some type: what a setting may or may not take.
#[hegel::test_helper]
fn setting_value(tc: &TestCase) -> Value {
    tc.draw(gs::sampled_from(vec![
        json!("hi"),
        json!("deep"),
        json!(""),
        json!(4),
        json!(-0.5),
        json!(true),
        json!(false),
        Value::Null,
        json!(["hi"]),
        json!({ "x": 1 }),
    ]))
}

/// What the store has for a plugin's settings, as the model keeps it.
#[derive(Debug, Clone)]
enum Kept {
    /// What was written, by key.
    Values(serde_json::Map<String, Value>),
    /// A file that can't be read.
    Unreadable,
}

/// For any settings a plugin declares (of each type, with or without a default) and any sequence
/// of values set, put back, written by an older plugin or left unreadable, each setting is the
/// value set while that fits it, else its default, and none without either; a value that doesn't
/// fit is refused, saying what the setting takes, and leaves what's kept as it was.
#[hegel::test(test_cases = 60, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_plugins_settings_are_what_was_set_while_it_fits(tc: TestCase) {
    let kinds = vec!["string", "number", "boolean", "enum", "file", "directory"];
    let mut declared = Vec::new();
    for i in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        let kind = tc.draw(gs::sampled_from(kinds.clone()));
        let choices = if kind == "enum" { vec!["deep", "peak"] } else { Vec::new() };
        let mut d = Declared { key: format!("s{i}"), kind, choices, default: None };
        let default = setting_value(&tc);
        d.default = (tc.draw(gs::booleans()) && d.accepts(&default)).then_some(default);
        declared.push(d);
    }
    let start = ECHO.find("\n[[contributes.settings]]").unwrap();
    let manifest = declared.iter().fold(ECHO[..start].to_string(), |m, d| m + &d.toml());
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let plugins = Plugins::new(vec![spec(dir.path(), &manifest, &[])], Arc::new(NoReads), Options::default()).unwrap();
    let file = store.root().join("plugins/echo/settings.json");
    let expected = |kept: &Kept| -> Value {
        let values = match kept {
            Kept::Values(v) => v.clone(),
            Kept::Unreadable => serde_json::Map::new(),
        };
        let effective = declared.iter().filter_map(|d| {
            let value = values.get(&d.key).filter(|v| d.accepts(v)).or(d.default.as_ref())?;
            Some((d.key.clone(), value.clone()))
        });
        Value::Object(effective.collect())
    };
    let mut kept = Kept::Values(serde_json::Map::new());
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(8)) {
        let key = match declared.len() {
            0 => "nope".to_string(),
            n => tc.draw(gs::sampled_from((0..n).map(|i| format!("s{i}")).chain(["nope".into()]).collect::<Vec<_>>())),
        };
        match tc.draw(gs::integers::<u8>().max_value(5)) {
            // An older plugin's values: kept as they are, whether or not they fit.
            0 => {
                let value = setting_value(&tc);
                let older = serde_json::Map::from_iter([(key.clone(), value), ("gone".to_string(), json!(1))]);
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(&file, serde_json::to_vec(&older).unwrap()).unwrap();
                kept = Kept::Values(older);
            }
            1 => {
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(&file, "{ not json").unwrap();
                kept = Kept::Unreadable;
            }
            step => {
                let value = (step > 2).then(|| setting_value(&tc));
                let set = plugins.set_setting("echo", &store, &key, value.clone());
                let Some(d) = declared.iter().find(|d| d.key == key) else {
                    assert_eq!(set.unwrap_err().code(), "not_found");
                    continue;
                };
                match value {
                    Some(v) if !d.accepts(&v) => {
                        let e = set.unwrap_err();
                        let takes = match d.kind {
                            "enum" => format!("one of {}", d.choices.join(", ")),
                            kind => format!("a {kind}"),
                        };
                        assert_eq!((e.code(), e.to_string()), ("invalid", format!("{key} is {takes}, not {v}")));
                    }
                    value => {
                        // Written anew over one that can't be read.
                        let mut values = match &kept {
                            Kept::Values(v) => v.clone(),
                            Kept::Unreadable => serde_json::Map::new(),
                        };
                        match value {
                            Some(v) => values.insert(key.clone(), v),
                            None => values.remove(&key),
                        };
                        kept = Kept::Values(values);
                        assert_eq!(serde_json::to_value(set.unwrap()).unwrap(), expected(&kept));
                    }
                }
            }
        }
        let settings = plugins.settings("echo", Some(&store)).unwrap();
        assert_eq!(serde_json::to_value(&settings).unwrap(), expected(&kept));
        let keys: Vec<&String> = settings.keys().collect();
        let in_order: Vec<&String> = declared.iter().map(|d| &d.key).filter(|k| keys.contains(k)).collect();
        assert_eq!(keys, in_order, "in the order the plugin declares them");
        if let Kept::Values(values) = &kept {
            let written: Value =
                serde_json::from_slice(&std::fs::read(&file).unwrap_or_else(|_| b"{}".to_vec())).unwrap();
            assert_eq!(written, Value::Object(values.clone()), "kept as written");
        }
    }
    let defaults = plugins.settings("echo", None).unwrap();
    assert_eq!(serde_json::to_value(defaults).unwrap(), expected(&Kept::Unreadable), "without a store, the defaults");
}

/// A plugin offered to the host: its id, whether locallibrary ships it, its command's and
/// operation's places on the command line, and whether it has a secret setting.
#[derive(Debug, Clone)]
struct Offered {
    id: &'static str,
    first_party: bool,
    command: Option<&'static str>,
    operation: Option<&'static str>,
    secret: bool,
}

/// For any plugins offered, the host takes them all, in order, or refuses the first that can't
/// be hosted: one locallibrary doesn't ship, one with a secret setting, one whose place on the
/// command line another plugin has, or a second with one id. Taken, each place finds its plugin's
/// command or operation, and every analyzer's fields are named by plugin, in order.
#[hegel::test(test_cases = 100, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_host_takes_exactly_the_plugins_it_can_run(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let mut offered = Vec::new();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let id = tc.draw(gs::sampled_from(vec!["a", "b", "c"]));
        let first_party = tc.draw(gs::integers::<u8>().max_value(7)) > 0;
        let command = tc.draw(gs::optional(gs::sampled_from(vec!["say", "play"])));
        let operation = tc.draw(gs::optional(gs::sampled_from(vec!["say", "mix", "playlist make"])));
        let operation = operation.filter(|o| Some(*o) != command);
        let secret = tc.draw(gs::integers::<u8>().max_value(7)) == 0;
        offered.push(Offered { id, first_party, command, operation, secret });
    }
    let specs: Vec<PluginSpec> = offered
        .iter()
        .map(|o| {
            let mut toml = ECHO.replace("id = \"echo\"", &format!("id = \"{}\"", o.id));
            if let Some(place) = o.command {
                toml = toml.replace(
                    "title = \"Answer the params\"",
                    &format!("title = \"Answer the params\"\ncli = {place:?}"),
                );
            }
            if let Some(place) = o.operation {
                toml = toml
                    .replace("title = \"Make a playlist\"", &format!("title = \"Make a playlist\"\ncli = {place:?}"));
            }
            if o.secret {
                toml = toml.replace("type = \"string\"\ndefault = \"hello\"", "type = \"secret\"");
            }
            // Parsed and checked as the host does: a secret setting is a valid manifest.
            let manifest = loclib_plugin::Manifest::parse(&toml).unwrap();
            PluginSpec { manifest, launch: launch_nothing(), first_party: o.first_party }
        })
        .collect();
    let mut places: Vec<&str> = Vec::new();
    let mut ids: Vec<&str> = Vec::new();
    let refused = offered.iter().find_map(|o| {
        if !o.first_party {
            return Some(format!("{}: third-party plugins aren't supported yet", o.id));
        }
        if o.secret {
            return Some(format!(
                "the {} plugin's setting \"greeting\" is a secret, which locallibrary can't keep yet",
                o.id
            ));
        }
        if let Some(place) = [o.command, o.operation].into_iter().flatten().find(|p| places.contains(p)) {
            return Some(format!("the {} plugin's command {place:?} is another plugin's too", o.id));
        }
        places.extend([o.command, o.operation].into_iter().flatten());
        if ids.contains(&o.id) {
            return Some(format!("two plugins are called {:?}", o.id));
        }
        ids.push(o.id);
        None
    });
    let hosted = Plugins::new(specs, Arc::new(NoReads), Options::default());
    let _ = dir;
    if let Some(refused) = refused {
        assert_eq!(hosted.err().unwrap().to_string(), refused);
        return;
    }
    let hosted = hosted.unwrap();
    assert_eq!(hosted.manifests().map(|m| m.id.as_str()).collect::<Vec<_>>(), ids);
    for o in &offered {
        assert_eq!(hosted.manifest(o.id).unwrap().id, o.id);
        if let Some(place) = o.command {
            assert!(
                matches!(hosted.cli(place), Some((id, Cli::Command(c))) if id == o.id && c.name == "echo"),
                "{place}"
            );
        }
        if let Some(place) = o.operation {
            assert!(matches!(hosted.cli(place), Some((id, Cli::Operation(op))) if id == o.id && op.name == "playlist"));
        }
    }
    for place in ["say", "play", "mix", "playlist make", "echo"] {
        let taken = offered.iter().any(|o| o.command == Some(place) || o.operation == Some(place));
        assert_eq!(hosted.cli(place).is_some(), taken, "{place}");
    }
    let fields: Vec<String> =
        hosted.fields().into_iter().map(|(name, kind)| format!("{name} {}", json!(kind))).collect();
    assert_eq!(fields, ids.iter().map(|id| format!("{id}.bytes \"integer\"")).collect::<Vec<_>>());
    let analyzers: Vec<(String, String, String)> =
        hosted.analyzers().into_iter().map(|a| (a.plugin, a.version, a.analyzer)).collect();
    let expected: Vec<(String, String, String)> =
        ids.iter().map(|id| (id.to_string(), "1".into(), "size".into())).collect();
    assert_eq!(analyzers, expected);
    assert_eq!(hosted.manifest("nope").err().unwrap().code(), "not_found");
    assert!(!hosted.is_running("nope"));
}

/// A view a command might answer, or something that isn't one, as JSON.
#[hegel::test_helper]
fn maybe_a_view(tc: &TestCase) -> (Value, bool) {
    let title = tc.draw(gs::optional(gs::sampled_from(vec!["USB", "日本"])));
    let mut view = match tc.draw(gs::integers::<u8>().max_value(6)) {
        0 => json!({ "kind": "list", "items": [{ "label": "a" }, { "label": "b", "detail": "c" }] }),
        1 => json!({ "kind": "table", "columns": ["Title"], "rows": [["x"], [1], [null]] }),
        2 => json!({ "kind": "kv", "pairs": [{ "label": "tracks", "value": 3 }] }),
        3 => json!({ "kind": "text", "text": "hello" }),
        4 => return (json!({ "kind": "chart", "points": [1, 2] }), false),
        5 => return (json!({ "kind": "table", "columns": "Title", "rows": [] }), false),
        _ => return (json!({ "text": "no kind" }), false),
    };
    if let Some(title) = title {
        view["title"] = json!(title);
    }
    (view, true)
}

/// For any answer a view command gives, the caller gets it as a view when it's one, and the
/// plugin failing, saying so, when it isn't; params that aren't what it takes never reach it.
#[hegel::test(test_cases = 4, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_view_commands_answer_is_a_view_or_the_plugin_failing(tc: TestCase) {
    let toml = ECHO.replacen("mode = \"headless\"", "mode = \"view\"", 1);
    let f = Fixture::new(|dir| vec![spec(dir, &toml, &[])], Options::default());
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
        // Params that don't fit the command's schema (an object) are refused before it's asked.
        let unfit = tc.draw(gs::sampled_from(vec![json!([1]), json!("x"), json!(4), json!(true)]));
        let e = f.command("echo", unfit.clone(), &Ctx::silent()).unwrap_err();
        assert_eq!(e.code(), "invalid", "{unfit}: {e}");
        let (answer, is_view) = maybe_a_view(&tc);
        let got = f.command("echo", answer.clone(), &Ctx::silent());
        match is_view {
            true => {
                let CommandOutput::View(view) = got.unwrap() else { panic!("a view") };
                assert_eq!(serde_json::to_value(view).unwrap(), answer);
            }
            false => {
                let said = failure(got.unwrap_err());
                assert!(said.starts_with("the Echo plugin failed: its view of echo isn't one: "), "{said}");
            }
        }
    }
}

/// How a plugin can be other than its manifest says.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Started {
    AsItSays,
    AnotherId,
    AnotherVersion,
    DescribedOtherwise,
    AnotherProtocol,
    NotThere,
    Silent,
}

/// For any plugins, each what its manifest says or not (another id or version, describing itself
/// otherwise, speaking another protocol, not there, or never answering), those that are run and
/// every other fails to start, saying why, and isn't left running.
#[hegel::test(test_cases = 3, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn only_a_plugin_that_is_what_its_manifest_says_runs(tc: TestCase) {
    use Started::*;
    let all = vec![AsItSays, AnotherId, AnotherVersion, DescribedOtherwise, AnotherProtocol, NotThere, Silent];
    let order: Vec<Started> = tc.draw(gs::permutations(all));
    let options = Options { handshake: Duration::from_secs(2), ..Options::default() };
    let f = Fixture::new(
        |dir| {
            let specs = order.iter().enumerate().map(|(i, how)| {
                let toml = ECHO.replace("id = \"echo\"", &format!("id = \"p{i}\""));
                let args: &[&str] = match how {
                    AnotherProtocol => &["--protocol", "9.0.0"],
                    Silent => &["--silent"],
                    _ => &[],
                };
                let mut s = spec(dir, &toml, args);
                match how {
                    AnotherId => s.manifest.id = format!("q{i}"),
                    AnotherVersion => s.manifest.version = "0.2.0".into(),
                    DescribedOtherwise => s.manifest.name = "Other".into(),
                    NotThere => s.launch.program = dir.join("no-such-program"),
                    _ => {}
                }
                s
            });
            specs.collect()
        },
        options,
    );
    for (i, how) in order.into_iter().enumerate() {
        let id = if how == AnotherId { format!("q{i}") } else { format!("p{i}") };
        let ran = f.plugins.run_command(&id, "echo", json!({ "n": i }), Context::default(), None, &Ctx::silent());
        let said = match how {
            AsItSays => {
                assert_eq!(ran.unwrap(), CommandOutput::Headless(json!({ "n": i })));
                assert!(f.plugins.is_running(&id));
                continue;
            }
            AnotherId => format!("the Echo plugin failed: it says it's p{i} 0.1.0, not q{i} 0.1.0"),
            AnotherVersion => format!("the Echo plugin failed: it says it's p{i} 0.1.0, not p{i} 0.2.0"),
            DescribedOtherwise => {
                "the Other plugin failed: it describes itself otherwise than the manifest locallibrary has for it"
                    .into()
            }
            AnotherProtocol => {
                format!("the Echo plugin failed: protocol \"9.0.0\" isn't compatible with {PROTOCOL_VERSION}")
            }
            NotThere => "the Echo plugin failed: it couldn't be started: ".into(),
            Silent => "the Echo plugin failed: it didn't answer hello in time".into(),
        };
        let e = failure(ran.unwrap_err());
        assert!(e.starts_with(&said), "{how:?}: {e}");
        assert!(!f.plugins.is_running(&id), "{how:?}");
    }
}

/// For any idle time, a plugin left with nothing to do is stopped once it has been idle that
/// long, exiting on its own (finishing what it does as it stops), and starts again when used.
#[hegel::test(test_cases = 3, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_idle_plugin_stops_on_its_own_and_starts_again(tc: TestCase) {
    // Longer than a plugin takes to shake hands: idle is counted from its start.
    let idle = Duration::from_millis(tc.draw(gs::sampled_from(vec![400, 700])));
    let f = Fixture::new(|dir| vec![spec(dir, ECHO, &[])], Options { idle, ..Options::default() });
    for round in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let file = f.store.root().join(format!("stopped-{round}"));
        f.echo_params(json!({ "stopped": file })).unwrap();
        eventually("an idle plugin stops on its own", || file.exists());
        // Taken out before it's asked to stop: by the time it has, it isn't running.
        assert!(!f.plugins.is_running("echo"));
    }
}
