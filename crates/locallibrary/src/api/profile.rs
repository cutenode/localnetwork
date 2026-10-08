//! A DJ's profile, built from their sets (or, until they declare some, their sessions), drives and
//! the store, and the choices about what it shows.

use chrono::{Datelike, NaiveDate};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{known_drives, resolve_drive};
use crate::device::Device;
use crate::error::Reason;
use crate::profile::{
    self, PrefsUpdate, ProfileDocument, ProfileOptions, ProfilePrefs, ProfileReport, Section, Sources,
};
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ProfileRequest {
    /// Drives to read. Default: the mounted rekordbox drives in the store's registry, so a drive
    /// someone else plugged in doesn't count as the DJ's; every mounted rekordbox drive when the
    /// registry is empty.
    #[serde(default)]
    pub drives: Vec<String>,
    /// Leave the store out (its sets, archived history lists and libraries).
    #[serde(default)]
    pub no_store: bool,
    /// Only these store libraries (names or ids). Default: the libraries the named drives are
    /// linked to, or every library.
    #[serde(default)]
    pub libraries: Vec<String>,
    /// Only sets played from this day: `2026`, `2026-09` or `2026-09-30`. Without declared sets,
    /// sessions first archived from it.
    #[serde(default)]
    pub since: Option<String>,
    /// Only sets played up to this day (same forms).
    #[serde(default)]
    pub until: Option<String>,
    /// Without declared sets: sessions with fewer tracks don't count (default 5).
    #[serde(default)]
    pub min_set_tracks: Option<usize>,
    /// Entries in ranked lists (default 10).
    #[serde(default)]
    pub top: Option<usize>,
    /// Skip analysis files: no phrase mood or cue statistics, but much faster.
    #[serde(default)]
    pub no_analysis: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProfileView {
    /// Everything worked out (stays on this computer).
    pub report: ProfileReport,
    pub prefs: ProfilePrefs,
    /// Sections the document includes.
    pub published: Vec<Section>,
    /// What a public profile would show.
    pub document: ProfileDocument,
}

impl ProfileView {
    pub fn new(report: ProfileReport, prefs: ProfilePrefs) -> ProfileView {
        let document = profile::document(&report, &prefs);
        let published = prefs.published().into_iter().collect();
        ProfileView { report, prefs, published, document }
    }
}

/// `2026` / `2026-09` / `2026-09-30` as the first (or, with `end`, last) day it covers.
pub fn parse_day(s: &str, end: bool) -> Result<NaiveDate> {
    let bad = || Error::invalid(format!("{s:?} isn't a date (use 2026, 2026-09 or 2026-09-30)"));
    let parts: Vec<&str> = s.trim().split('-').collect();
    let num = |i: usize| parts.get(i).and_then(|p| p.parse::<u32>().ok()).ok_or_else(bad);
    let year = num(0)? as i32;
    match parts.len() {
        1 if end => NaiveDate::from_ymd_opt(year, 12, 31),
        1 => NaiveDate::from_ymd_opt(year, 1, 1),
        2 if end => {
            let first = NaiveDate::from_ymd_opt(year, num(1)?, 1).ok_or_else(bad)?;
            first.checked_add_months(chrono::Months::new(1)).and_then(|d| d.pred_opt())
        }
        2 => NaiveDate::from_ymd_opt(year, num(1)?, 1),
        3 => NaiveDate::from_ymd_opt(year, num(1)?, num(2)?),
        _ => None,
    }
    .filter(|d| d.year() > 1900)
    .ok_or_else(bad)
}

fn options(req: &ProfileRequest, prefs: &ProfilePrefs) -> Result<ProfileOptions> {
    let defaults = ProfileOptions::default();
    let options = ProfileOptions {
        since: req.since.as_deref().map(|s| parse_day(s, false)).transpose()?,
        until: req.until.as_deref().map(|s| parse_day(s, true)).transpose()?,
        min_set_tracks: req.min_set_tracks.unwrap_or(defaults.min_set_tracks).max(2),
        top: req.top.unwrap_or(defaults.top).max(1),
        analysis: !req.no_analysis,
        genre_aliases: prefs.genre_aliases.clone(),
    };
    if let (Some(a), Some(b)) = (options.since, options.until)
        && a > b
    {
        return Err(Error::invalid(format!("the start ({a}) is after the end ({b})"))
            .with("since", a)
            .with("until", b));
    }
    Ok(options)
}

/// The report alone, with `prefs`' genre spellings.
pub fn profile_report(
    store: Option<&Store>,
    req: &ProfileRequest,
    prefs: &ProfilePrefs,
    ctx: &Ctx<'_>,
) -> Result<ProfileReport> {
    let options = options(req, prefs)?;
    let drives: Vec<Device> = if req.drives.is_empty() {
        let mounted = known_drives(false).into_iter().filter(Device::has_export);
        // The store's registry says which drives are the DJ's, even when its data is left out.
        let theirs = match store {
            Some(s) => s.drives()?,
            None => Default::default(),
        };
        if theirs.is_empty() {
            mounted.collect()
        } else {
            mounted.filter(|d| d.id().is_some_and(|id| theirs.contains_key(&id))).collect()
        }
    } else {
        let mut out: Vec<Device> = Vec::new();
        for spec in &req.drives {
            let d = resolve_drive(spec)?;
            if !d.has_export() {
                return Err(Error::invalid(format!("{} has no rekordbox export", d.name())));
            }
            if !out.iter().any(|x| x.root() == d.root()) {
                out.push(d);
            }
        }
        out
    };
    let store = store.filter(|_| !req.no_store);
    let libraries: Option<Vec<Uuid>> = match store {
        None => None,
        Some(s) if !req.libraries.is_empty() => Some(
            req.libraries
                .iter()
                .map(|name| {
                    s.find_library(name)?.map(|l| l.id).ok_or_else(|| Error::NotFound(format!("library {name:?}")))
                })
                .collect::<Result<_>>()?,
        ),
        Some(_) if !req.drives.is_empty() => {
            let linked: Vec<Uuid> = drives.iter().filter_map(|d| d.state().and_then(|s| s.library_id)).collect();
            (!linked.is_empty()).then_some(linked)
        }
        Some(_) => None,
    };
    if drives.is_empty() && store.is_none() {
        return Err(Error::NotFound(
            "no rekordbox drive is mounted and there's no store (pulling a drive starts one)".into(),
        )
        .because(Reason::NoStore));
    }
    profile::build(&Sources { drives, store, libraries }, &options, ctx)
}

/// The report, the DJ's preferences, and the document they allow.
pub fn profile(store: Option<&Store>, req: &ProfileRequest, ctx: &Ctx<'_>) -> Result<ProfileView> {
    let prefs = match store {
        Some(s) => ProfilePrefs::load(s)?,
        None => ProfilePrefs::default(),
    };
    let report = profile_report(store, req, &prefs, ctx)?;
    Ok(ProfileView::new(report, prefs))
}

/// Just the document: what a public profile would show.
pub fn profile_document(store: Option<&Store>, req: &ProfileRequest, ctx: &Ctx<'_>) -> Result<ProfileDocument> {
    Ok(profile(store, req, ctx)?.document)
}

pub fn profile_prefs(store: &Store) -> Result<ProfilePrefs> {
    ProfilePrefs::load(store)
}

/// Change the preferences (nothing is saved when any change is invalid).
pub fn profile_prefs_update(store: &Store, update: &PrefsUpdate) -> Result<ProfilePrefs> {
    let mut prefs = ProfilePrefs::load(store)?;
    prefs.apply(update)?;
    prefs.save(store)?;
    Ok(prefs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_cover_years_and_months() {
        assert_eq!(parse_day("2026", false).unwrap(), NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        assert_eq!(parse_day("2026", true).unwrap(), NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
        assert_eq!(parse_day("2026-02", true).unwrap(), NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
        assert_eq!(parse_day("2026-09-30", false).unwrap(), NaiveDate::from_ymd_opt(2026, 9, 30).unwrap());
        assert!(parse_day("last week", false).is_err());
        assert!(parse_day("2026-13", false).is_err());
    }

    #[test]
    fn a_month_starts_on_its_first_and_a_day_has_three_parts_at_most() {
        assert_eq!(parse_day("2026-09", false).unwrap(), NaiveDate::from_ymd_opt(2026, 9, 1).unwrap());
        assert!(parse_day("2026-09-30-12", false).is_err());
        assert!(parse_day("1900", false).is_err(), "too long ago to be a set");
    }
}
