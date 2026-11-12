//! Scoring tempo and key analyzers against annotated datasets.

use schemars::JsonSchema;
use serde::Deserialize;

use super::resolve_drive;
use crate::Result;
use crate::fields::{Analyzer, Fields};
use crate::plugin::Audio;
use crate::progress::Ctx;
use crate::sound::bench::{self, BenchOptions, BenchReport};
use crate::store::Store;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AnalysisBenchRequest {
    /// A drive (or library) whose stored keys and tempos are scored too, and whose tracks are
    /// analysed when no `audio` folder is given.
    #[serde(default)]
    pub drive: Option<String>,
    #[serde(default, flatten)]
    pub options: BenchOptions,
}

/// Score rekordbox's stored keys and tempos (with a drive) and what the analyzers chosen in
/// `store` for the `key` and `bpm` roles hear against GiantSteps annotations, the app's `measure`
/// running them on the audio every run (nothing is kept, see [`bench::bench`]). Read-only.
pub fn analysis_bench(
    req: &AnalysisBenchRequest,
    store: &Store,
    measure: impl Fn(&Analyzer, Audio<'_>) -> Result<Vec<Result<Fields>>>,
    ctx: &Ctx<'_>,
) -> Result<BenchReport> {
    let device = req.drive.as_deref().map(resolve_drive).transpose()?;
    let library = device.as_ref().map(|d| d.library()).transpose()?;
    bench::bench(&req.options, device.as_ref().zip(library.as_ref()), store, measure, ctx)
}
