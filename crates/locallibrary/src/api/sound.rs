//! Scoring tempo and key analyzers against annotated datasets.

use schemars::JsonSchema;
use serde::Deserialize;

use super::resolve_drive;
use crate::Result;
use crate::progress::Ctx;
use crate::sound::Cache;
use crate::sound::bench::{self, BenchOptions, BenchReport};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AnalysisBenchRequest {
    /// A drive (or library) whose stored keys and tempos are scored too, and whose tracks are
    /// analysed when no `audio` folder is given.
    #[serde(default)]
    pub drive: Option<String>,
    #[serde(default, flatten)]
    pub options: BenchOptions,
}

/// Score rekordbox's stored keys and tempos (with a drive) and loclib-analysis's estimates
/// against GiantSteps annotations. Analyses are cached in the store, like doctor's. Read-only.
pub fn analysis_bench(req: &AnalysisBenchRequest, ctx: &Ctx<'_>) -> Result<BenchReport> {
    let device = req.drive.as_deref().map(resolve_drive).transpose()?;
    let library = device.as_ref().map(|d| d.library()).transpose()?;
    let (cache, _temp) = Cache::of_default_store()?;
    bench::bench(&req.options, device.as_ref().zip(library.as_ref()), &cache, ctx)
}
