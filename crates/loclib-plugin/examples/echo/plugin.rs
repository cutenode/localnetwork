//! The echo plugin: one of each kind of contribution, doing as little as it can.

use std::time::Duration;

use loclib_plugin::{
    Analysis, AnalyzeTrack, Exported, FieldValue, Host, Manifest, OpPlan, PlanOp, Plugin, RpcError, RunCommand, View,
    WriteExport, params,
};
use loclib_progress::{Event, Phase, Steps};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn manifest() -> Manifest {
    Manifest::parse(include_str!("locallibrary-plugin.toml")).expect("the manifest parses")
}

pub struct Echo;

/// `tracks.query`'s result, as far as this plugin reads it.
#[derive(Deserialize)]
struct TracksPage {
    tracks: Vec<TrackRow>,
}

#[derive(Deserialize)]
struct TrackRow {
    title: String,
    file_path: String,
}

fn tracks(host: &Host, drive: &str) -> Result<Vec<TrackRow>, RpcError> {
    Ok(host.call::<_, TracksPage>("tracks.query", &json!({ "drive": drive }))?.tracks)
}

#[derive(Deserialize)]
struct PlaylistParams {
    name: String,
}

impl Plugin for Echo {
    fn run_command(&self, req: RunCommand, host: &Host) -> Result<Value, RpcError> {
        match req.command.as_str() {
            "echo" => Ok(req.params),
            "titles" => {
                let drive = req.context.drive.unwrap_or_default();
                let rows = tracks(host, &drive)?.into_iter().map(|t| vec![Value::from(t.title)]).collect();
                let view = View::Table { title: Some(drive), columns: vec!["Title".into()], rows };
                Ok(serde_json::to_value(view).expect("a view is JSON"))
            }
            "steps" => {
                let ctx = host.ctx();
                let steps = Steps::new(&ctx, vec!["scan"]);
                steps.begin("scan");
                for track in ["a.mp3", "b.mp3", "c.mp3"] {
                    let item = Event::Item { phase: Phase::Scan, path: track.into(), bytes: 0 };
                    steps.ctx().for_target("USB", |ctx| ctx.emit(item));
                }
                Ok(Value::Null)
            }
            _ => {
                let ctx = host.ctx();
                ctx.phase_start(Phase::Scan, 0, 0);
                // Held back, then sent once it has waited, though no event follows.
                ctx.emit(Event::Item { phase: Phase::Scan, path: "a.mp3".into(), bytes: 0 });
                loop {
                    ctx.check()?;
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }

    fn analyze_track(&self, req: AnalyzeTrack, _: &Host) -> Result<Analysis, RpcError> {
        let bytes = std::fs::metadata(&req.track.audio_path).map_err(|e| RpcError::failed("io", e.to_string()))?.len();
        let bytes = FieldValue::Integer(bytes.try_into().unwrap_or(i64::MAX));
        Ok(Analysis { fields: [("bytes".into(), bytes)].into_iter().collect() })
    }

    fn write_export(&self, req: WriteExport, host: &Host) -> Result<Exported, RpcError> {
        if req.dry_run {
            return Ok(Exported { files: vec![req.output], warnings: Vec::new() });
        }
        let lines: Vec<String> = tracks(host, &req.source.drive)?.into_iter().map(|t| t.file_path).collect();
        std::fs::write(&req.output, format!("#EXTM3U\n{}\n", lines.join("\n")))
            .map_err(|e| RpcError::failed("io", e.to_string()))?;
        Ok(Exported { files: vec![req.output], warnings: Vec::new() })
    }

    fn plan_op(&self, req: PlanOp, _: &Host) -> Result<OpPlan, RpcError> {
        let PlaylistParams { name } = params(req.params)?;
        Ok(OpPlan {
            summary: format!("make the playlist {name}"),
            warnings: Vec::new(),
            edits: vec![json!({ "op": "create_playlist", "path": name })],
        })
    }
}
