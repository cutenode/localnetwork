//! The test plugin's hooks: loclib-plugin's echo example, with what the tests need besides (see
//! `main.rs`). The CLI's tests serve it from the locallibrary binary too
//! (`LOCALLIBRARY_TEST_PLUGINS`, in builds with debug assertions).

// The echo plugin's manifest and hooks are for `main.rs`: locallibrary serves the manifest a
// test gives, and these hooks.
#[allow(dead_code)]
#[path = "../../../../loclib-plugin/examples/echo/plugin.rs"]
pub mod echo;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use loclib_plugin::{
    Analysis, AnalyzeTrack, Candidate, Candidates, Choice, Context, EventNotify, Exported, ExportedFile, Fetch,
    FieldValue, GetBlob, GetManifest, HasBlobs, Hooks, Host, HostNotify, ListItem, OpAnswer, OpPlan, PlanOp,
    ProvideCandidates, PublishSet, PutBlob, PutManifest, RpcError, RunCommand, StartService, Suggested, View,
    WriteExport, params,
};
use serde_json::{Value, json};

pub struct Test {
    pub wrong_fields: bool,
}

/// How long it takes to stop once its host has closed its input, in milliseconds (`linger`).
static LINGER: AtomicU64 = AtomicU64::new(0);

/// The file it writes once it has stopped, after lingering (`stopped`): a killed plugin never
/// writes it.
static STOPPED: Mutex<Option<PathBuf>> = Mutex::new(None);

impl Drop for Test {
    /// Dropped once it has stopped serving.
    fn drop(&mut self) {
        std::thread::sleep(Duration::from_millis(LINGER.load(Ordering::SeqCst)));
        if let Some(file) = STOPPED.lock().unwrap().take() {
            std::fs::write(file, "stopped").unwrap();
        }
    }
}

impl Test {
    /// Its hooks, but those of the features `without` names (`commands`...): it doesn't say it
    /// has them.
    pub fn hooks(self, without: &[String]) -> Hooks {
        let test = Arc::new(self);
        let has = |feature: &str| !without.iter().any(|w| w == feature);
        let mut hooks = Hooks::new();
        if has("commands") {
            let test = test.clone();
            hooks = hooks.on(move |req: RunCommand, host: &Host| test.run_command(req, host));
        }
        if has("analyzers") {
            let test = test.clone();
            hooks = hooks.on(move |req: AnalyzeTrack, host: &Host| test.analyze_track(req, host));
        }
        if has("exporters") {
            let test = test.clone();
            hooks = hooks.on(move |req: WriteExport, host: &Host| test.write_export(req, host));
        }
        if has("operations") {
            let test = test.clone();
            hooks = hooks.on(move |req: PlanOp, host: &Host| test.plan_op(req, host));
        }
        if has("destinations") {
            // Echo's, each noting what it's asked (`blobs/put <hash>`) as a line of `asked` in
            // its folder before it answers (its log is read on a thread of the host's own, so a
            // line there can come after the answer), and publishing as the `answer` setting says
            // when it has one.
            let said = |method: &str, what: &str| {
                let mut asked = OpenOptions::new().create(true).append(true).open("asked").expect("its folder");
                writeln!(asked, "{method} {what}").expect("its folder");
            };
            hooks = hooks
                .on(move |req: HasBlobs, host: &Host| {
                    said("blobs/has", &req.hashes.len().to_string());
                    echo::has_blobs(req, host)
                })
                .on(move |req: PutBlob, host: &Host| {
                    said("blobs/put", &req.hash);
                    echo::put_blob(req, host)
                })
                .on(move |req: GetBlob, host: &Host| {
                    said("blobs/get", &req.hash);
                    echo::get_blob(req, host)
                })
                .on(move |req: PutManifest, host: &Host| {
                    said("manifest/put", &req.name);
                    echo::put_manifest(req, host)
                })
                .on(move |req: GetManifest, host: &Host| {
                    said("manifest/get", &req.name);
                    echo::get_manifest(req, host)
                })
                .on(move |req: PublishSet, host: &Host| {
                    said("set/publish", &serde_json::to_string(&req).expect("a request is JSON"));
                    match req.context.settings.get("answer").and_then(Value::as_str) {
                        Some(answer) => {
                            serde_json::from_str(answer).map_err(|e| RpcError::failed("invalid", e.to_string()))
                        }
                        None => echo::publish_set(req, host),
                    }
                });
        }
        if has("providers") {
            hooks = hooks.on(move |req: ProvideCandidates, host: &Host| test.provide(req, host));
        }
        if has("events") {
            hooks = hooks.on_event(heard);
        }
        if has("services") {
            hooks = hooks.on(serve_service);
        }
        hooks
    }

    fn run_command(&self, req: RunCommand, host: &Host) -> Result<Value, RpcError> {
        let p = &req.params;
        if let Some(line) = p["log"].as_str() {
            eprintln!("{line}");
        }
        if let Some(kind) = p["fail"].as_str() {
            let mut e = RpcError::failed(kind, p["message"].as_str().unwrap_or("it failed"));
            e.data.as_mut().expect("a failure has data").fields = p["fields"].as_object().cloned().unwrap_or_default();
            return Err(e);
        }
        if let Some(name) = p["env"].as_str() {
            return Ok(json!(std::env::var(name).ok()));
        }
        if let Some(ms) = p["linger"].as_u64() {
            LINGER.store(ms, Ordering::SeqCst);
        }
        if let Some(file) = p["stopped"].as_str() {
            *STOPPED.lock().unwrap() = Some(file.into());
        }
        if let Some(code) = p["exit"].as_i64() {
            std::process::exit(code as i32);
        }
        if p["stuck"] == true {
            // Never checks for being cancelled.
            loop {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        if p["context"] == true {
            return Ok(serde_json::to_value(&req.context).expect("a context is JSON"));
        }
        if p["cwd"] == true {
            return Ok(json!(std::env::current_dir().map_err(|e| RpcError::failed("io", e.to_string()))?));
        }
        if let Some(choice) = asked(p) {
            return Ok(serde_json::to_value(View::Choose(choice)).expect("a view is JSON"));
        }
        if let Some(param) = p["ask"]["param"].as_str() {
            let text = p[param].to_string();
            return Ok(serde_json::to_value(View::Text { title: None, text }).expect("a view is JSON"));
        }
        if let Some(method) = p["call"].as_str() {
            return Ok(match host.call::<_, Value>(method, &p["params"]) {
                Ok(result) => json!({ "result": result }),
                Err(error) => json!({ "error": error }),
            });
        }
        write_blobs(&req.context, p)?;
        if p["fetch"].is_object() {
            let mut fetch: Fetch = params(p["fetch"].clone())?;
            // A name, in this request's scratch folder.
            let scratch = req.context.scratch.clone().unwrap_or_default();
            for blob in [&mut fetch.body_blob, &mut fetch.into_blob].into_iter().flatten() {
                *blob = scratch.join(&*blob);
            }
            return Ok(match host.fetch(&fetch) {
                Ok(fetched) => {
                    // What it was answered, in its log too.
                    eprintln!("fetched {}", fetched.body.as_deref().unwrap_or_default());
                    let read =
                        fetched.blob.as_ref().map(|b| String::from_utf8_lossy(&std::fs::read(b).unwrap()).into_owned());
                    json!({ "result": fetched, "read": read })
                }
                Err(error) => json!({ "error": error }),
            });
        }
        if let Some(rows) = p["partial"].as_array() {
            // Each row as a part of its own, then the table of them all.
            let rows: Vec<Vec<Value>> = rows.iter().map(|r| vec![r.clone()]).collect();
            for row in &rows {
                host.partial_rows(vec![row.clone()]);
            }
            let view = View::Table { title: None, columns: vec!["Row".into()], rows };
            return Ok(serde_json::to_value(view).expect("a view is JSON"));
        }
        if let Some(items) = p["partial_items"].as_array() {
            let items: Vec<ListItem> =
                items.iter().map(|i| ListItem { label: i.as_str().unwrap_or_default().into(), detail: None }).collect();
            host.partial_items(items.clone());
            return Ok(serde_json::to_value(View::List { title: None, items }).expect("a view is JSON"));
        }
        echo::run_command(req, host)
    }

    fn analyze_track(&self, req: AnalyzeTrack, host: &Host) -> Result<Analysis, RpcError> {
        if self.wrong_fields {
            return Ok(Analysis {
                fields: [("bytes".into(), FieldValue::String("many".into()))].into_iter().collect(),
            });
        }
        if req.analyzer == "heard" {
            // The fields are what the test wrote in the audio file, as JSON.
            let text =
                std::fs::read_to_string(&req.track.audio_path).map_err(|e| RpcError::failed("io", e.to_string()))?;
            let fields = serde_json::from_str(&text).map_err(|e| RpcError::failed("parse", e.to_string()))?;
            return Ok(Analysis { fields });
        }
        if req.analyzer == "order" {
            let audio = &req.track.audio_path;
            let size = std::fs::metadata(audio).ok().map(|m| m.len());
            if size == Some(0) {
                return Err(RpcError::failed("invalid", format!("{} is empty", audio.display())));
            }
            let a = if size == Some(1) { FieldValue::Null } else { FieldValue::Integer(2) };
            let fields = [("a", a), ("b", FieldValue::Integer(1))].map(|(name, v)| (name.to_string(), v));
            return Ok(Analysis { fields: fields.into_iter().collect() });
        }
        echo::analyze_track(req, host)
    }

    fn write_export(&self, req: WriteExport, host: &Host) -> Result<Exported, RpcError> {
        if let Some(outside) = req.options["outside"].as_str() {
            let file = ExportedFile { path: outside.into(), what: String::new(), bytes: 0 };
            return Ok(Exported { files: vec![file], ..Exported::default() });
        }
        let extra = req.options["extra"].as_str().filter(|_| !req.dry_run).map(std::path::PathBuf::from);
        let playlists = req.source.playlists.clone();
        let mut exported = echo::write_export(req, host)?;
        if !playlists.is_empty() {
            exported.notes.push(format!("playlists: {}", playlists.join(", ")));
        }
        if let Some(extra) = extra {
            std::fs::write(&extra, "more").map_err(|e| RpcError::failed("io", e.to_string()))?;
            exported.files.push(ExportedFile { path: extra, what: String::new(), bytes: 4 });
        }
        Ok(exported)
    }

    fn plan_op(&self, req: PlanOp, host: &Host) -> Result<OpAnswer, RpcError> {
        if let Some(choice) = asked(&req.params) {
            return Ok(choice.into());
        }
        write_blobs(&req.context, &req.params)?;
        if let Some(edits) = req.params["edits"].as_array() {
            let inputs = req.params["inputs"].as_array().into_iter().flatten().filter_map(Value::as_str);
            let inputs = inputs.map(std::path::PathBuf::from).collect();
            let (summary, warnings) = ("as given".into(), vec!["given".into()]);
            return Ok(OpPlan { summary, warnings, edits: edits.clone(), inputs }.into());
        }
        echo::plan_op(req, host).map(OpAnswer::from)
    }

    /// Echo's provider (`tags`), or any other answering as its `answer` setting says (JSON):
    /// failing with the error `fail` is, else its `candidates`, with one for each track asked of
    /// each of `each` (a candidate without its track), and `warnings`, once the files `blobs`
    /// gives (by name, their bytes) are in the scratch folder.
    fn provide(&self, req: ProvideCandidates, host: &Host) -> Result<Candidates, RpcError> {
        if req.provider == "tags" {
            return echo::provide(req, host);
        }
        let answer: Value = req.context.settings.get("answer").and_then(Value::as_str).map_or(Ok(json!({})), |a| {
            serde_json::from_str(a).map_err(|e| RpcError::failed("invalid", e.to_string()))
        })?;
        if let Some(kind) = answer["fail"].as_str() {
            return Err(RpcError::failed(kind, "it failed"));
        }
        write_blobs(&req.context, &answer)?;
        let mut candidates: Vec<Candidate> = params(answer.get("candidates").cloned().unwrap_or(json!([])))?;
        for each in answer["each"].as_array().into_iter().flatten() {
            for t in &req.tracks {
                let mut c = each.clone();
                c["track"] = t.id.into();
                candidates.push(params(c)?);
            }
        }
        let warnings = params(answer.get("warnings").cloned().unwrap_or(json!([])))?;
        Ok(Candidates { candidates, warnings })
    }
}

/// An event: logged (`event <name> <drive> <payload>`), and told back to the user, suggesting
/// echo's `playlist` named after it, or as its payload's `tell` says.
fn heard(e: EventNotify, host: &Host) -> Result<(), RpcError> {
    let name = e.event.name();
    eprintln!("event {name} {} {}", e.drive.as_deref().unwrap_or("-"), e.payload);
    let operation = Some(Suggested { name: "playlist".into(), params: json!({ "name": name }) });
    let told = serde_json::from_value(e.payload["tell"].clone());
    host.notify(&told.unwrap_or(HostNotify { message: format!("heard {name}"), operation }));
    Ok(())
}

/// A service, as its name says: `crash` exits as it starts, `quits` answers at once, and any
/// other tells the user it started and runs until it's cancelled. Each logs `service <name>
/// started` first.
fn serve_service(req: StartService, host: &Host) -> Result<(), RpcError> {
    eprintln!("service {} started", req.service);
    match req.service.as_str() {
        "crash" => std::process::exit(3),
        "quits" => Ok(()),
        name => {
            host.notify(&HostNotify { message: format!("{name} is running"), operation: None });
            loop {
                host.ctx().check()?;
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

/// The question `p["ask"]` asks, while `p` doesn't answer it (has no value of its param), or
/// with `ask_again` whatever it has.
fn asked(p: &Value) -> Option<Choice> {
    let choice: Choice = serde_json::from_value(p.get("ask")?.clone()).ok()?;
    (p.get(&choice.param).is_none() || p["ask_again"] == true).then_some(choice)
}

/// Writes the files `p["blobs"]` gives (by name, their bytes) into the request's scratch folder,
/// and makes the links `p["links"]` and hard links `p["hard_links"]` give there (by name, to a
/// path), for the answer to name.
fn write_blobs(context: &Context, p: &Value) -> Result<(), RpcError> {
    let io = |e: std::io::Error| RpcError::failed("io", e.to_string());
    let scratch = || context.scratch.clone().ok_or_else(|| RpcError::failed("invalid", "no scratch folder"));
    for (name, bytes) in p["blobs"].as_object().into_iter().flatten() {
        let bytes: Vec<u8> = params(bytes.clone())?;
        std::fs::write(scratch()?.join(name), bytes).map_err(io)?;
    }
    for (name, to) in p["hard_links"].as_object().into_iter().flatten() {
        std::fs::hard_link(to.as_str().unwrap_or_default(), scratch()?.join(name)).map_err(io)?;
    }
    // Windows lets only an administrator (or developer mode) make one.
    #[cfg(unix)]
    for (name, to) in p["links"].as_object().into_iter().flatten() {
        std::os::unix::fs::symlink(to.as_str().unwrap_or_default(), scratch()?.join(name)).map_err(io)?;
    }
    Ok(())
}
