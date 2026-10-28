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
    Analysis, AnalyzeAction, AnalyzeTrack, Blob, Candidate, Candidates, Choice, Context, EventNotify, ExportAction,
    Exported, ExportedFile, Fetch, FetchAction, FieldValue, Findings, GetBlob, GetManifest, HasBlobs, Hooks, Host,
    HostAction, HostNotify, ImportAnswer, LibraryPage, ListItem, Manifest, OpAnswer, OpPlan, PlanCheck, PlanOp,
    ProvideCandidates, PublishSet, Published, PutBlob, PutManifest, ReadImport, ReadLibrary, RenderPanel, RpcError,
    RunCheck, RunCommand, SourcePlaylist, SourceTrack, StartService, Suggested, View, WriteExport, params,
};
use loclib_progress::Phase;
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
        if has("importers") {
            hooks = hooks.on(read_import);
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
                    if let Some(fetches) = req.context.settings.get("upload").and_then(Value::as_str) {
                        return upload(req.clone(), fetches, host);
                    }
                    match req.context.settings.get("answer").and_then(Value::as_str) {
                        Some(answer) => {
                            serde_json::from_str(answer).map_err(|e| RpcError::failed("invalid", e.to_string()))
                        }
                        None => echo::publish_set(req, host),
                    }
                });
        }
        if has("checks") {
            hooks = hooks.on(|req: RunCheck, host: &Host| match req.check.as_str() {
                "untitled" => echo::run_check(req, host),
                _ => checked(&req.context, &[], host),
            });
            hooks = hooks.on(|req: PlanCheck, host: &Host| match req.check.as_str() {
                "edits" => echo::plan_check(req, host),
                _ => checked(&req.context, &req.edits, host),
            });
        }
        if has("panels") {
            let test = test.clone();
            hooks = hooks.on(move |req: RenderPanel, host: &Host| test.render_panel(req, host));
        }
        if has("sources") {
            hooks = hooks.on(|req: ReadLibrary, _: &Host| read_library(req));
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
        if let Ok(choice) = serde_json::from_value::<Choice>(p["ask"].clone()) {
            let text = choice.questions().map(|q| p[&q.param].to_string()).collect::<Vec<_>>().join(" ");
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
            let files = fetch.multipart.iter_mut().filter_map(|p| p.file.as_mut());
            for blob in [&mut fetch.body_blob, &mut fetch.into_blob].into_iter().flatten().chain(files) {
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
        if let Some(view) = p.get("view") {
            return Ok(view.clone());
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
            // The fields are what the test wrote in the audio file, as JSON, once the files its
            // `blobs` give are in the scratch folder (a waveform's).
            let text =
                std::fs::read_to_string(&req.track.audio_path).map_err(|e| RpcError::failed("io", e.to_string()))?;
            let mut heard: Value = serde_json::from_str(&text).map_err(|e| RpcError::failed("parse", e.to_string()))?;
            write_blobs(&req.context, &heard)?;
            heard.as_object_mut().map(|o| o.remove("blobs"));
            return Ok(Analysis { fields: params(heard)? });
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
        if req.source.set.is_some() || req.source.mix.is_some() {
            return write_tracklist(req, host);
        }
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

    /// Echo's panel (`about`), or any other answering as its `panel` setting says (JSON):
    /// failing with the error `fail` is, else after `sleep` milliseconds (failing as cancelled
    /// once the host cancels it), once the files `blobs` gives are in the scratch folder, its
    /// `view`.
    fn render_panel(&self, req: RenderPanel, host: &Host) -> Result<View, RpcError> {
        if req.panel == "about" {
            return echo::render_panel(req, host);
        }
        let answer: Value = req.context.settings.get("panel").and_then(Value::as_str).map_or(Ok(json!({})), |a| {
            serde_json::from_str(a).map_err(|e| RpcError::failed("invalid", e.to_string()))
        })?;
        if let Some(kind) = answer["fail"].as_str() {
            return Err(RpcError::failed(kind, "it failed"));
        }
        let until = std::time::Instant::now() + Duration::from_millis(answer["sleep"].as_u64().unwrap_or(0));
        while std::time::Instant::now() < until {
            host.ctx().check()?;
            std::thread::sleep(Duration::from_millis(5));
        }
        write_blobs(&req.context, &answer)?;
        params(answer.get("view").cloned().unwrap_or(json!({ "kind": "text", "text": "" })))
    }
}

/// A set exporter's or a mix exporter's: the set's tracks (`sets.get`), or the mix's plays
/// (`mix.reports.get`), a line each (`Artist - Title`), with the drive and the set or the mix it
/// was given in notes.
fn write_tracklist(req: WriteExport, host: &Host) -> Result<Exported, RpcError> {
    let names: Vec<String> = match (&req.source.set, &req.source.mix) {
        (Some(set), _) => {
            let set: Value = host.call("sets.get", &json!({ "set": set }))?;
            let tracks = set["tracks"].as_array().cloned().unwrap_or_default();
            tracks
                .iter()
                .map(|t| format!("{} - {}", t["artist"].as_str().unwrap_or(""), t["title"].as_str().unwrap_or("")))
                .collect()
        }
        (None, mix) => {
            let mix: Value = host.call("mix.reports.get", &json!({ "id": mix }))?;
            let plays = mix["plays"].as_array().cloned().unwrap_or_default();
            plays
                .iter()
                .map(|p| {
                    format!(
                        "{} - {}",
                        p["track"]["artist"].as_str().unwrap_or(""),
                        p["track"]["title"].as_str().unwrap_or("")
                    )
                })
                .collect()
        }
    };
    let text: String = names.iter().map(|n| format!("{n}\n")).collect();
    if !req.dry_run {
        std::fs::write(&req.output, &text).map_err(|e| RpcError::failed("io", e.to_string()))?;
    }
    let file = ExportedFile { path: req.output, what: "tracklist".into(), bytes: text.len() as u64 };
    let counts = [("tracks".to_string(), names.len() as u64)].into_iter().collect();
    let of = req.source.set.as_ref().map(|s| format!("set: {s}"));
    let notes = vec![
        format!("drive: {}", req.source.drive),
        of.unwrap_or_else(|| format!("mix: {}", req.source.mix.unwrap_or_default())),
    ];
    Ok(Exported { files: vec![file], counts, notes, ..Exported::default() })
}

/// An importer: the file as JSON, what it answers (`{"tracks": ..., "sessions": ...}`, as the
/// test wrote it), in a file in its scratch folder too with the `blob` option (named as it says);
/// any other file as echo's importer reads it.
fn read_import(req: ReadImport, host: &Host) -> Result<ImportAnswer, RpcError> {
    let text = std::fs::read_to_string(&req.file).map_err(|e| RpcError::failed("io", e.to_string()))?;
    let Ok(answer) = serde_json::from_str::<Value>(&text) else { return echo::read_import(req, host).map(Into::into) };
    if let Some(name) = req.options["blob"].as_str() {
        let scratch = req.context.scratch.clone().ok_or_else(|| RpcError::failed("invalid", "no scratch folder"))?;
        std::fs::write(scratch.join(name), &text).map_err(|e| RpcError::failed("io", e.to_string()))?;
        return Ok(Blob { blob: name.into() }.into());
    }
    Ok(ImportAnswer::Read(params(answer)?))
}

/// An event: logged (`event <name> <drive> <payload>`), and told back to the user, suggesting
/// echo's `playlist` named after it, or as its payload's `tell` says.
fn heard(e: EventNotify, host: &Host) -> Result<(), RpcError> {
    let name = e.event.name();
    eprintln!("event {name} {} {}", e.drive.as_deref().unwrap_or("-"), e.payload);
    let operation = Some(Suggested { name: "playlist".into(), params: json!({ "name": name }) });
    let told = serde_json::from_value(e.payload["tell"].clone());
    host.notify(&told.unwrap_or(HostNotify { message: format!("heard {name}"), operation, action: None }));
    Ok(())
}

/// What the `suggests` service suggests: each of the host's actions echo has contributions for
/// (its metadata provider, its exporter and its analyzer), on the drive `USB`.
fn suggestions(m: &Manifest) -> [(&'static str, HostAction); 3] {
    let drive = "USB".to_string();
    let fetch = FetchAction { drive: drive.clone(), tracks: vec![1], provider: Some("tags".into()) };
    let format = m.contributes.exporters[0].format.clone();
    let export = ExportAction { format, drive: Some(drive.clone()), ..ExportAction::default() };
    let analyze = AnalyzeAction { analyzer: m.contributes.analyzers[0].name.clone(), drive, tracks: Vec::new() };
    [
        ("fetch track 1's tags", HostAction::TrackFetch(fetch)),
        ("export USB", HostAction::Export(export)),
        ("analyze USB", HostAction::Analyze(analyze)),
    ]
}

/// A service, as its name says: `crash` exits as it starts, `quits` answers at once, `suggests`
/// suggests each of [`suggestions`] and any other tells the user it started, and both run until
/// they're cancelled. Each logs `service <name> started` first.
fn serve_service(req: StartService, host: &Host) -> Result<(), RpcError> {
    eprintln!("service {} started", req.service);
    match req.service.as_str() {
        "crash" => std::process::exit(3),
        "quits" => Ok(()),
        name => {
            match name {
                "suggests" => {
                    suggestions(host.manifest()).iter().for_each(|(m, a)| host.notify(&HostNotify::action(*m, a)));
                }
                _ => host.notify(&HostNotify { message: format!("{name} is running"), ..HostNotify::default() }),
            }
            loop {
                host.ctx().check()?;
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

/// Any source: the files in its folders granted (and below), but `.json` files, each a track with
/// what the `.json` file named after it (`a.wav.json`) gives besides its path, once the files that
/// file's `blobs` give are in the scratch folder; and the playlists a folder's `playlists.json`
/// gives (`{"Sets/Friday": ["a.wav"]}`, files by their path in the folder). Its options: `also`,
/// paths to give as tracks as they are; `per_page`, how many tracks a page has (playlists come
/// on the last); `as_blob`, each page in a blob; `again`, every page says there's one after it.
fn read_library(req: ReadLibrary) -> Result<LibraryPage, RpcError> {
    let io = |e: std::io::Error| RpcError::failed("io", e.to_string());
    let (mut tracks, mut playlists) = (Vec::new(), Vec::new());
    for root in &req.roots {
        let mut files = Vec::new();
        let mut folders = vec![root.clone()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).map_err(io)? {
                let path = entry.map_err(io)?.path();
                match path.is_dir() {
                    true => folders.push(path),
                    false => files.push(path),
                }
            }
        }
        files.sort();
        for file in files.into_iter().filter(|f| f.extension().is_none_or(|e| e != "json")) {
            let mut told: Value = match std::fs::read_to_string(format!("{}.json", file.display())) {
                Ok(text) => serde_json::from_str(&text).map_err(|e| RpcError::failed("parse", e.to_string()))?,
                Err(_) => json!({}),
            };
            write_blobs(&req.context, &told)?;
            told.as_object_mut().map(|o| o.remove("blobs"));
            told["path"] = json!(file);
            tracks.push(params::<SourceTrack>(told)?);
        }
        if let Ok(text) = std::fs::read_to_string(root.join("playlists.json")) {
            let lists: indexmap::IndexMap<String, Vec<String>> =
                serde_json::from_str(&text).map_err(|e| RpcError::failed("parse", e.to_string()))?;
            playlists.extend(
                lists
                    .into_iter()
                    .map(|(path, names)| SourcePlaylist { path, tracks: names.iter().map(|n| root.join(n)).collect() }),
            );
        }
    }
    let also = req.options["also"].as_array().into_iter().flatten().filter_map(Value::as_str);
    tracks.extend(also.map(|p| SourceTrack { path: p.into(), ..SourceTrack::default() }));
    let from: usize = req.cursor.as_deref().map_or(0, |c| c.parse().unwrap_or(0));
    let per_page = req.options["per_page"].as_u64().map_or(tracks.len().max(1), |n| n as usize);
    let to = (from + per_page).min(tracks.len());
    let last = to >= tracks.len() && req.options["again"] != true;
    let page = LibraryPage {
        tracks: tracks.drain(from.min(to)..to).collect(),
        playlists: if last { playlists } else { Vec::new() },
        next: (!last).then(|| to.to_string()),
        ..LibraryPage::default()
    };
    if req.options["as_blob"] != true {
        return Ok(page);
    }
    let scratch = req.context.scratch.clone().ok_or_else(|| RpcError::failed("invalid", "no scratch folder"))?;
    std::fs::write(scratch.join("page.json"), serde_json::to_vec(&page).expect("a page is JSON")).map_err(io)?;
    Ok(LibraryPage { blob: Some("page.json".into()), next: page.next, ..LibraryPage::default() })
}

/// What a check (or a guard) other than echo's answers, as its `findings` setting says (JSON):
/// failing with the error `fail` is, waiting until it's cancelled with `wait`, never stopping with
/// `stuck`, else its `findings`; with `about_edits`, only when it's given `edits` (a guard's).
fn checked(context: &Context, edits: &[Value], host: &Host) -> Result<Findings, RpcError> {
    let answer: Value =
        context.settings.get("findings").and_then(Value::as_str).map_or(Ok(json!({})), |a| {
            serde_json::from_str(a).map_err(|e| RpcError::failed("invalid", e.to_string()))
        })?;
    if let Some(kind) = answer["fail"].as_str() {
        return Err(RpcError::failed(kind, "it failed"));
    }
    if answer["wait"] == true {
        loop {
            host.ctx().check()?;
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    if answer["stuck"] == true {
        // Never checks for being cancelled.
        loop {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    if answer["about_edits"] == true && edits.is_empty() {
        return Ok(Findings::default());
    }
    Ok(Findings { findings: params(answer.get("findings").cloned().unwrap_or(json!([])))? })
}

/// The question `p["ask"]` asks, while `p` doesn't answer it (has no value of its param), or
/// with `ask_again` whatever it has.
fn asked(p: &Value) -> Option<Choice> {
    let choice: Choice = serde_json::from_value(p.get("ask")?.clone()).ok()?;
    (p.get(&choice.param).is_none() || p["ask_again"] == true).then_some(choice)
}

/// Publishes a set by sending the `net/fetch` requests `fetches` (JSON) names, `recording` in a
/// file's place being the set's, between the start and end of an upload of the recording's size:
/// a request that fails fails it, or for a dry run is a warning. Then as echo does.
fn upload(req: PublishSet, fetches: &str, host: &Host) -> Result<Published, RpcError> {
    let mut fetches: Vec<Fetch> =
        serde_json::from_str(fetches).map_err(|e| RpcError::failed("invalid", e.to_string()))?;
    let size = std::fs::metadata(&req.recording).map_err(|e| RpcError::failed("io", e.to_string()))?.len();
    let ctx = host.ctx();
    ctx.phase_start(Phase::Upload, 0, size);
    let mut warnings = Vec::new();
    for fetch in &mut fetches {
        let files = fetch.multipart.iter_mut().filter_map(|p| p.file.as_mut());
        for file in fetch.body_blob.iter_mut().chain(files).filter(|f| f.as_os_str() == "recording") {
            *file = req.recording.clone();
        }
        match host.fetch(fetch) {
            Ok(_) => {}
            Err(e) if req.dry_run => warnings.push(e.message),
            Err(e) => return Err(e),
        }
    }
    ctx.phase_end(Phase::Upload);
    let published = echo::publish_set(req, host)?;
    Ok(Published { warnings: [published.warnings, warnings].concat(), ..published })
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
