//! Mixes: which of a drive's tracks a recorded DJ set plays.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{default_drive, known_drives, registry, resolve_drive};
use crate::device::Device;
use crate::mix::index::{Cache, IndexReport};
use crate::mix::report::{self, Drive, PlayExtras, SavedReport, ShownFields};
use crate::mix::{Identifier, MixReport};
use crate::profile::ProfilePrefs;
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Result, fsutil};

/// The drive whose library mixes are matched against: the one named, else the one a command
/// takes when given none (see [`default_drive`]).
pub fn mix_drive(spec: Option<&str>) -> Result<Device> {
    match spec {
        Some(spec) => resolve_drive(spec),
        None => default_drive(registry().as_ref(), "match against"),
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct IndexMixLibrary {
    /// Default: the only mounted rekordbox drive.
    #[serde(default)]
    pub drive: Option<String>,
}

/// Fingerprint a drive's tracks ahead of identifying mixes (identifying does this on demand).
/// Fingerprints are cached in the store, so each audio file is decoded once.
pub fn mix_index(store: &Store, req: &IndexMixLibrary, ctx: &Ctx<'_>) -> Result<IndexReport> {
    let device = mix_drive(req.drive.as_deref())?;
    Ok(Identifier::new(&device, &Cache::in_store(store), ctx)?.report)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct IdentifyMix {
    /// Audio file or Audacity project (`.aup3`/`.aup4`).
    pub mix: PathBuf,
    #[serde(default)]
    pub drive: Option<String>,
    /// Playback speeds to search, as a fraction: 0.16 is ±16%.
    #[serde(default)]
    pub tempo_range: Option<f64>,
    /// The mix's cover, an image file, for its report and export (instead of one embedded in
    /// the recording). Checked before identifying.
    #[serde(default)]
    pub cover: Option<PathBuf>,
}
/// `mix.identify`'s request ([`super::mix_identify_cached`]): the mix, and what to answer with
/// its report.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct IdentifyMixRequest {
    #[serde(flatten)]
    pub identify: IdentifyMix,
    /// Also each play's overview waveform, hot cues and phrases (`extras`), for following
    /// along without asking for every track's analysis.
    #[serde(default)]
    pub extras: bool,
    /// Keep the report in the store's reports folder (`saved` is its id; see
    /// [`mix_reports_list`]).
    #[serde(default)]
    pub save: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ExportMix {
    #[serde(flatten)]
    pub identify: IdentifyMix,
    /// Also write the document to this file.
    #[serde(default)]
    pub output: Option<PathBuf>,
    /// Replace a file already at `output` (refused otherwise).
    #[serde(default)]
    pub replace: bool,
}

/// A mix's report with what a front end needs to show it and follow along.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct IdentifiedMix {
    #[serde(flatten)]
    pub report: MixReport,
    /// Audio a browser can play for the mix: the recording, or for an Audacity project an export
    /// next to it (the file is there; `null` when there's none).
    pub audio: Option<PathBuf>,
    /// Each play's overview waveform, hot cues and phrases from the drive, in the order of
    /// `plays` (asked for with `extras`; empty when the drive isn't mounted).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extras: Vec<PlayExtras>,
    /// The id the report is kept under in the store's reports (`mix.reports.get`), when saved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved: Option<String>,
}

impl IdentifiedMix {
    /// `report`, with its audio and the extras `drive` gives (when asked for).
    pub(crate) fn of(report: MixReport, drive: Option<Drive<'_>>, extras: bool) -> IdentifiedMix {
        let extras = match (extras, drive) {
            (true, Some(d)) => report.plays.iter().enumerate().map(|(i, p)| report::extras(d, p, i, false)).collect(),
            _ => Vec::new(),
        };
        let audio = report::playable_audio(&report.mix.path).filter(|a| a.is_file());
        IdentifiedMix { report, audio, extras, saved: None }
    }
}

/// The drive a kept report was matched against, when it's mounted: by its identity (or, for a
/// drive without one, its name).
fn report_drive(r: &MixReport) -> Option<Device> {
    known_drives(true).into_iter().find(|d| r.matched_against(d))
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ReportRef {
    /// The report's id (`mix.reports.list`).
    pub id: String,
    /// Also each play's waveform, hot cues and phrases, from the drive the mix was matched
    /// against when it's mounted.
    #[serde(default)]
    pub extras: bool,
}

/// The reports kept in the store's reports folder, newest first.
pub fn mix_reports_list(store_root: &Path) -> Result<Vec<SavedReport>> {
    report::list(store_root)
}

/// A report kept in the store's reports folder.
pub fn mix_report_get(store_root: &Path, req: &ReportRef) -> Result<IdentifiedMix> {
    let r = report::load(store_root, &req.id)?;
    let device = req.extras.then(|| report_drive(&r)).flatten();
    let library = device.as_ref().and_then(|d| d.library().ok());
    let drive = device.as_ref().zip(library.as_ref()).map(|(d, l)| Drive::new(d, l));
    let mut got = IdentifiedMix::of(r, drive, req.extras);
    got.saved = Some(req.id.clone());
    Ok(got)
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ReportHtml {
    /// The report's id (`mix.reports.list`).
    pub id: String,
    /// Also answer the page itself (otherwise just where it is).
    #[serde(default)]
    pub inline: bool,
}

/// Where a report's interactive page was written.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ReportPage {
    pub id: String,
    /// `<id>.html` in the store's reports folder: open it in a browser or a webview.
    pub path: PathBuf,
    pub bytes: u64,
    /// The page, when asked for with `inline`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
}

/// Write a kept report's interactive page into the store's reports folder (again), with the
/// tracks' waveforms, cues, phrases, covers and the `fields` shown from the drive when it's
/// mounted, the mix's audio linked relative to the page, and the tracks the profile hides
/// unnamed.
pub fn mix_report_html(store: &Store, req: &ReportHtml, fields: Option<ShownFields<'_>>) -> Result<ReportPage> {
    let r = report::load(store.root(), &req.id)?;
    let device = report_drive(&r);
    let library = device.as_ref().and_then(|d| d.library().ok());
    let drive = device.as_ref().zip(library.as_ref()).map(|(d, l)| Drive { fields, ..Drive::new(d, l) });
    let path = report::reports_dir(store.root()).join(format!("{}.html", req.id));
    let audio = report::playable_audio(&r.mix.path).filter(|a| a.is_file());
    let page = report::page(&r, drive, audio.as_deref(), &path, &ProfilePrefs::load(store)?, false);
    fsutil::write_atomic(&path, page.as_bytes())?;
    Ok(ReportPage { id: req.id.clone(), path, bytes: page.len() as u64, html: req.inline.then_some(page) })
}
