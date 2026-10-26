//! What locallibrary publishes to a DJ's Atmosphere account, as AT Protocol records: My Settings
//! ([`settings_record`]), the public profile ([`profile_record`]), sets ([`set_record`]) and their
//! recordings ([`recording`]).
//!
//! Each record fits its Lexicon ([`crate::atmosphere::lexicon`]). Everything in an account's
//! repository is public and copied around the network, so records only carry what the DJ chose
//! to show: the settings profile's base values (per-drive overrides stay on the computer), the
//! profile document as its preferences allow, and a set's tracks with the ones the DJ hid left
//! unnamed. The AT Protocol has no fractional numbers: shares become whole percents and tempos
//! whole BPM (a set's tracks keep hundredths of a BPM).
//!
//! Publishing is a [`plan`] (the exact record, what's published in its place now as Local
//! Network's directory has it, and whether the session may write it) and then [`apply`], which
//! writes it only to the account the plan was for, and only over the version the plan read (a
//! first publish only if there's still nothing there). [`import_plan`] reads someone's published
//! My Settings from the directory.

use indexmap::IndexMap;
use std::collections::BTreeSet;
use std::path::Path;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::atmosphere::directory::{self, Fetched};
use crate::atmosphere::repo::{self, Resolved};
use crate::atmosphere::{self, AccountInfo, lexicon};
use crate::harmony::Camelot;
use crate::mix::{MixReport, Play, Transition};
use crate::profile::{Count, DocShare, ProfileDocument};
use crate::sets::{EntryOrigin, Set, SetEntry};
use crate::settings::{self, BaseChange, Category};
use crate::{Error, Result};

pub mod recording;

pub use crate::profile::camel;

/// `jogMode` → `jog_mode`.
pub fn snake(field: &str) -> String {
    let mut out = String::new();
    for c in field.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

pub use crate::profile::timestamp;

fn group(category: Category) -> &'static str {
    match category {
        Category::Player => "player",
        Category::Display => "display",
        Category::Mixer => "mixer",
    }
}

const GROUPS: [&str; 3] = ["player", "display", "mixer"];

// ---------------------------------------------------------------------------------------------
// My Settings
// ---------------------------------------------------------------------------------------------

/// The settings record for a settings profile's base values (setting key → value id).
pub fn settings_record(base: &IndexMap<String, String>, now: DateTime<Utc>) -> Value {
    let mut groups: IndexMap<&str, Map<String, Value>> = IndexMap::new();
    for (key, value) in base {
        if let Ok((def, choice)) = settings::validate(key, value) {
            groups.entry(group(def.category)).or_default().insert(camel(def.key), choice.id.into());
        }
    }
    let mut record = Map::new();
    record.insert("$type".into(), lexicon::SETTINGS.into());
    for (name, values) in groups {
        record.insert(name.into(), Value::Object(values));
    }
    record.insert("updatedAt".into(), timestamp(now).into());
    Value::Object(record)
}

/// The settings a published settings record holds, as a settings profile's base values.
#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct PublishedSettings {
    pub values: IndexMap<String, String>,
    /// `group.field = value` entries this version of locallibrary doesn't know (newer firmware,
    /// or a newer locallibrary), left out.
    pub skipped: Vec<String>,
    pub updated_at: Option<String>,
}

/// Read a settings record (someone else's, or written by another version of locallibrary). Each
/// value is checked against the setting it's for (the settings Lexicon lists the same choices),
/// and a setting or value this version doesn't know (newer firmware's, or a newer locallibrary's)
/// is skipped, as the Lexicon asks. A change that would make a value mean something else gets a
/// new record type rather than a version number, so there's no version to check.
pub fn settings_from_record(record: &Value) -> Result<PublishedSettings> {
    if record["$type"] != lexicon::SETTINGS {
        return Err(Error::invalid(format!(
            "what's published there isn't a My Settings record (its $type is {})",
            crate::export::one_line(&record["$type"].to_string())
        )));
    }
    let mut out =
        PublishedSettings { updated_at: record["updatedAt"].as_str().map(String::from), ..Default::default() };
    for name in GROUPS {
        for (field, value) in record[name].as_object().into_iter().flatten() {
            let known = value
                .as_str()
                .and_then(|v| settings::validate(&snake(field), v).ok())
                .filter(|(def, _)| group(def.category) == name);
            match known {
                Some((def, choice)) => {
                    out.values.insert(def.key.to_string(), choice.id.to_string());
                }
                // The value is JSON, escaped; the field name is whatever the record says.
                None => out.skipped.push(format!("{name}.{} = {value}", crate::export::one_line(field))),
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// The profile
// ---------------------------------------------------------------------------------------------

/// A share (0 to 1) as a whole percent.
fn percent(x: f64) -> i64 {
    (x * 100.0).round().clamp(0.0, 100.0) as i64
}

fn bpm(x: f64) -> i64 {
    x.round().max(0.0) as i64
}

/// A count as the shared `count` definition has it.
fn count(c: Count) -> Value {
    match c {
        Count::Exact { count } => json!({ "count": count }),
        Count::AtLeast { at_least } => json!({ "atLeast": at_least }),
    }
}

/// Leave out `None`s.
fn put(m: &mut Map<String, Value>, key: &str, v: Option<impl Into<Value>>) {
    if let Some(v) = v {
        m.insert(key.into(), v.into());
    }
}

/// The most signature tracks a profile record names (its Lexicon's limit).
pub const SIGNATURE_LIMIT: usize = 50;

/// The profile record for a profile document (which already holds only what the DJ chose to
/// show, rounded as they asked).
pub fn profile_record(doc: &ProfileDocument) -> Value {
    let mut r = Map::new();
    r.insert("$type".into(), lexicon::PROFILE.into());
    r.insert("generatedAt".into(), timestamp(doc.generated_at).into());
    r.insert("precision".into(), serde_json::to_value(doc.precision).unwrap_or_default());
    r.insert("basis".into(), serde_json::to_value(doc.basis).unwrap_or_default());
    put(&mut r, "headline", doc.headline.clone());
    if !doc.tags.is_empty() {
        let tags: Vec<Value> = doc.tags.iter().map(|t| json!({ "name": t })).collect();
        r.insert("tags".into(), tags.into());
    }
    if let Some(s) = &doc.sets {
        let mut sets = Map::new();
        sets.insert("total".into(), count(s.count));
        put(&mut sets, "since", s.since);
        sets.insert("medianTracks".into(), s.median_tracks.into());
        r.insert("sets".into(), Value::Object(sets));
    }
    if let Some(t) = &doc.tempo {
        r.insert("tempo".into(), json!({ "low": bpm(t.low), "high": bpm(t.high), "median": bpm(t.median) }));
    }
    if let Some(a) = &doc.arc {
        r.insert(
            "arc".into(),
            json!({
                "shape": serde_json::to_value(a.shape).unwrap_or_default(),
                "builds": percent(a.builds),
                "steady": percent(a.steady),
                "windsDown": percent(a.winds_down),
            }),
        );
    }
    if let Some(m) = &doc.mixing {
        let mut mixing = Map::new();
        put(&mut mixing, "harmonic", m.harmonic.map(percent));
        put(&mut mixing, "smoothTempo", m.smooth_tempo.map(percent));
        put(&mut mixing, "genreChanges", m.genre_changes.map(percent));
        r.insert("mixing".into(), Value::Object(mixing));
    }
    if let Some(k) = &doc.keys {
        let mut keys = Map::new();
        let top: Vec<Value> = k.top.iter().map(|key| json!({ "key": key })).collect();
        keys.insert("top".into(), top.into());
        put(&mut keys, "minor", k.minor.map(percent));
        r.insert("keys".into(), Value::Object(keys));
    }
    if let Some(e) = &doc.energy {
        r.insert("energy".into(), json!({ "high": percent(e.high), "mid": percent(e.mid), "low": percent(e.low) }));
    }
    let shares = |list: &Vec<DocShare>| -> Vec<Value> {
        list.iter().map(|s| json!({ "name": s.name, "share": percent(s.share) })).collect()
    };
    put(&mut r, "genres", doc.genres.as_ref().map(shares));
    put(&mut r, "labels", doc.labels.as_ref().map(shares));
    put(&mut r, "artists", doc.artists.as_ref().map(shares));
    if let Some(e) = &doc.era {
        let mut era = Map::new();
        put(&mut era, "medianYear", e.median_year);
        put(&mut era, "recent", e.recent.map(percent));
        r.insert("era".into(), Value::Object(era));
    }
    if let Some(tracks) = &doc.signature {
        let tracks: Vec<Value> = tracks
            .iter()
            .take(SIGNATURE_LIMIT)
            .map(|t| {
                let mut m = Map::new();
                put(&mut m, "artist", Some(t.artist.clone()).filter(|a| !a.is_empty()));
                m.insert("title".into(), t.title.clone().into());
                Value::Object(m)
            })
            .collect();
        r.insert("signature".into(), tracks.into());
    }
    if let Some(p) = &doc.prep {
        r.insert(
            "prep".into(),
            json!({ "hotCues": percent(p.hot_cues), "memoryCues": percent(p.memory_cues), "loops": percent(p.loops) }),
        );
    }
    if let Some(c) = &doc.collection {
        r.insert("collection".into(), json!({ "tracks": count(c.tracks), "playlists": count(c.playlists) }));
    }
    if let Some(style) = &doc.style {
        r.insert("style".into(), json!(style));
    }
    Value::Object(r)
}

// ---------------------------------------------------------------------------------------------
// Sets
// ---------------------------------------------------------------------------------------------

/// The most tracks a set record holds (its Lexicon's limit). Tracks with every field filled and
/// text at the Lexicon's grapheme limits (ASCII) are about 1.5 KB of JSON each: 500 of them fit the
/// 1 MiB a record may be as DAG-CBOR, and the 1,000,000-byte request a PDS takes to write one.
pub const SET_TRACK_LIMIT: usize = 500;

/// The longest request writing a record a PDS from before May 2026 takes: the reference PDS's
/// JSON limit was 150 KiB for every request until bluesky-social/atproto#4989.
pub const OLDER_PDS_REQUEST_BYTES: usize = 150 * 1024;

/// The most graphemes the set Lexicon takes in an artist's or a release's name, at ten bytes each.
const NAME_LIMIT: usize = 256;

/// The most artists a set's track lists (its Lexicon's limit).
const ARTIST_LIMIT: usize = 32;

/// How long two tracks must be heard together to call the transition a blend, as the website's
/// mix report does: fingerprints don't hear fades, so a shorter overlap, or even a gap, may be a
/// quick mix as well as a cut.
const BLEND_SECS: f64 = 8.0;

/// The longest transition the Lexicon takes, in milliseconds.
const TRANSITION_LIMIT_MS: f64 = 600_000.0;

/// `mbid:<uuid>`, as teal.fm writes a MusicBrainz ID.
fn mbid(id: Uuid) -> String {
    format!("mbid:{id}")
}

/// Whether the Lexicon takes `s` as an artist's or a release's name: a longer one is left out
/// rather than cut short.
fn fits_name(s: &str) -> bool {
    s.chars().count() <= NAME_LIMIT && s.len() <= NAME_LIMIT * 10
}

/// The artists `e` credits, one by one, as the profile counts them, with their MusicBrainz IDs
/// when the tags have as many as the credit names (Picard tags them in the credit's order).
/// `None` when that says no more than the credit line: one artist, and no ID.
fn artists(e: &SetEntry) -> Option<Vec<Value>> {
    let names = crate::profile::artist_credits(&e.artist);
    let ids = &e.musicbrainz.artists;
    let paired = ids.len() == names.len();
    let known = names.len() > 1 || (paired && !ids.is_empty());
    if !known || names.len() > ARTIST_LIMIT || !fits_name(e.artist.trim()) {
        return None;
    }
    let artist = |(i, name): (usize, String)| {
        let mut m = Map::new();
        m.insert("artistName".into(), name.into());
        put(&mut m, "artistMbId", ids.get(i).filter(|_| paired).map(|id| mbid(*id)));
        Value::Object(m)
    };
    Some(names.into_iter().enumerate().map(artist).collect())
}

/// The Lexicon's name for a harmony a mix report describes.
fn harmony(described: &str) -> Option<&'static str> {
    use crate::harmony::Relation::*;
    [(Same, "sameKey"), (Adjacent, "adjacent"), (Relative, "relative"), (EnergyBoost, "energyBoost")]
        .into_iter()
        .find(|(r, _)| r.describe() == described)
        .map(|(_, name)| name)
        .or((described == "clash").then_some("clash"))
}

/// How a track came in, as a recording heard it. With `unidentified` (either track isn't named)
/// only when is said, not how the keys and tempo moved.
fn transition(t: &Transition, unidentified: bool) -> Value {
    let mut m = Map::new();
    m.insert("technique".into(), if t.overlap >= BLEND_SECS { "blend" } else { "unknown" }.into());
    let ms = (t.overlap * 1000.0).round();
    put(&mut m, "durationMs", (ms > 0.0 && ms <= TRANSITION_LIMIT_MS).then_some(ms as i64));
    if !unidentified {
        put(&mut m, "harmony", t.harmony.as_deref().and_then(harmony));
        put(&mut m, "tempoChangePermille", t.tempo_change.map(|c| (c * 10.0).round() as i64));
    }
    Value::Object(m)
}

/// Whether `e` is the play `p` of the recording its set was built from.
fn is_play(e: &SetEntry, p: &Play) -> bool {
    e.origin == EntryOrigin::Recording
        && e.start_seconds == Some(p.start_second())
        && e.file_key() == Some(crate::paths::file_key(&p.track.file_path))
}

/// The transition `report` heard from `before` into `e`, when they're plays it heard one after
/// the other.
fn heard<'a>(report: &'a MixReport, before: &SetEntry, e: &SetEntry) -> Option<&'a Transition> {
    let is = |i: usize, e: &SetEntry| report.plays.get(i).is_some_and(|p| is_play(e, p));
    report.transitions.iter().find(|t| is(t.from, before) && is(t.to, e))
}

/// One track of a set record: named as the set has it, or, unidentified, only its place (when it
/// starts, and came in).
fn set_track(e: &SetEntry, unidentified: bool, transition_in: Option<Value>) -> Value {
    let mut m = Map::new();
    if unidentified {
        m.insert("unidentified".into(), true.into());
        put(&mut m, "startSeconds", e.start_seconds);
        put(&mut m, "transitionIn", transition_in);
        return Value::Object(m);
    }
    let text = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
    let isrc = e.isrc.trim().replace('-', "").to_ascii_uppercase();
    put(&mut m, "title", text(&e.title));
    put(&mut m, "artist", text(&e.artist));
    put(&mut m, "artists", artists(e));
    put(&mut m, "releaseName", text(&e.album).filter(|a| fits_name(a)));
    put(&mut m, "isrc", (isrc.len() == 12 && isrc.chars().all(|c| c.is_ascii_alphanumeric())).then_some(isrc));
    put(&mut m, "recordingMbId", e.musicbrainz.recording.map(mbid));
    put(&mut m, "releaseMbId", e.musicbrainz.release.map(mbid));
    // An infinite tempo is unknown, as `sets.json` keeps it.
    put(&mut m, "bpmCentis", (e.bpm > 0.0 && e.bpm.is_finite()).then(|| (e.bpm * 100.0).round() as u32));
    put(&mut m, "key", Camelot::parse(&e.key).map(Camelot::code));
    put(&mut m, "startSeconds", e.start_seconds);
    put(&mut m, "transitionIn", transition_in);
    Value::Object(m)
}

/// The set record for `set`. Its tracks marked `unnamed`, and those at the positions (from 1) in
/// `hidden`, are published unidentified. `report`, the mix report of the recording the set was
/// built from, says how each track came in, where the set still has two plays it heard one after
/// the other. Its link goes with the name the plugin that published it there gave it, and the
/// record its recording can be played from (a plyr.fm track) with it. The DJ's notes, the
/// recording on this computer, the sessions and the files' paths stay here.
pub fn set_record(set: &Set, hidden: &BTreeSet<usize>, report: Option<&MixReport>, now: DateTime<Utc>) -> Value {
    let text = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let mut r = Map::new();
    r.insert("$type".into(), lexicon::SET.into());
    put(&mut r, "title", text(&set.title));
    r.insert("date".into(), set.date.format("%Y-%m-%d").to_string().into());
    put(&mut r, "kind", text(&set.kind));
    put(&mut r, "venue", text(&set.venue));
    // The address Lexicon needs a country.
    if let Some(country) = text(&set.country) {
        let mut place = Map::new();
        place.insert("$type".into(), lexicon::ADDRESS.into());
        put(&mut place, "name", text(&set.venue));
        put(&mut place, "locality", text(&set.city));
        place.insert("country".into(), country.into());
        r.insert("location".into(), Value::Object(place));
    }
    put(&mut r, "event", text(&set.event));
    if !set.with.is_empty() {
        let played_with: Vec<Value> = set
            .with
            .iter()
            .map(|p| {
                let mut m = Map::new();
                m.insert("name".into(), p.name.trim().into());
                put(&mut m, "did", text(&p.did));
                Value::Object(m)
            })
            .collect();
        r.insert("playedWith".into(), played_with.into());
    }
    let links = text(&set.link).map(|uri| {
        let mut link = Map::new();
        link.insert("uri".into(), uri.into());
        put(&mut link, "name", text(&set.link_name));
        json!([link])
    });
    put(&mut r, "links", links);
    put(&mut r, "recording", set.recording_record.as_ref().map(|rec| json!(rec)));
    let sources: Vec<Value> =
        [(EntryOrigin::Session, "history"), (EntryOrigin::Recording, "mix"), (EntryOrigin::Added, "added")]
            .into_iter()
            .filter(|(origin, _)| set.tracks.iter().any(|e| e.origin == *origin))
            .map(|(_, source)| json!({ "kind": source }))
            .collect();
    if !sources.is_empty() {
        r.insert("sources".into(), sources.into());
    }
    let unidentified: Vec<bool> =
        set.tracks.iter().enumerate().map(|(i, e)| e.unnamed || hidden.contains(&(i + 1))).collect();
    let tracks: Vec<Value> = set
        .tracks
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let transition_in = report
                .zip(i.checked_sub(1))
                .and_then(|(r, b)| heard(r, &set.tracks[b], e))
                .map(|t| transition(t, unidentified[i] || unidentified[i - 1]));
            set_track(e, unidentified[i], transition_in)
        })
        .collect();
    r.insert("tracks".into(), tracks.into());
    r.insert("createdAt".into(), timestamp(now).into());
    Value::Object(r)
}

/// The store file where `plays publish`, in earlier versions, kept the record key of each history
/// list it published, by account and the list's fingerprint. Nothing writes it now: a set built
/// from one of those lists alone takes over its key (see [`crate::api::set_publish_plan`]).
pub const SET_KEYS_FILE: &str = "atmosphere-sets.json";

/// The key `did` published the history list (session) `fingerprint` under with `plays publish`,
/// if it did.
pub fn published_set_key(root: &Path, did: &str, fingerprint: &str) -> Result<Option<String>> {
    let keys: IndexMap<String, IndexMap<String, String>> =
        crate::fsutil::read_json_opt(&root.join(SET_KEYS_FILE))?.unwrap_or_default();
    Ok(keys.get(did).and_then(|k| k.get(fingerprint)).cloned())
}

// ---------------------------------------------------------------------------------------------
// Publishing
// ---------------------------------------------------------------------------------------------

/// A record ready to publish, and what's published in its place now.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PublishPlan {
    /// The account it goes to. `None`: nobody's signed in, so it can only be looked at.
    pub account: Option<AccountInfo>,
    pub collection: String,
    pub rkey: String,
    /// `at://did/collection/rkey` (`None` while signed out).
    pub uri: Option<String>,
    /// The record exactly as it will be written.
    pub record: Value,
    /// What's published there now, as Local Network's directory has it (`None`: nothing yet, as
    /// far as it knows).
    pub current: Option<Fetched>,
    /// Whether publishing changes anything (time stamps aside).
    pub changed: bool,
    /// The scope the session lacks to write the record: sign in again allowing publishing
    /// (`account.login.begin` with `publish: true`) before applying.
    pub missing_scope: Option<String>,
    /// What the DJ should know before publishing, in words: guesses made, things left out.
    pub notes: Vec<String>,
}

/// A record published.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Published {
    pub uri: String,
    /// Its content hash.
    pub cid: String,
    /// False when what's published was already the same, so nothing was written.
    pub written: bool,
    pub record: Value,
}

/// Fields that only say when: two records that differ in nothing else are the same.
const TIME_FIELDS: [&str; 3] = ["updatedAt", "generatedAt", "createdAt"];

/// Whether `published`, as the directory keeps it, says what `record` (of `collection`) does, time
/// stamps aside. The directory's record scripts may add fields of their own and leave `$type`
/// out, so only the fields `collection`'s Lexicon defines count.
fn same_content(collection: &str, published: &Value, record: &Value) -> bool {
    let defined = lexicon::document(collection).map(|d| &d["defs"]["main"]["record"]["properties"]);
    let strip = |v: &Value| -> Map<String, Value> {
        let fields = v.as_object().into_iter().flatten();
        fields
            .filter(|(k, _)| defined.is_some_and(|d| d.get(k.as_str()).is_some()) && !TIME_FIELDS.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    strip(published) == strip(record)
}

/// Plan publishing `record` as `collection/rkey` in the signed-in account's repository (reading
/// what's there now from Local Network's directory). A record replacing one keeps its `createdAt`.
pub fn plan(root: &Path, collection: &str, rkey: &str, record: Value) -> Result<PublishPlan> {
    let account = atmosphere::status(root)?;
    let mut notes = Vec::new();
    let current = match &account {
        Some(info) => {
            notes.extend(directory::request_index(root, info)?);
            directory::get_record(&info.did, collection, rkey)?
        }
        None => None,
    };
    let mut plan = plan_over(account, collection, rkey, record, current)?;
    plan.notes = notes;
    Ok(plan)
}

/// [`plan`], for `account` (the signed-in one), with what's published as `collection/rkey` in its
/// repository already read.
pub fn plan_over(
    account: Option<AccountInfo>,
    collection: &str,
    rkey: &str,
    mut record: Value,
    current: Option<Fetched>,
) -> Result<PublishPlan> {
    lexicon::validate(collection, &record)?;
    let mut plan = PublishPlan {
        account,
        collection: collection.to_string(),
        rkey: rkey.to_string(),
        uri: None,
        record: Value::Null,
        current: None,
        changed: true,
        missing_scope: None,
        notes: Vec::new(),
    };
    if let Some(info) = &plan.account {
        plan.uri = Some(repo::at_uri(&info.did, collection, rkey));
        plan.missing_scope = atmosphere::missing_scope(info, collection);
        plan.current = current;
        if let Some(created) = plan.current.as_ref().and_then(|c| c.value.get("createdAt"))
            && record.get("createdAt").is_some()
            && created.is_string()
        {
            record["createdAt"] = created.clone();
        }
        plan.changed = plan.current.as_ref().is_none_or(|c| !same_content(collection, &c.value, &record));
    }
    plan.record = record;
    Ok(plan)
}

/// Publish a plan's record: written only when it changes something, only to the account the plan
/// is for, and only over the version the plan read (otherwise a conflict).
pub fn apply(root: &Path, plan: &PublishPlan) -> Result<Published> {
    let info = plan.account.as_ref().ok_or_else(atmosphere::not_signed_in)?;
    if let Some(current) = plan.current.as_ref().filter(|_| !plan.changed) {
        return Ok(Published {
            uri: current.uri.clone(),
            cid: current.cid.clone(),
            written: false,
            record: plan.record.clone(),
        });
    }
    if plan.missing_scope.is_some() {
        return Err(repo::scope_error(info, &plan.collection));
    }
    let swap = plan.current.as_ref().map(|c| c.cid.as_str());
    let written = repo::put_record(root, &info.did, &plan.collection, &plan.rkey, &plan.record, swap)?;
    Ok(Published { uri: written.uri, cid: written.cid, written: true, record: plan.record.clone() })
}

/// Someone's published My Settings, and how importing them changes the settings profile.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SettingsImportPlan {
    pub from: Resolved,
    pub uri: String,
    pub cid: String,
    pub published: PublishedSettings,
    /// Whether importing replaces the whole base, so settings they didn't publish become unset.
    /// Otherwise their settings are added to the base, over the values it has for them.
    pub replace: bool,
    /// Base values that change.
    pub changes: Vec<BaseChange>,
}

impl SettingsImportPlan {
    /// `base` with the published settings imported.
    pub fn imported(&self, base: &IndexMap<String, String>) -> IndexMap<String, String> {
        let mut after = if self.replace { IndexMap::new() } else { base.clone() };
        after.extend(self.published.values.clone());
        after.sort_unstable_keys();
        after
    }
}

/// Read the My Settings `from` published, from Local Network's directory, and work out how
/// importing them (adding them to `base`, or with `replace` replacing it) changes `base`.
pub fn import_plan(from: Resolved, base: &IndexMap<String, String>, replace: bool) -> Result<SettingsImportPlan> {
    let fetched = directory::get_record(&from.did, lexicon::SETTINGS, "self")?.ok_or_else(|| {
        Error::NotFound(format!(
            "My Settings published by {} (Local Network's directory doesn't have them yet)",
            from.name()
        ))
    })?;
    let published = settings_from_record(&fetched.value)?;
    let mut plan =
        SettingsImportPlan { from, uri: fetched.uri, cid: fetched.cid, published, replace, changes: Vec::new() };
    let after = plan.imported(base);
    let mut keys: Vec<&String> = base.keys().chain(after.keys()).collect();
    keys.sort();
    keys.dedup();
    plan.changes = keys
        .into_iter()
        .map(|k| BaseChange { key: k.clone(), from: base.get(k).cloned(), to: after.get(k).cloned() })
        .filter(|c| c.from != c.to)
        .collect();
    Ok(plan)
}

#[cfg(test)]
mod tests;
