//! The discogs plugin: cover art and tags found on Discogs for the tracks locallibrary asks about
//! (an artwork provider and a metadata one). Each album is looked up once (see `discogs`), by its
//! record label too when a track of it has one, which it reads from the library: a provider is
//! given no track's label. A track without an album is looked up by its title. For artwork, the
//! front cover of each release found is downloaded from Discogs' image server, a candidate for
//! each of the album's tracks; for metadata, each release's title, year, label and style. Discogs
//! answers searches only for a signed-in account: each names a `discogs` account
//! (discogs-account's), whose personal access token the host fills in, and without one the
//! providers answer nothing but how to sign in. The host makes the requests (`net/fetch`), a
//! second apart for Discogs' API as the manifest asks, and the covers go to it as files in the
//! request's scratch folder, which it removes once it has read them.

mod discogs;

use std::fmt;
use std::io::{Read, Write};

use indexmap::IndexMap;
use loclib_model::model::normalize;
use loclib_plugin::{
    Blob, Candidate, Candidates, Hooks, Host, ProvideCandidates, ProviderKind, RpcError, TrackMetadata, TrackTags,
};
use loclib_progress::{Event, Phase};

use discogs::{By, Discogs, Found};

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(provide), input, output);
}

/// Where candidates are from.
const SOURCE: &str = "Discogs";

/// A candidate of the one release (of its editions) named as the album is, on the album's label.
const BY_LABEL: f64 = 0.95;
/// A candidate of the one release named as the album is, by its artist.
const BY_NAME: f64 = 0.9;
/// A candidate of the one release a track's title is on: maybe not the album the user means.
const BY_TITLE: f64 = 0.6;
/// A candidate of one of several releases: the user says which.
const ONE_OF_SEVERAL: f64 = 0.5;
/// The most releases an album's candidates are offered from: each cover is a download.
const MAX_RELEASES: usize = 5;

/// Where a Discogs account's personal access token is made.
const TOKENS: &str = "https://www.discogs.com/settings/developers";
/// How to sign in for this plugin.
const SIGN_IN: &str = "locallibrary account discogs sign-in --for discogs";

/// Tracks looked up together: an album's, or a track without one, by its title.
pub(crate) struct Lookup {
    /// The album's artist, else the track's.
    pub(crate) artist: String,
    /// The album's name, else the track's title.
    pub(crate) name: String,
    /// It's a track without an album, looked up by its title.
    pub(crate) single: bool,
    /// The first record label its tracks have.
    pub(crate) label: Option<String>,
    tracks: Vec<u32>,
}

impl fmt::Display for Lookup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.artist.is_empty() {
            true => f.write_str(&self.name),
            false => write!(f, "{} - {}", self.artist, self.name),
        }
    }
}

/// `provide/candidates`: for each album of the tracks asked about, a candidate for each of its
/// tracks of each release found (of at most [`MAX_RELEASES`]), its cover or its tags as the
/// provider asked finds them, and a warning for each album with none. Without an account it may
/// use, or with one whose token Discogs refuses, it looks up nothing more and says how to sign in.
fn provide(req: ProvideCandidates, host: &Host) -> Result<Candidates, RpcError> {
    // Covers go to the host as files; tags go in the answer.
    let scratch = match req.kind {
        ProviderKind::Artwork => Some(req.context.scratch.as_deref().ok_or_else(|| {
            RpcError::invalid_params("the host gives the plugin no scratch folder for the covers (the blobs feature)")
        })?),
        ProviderKind::Metadata => None,
    };
    let mut warnings = Vec::new();
    let lookups = lookups(&req.tracks, &labels(host, &req)?, &mut warnings);
    let ctx = host.ctx();
    ctx.phase_start(Phase::Scan, lookups.len() as u64, 0);
    let mut api = Discogs::new(host);
    // Each release's cover, by its id: its file, or why there's none.
    let mut covers: IndexMap<u64, Result<String, String>> = IndexMap::new();
    let mut candidates = Vec::new();
    for lookup in &lookups {
        ctx.emit(Event::Item { phase: Phase::Scan, path: lookup.to_string(), bytes: 0 });
        let (releases, by) = match api.find(lookup) {
            Err(e) if matches!(e.kind(), Some("access" | "scope")) => {
                let why = format!("Discogs answers searches only for a signed-in account ({})", e.message);
                warnings.push(format!("{why}: make a personal access token at {TOKENS} and sign in with `{SIGN_IN}`"));
                break;
            }
            Err(e) => return Err(e),
            Ok(Found::Refused(401)) => {
                let why = "Discogs refused the account's personal access token (401)";
                warnings.push(format!("{why}: make another at {TOKENS} and sign in again with `{SIGN_IN}`"));
                break;
            }
            Ok(Found::Refused(status)) => {
                warnings.push(format!("{lookup}: left out, as Discogs answered {status}"));
                continue;
            }
            Ok(Found::Releases { releases, .. }) if releases.is_empty() => {
                warnings.push(format!("{lookup}: not on Discogs"));
                continue;
            }
            Ok(Found::Releases { releases, by }) => (releases, by),
        };
        let score = match (releases.len(), by) {
            (1, By::Label) => BY_LABEL,
            (1, By::Name) => BY_NAME,
            (1, By::Title) => BY_TITLE,
            _ => ONE_OF_SEVERAL,
        };
        for release in releases.into_iter().take(MAX_RELEASES) {
            let label = release.to_string();
            let offer = |artwork: Option<Blob>, metadata: Option<TrackMetadata>| -> Vec<Candidate> {
                let each = |&track: &u32| Candidate {
                    track,
                    score,
                    source: SOURCE.into(),
                    label: label.clone(),
                    artwork: artwork.clone(),
                    metadata: metadata.clone(),
                };
                lookup.tracks.iter().map(each).collect()
            };
            let Some(scratch) = scratch else {
                let tags = TrackMetadata {
                    album: Some(release.album.clone()),
                    year: release.year,
                    genre: release.genre.clone(),
                    label: release.label.clone(),
                    ..TrackMetadata::default()
                };
                candidates.extend(offer(None, Some(tags)));
                continue;
            };
            let cover = match covers.get(&release.id) {
                Some(had) => had.clone(),
                None => {
                    let new = discogs::cover(host, scratch, &release)?;
                    covers.insert(release.id, new.clone());
                    new
                }
            };
            match cover {
                Ok(blob) => candidates.extend(offer(Some(Blob { blob }), None)),
                Err(why) => warnings.push(format!("{lookup}, {label}: {why}")),
            }
        }
    }
    ctx.phase_end(Phase::Scan);
    Ok(Candidates { candidates, warnings })
}

/// The record label of each track asked about with an album, by its id, as the library has it:
/// what an album's first search narrows to. Read only when there's one, from the drive asked
/// about.
fn labels(host: &Host, req: &ProvideCandidates) -> Result<IndexMap<u32, String>, RpcError> {
    let ids: Vec<u32> = req.tracks.iter().filter(|t| !t.album.trim().is_empty()).map(|t| t.id).collect();
    let Some(drive) = req.context.drive.as_deref().filter(|_| !ids.is_empty()) else { return Ok(IndexMap::new()) };
    let read = host.drive(drive).tracks(&ids)?;
    Ok(read.tracks.into_iter().map(|t| (t.id, t.label)).collect())
}

/// `tracks` by album (by artist and name, ignoring letter case and spacing), and those without one
/// by title, in the order they come, each album with the first of its tracks' `labels`. A track
/// with neither album nor title is warned about.
fn lookups(tracks: &[TrackTags], labels: &IndexMap<u32, String>, warnings: &mut Vec<String>) -> Vec<Lookup> {
    let mut lookups: IndexMap<(String, String, bool), Lookup> = IndexMap::new();
    for t in tracks {
        let artist = [&t.album_artist, &t.artist].into_iter().map(|a| a.trim()).find(|a| !a.is_empty());
        let artist = artist.unwrap_or_default();
        let (name, single) = match (t.album.trim(), t.title.trim()) {
            ("", "") => {
                warnings.push(format!("track #{} has no album or title to look up", t.id));
                continue;
            }
            ("", title) => (title, true),
            (album, _) => (album, false),
        };
        let lookup = lookups.entry((normalize(artist), normalize(name), single)).or_insert_with(|| Lookup {
            artist: artist.into(),
            name: name.into(),
            single,
            label: None,
            tracks: Vec::new(),
        });
        let label = labels.get(&t.id).map(|l| l.trim()).filter(|l| !l.is_empty());
        lookup.label = lookup.label.take().or_else(|| label.map(String::from));
        lookup.tracks.push(t.id);
    }
    lookups.into_values().collect()
}

#[cfg(test)]
mod tests;
