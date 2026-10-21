//! The echo plugin: one of each kind of contribution, doing as little as it can.

use std::time::Duration;

use loclib_plugin::{
    Analysis, AnalyzeTrack, Candidate, Candidates, CheckFinding, Exported, ExportedFile, FieldValue, Finding, Findings,
    Fix, Hooks, Host, Imported, ImportedTrack, Level, OpPlan, Pair, PlanCheck, PlanOp, ProvideCandidates, ReadImport,
    RenderPanel, RpcError, RunCheck, RunCommand, TrackMatch, TrackMetadata, TracksQuery, View, WriteExport, params,
};
use loclib_progress::{Event, Phase, Steps};
use serde::Deserialize;
use serde_json::{Value, json};

pub const MANIFEST: &str = include_str!("locallibrary-plugin.toml");

/// Its hooks: one of each kind.
pub fn hooks() -> Hooks {
    Hooks::new()
        .on(run_command)
        .on(analyze_track)
        .on(write_export)
        .on(read_import)
        .on(plan_op)
        .on(provide)
        .on(render_panel)
        .on(run_check)
        .on(plan_check)
}

#[derive(Deserialize)]
struct PlaylistParams {
    name: String,
}

/// `command/run`.
pub fn run_command(req: RunCommand, host: &Host) -> Result<Value, RpcError> {
    match req.command.as_str() {
        "echo" => Ok(req.params),
        "titles" => {
            let drive = req.context.drive.unwrap_or_default();
            let tracks = host.drive(&drive).query(&TracksQuery::default())?;
            let rows = tracks.into_iter().map(|t| vec![Value::from(t.title)]).collect();
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

/// `track/analyze`.
pub fn analyze_track(req: AnalyzeTrack, host: &Host) -> Result<Analysis, RpcError> {
    let bytes = std::fs::metadata(&req.track.audio_path).map_err(|e| RpcError::failure(&e, "io", &host.ctx()))?.len();
    let bytes = FieldValue::Integer(bytes.try_into().unwrap_or(i64::MAX));
    Ok(Analysis { fields: [("bytes".into(), bytes)].into_iter().collect() })
}

/// `export/write`.
pub fn write_export(req: WriteExport, host: &Host) -> Result<Exported, RpcError> {
    let tracks = host.drive(&req.source.drive).query(&TracksQuery::default())?;
    let lines: Vec<String> = tracks.into_iter().map(|t| t.file_path).collect();
    let text = format!("#EXTM3U\n{}\n", lines.join("\n"));
    if !req.dry_run {
        std::fs::write(&req.output, &text).map_err(|e| RpcError::failure(&e, "io", &host.ctx()))?;
    }
    let file = ExportedFile { path: req.output, what: "playlist".into(), bytes: text.len() as u64 };
    let counts = [("tracks".to_string(), lines.len() as u64)].into_iter().collect();
    Ok(Exported { files: vec![file], counts, ..Exported::default() })
}

/// `import/read`: each line of the file is a track's audio file, given the My Tag `Genre/Echo`.
pub fn read_import(req: ReadImport, host: &Host) -> Result<Imported, RpcError> {
    let text = std::fs::read_to_string(&req.file).map_err(|e| RpcError::failure(&e, "io", &host.ctx()))?;
    let tracks = text.lines().map(str::trim).filter(|l| !l.is_empty()).map(|path| ImportedTrack {
        find: TrackMatch { path: Some(path.into()), ..TrackMatch::default() },
        my_tags: vec!["Genre/Echo".into()],
        ..ImportedTrack::default()
    });
    Ok(Imported { tracks: tracks.collect(), ..Imported::default() })
}

/// `op/plan`.
pub fn plan_op(req: PlanOp, _: &Host) -> Result<OpPlan, RpcError> {
    let PlaylistParams { name } = params(req.params)?;
    Ok(OpPlan {
        summary: format!("make the playlist {name}"),
        warnings: Vec::new(),
        edits: vec![json!({ "op": "create_playlist", "path": name })],
        inputs: Vec::new(),
    })
}

/// `provide/candidates`: each track's tags, as they are.
pub fn provide(req: ProvideCandidates, _: &Host) -> Result<Candidates, RpcError> {
    let tagged = |t: &str| (!t.is_empty()).then(|| t.to_string());
    let candidates = req.tracks.into_iter().map(|t| Candidate {
        track: t.id,
        score: 1.0,
        source: "Echo".into(),
        label: format!("{} - {}", t.artist, t.title),
        artwork: None,
        metadata: Some(TrackMetadata {
            title: tagged(&t.title),
            artist: tagged(&t.artist),
            album: tagged(&t.album),
            ..TrackMetadata::default()
        }),
    });
    Ok(Candidates { candidates: candidates.collect(), warnings: Vec::new() })
}

/// `check/run`: the drive's tracks without a title, its fix making the playlist `Untitled` (its
/// operation, as it does).
pub fn run_check(req: RunCheck, host: &Host) -> Result<Findings, RpcError> {
    let drive = req.context.drive.unwrap_or_default();
    let untitled: Vec<u32> = host
        .drive(&drive)
        .query(&TracksQuery::default())?
        .into_iter()
        .filter(|t| t.title.is_empty())
        .map(|t| t.id)
        .collect();
    if untitled.is_empty() {
        return Ok(Findings::default());
    }
    let finding = Finding {
        tracks: untitled.clone(),
        ..Finding::new(Level::Warning, req.check, format!("{} track(s) have no title", untitled.len()))
    };
    let fix = Fix { operation: "playlist".into(), params: json!({ "name": "Untitled" }) };
    Ok(Findings { findings: vec![CheckFinding { finding, fix: Some(fix) }] })
}

/// `plan/check`: how many edits the plan makes.
pub fn plan_check(req: PlanCheck, _: &Host) -> Result<Findings, RpcError> {
    let message = format!("{} makes {} edit(s)", req.operation, req.edits.len());
    let finding = Finding::new(Level::Info, req.check, message);
    Ok(Findings { findings: vec![CheckFinding { finding, fix: None }] })
}

/// `panel/render`: the track's id and title.
pub fn render_panel(req: RenderPanel, host: &Host) -> Result<View, RpcError> {
    let drive = req.context.drive.unwrap_or_default();
    let found = host.drive(&drive).tracks(&req.context.tracks)?;
    let pairs = found.tracks.into_iter().flat_map(|t| {
        [Pair { label: "id".into(), value: t.id.into() }, Pair { label: "title".into(), value: t.title.into() }]
    });
    Ok(View::Kv { title: None, pairs: pairs.collect() })
}
