//! Providers (`provide/candidates`, the `providers` feature): what plugins find for a drive's
//! tracks, cover art or tags. The host asks every provider of a kind about the tracks the user
//! chose, merges what they answer, best first for each track, and picks for the user the one
//! candidate alone scoring the `providers.auto_pick` setting or more, when there's one. The user
//! picks the others ([`Offer::choice`]), and the host makes the edits itself ([`plan_provided`]:
//! `SetArtwork` for an image, `EditTracks` for tags) as one edit plan, confirmed and applied as any
//! is. A provider plans no edit and needs no capability to: the host has them, the user running
//! it. An image is held once however many tracks it's offered for, by its id, and what a call
//! holds is bounded (see [`super::Options`]): past that a provider's other candidates are left
//! out, with a warning.

use std::io::Read;

use base64::prelude::*;
use indexmap::IndexMap;
use loclib_plugin::{
    Candidates, Choice, ChoiceItem, Context, ProvideCandidates, Provider, ProviderKind, TrackMetadata, TrackTags,
    method,
};
use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::blobs::Scratch;
use super::process::Slot;
use super::{Plugins, host_settings, set_host_setting};
use crate::api;
use crate::artwork::{Artwork, ImageSource};
use crate::edit::{self, Edit, EditOptions, EditPlan, TrackFields};
use crate::model::{Track, is_isrc};
use crate::op::{Planned, Touched};
use crate::progress::Ctx;
use crate::query::TrackSelector;
use crate::store::Store;
use crate::{Error, Result};

/// What the `providers.auto_pick` setting is unless it's set.
pub const AUTO_PICK: f64 = 0.9;

/// The host's setting saying when a candidate is picked for the user.
const AUTO_PICK_SETTING: &str = "providers.auto_pick";

/// What to find, and for which tracks.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Provide {
    pub kind: ProviderKind,
    /// The library or drive whose tracks to find it for.
    pub drive: String,
    /// Which tracks [default: every one].
    #[serde(default)]
    pub tracks: TrackSelector,
    /// Only this plugin's providers, or this provider: `<plugin>` or `<plugin>.<provider>`.
    #[serde(default)]
    pub from: Option<String>,
    /// Also for tracks with what's found already, to replace it: otherwise only for tracks without
    /// cover art, or without one of the tags a provider finds.
    #[serde(default)]
    pub replace: bool,
}

/// What the providers found.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Provided {
    pub kind: ProviderKind,
    /// How many tracks they were asked about.
    pub asked: usize,
    /// Each track something was found for (tags only when they'd change it), in the library's
    /// order.
    pub offers: Vec<Offer>,
    /// The images the candidates are of, each once, by id (its content's hash): in base64.
    pub images: IndexMap<String, String>,
    /// A provider that failed (what the others found is offered), one that answered more than is
    /// kept (the rest left out), and what each said it found nothing for.
    pub warnings: Vec<String>,
}

/// What was found for a track.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Offer {
    pub track: u32,
    /// The track, as it's named (`Artist - Title`).
    pub name: String,
    /// Best first.
    pub candidates: Vec<Found>,
    /// The one picked for the user, by its place in `candidates`: the only one scoring the
    /// `providers.auto_pick` setting or more.
    pub picked: Option<usize>,
}

/// A candidate a provider found for a track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Found {
    /// Its provider, `<plugin>.<provider>`.
    pub provider: String,
    /// Where it's from, as the provider says.
    pub source: String,
    /// What it is, in a line.
    pub label: String,
    /// How sure the provider is that it's the track's, 0 to 1.
    pub score: f64,
    /// The image, by its id in the answer's `images`: an artwork provider's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artwork: Option<String>,
    /// The tags: a metadata provider's, each it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<TrackMetadata>,
}

/// The edits to make of what was found.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PlanProvided {
    pub kind: ProviderKind,
    /// The library or drive whose tracks they're for.
    pub drive: String,
    /// The candidates chosen, each for its track, as they were found.
    pub picks: Vec<Pick>,
    /// The images they're of, by id, as they were found: the answer's `images`, or those picked.
    #[serde(default)]
    pub images: IndexMap<String, String>,
    /// Also over what the tracks have: otherwise a track with cover art keeps it, and a tag it
    /// has is kept.
    #[serde(default)]
    pub replace: bool,
}

/// A candidate chosen for a track.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Pick {
    pub track: u32,
    pub candidate: Found,
}

impl Offer {
    /// The question for the user: which of the candidates, or none, as the param named after the
    /// track's id (`812`). An item's value is its candidate's place in `candidates`, counted from
    /// 1, `null` for none: what [`Offer::chosen`] takes.
    pub fn choice(&self) -> Choice {
        let items = self.candidates.iter().enumerate().map(|(i, c)| {
            let tags = c.metadata.as_ref().map(said).filter(|t| !t.is_empty());
            let detail = format!("{}, {:.0}%", c.source, c.score * 100.0);
            let detail = tags.map_or_else(|| detail.clone(), |t| format!("{detail}: {t}"));
            ChoiceItem { label: c.label.clone(), detail: Some(detail), value: (i + 1).into() }
        });
        let none = ChoiceItem { label: "none of these".into(), detail: None, value: Value::Null };
        let items = items.chain([none]).collect();
        Choice { title: self.name.clone(), param: self.track.to_string(), multi: false, items, more: Vec::new() }
    }

    /// The candidate picked for the user, chosen ([`Offer::picked`]).
    pub fn pick(&self) -> Option<Pick> {
        self.chosen(&(self.picked? + 1).into())
    }

    /// The candidate an answer to [`Offer::choice`] chose (`value`), if it chose one.
    pub fn chosen(&self, value: &Value) -> Option<Pick> {
        let i = usize::try_from(value.as_u64()?).ok()?.checked_sub(1)?;
        Some(Pick { track: self.track, candidate: self.candidates.get(i)?.clone() })
    }
}

/// A candidate's tags, as a line: the values it has, in order.
fn said(m: &TrackMetadata) -> String {
    let year = m.year.map(|y| y.to_string());
    let all = [&m.title, &m.artist, &m.album, &year, &m.genre, &m.label, &m.isrc];
    all.into_iter().flatten().map(String::as_str).collect::<Vec<_>>().join(", ")
}

/// The score a candidate alone at or above it is picked for the user with: the setting in
/// `store`, else [`AUTO_PICK`].
pub fn auto_pick(store: Option<&Store>) -> f64 {
    host_settings(store).get(AUTO_PICK_SETTING).and_then(Value::as_f64).unwrap_or(AUTO_PICK)
}

/// Picks a candidate alone scoring `score` or more for the user (more than 1: never), or with
/// `None` at [`AUTO_PICK`] again. Answers the setting then.
pub fn set_auto_pick(store: &Store, score: Option<f64>) -> Result<f64> {
    if let Some(s) = score.filter(|s| !(s.is_finite() && *s >= 0.0)) {
        return Err(Error::invalid(format!("a score is 0 to 1 (more than 1: never), not {s}")));
    }
    set_host_setting(store, AUTO_PICK_SETTING, score.map(Value::from))?;
    Ok(auto_pick(Some(store)))
}

/// Whether `t` lacks something a provider of `kind` finds: cover art, or a tag.
fn lacks(kind: ProviderKind, t: &Track) -> bool {
    match kind {
        ProviderKind::Artwork => t.artwork_path.is_none(),
        ProviderKind::Metadata => {
            [&t.title, &t.artist, &t.album, &t.genre, &t.label, &t.isrc].iter().any(|s| s.trim().is_empty())
                || t.year == 0
        }
    }
}

impl Plugins {
    /// Every provider (of `kind`, when it says), with its plugin's id, in the order the app gave
    /// the plugins.
    pub fn providers(&self, kind: Option<ProviderKind>) -> Vec<(&str, &Provider)> {
        let all = self.manifests().flat_map(|m| m.contributes.providers.iter().map(move |p| (m.id.as_str(), p)));
        all.filter(|(_, p)| kind.is_none_or(|k| p.kind == k)).collect()
    }

    /// Asks the providers of `req.kind` (`req.from`'s alone, when it names one) about the tracks
    /// of `req.drive` it chooses: what each found, best first, the one alone scoring the
    /// `providers.auto_pick` setting or more picked. Tags that would change nothing on a track
    /// aren't offered, and a track left with nothing isn't asked about. A provider that fails is a
    /// warning: the others' are offered.
    pub fn provide(&self, req: &Provide, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<Provided> {
        let all = self.providers(Some(req.kind));
        let named = |(plugin, p): &(&str, &Provider)| format!("{plugin}.{}", p.name);
        let chosen: Vec<&(&str, &Provider)> =
            (all.iter()).filter(|x| req.from.as_deref().is_none_or(|f| f == x.0 || f == named(x))).collect();
        if chosen.is_empty() {
            let kind = req.kind.name();
            return Err(Error::invalid(match (&req.from, all.is_empty()) {
                (Some(from), false) => {
                    let all: Vec<String> = all.iter().map(named).collect();
                    format!("{from} provides no {kind}: {} do", all.join(", "))
                }
                _ => format!("no plugin provides {kind}"),
            }));
        }
        let device = api::resolve_drive(&req.drive)?;
        let library = device.library()?;
        let ids: FxHashSet<u32> = match req.tracks.is_empty() {
            true => library.tracks.iter().map(|t| t.id).collect(),
            false => req.tracks.select_on(&device, &library)?.into_iter().collect(),
        };
        let wanted: Vec<&Track> =
            library.tracks.iter().filter(|t| ids.contains(&t.id) && (req.replace || lacks(req.kind, t))).collect();
        let tags: Vec<TrackTags> = wanted.iter().map(|t| tags_of(t)).collect();
        let asked: FxHashSet<u32> = tags.iter().map(|t| t.id).collect();
        let (mut held, mut warnings) = (Held::default(), Vec::new());
        for (plugin, provider) in chosen.into_iter().filter(|_| !tags.is_empty()) {
            let slot = self.slot(plugin)?;
            let context = Context {
                drive: Some(req.drive.clone()),
                tracks: tags.iter().map(|t| t.id).collect(),
                ..Context::default()
            };
            let (context, _, _given) = slot.context(context, store)?;
            let ask =
                ProvideCandidates { provider: provider.name.clone(), kind: req.kind, tracks: tags.clone(), context };
            let read =
                |answer, scratch: &Scratch, _: &_| slot.candidates(provider, req.kind, &asked, answer, scratch, &held);
            match slot.call_with(method::PROVIDE, &ask, store, ctx, &|_| {}, read) {
                Ok((candidates, images, said)) => {
                    warnings.extend(said.into_iter().map(|w| format!("{}: {w}", provider.title)));
                    held.images.extend(images);
                    for (track, c) in candidates {
                        held.found.entry(track).or_default().push(c);
                    }
                }
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => warnings.push(format!("{}: {e}", provider.title)),
            }
        }
        let threshold = auto_pick(store);
        let offers = wanted.iter().filter_map(|t| {
            let mut candidates = held.found.remove(&t.id)?;
            // Best first, the providers' order kept between equals; the same found twice once.
            candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
            let mut seen = FxHashSet::default();
            candidates.retain(|c| seen.insert((c.artwork.clone(), c.metadata.clone())));
            // Tags that would change nothing aren't offered, nor picked: a track lacking only an
            // ISRC is asked about, and a provider may find just what it has.
            candidates.retain(|c| c.metadata.as_ref().is_none_or(|m| !fields_of(t, m, req.replace).is_empty()));
            if candidates.is_empty() {
                return None;
            }
            let sure = candidates.iter().filter(|c| c.score >= threshold).count();
            Some(Offer { track: t.id, name: t.display_name(), picked: (sure == 1).then_some(0), candidates })
        });
        let offers = offers.collect();
        Ok(Provided { kind: req.kind, asked: tags.len(), offers, images: held.images, warnings })
    }
}

/// What a [`Plugins::provide`] holds of what its providers found so far: each track's candidates,
/// and the images they're of, by id, in base64.
#[derive(Default)]
struct Held {
    found: FxHashMap<u32, Vec<Found>>,
    images: IndexMap<String, String>,
}

/// What a provider answered, as the host takes it: each candidate with its track, the images new
/// to what's [`Held`] they're of, and its warnings.
type Answered = (Vec<(u32, Found)>, IndexMap<String, String>, Vec<String>);

/// `t` as a provider looks it up.
fn tags_of(t: &Track) -> TrackTags {
    TrackTags {
        id: t.id,
        artist: t.artist.clone(),
        title: t.title.clone(),
        album: t.album.clone(),
        album_artist: t.album_artist.clone(),
        isrc: t.isrc.clone(),
        duration: t.duration_secs,
    }
}

impl Slot {
    /// What `provider` (of `kind`) answered about the tracks `asked`, each candidate with its
    /// track, the images it's of that `held` hasn't, and what it said it found nothing for:
    /// candidates only for those tracks, scoring 0 to 1, each an image (read from `scratch`, once
    /// for each file) or tags as its kind gives. Tags are trimmed, blank ones and ISRCs that aren't
    /// one left out (and a candidate with none). Past [`super::Options::track_candidates`] for a
    /// track or [`super::Options::candidates`] in all, with what's `held`, or an image past
    /// [`super::Options::max_blobs`] of them in base64, the rest are left out, saying so.
    fn candidates(
        &self,
        provider: &Provider,
        kind: ProviderKind,
        asked: &FxHashSet<u32>,
        answer: Candidates,
        scratch: &Scratch,
        held: &Held,
    ) -> Result<Answered> {
        let options = &self.host.options;
        let name = format!("{}.{}", self.manifest.id, provider.name);
        let mut each: FxHashMap<u32, usize> = held.found.iter().map(|(t, c)| (*t, c.len())).collect();
        let mut all: usize = each.values().sum();
        let mut size: u64 = held.images.values().map(|i| i.len() as u64).sum();
        // Each file's image, by name: its id, or none past the size.
        let (mut ids, mut images): (FxHashMap<String, Option<String>>, IndexMap<String, String>) = Default::default();
        let (mut found, mut warnings) = (Vec::new(), answer.warnings);
        let mut past = |w: String| {
            if !warnings.contains(&w) {
                warnings.push(w);
            }
        };
        for (i, c) in answer.candidates.into_iter().enumerate() {
            if !asked.contains(&c.track) {
                return Err(self.misbehaved(format!("its candidate {i} is for track #{}, not asked about", c.track)));
            }
            if !(0.0..=1.0).contains(&c.score) {
                return Err(self.misbehaved(format!("its candidate {i} scores {}, not 0 to 1", c.score)));
            }
            if all >= options.candidates {
                past(format!("more than {} candidates, the rest left out", options.candidates));
                continue;
            }
            if each.get(&c.track).is_some_and(|&n| n >= options.track_candidates) {
                past(format!("more than {} candidates for a track, the rest left out", options.track_candidates));
                continue;
            }
            let (artwork, metadata) = match (kind, c.artwork, c.metadata) {
                (ProviderKind::Artwork, Some(blob), None) => {
                    let id = match ids.get(&blob.blob) {
                        Some(id) => id.clone(),
                        None => {
                            let bytes = self.image(i, &blob.blob, scratch)?;
                            let id = blake3::hash(&bytes).to_hex()[..32].to_string();
                            let new = !held.images.contains_key(&id) && !images.contains_key(&id);
                            let more = bytes.len().div_ceil(3) as u64 * 4;
                            let fits = !new || size + more <= options.max_blobs;
                            if new && fits {
                                size += more;
                                images.insert(id.clone(), BASE64_STANDARD.encode(&bytes));
                            }
                            let id = fits.then_some(id);
                            ids.insert(blob.blob, id.clone());
                            id
                        }
                    };
                    let Some(id) = id else {
                        let most = crate::op::bytes(options.max_blobs);
                        past(format!("images coming to more than {most}, the rest left out"));
                        continue;
                    };
                    (Some(id), None)
                }
                (ProviderKind::Metadata, None, Some(m)) => match tidy(m) {
                    m if m == TrackMetadata::default() => continue,
                    m => (None, Some(m)),
                },
                _ => {
                    return Err(self.misbehaved(format!(
                        "its candidate {i} isn't {}'s: an image for artwork, tags for metadata",
                        kind.name()
                    )));
                }
            };
            let (source, label, score) = (c.source, c.label, c.score);
            *each.entry(c.track).or_default() += 1;
            all += 1;
            found.push((c.track, Found { provider: name.clone(), source, label, score, artwork, metadata }));
        }
        Ok((found, images, warnings))
    }

    /// The image of candidate `i`, the file `blob` in `scratch`, once it's one.
    fn image(&self, i: usize, blob: &str, scratch: &Scratch) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let read = scratch.blob(blob).and_then(|mut f| f.read_to_end(&mut bytes).map_err(|e| e.to_string()));
        read.map_err(|why| self.misbehaved(format!("its candidate {i}: {why}")))?;
        Artwork::from_bytes(&bytes).map_err(|e| self.misbehaved(format!("its candidate {i}'s image: {e}")))?;
        Ok(bytes)
    }
}

/// `m`'s tags trimmed, without blank ones, a year of 0, or an ISRC that isn't one (once its
/// hyphens and spaces are left out, in capitals).
fn tidy(m: TrackMetadata) -> TrackMetadata {
    let text = |v: Option<String>| v.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let isrc = m.isrc.map(|i| i.chars().filter(|c| !matches!(c, '-' | ' ')).collect::<String>().to_ascii_uppercase());
    TrackMetadata {
        title: text(m.title),
        artist: text(m.artist),
        album: text(m.album),
        year: m.year.filter(|&y| y > 0),
        genre: text(m.genre),
        label: text(m.label),
        isrc: isrc.filter(|i| is_isrc(i)),
    }
}

/// Plans the edits of the candidates `req` picks, on the tracks of `req.drive`
/// (`plugins.provide`): an image (of `req.images`) set on the tracks it was picked for (a
/// `SetArtwork` for each), or tags written to each track (an `EditTracks`). A track keeps its
/// cover art and the tags it has unless `req.replace`; a pick that changes nothing is left out.
pub fn plan_provided(req: &PlanProvided, ctx: &Ctx<'_>) -> Result<Planned<EditPlan>> {
    let device = api::resolve_drive(&req.drive)?;
    let library = device.library()?;
    let mut sources: Vec<&str> = Vec::new();
    // Each image's tracks, by its id.
    let mut images: IndexMap<&str, Vec<u32>> = IndexMap::new();
    let mut edits = Vec::new();
    for p in &req.picks {
        let t = library.track(p.track).ok_or_else(|| Error::no_track(p.track))?;
        let c = &p.candidate;
        let wrong = || Error::invalid(format!("the candidate for #{} isn't {}'s", t.id, req.kind.name()));
        match req.kind {
            ProviderKind::Artwork => {
                let image = c.artwork.as_deref().ok_or_else(wrong)?;
                if !req.images.contains_key(image) {
                    return Err(Error::invalid(format!("the image {image} picked for #{} isn't given", t.id)));
                }
                if !req.replace && t.artwork_path.is_some() {
                    continue;
                }
                images.entry(image).or_default().push(t.id);
            }
            ProviderKind::Metadata => {
                let fields = fields_of(t, c.metadata.as_ref().ok_or_else(wrong)?, req.replace);
                if fields.is_empty() {
                    continue;
                }
                let tracks = TrackSelector { ids: vec![t.id], ..TrackSelector::default() };
                edits.push(Edit::EditTracks { tracks, fields });
            }
        }
        if !sources.contains(&c.source.as_str()) {
            sources.push(&c.source);
        }
    }
    let tracks: usize = images.values().map(Vec::len).sum::<usize>() + edits.len();
    edits.extend(images.into_iter().map(|(image, ids)| Edit::SetArtwork {
        tracks: TrackSelector { ids, ..TrackSelector::default() },
        image: ImageSource::Base64(req.images[image].clone()),
    }));
    let plan = edit::plan(&device, &edits, &EditOptions { lenient: false, no_journal: false }, ctx)?;
    let mut planned = api::planned_edit("plugins.provide", plan, Touched::library(&device, true), ctx)?;
    let what = match req.kind {
        ProviderKind::Artwork => "set cover art on",
        ProviderKind::Metadata => "fill in the details of",
    };
    let from = match sources.is_empty() {
        true => String::new(),
        false => format!(" from {}", sources.join(", ")),
    };
    planned.summary.insert(0, format!("{what} {tracks} track(s){from}"));
    Ok(planned)
}

/// The tags of `m` to write to `t`: each it has that `t` doesn't (over one `t` has, with
/// `replace`).
fn fields_of(t: &Track, m: &TrackMetadata, replace: bool) -> TrackFields {
    let set = |new: &Option<String>, has: &str| new.clone().filter(|v| v != has && (replace || has.trim().is_empty()));
    TrackFields {
        title: set(&m.title, &t.title),
        artist: set(&m.artist, &t.artist),
        album: set(&m.album, &t.album),
        genre: set(&m.genre, &t.genre),
        label: set(&m.label, &t.label),
        isrc: set(&m.isrc, &t.isrc),
        year: m.year.filter(|&y| y != t.year && (replace || t.year == 0)),
        ..TrackFields::default()
    }
}
