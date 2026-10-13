//! The plugin the plugin host's tests run (`tests/it/plugins.rs`): loclib-plugin's echo example,
//! with what they need besides. Its `echo` command logs (`log`), exits (`exit`), never stops
//! (`stuck`), answers its context (`context`) or calls back (`call` with `params`) when its
//! params say so; its operation answers the `edits` it's given, and its exporter would write to
//! `outside`, or writes `extra` too without saying so first. `--manifest <file>` serves another
//! manifest, `--protocol <version>` answers `hello` with another protocol and stops, `--silent`
//! never answers, and `--wrong-fields` answers fields of the wrong type.

#[path = "../../../loclib-plugin/examples/echo/plugin.rs"]
mod echo;

use std::io::BufRead;
use std::time::Duration;

use loclib_plugin::{
    Analysis, AnalyzeTrack, Exported, FieldValue, Host, Manifest, OpPlan, PlanOp, Plugin, RpcError, RunCommand,
    WriteExport, serve,
};
use serde_json::{Value, json};

struct Test {
    wrong_fields: bool,
}

impl Plugin for Test {
    fn run_command(&self, req: RunCommand, host: &Host) -> Result<Value, RpcError> {
        let p = &req.params;
        if let Some(line) = p["log"].as_str() {
            eprintln!("{line}");
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
        if let Some(method) = p["call"].as_str() {
            return Ok(match host.call::<_, Value>(method, &p["params"]) {
                Ok(result) => json!({ "result": result }),
                Err(error) => json!({ "error": error }),
            });
        }
        echo::Echo.run_command(req, host)
    }

    fn analyze_track(&self, req: AnalyzeTrack, host: &Host) -> Result<Analysis, RpcError> {
        if self.wrong_fields {
            return Ok(Analysis {
                fields: [("bytes".into(), FieldValue::String("many".into()))].into_iter().collect(),
            });
        }
        echo::Echo.analyze_track(req, host)
    }

    fn write_export(&self, req: WriteExport, host: &Host) -> Result<Exported, RpcError> {
        if let Some(outside) = req.options["outside"].as_str() {
            return Ok(Exported { files: vec![outside.into()], warnings: Vec::new() });
        }
        let extra = req.options["extra"].as_str().filter(|_| !req.dry_run).map(std::path::PathBuf::from);
        let mut exported = echo::Echo.write_export(req, host)?;
        if let Some(extra) = extra {
            std::fs::write(&extra, "more").map_err(|e| RpcError::failed("io", e.to_string()))?;
            exported.files.push(extra);
        }
        Ok(exported)
    }

    fn plan_op(&self, req: PlanOp, host: &Host) -> Result<OpPlan, RpcError> {
        if let Some(edits) = req.params["edits"].as_array() {
            return Ok(OpPlan { summary: "as given".into(), warnings: vec!["given".into()], edits: edits.clone() });
        }
        echo::Echo.plan_op(req, host)
    }
}

fn main() {
    let (mut manifest, mut wrong_fields) = (echo::manifest(), false);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().expect("a value");
        match arg.as_str() {
            "--manifest" => manifest = Manifest::parse(&std::fs::read_to_string(value()).unwrap()).unwrap(),
            "--protocol" => return hello(&value()),
            "--silent" => return std::io::stdin().lines().for_each(drop),
            "--wrong-fields" => wrong_fields = true,
            other => panic!("unknown argument {other}"),
        }
    }
    serve(&manifest, Test { wrong_fields }, std::io::stdin(), std::io::stdout());
}

/// Answers `hello` speaking `protocol`, and stops.
fn hello(protocol: &str) {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).unwrap();
    let hello: Value = serde_json::from_str(&line).unwrap();
    let plugin = json!({ "id": "echo", "version": "0.1.0" });
    println!(
        "{}",
        json!({ "jsonrpc": "2.0", "id": hello["id"], "result": { "protocol": protocol, "plugin": plugin } })
    );
}
