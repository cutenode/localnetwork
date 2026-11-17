//! Which Discogs releases an album could be, found by a search of its database: by the album's
//! name, artist and record label when a track of it has a label, else (or when that finds none)
//! by its name and artist; and the releases a track without an album is on, by its title and
//! artist. A search answers releases, each titled `Artist - Title`; only those titled as the album
//! is count for an album. Releases of one master (the editions of an album) are one release here,
//! the first's, with the earliest year of those on the search's page. Each names its labels,
//! catalogue number, genres, styles and cover. Every search names the account, whose personal
//! access token the host fills in; a cover is fetched from Discogs' image server without it.

use std::fmt;
use std::path::Path;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use loclib_model::model::normalize;
use loclib_plugin::{Fetch, Fetched, Host, RpcError};
use loclib_progress::Ctx;
use serde::Deserialize;

use crate::Lookup;

/// Where Discogs' API is.
pub(crate) const API: &str = "https://api.discogs.com";
/// Where Discogs serves images: a cover anywhere else (a placeholder) is none.
pub(crate) const IMAGES: &str = "https://i.discogs.com/";
/// Names the account in a request's `Authorization` header: the host fills in its token.
pub(crate) const AUTHORIZATION: &str = "{account:discogs}";
/// How many releases a search asks for: an album's editions come first.
pub(crate) const PER_PAGE: &str = "25";

/// How many times a search is sent while Discogs answers 429 (too many requests).
pub(crate) const TRIES: usize = 3;
/// A second, or a thousandth of one in this crate's tests, so they wait out every `Retry-After`
/// and window without spending seconds on it.
pub(crate) const SECOND: Duration = if cfg!(test) { Duration::from_millis(1) } else { Duration::from_secs(1) };
/// Discogs counts an account's requests over a moving minute, in seconds: waited out when it says
/// none are left, or answers 429 without saying how long to wait.
pub(crate) const WINDOW: u32 = 60;
/// The longest `Retry-After` waited out, in seconds. Discogs asking for longer leaves the album
/// out, saying so.
pub(crate) const LONGEST_WAIT: u64 = 120;
/// How often a wait notices the hook being cancelled.
const CHECK_EVERY: Duration = Duration::from_millis(50);

/// The smallest image locallibrary takes as artwork, either way.
const SMALLEST: u32 = 16;

/// What a search answers, as far as this plugin reads it.
#[derive(Deserialize)]
struct Results {
    results: Vec<Hit>,
}

/// A release a search found.
#[derive(Deserialize)]
struct Hit {
    id: u64,
    /// `Artist - Title`.
    title: String,
    /// Its master's id, none (or 0) for a release that has none.
    #[serde(default)]
    master_id: Option<u64>,
    /// `2017`, when Discogs knows.
    #[serde(default)]
    year: Option<String>,
    #[serde(default)]
    label: Vec<String>,
    #[serde(default)]
    catno: Option<String>,
    #[serde(default)]
    genre: Vec<String>,
    #[serde(default)]
    style: Vec<String>,
    /// Its front cover, or a placeholder on another server when it has none.
    #[serde(default)]
    cover_image: Option<String>,
}

/// A release an album (or a track) could be, with what its master's releases on the search's
/// page say of it.
pub(crate) struct Release {
    /// The first release's id: its cover is fetched once by it.
    pub(crate) id: u64,
    /// Its title as Discogs spells it.
    pub(crate) album: String,
    /// The year the earliest came out (a reissue's isn't the album's).
    pub(crate) year: Option<u16>,
    /// The first record label one names, without Discogs' number telling apart labels of one name.
    pub(crate) label: Option<String>,
    pub(crate) catno: Option<String>,
    /// Its first style (`Techno`), else its first genre (`Electronic`).
    pub(crate) genre: Option<String>,
    /// The first cover one has on Discogs' image server.
    pub(crate) cover: Option<String>,
}

impl fmt::Display for Release {
    /// What its candidates are shown as: `Bicep, Ninja Tune ZEN242, 2017`, leaving out what it
    /// lacks.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let issued: Vec<&str> = [&self.label, &self.catno].into_iter().flatten().map(String::as_str).collect();
        let issued = Some(issued.join(" ")).filter(|i| !i.is_empty());
        let parts = [Some(self.album.clone()), issued, self.year.map(|y| y.to_string())];
        f.write_str(&parts.into_iter().flatten().collect::<Vec<_>>().join(", "))
    }
}

/// How a lookup found its releases, which says how sure it is of them.
pub(crate) enum By {
    /// The album's name, artist and record label.
    Label,
    /// The album's name and artist.
    Name,
    /// A track's title and artist: the releases it's on, which needn't be the album meant.
    Title,
}

/// What looking up found.
pub(crate) enum Found {
    /// The releases it could be, as Discogs lists them (none, or several).
    Releases { releases: Vec<Release>, by: By },
    /// Discogs refused the search with this status.
    Refused(u16),
}

/// Discogs' API, asked through the host, keeping to the account's limit.
pub(crate) struct Discogs<'h> {
    host: &'h Host,
    /// When an answer said the account had no requests left this minute: the next waits it out.
    spent: Option<Instant>,
}

impl<'h> Discogs<'h> {
    pub(crate) fn new(host: &'h Host) -> Discogs<'h> {
        Discogs { host, spent: None }
    }

    /// The releases `lookup` could be: by its label first when it has one, then by its name; by
    /// its title for a track without an album. Fails as [`Discogs::search`] does.
    pub(crate) fn find(&mut self, lookup: &Lookup) -> Result<Found, RpcError> {
        let artist = [("artist", lookup.artist.as_str())].into_iter().filter(|(_, a)| !a.is_empty());
        if lookup.single {
            let params: Vec<_> = artist.chain([("track", lookup.name.as_str())]).collect();
            return Ok(match self.search(&params)? {
                Ok(hits) => Found::Releases { releases: releases(hits, None), by: By::Title },
                Err(status) => Found::Refused(status),
            });
        }
        let album = [("release_title", lookup.name.as_str())];
        if let Some(label) = &lookup.label {
            let params: Vec<_> = artist.clone().chain(album).chain([("label", label.as_str())]).collect();
            let releases = match self.search(&params)? {
                Ok(hits) => releases(hits, Some(lookup.name.as_str())),
                Err(status) => return Ok(Found::Refused(status)),
            };
            // Tags spell labels many ways (`Ninja Tune Ltd.`): by name alone, then.
            if !releases.is_empty() {
                return Ok(Found::Releases { releases, by: By::Label });
            }
        }
        let params: Vec<_> = artist.chain(album).collect();
        Ok(match self.search(&params)? {
            Ok(hits) => Found::Releases { releases: releases(hits, Some(lookup.name.as_str())), by: By::Name },
            Err(status) => Found::Refused(status),
        })
    }

    /// Searches Discogs' database for releases matching `params`: what it found, or the status it
    /// refused the search with. Over Discogs' limit (429), the search waits as long as Discogs
    /// asks (`Retry-After`), else a whole window, and goes again, [`TRIES`] times at most; asked to
    /// wait past [`LONGEST_WAIT`], it doesn't. Fails when Discogs can't be reached, when the
    /// account can't be used (`access` or `scope`, as the host fails the request), or when the
    /// hook is cancelled.
    fn search(&mut self, params: &[(&str, &str)]) -> Result<Result<Vec<Hit>, u16>, RpcError> {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("type", "release")
            .extend_pairs(params)
            .append_pair("per_page", PER_PAGE)
            .finish();
        let headers = IndexMap::from([("Authorization".to_string(), AUTHORIZATION.to_string())]);
        let fetch = Fetch { headers, ..Fetch::get(format!("{API}/database/search?{query}")) };
        let ctx = self.host.ctx();
        let mut tries = 1;
        loop {
            if let Some(spent) = self.spent.take() {
                pause(&ctx, (SECOND * WINDOW).saturating_sub(spent.elapsed()))?;
            }
            let fetched = self.host.fetch(&fetch).map_err(|e| match e.kind() {
                Some("cancelled" | "access" | "scope") => e,
                _ => RpcError { message: format!("couldn't reach Discogs: {}", e.message), ..e },
            })?;
            eprintln!("discogs: search {query}: {}", fetched.status);
            if fetched.headers.get("x-discogs-ratelimit-remaining").is_some_and(|left| left.trim() == "0") {
                self.spent = Some(Instant::now());
            }
            match fetched.status {
                429 if tries < TRIES => {
                    let wait = retry_after(&fetched).unwrap_or(WINDOW.into());
                    if wait > LONGEST_WAIT {
                        return Ok(Err(429));
                    }
                    // At most LONGEST_WAIT, which fits.
                    pause(&ctx, SECOND * wait as u32)?;
                    // Waited out, as Discogs asked.
                    self.spent = None;
                    tries += 1;
                }
                200 => {
                    let answer = serde_json::from_str(fetched.body.as_deref().unwrap_or_default()).map_err(|e| {
                        RpcError::failed("external", format!("Discogs answered what isn't a search result: {e}"))
                    });
                    return answer.map(|Results { results }| Ok(results));
                }
                status => return Ok(Err(status)),
            }
        }
    }
}

/// How many seconds Discogs asks to wait (`Retry-After`, a number of them), when it says. One too
/// big to count is longer than any wait.
fn retry_after(answer: &Fetched) -> Option<u64> {
    let said = answer.headers.get("retry-after")?.trim();
    let digits = !said.is_empty() && said.bytes().all(|b| b.is_ascii_digit());
    digits.then(|| said.parse().unwrap_or(u64::MAX))
}

/// Waits `wait`, failing with `cancelled` once `ctx` is.
fn pause(ctx: &Ctx<'_>, wait: Duration) -> Result<(), RpcError> {
    let until = Instant::now() + wait;
    loop {
        ctx.check()?;
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(());
        }
        std::thread::sleep(left.min(CHECK_EVERY));
    }
}

/// `hits` as releases, each master's once, in their order: only those titled `album` when it's
/// an album's.
fn releases(hits: Vec<Hit>, album: Option<&str>) -> Vec<Release> {
    // Each release's master (or, without one, the release itself), by its id.
    let mut found: IndexMap<(bool, u64), Release> = IndexMap::new();
    for hit in hits {
        let title = title_of(&hit.title);
        if album.is_some_and(|album| !same(title, album)) {
            continue;
        }
        let key = match hit.master_id.filter(|&m| m > 0) {
            Some(master) => (true, master),
            None => (false, hit.id),
        };
        let year = hit.year.and_then(|y| y.trim().parse().ok()).filter(|&y: &u16| y > 0);
        let label = hit.label.iter().map(|l| plain_label(l)).find(|l| !l.is_empty() && !l.starts_with(NO_LABEL));
        let release = Release {
            id: hit.id,
            album: title.to_string(),
            year,
            label: label.map(String::from),
            catno: hit.catno.map(|c| c.trim().to_string()).filter(|c| !c.is_empty() && c != "none"),
            genre: hit.style.into_iter().chain(hit.genre).map(|g| g.trim().to_string()).find(|g| !g.is_empty()),
            cover: hit.cover_image.filter(|c| c.starts_with(IMAGES)),
        };
        match found.get_mut(&key) {
            Some(had) => {
                had.year = had.year.into_iter().chain(release.year).min();
                had.label = had.label.take().or(release.label);
                had.catno = had.catno.take().or(release.catno);
                had.genre = had.genre.take().or(release.genre);
                had.cover = had.cover.take().or(release.cover);
            }
            None => {
                found.insert(key, release);
            }
        }
    }
    found.into_values().collect()
}

/// What Discogs credits a release with when it came out on no label (`Not On Label (Bicep
/// Self-released)`).
const NO_LABEL: &str = "Not On Label";

/// A label's name without the number Discogs tells apart labels of one name with: `Warp (2)` is
/// `Warp`.
fn plain_label(name: &str) -> &str {
    let name = name.trim();
    let numbered = name.strip_suffix(')').and_then(|n| n.rsplit_once(" ("));
    match numbered {
        Some((plain, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => plain.trim_end(),
        _ => name,
    }
}

/// A release's title from a search's `Artist - Title`.
fn title_of(title: &str) -> &str {
    title.split_once(" - ").map_or(title, |(_, title)| title).trim()
}

/// Whether two titles are the same but for letter case, spacing, Unicode forms, and the curly
/// quotes and hyphens a title may have where tags have straight ones.
fn same(a: &str, b: &str) -> bool {
    let plain = |s: &str| normalize(s).replace(['\u{2018}', '\u{2019}'], "'").replace(['\u{2010}', '\u{2011}'], "-");
    plain(a) == plain(b)
}

/// `release`'s cover, downloaded as its file in `scratch` (its name there), or what the warning
/// says instead. Fails when the hook is cancelled.
pub(crate) fn cover(host: &Host, scratch: &Path, release: &Release) -> Result<Result<String, String>, RpcError> {
    let Some(url) = &release.cover else { return Ok(Err("Discogs has no cover for it".into())) };
    let name = format!("cover-{}", release.id);
    let file = scratch.join(&name);
    let fetched = match host.fetch(&Fetch { into_blob: Some(file.clone()), ..Fetch::get(url) }) {
        Err(e) if e.kind() == Some("cancelled") => return Err(e),
        Err(e) => return Ok(Err(format!("its cover didn't download: {}", e.message))),
        Ok(fetched) => fetched,
    };
    eprintln!("discogs: cover of release {}: {}", release.id, fetched.status);
    let usable = match fetched.status {
        200 => usable(&file),
        status => Err(format!("its cover didn't download: Discogs answered {status}")),
    };
    // locallibrary refuses the whole answer when a candidate's file isn't an image it takes.
    if usable.is_err() {
        let _ = std::fs::remove_file(&file);
    }
    Ok(usable.map(|()| name))
}

/// Whether `file` is an image locallibrary takes as artwork, or why not.
fn usable(file: &Path) -> Result<(), String> {
    let bytes = std::fs::read(file).map_err(|e| format!("its cover didn't download: {e}"))?;
    let image = image::load_from_memory(&bytes).map_err(|e| format!("its cover isn't an image: {e}"))?;
    match image.width() < SMALLEST || image.height() < SMALLEST {
        true => Err(format!("its cover is too small ({} x {} px)", image.width(), image.height())),
        false => Ok(()),
    }
}

#[cfg(test)]
mod tests;
