//! What locallibrary publishes to a DJ's Atmosphere account, as AT Protocol records: My Settings
//! ([`settings_record`]), the public profile ([`profile_record`]) and sets ([`set_record`]).
//!
//! Each record fits its Lexicon ([`crate::atmosphere::lexicon`]). Everything in an account's
//! repository is public and copied around the network, so records only carry what the DJ chose
//! to show: the settings profile's base values (per-drive overrides stay on the computer), the
//! profile document as its preferences allow, and a set's tracks with the ones the DJ hid left
//! unnamed. The AT Protocol has no fractional numbers: shares become whole percents and tempos
//! whole BPM (a set's tracks keep hundredths of a BPM).
//!
//! Publishing is a [`plan`] (the exact record, what's published in its place now, and whether
//! the session may write it) and then [`apply`], which writes it only to the account the plan was
//! for, and only if what's published hasn't changed since the plan read it (a first publish only
//! if there's still nothing there). [`import_plan`] reads someone's published My Settings.

use indexmap::IndexMap;
use std::collections::BTreeSet;
use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::atmosphere::repo::{self, Fetched, Resolved};
use crate::atmosphere::{self, AccountInfo, lexicon};
use crate::harmony::Camelot;
use crate::profile::{Count, DocShare, ProfileDocument};
use crate::sets::{EntryOrigin, Set, SetEntry};
use crate::settings::{self, BaseChange, Category};
use crate::{Error, Result};

/// `jog_mode` → `jogMode`: setting keys as record fields.
pub fn camel(key: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in key.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

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

/// A time as records carry it: UTC, to the millisecond.
pub fn timestamp(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

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

/// One track of a set record: named as the set has it, or, unidentified, only its place (and
/// when it starts).
fn set_track(e: &SetEntry, unidentified: bool) -> Value {
    let mut m = Map::new();
    if unidentified {
        m.insert("unidentified".into(), true.into());
        put(&mut m, "startSeconds", e.start_seconds);
        return Value::Object(m);
    }
    let text = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
    let isrc = e.isrc.trim().replace('-', "").to_ascii_uppercase();
    put(&mut m, "title", text(&e.title));
    put(&mut m, "artist", text(&e.artist));
    put(&mut m, "isrc", (isrc.len() == 12 && isrc.chars().all(|c| c.is_ascii_alphanumeric())).then_some(isrc));
    put(&mut m, "bpmCentis", (e.bpm > 0.0).then(|| (e.bpm * 100.0).round() as u32));
    put(&mut m, "key", Camelot::parse(&e.key).map(Camelot::code));
    put(&mut m, "startSeconds", e.start_seconds);
    Value::Object(m)
}

/// The set record for `set`. Its tracks marked `unnamed`, and those at the positions (from 1) in
/// `hidden`, are published unidentified. The DJ's notes, the recording, the sessions and the
/// files' paths stay here.
pub fn set_record(set: &Set, hidden: &BTreeSet<usize>, now: DateTime<Utc>) -> Value {
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
    put(&mut r, "links", text(&set.link).map(|uri| json!([{ "uri": uri }])));
    let sources: Vec<Value> =
        [(EntryOrigin::Session, "history"), (EntryOrigin::Recording, "mix"), (EntryOrigin::Added, "added")]
            .into_iter()
            .filter(|(origin, _)| set.tracks.iter().any(|e| e.origin == *origin))
            .map(|(_, source)| json!({ "kind": source }))
            .collect();
    if !sources.is_empty() {
        r.insert("sources".into(), sources.into());
    }
    let tracks: Vec<Value> =
        set.tracks.iter().enumerate().map(|(i, e)| set_track(e, e.unnamed || hidden.contains(&(i + 1)))).collect();
    r.insert("tracks".into(), tracks.into());
    r.insert("createdAt".into(), timestamp(now).into());
    Value::Object(r)
}

/// The store file where `plays publish`, in earlier versions, kept the record key of each history
/// list it published, by account and the list's fingerprint. Nothing writes it now: a set built
/// from one of those lists alone takes over its key (see [`crate::api::set_publish_plan`]).
pub const SET_KEYS_FILE: &str = "atmosphere-sets.json";

/// The collection `plays publish` wrote those sets to, before sets moved to [`lexicon::SET`].
/// Only read, for when a set first went out; nothing writes it now.
pub const PLAYS_PUBLISH_COLLECTION: &str = "computer.localnetwork.locallibrary.set";

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
    /// What's published there now (`None`: nothing yet).
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
    pub cid: Option<String>,
    /// False when what's published was already the same, so nothing was written.
    pub written: bool,
    pub record: Value,
}

/// Fields that only say when: two records that differ in nothing else are the same.
const TIME_FIELDS: [&str; 3] = ["updatedAt", "generatedAt", "createdAt"];

fn same_content(a: &Value, b: &Value) -> bool {
    let strip = |v: &Value| {
        let mut v = v.clone();
        if let Some(m) = v.as_object_mut() {
            for f in TIME_FIELDS {
                m.remove(f);
            }
        }
        v
    };
    strip(a) == strip(b)
}

/// Plan publishing `record` as `collection/rkey` in the signed-in account's repository (reading
/// what's there now from its server). A record replacing one keeps its `createdAt`.
pub fn plan(root: &Path, collection: &str, rkey: &str, record: Value) -> Result<PublishPlan> {
    let account = atmosphere::status(root)?;
    let current = match &account {
        Some(info) => repo::get_record(&info.pds, &info.did, collection, rkey)?,
        None => None,
    };
    plan_over(account, collection, rkey, record, current)
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
        plan.changed = plan.current.as_ref().is_none_or(|c| !same_content(&c.value, &record));
    }
    plan.record = record;
    Ok(plan)
}

/// Publish a plan's record: written only when it changes something, only to the account the plan
/// is for, and only if what's published is still what the plan read (otherwise a conflict).
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
    let swap = plan.current.as_ref().and_then(|c| c.cid.as_deref());
    let written = repo::put_record(root, &info.did, &plan.collection, &plan.rkey, &plan.record, swap)?;
    Ok(Published { uri: written.uri, cid: Some(written.cid), written: true, record: plan.record.clone() })
}

/// Someone's published My Settings, and how importing them changes the settings profile.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SettingsImportPlan {
    pub from: Resolved,
    pub uri: String,
    pub cid: Option<String>,
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

/// Read the My Settings `from` published, and work out how importing them (adding them to `base`,
/// or with `replace` replacing it) changes `base`.
pub fn import_plan(from: Resolved, base: &IndexMap<String, String>, replace: bool) -> Result<SettingsImportPlan> {
    let fetched = repo::get_record(&from.pds, &from.did, lexicon::SETTINGS, "self")?
        .ok_or_else(|| Error::NotFound(format!("My Settings published by {}", from.name())))?;
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
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::profile::{
        ArcShape, Basis, DocArc, DocCollection, DocEnergy, DocEra, DocKeys, DocMixing, DocPrep, DocSets, DocShare,
        DocTempo, DocTrack, Precision,
    };

    fn now() -> DateTime<Utc> {
        "2026-10-05T12:00:00.123456Z".parse().unwrap()
    }

    #[test]
    fn keys_and_fields_convert_both_ways() {
        for def in settings::SCHEMA {
            assert_eq!(snake(&camel(def.key)), def.key);
        }
        assert_eq!(camel("channel_fader_curve_long"), "channelFaderCurveLong");
        assert_eq!(timestamp(now()), "2026-10-05T12:00:00.123Z");
    }

    #[test]
    fn every_setting_fits_the_settings_lexicon_and_reads_back() {
        let base: IndexMap<String, String> =
            settings::SCHEMA.iter().map(|d| (d.key.to_string(), d.choices.last().unwrap().id.to_string())).collect();
        let record = settings_record(&base, now());
        lexicon::validate(lexicon::SETTINGS, &record).unwrap();
        assert_eq!(record["player"]["jogMode"], "vinyl");
        assert_eq!(record["mixer"]["crossfaderCurve"], "fast-cut");
        let back = settings_from_record(&record).unwrap();
        assert_eq!(back.values, base);
        assert!(back.skipped.is_empty());
        assert_eq!(back.updated_at.as_deref(), Some("2026-10-05T12:00:00.123Z"));

        // A partial profile publishes only what it sets.
        let partial: IndexMap<String, String> = [("quantize".to_string(), "off".to_string())].into();
        let record = settings_record(&partial, now());
        lexicon::validate(lexicon::SETTINGS, &record).unwrap();
        assert!(record.get("display").is_none() && record.get("mixer").is_none());
    }

    #[test]
    fn settings_this_version_doesnt_know_are_skipped() {
        let record = json!({
            "$type": lexicon::SETTINGS,
            "player": { "quantize": "off", "jogMode": "hover", "slipReverse": "on" },
            "mixer": { "quantize": "on" },
            "updatedAt": "2026-10-05T12:00:00.000Z",
        });
        let s = settings_from_record(&record).unwrap();
        assert_eq!(s.values, [("quantize".to_string(), "off".to_string())].into());
        assert_eq!(s.skipped.len(), 3, "{:?}", s.skipped);
        // A change that breaks readers gets a new record type, so there's no version to refuse: a
        // top-level field this version doesn't know (an early draft's `version`) is passed over.
        let drafted = json!({ "$type": lexicon::SETTINGS, "version": 2, "player": { "quantize": "off" } });
        assert_eq!(
            settings_from_record(&drafted).unwrap().values,
            [("quantize".to_string(), "off".to_string())].into()
        );
        // Another record type isn't taken for My Settings.
        let other = json!({ "$type": lexicon::PROFILE, "player": { "quantize": "off" } });
        assert!(settings_from_record(&other).unwrap_err().to_string().contains("isn't a My Settings record"));
        // A field name is whatever the record says: none of it reaches a terminal as a control.
        let tricky = json!({ "$type": lexicon::SETTINGS, "player": { "x\u{1b}]0;owned\u{7}": "on" } });
        let s = settings_from_record(&tricky).unwrap();
        assert!(!s.skipped[0].chars().any(char::is_control), "{:?}", s.skipped);
    }

    fn full_document(precision: Precision) -> ProfileDocument {
        let at_least =
            |n| if precision == Precision::Exact { Count::Exact { count: n } } else { Count::AtLeast { at_least: n } };
        ProfileDocument {
            version: crate::profile::DOCUMENT_VERSION,
            generated_at: now(),
            precision,
            basis: Basis::Plays,
            headline: Some("Techno and House · 122-128 BPM · 45+ sets".into()),
            tags: vec!["Mixes in key".into(), "Plays with quantize off".into()],
            sets: Some(DocSets { count: at_least(45), since: Some(2019), median_tracks: 24 }),
            tempo: Some(DocTempo { low: 122.0, high: 128.4, median: 125.5 }),
            arc: Some(DocArc { shape: ArcShape::WindsDown, builds: 0.2, steady: 0.25, winds_down: 0.55 }),
            mixing: Some(DocMixing { harmonic: Some(0.7), smooth_tempo: Some(0.853), genre_changes: None }),
            keys: Some(DocKeys { top: vec!["8A".into(), "5A".into(), "9A".into()], minor: Some(0.8) }),
            energy: Some(DocEnergy { high: 0.85, mid: 0.1, low: 0.1 }),
            genres: Some(vec![DocShare { name: "Techno".into(), share: 0.6 }]),
            labels: Some(vec![DocShare { name: "Ninja Tune".into(), share: 0.15 }]),
            artists: Some(vec![DocShare { name: "Bicep".into(), share: 0.054 }]),
            era: Some(DocEra { median_year: Some(2021), recent: Some(0.4) }),
            signature: Some(vec![
                DocTrack { artist: "Bicep".into(), title: "Glue".into() },
                DocTrack { artist: String::new(), title: "ID".into() },
            ]),
            prep: Some(DocPrep { hot_cues: 0.95, memory_cues: 0.5, loops: 0.05 }),
            collection: Some(DocCollection { tracks: at_least(3500), playlists: Count::Exact { count: 7 } }),
            style: Some(
                crate::profile::STYLE_SETTINGS
                    .iter()
                    .map(|k| (camel(k), settings::find(k).unwrap().default.into()))
                    .collect(),
            ),
        }
    }

    #[test]
    fn a_profile_document_with_every_section_fits_the_profile_lexicon() {
        for precision in [Precision::Rounded, Precision::Exact] {
            let record = profile_record(&full_document(precision));
            lexicon::validate(lexicon::PROFILE, &record).unwrap();
            assert_eq!(record["tempo"], json!({ "low": 122, "high": 128, "median": 126 }));
            assert_eq!(record["mixing"], json!({ "harmonic": 70, "smoothTempo": 85 }));
            assert_eq!(record["arc"]["shape"], "windsDown");
            assert_eq!(record["signature"][1], json!({ "title": "ID" }));
            // Lists hold objects, so their entries can gain fields.
            assert_eq!(record["tags"], json!([{ "name": "Mixes in key" }, { "name": "Plays with quantize off" }]));
            assert_eq!(record["keys"]["top"], json!([{ "key": "8A" }, { "key": "5A" }, { "key": "9A" }]));
            assert_eq!(record["labels"], json!([{ "name": "Ninja Tune", "share": 15 }]));
            assert_eq!(record["artists"], json!([{ "name": "Bicep", "share": 5 }]));
            assert!(record.get("version").is_none(), "a breaking change gets a new record type instead");
        }
        let rounded = profile_record(&full_document(Precision::Rounded));
        assert_eq!(rounded["sets"], json!({ "total": { "atLeast": 45 }, "since": 2019, "medianTracks": 24 }));
        assert_eq!(rounded["collection"]["tracks"], json!({ "atLeast": 3500 }));
        assert_eq!(rounded["precision"], "rounded");

        // Sections left out of the document stay out of the record.
        let mut doc = full_document(Precision::Rounded);
        doc.signature = None;
        doc.artists = None;
        let record = profile_record(&doc);
        lexicon::validate(lexicon::PROFILE, &record).unwrap();
        assert!(record.get("signature").is_none() && record.get("artists").is_none());
    }

    #[test]
    fn a_set_fits_the_set_lexicon_and_unidentified_tracks_say_nothing() {
        let mut track = loclib_model::test_util::track(1, "Bicep", "Glue", "/Contents/Bicep/Glue.mp3");
        track.isrc = "GB-AHT-17-00123".into();
        track.bpm = 129.98;
        track.key = "Am".into();
        let named = SetEntry { start_seconds: Some(0), ..SetEntry::from_track(&track, EntryOrigin::Recording, None) };
        let unnamed = SetEntry { unnamed: true, start_seconds: Some(312), ..named.clone() };
        let mut set = Set {
            id: "3m2abcdefgh22".into(),
            title: Some("Friday warm-up".into()),
            date: NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(),
            venue: Some(" ".into()),
            city: Some("London".into()),
            tracks: vec![named.clone(), unnamed, named, SetEntry::added("ID")],
            created_at: now(),
            updated_at: now(),
            ..Default::default()
        };
        let record = set_record(&set, &[3].into(), now());
        lexicon::validate(lexicon::SET, &record).unwrap();
        assert_eq!(
            record["tracks"][0],
            json!({ "title": "Glue", "artist": "Bicep", "isrc": "GBAHT1700123", "bpmCentis": 12998, "key": "8A", "startSeconds": 0 })
        );
        assert_eq!(record["tracks"][1], json!({ "unidentified": true, "startSeconds": 312 }), "marked unnamed");
        assert_eq!(record["tracks"][2], json!({ "unidentified": true, "startSeconds": 0 }), "hidden");
        assert_eq!(record["tracks"][3], json!({ "title": "ID" }));
        assert_eq!(
            (&record["date"], &record["sources"]),
            (&json!("2026-10-03"), &json!([{ "kind": "mix" }, { "kind": "added" }]))
        );
        // Blank fields stay out, and without a country there's no address.
        assert!(record.get("venue").is_none() && record.get("location").is_none());
        assert!(record.get("playedWith").is_none() && record.get("links").is_none());

        set.country = Some("GB".into());
        set.link = Some("https://www.mixcloud.com/dj/friday-warm-up/".into());
        let record = set_record(&set, &BTreeSet::new(), now());
        lexicon::validate(lexicon::SET, &record).unwrap();
        assert_eq!(record["location"], json!({ "$type": lexicon::ADDRESS, "locality": "London", "country": "GB" }));
        assert_eq!(record["links"], json!([{ "uri": "https://www.mixcloud.com/dj/friday-warm-up/" }]));
    }
}
