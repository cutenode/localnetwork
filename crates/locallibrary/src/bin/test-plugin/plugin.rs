//! The test plugin's hooks: loclib-plugin's echo example, with what the tests need besides (see
//! `main.rs`). The CLI's tests serve it from the locallibrary binary too
//! (`LOCALLIBRARY_TEST_PLUGINS`, in builds with debug assertions).

// The echo plugin's manifest and hooks are for `main.rs`: locallibrary serves the manifest a
// test gives, and these hooks.
#[allow(dead_code)]
#[path = "../../../../loclib-plugin/examples/echo/plugin.rs"]
pub mod echo;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use loclib_plugin::{
    Analysis, AnalyzeTrack, Exported, ExportedFile, FieldValue, Hooks, Host, OpPlan, PlanOp, RpcError, RunCommand,
    WriteExport,
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
            hooks = hooks.on(move |req: PlanOp, host: &Host| test.plan_op(req, host));
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
        if let Some(method) = p["call"].as_str() {
            return Ok(match host.call::<_, Value>(method, &p["params"]) {
                Ok(result) => json!({ "result": result }),
                Err(error) => json!({ "error": error }),
            });
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

    fn plan_op(&self, req: PlanOp, host: &Host) -> Result<OpPlan, RpcError> {
        if let Some(edits) = req.params["edits"].as_array() {
            let inputs = req.params["inputs"].as_array().into_iter().flatten().filter_map(Value::as_str);
            let inputs = inputs.map(std::path::PathBuf::from).collect();
            let (summary, warnings) = ("as given".into(), vec!["given".into()]);
            return Ok(OpPlan { summary, warnings, edits: edits.clone(), inputs });
        }
        echo::plan_op(req, host)
    }
}
