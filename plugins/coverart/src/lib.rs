//! The coverart plugin: cover art and tags found online for the tracks locallibrary asks about (an
//! artwork provider and a metadata one). Each album is looked up once on MusicBrainz (see
//! `musicbrainz`), as either provider asks. For artwork, the front cover of each release group it
//! could be is downloaded from the Cover Art Archive (see `covers`), a candidate for each of the
//! album's tracks. For metadata, each release group's album title and year, and its label when it
//! was found by name, are a candidate for each of its tracks, with its genre (and the year it
//! first came out) when it's surely the album, and the title and artist of the recording an ISRC
//! found for the track with that ISRC. A track without an album, looked up by its title, gets only
//! a single's. locallibrary offers them, makes the edits chosen, and journals them. The host makes
//! its requests (`net/fetch`), a second apart for MusicBrainz as the manifest asks, and the covers
//! go to it as files in the request's scratch folder, which it removes once it has read them:
//! nothing downloaded is kept, as it's cheap to fetch again. Once a drive is plugged in or its
//! databases change, it suggests fetching covers for its tracks without artwork (see [`new_tracks`]).

mod covers;
mod musicbrainz;

use std::fmt;
use std::io::{Read, Write};
use std::path::Path;

use indexmap::IndexMap;
use loclib_model::model::normalize;
use loclib_plugin::{
    Blob, Candidate, Candidates, EventNotify, FetchAction, Hooks, Host, HostAction, HostNotify, MAX_SUGGESTED_TRACKS,
    ProvideCandidates, ProviderKind, RpcError, TrackMetadata, TrackTags,
};
use loclib_progress::{Event, Phase};

use musicbrainz::{Found, Group, Lookup, ReleaseGroup};

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(provide).on_event(new_tracks), input, output);
}

/// The artwork provider's name, as the manifest has it.
const ARTWORK_PROVIDER: &str = "covers";

/// `drive.mounted` and `drive.changed`: once a drive is plugged in or its databases change, its
/// tracks without artwork (the first [`MAX_SUGGESTED_TRACKS`]), suggested for its covers. The
/// host leaves out the tracks it suggested before, so only new ones reach the user.
fn new_tracks(e: EventNotify, host: &Host) -> Result<(), RpcError> {
    let Some(drive) = e.new_tracks() else { return Ok(()) };
    let library = host.drive(drive).library()?;
    let bare = library.tracks.iter().filter(|t| t.artwork_id.is_none()).map(|t| t.id);
    let tracks: Vec<u32> = bare.take(MAX_SUGGESTED_TRACKS).collect();
    if tracks.is_empty() {
        return Ok(());
    }
    let fetch = FetchAction { drive: drive.into(), tracks, provider: Some(ARTWORK_PROVIDER.into()) };
    host.notify(&HostNotify::action("new tracks have no cover art", &HostAction::ArtFetch(fetch)));
    Ok(())
}

/// Where the covers are from, as candidates say.
const SOURCE: &str = "Cover Art Archive";
/// Where the tags are from.
const TAGS_SOURCE: &str = "MusicBrainz";

/// A candidate of the one release group an album's ISRC is on: an ISRC names the recording, so
/// it's surer than a name, which another album can have too.
const BY_ISRC: f64 = 0.95;
/// A candidate of the one release group named as the album is.
const BY_NAME: f64 = 0.9;
/// A candidate of one of several release groups named as the album is: the user says which.
const ONE_OF_SEVERAL: f64 = 0.5;
/// The most release groups an album's candidates are offered from: each cover is a download.
const MAX_GROUPS: usize = 5;

/// Tracks looked up together: an album's, or a track without one, by its title (a single).
pub(crate) struct Album {
    /// The album's artist, else the track's.
    pub(crate) artist: String,
    /// The album's name, else the track's title.
    pub(crate) name: String,
    /// The first of its tracks' ISRCs.
    pub(crate) isrc: Option<String>,
    tracks: Vec<Track>,
}

/// A track of an [`Album`].
struct Track {
    id: u32,
    isrc: Option<String>,
    /// It has no album: it was looked up by its title, as a single.
    single: bool,
}

impl fmt::Display for Album {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.artist.is_empty() {
            true => f.write_str(&self.name),
            false => write!(f, "{} - {}", self.artist, self.name),
        }
    }
}

/// `provide/candidates`: for each album of the tracks asked about, a candidate for each of its
/// tracks of each release group found (of at most [`MAX_GROUPS`]), its cover or its tags as the
/// provider asked finds them, and a warning for each album with none.
fn provide(req: ProvideCandidates, host: &Host) -> Result<Candidates, RpcError> {
    // Covers go to the host as files; tags go in the answer.
    let scratch = match req.kind {
        ProviderKind::Artwork => Some(req.context.scratch.as_deref().ok_or_else(|| {
            RpcError::invalid_params("the host gives the plugin no scratch folder for the covers (the blobs feature)")
        })?),
        ProviderKind::Metadata => None,
    };
    let mut warnings = Vec::new();
    let albums = albums(&req.tracks, &mut warnings);
    let ctx = host.ctx();
    ctx.phase_start(Phase::Scan, albums.len() as u64, 0);
    // Each release group's cover, by its id: its file, or why there's none; and its lookup, or
    // the status MusicBrainz refused it with.
    let mut covers: Fetched<Result<String, String>> = IndexMap::new();
    let mut lookups: Fetched<Result<Lookup, u16>> = IndexMap::new();
    let mut candidates = Vec::new();
    // Each album's requests notice the hook being cancelled.
    for album in &albums {
        ctx.emit(Event::Item { phase: Phase::Scan, path: album.to_string(), bytes: 0 });
        let (groups, by_isrc) = match musicbrainz::release_group(host, album)? {
            Found::Groups { groups, .. } if groups.is_empty() => {
                warnings.push(format!("{album}: not on MusicBrainz"));
                continue;
            }
            Found::Groups { groups, by_isrc } => (groups, by_isrc),
            Found::Refused(status) => {
                warnings.push(format!("{album}: left out, as MusicBrainz answered {status}"));
                continue;
            }
        };
        let score = match (groups.len(), by_isrc) {
            (1, true) => BY_ISRC,
            (1, false) => BY_NAME,
            _ => ONE_OF_SEVERAL,
        };
        let groups: Vec<Group> = groups.into_iter().take(MAX_GROUPS).collect();
        candidates.extend(match scratch {
            Some(scratch) => artwork(host, scratch, album, groups, score, &mut covers, &mut warnings)?,
            None => metadata(host, album, groups, score, &mut lookups, &mut warnings)?,
        });
    }
    ctx.phase_end(Phase::Scan);
    Ok(Candidates { candidates, warnings })
}

/// The candidates for `album`'s tracks of `groups`' covers, each a file in `scratch` downloaded
/// once for each release group (kept in `covers`). Why a release group's cover can't be had is a
/// warning.
fn artwork(
    host: &Host,
    scratch: &Path,
    album: &Album,
    groups: Vec<Group>,
    score: f64,
    covers: &mut Fetched<Result<String, String>>,
    warnings: &mut Vec<String>,
) -> Result<Vec<Candidate>, RpcError> {
    let several = groups.len() > 1;
    let mut found = Vec::new();
    for Group { group, .. } in groups {
        let label = label(&group);
        match once(covers, &group.id, || covers::front(host, scratch, &group.id))? {
            Ok(blob) => found.extend(album.tracks.iter().map(|t| Candidate {
                track: t.id,
                score,
                source: SOURCE.into(),
                label: label.clone(),
                artwork: Some(Blob { blob: blob.clone() }),
                metadata: None,
            })),
            Err(why) if several => warnings.push(format!("{album}, {label}: {why}")),
            Err(why) => warnings.push(format!("{album}: {why}")),
        }
    }
    Ok(found)
}

/// The candidates for `album`'s tracks of `groups`' tags. A release group is looked up for its
/// genre and the year it first came out (once for each, kept in `lookups`) only when it alone was
/// found: another's may not be the album's. Why it can't be had, and a track without an album that
/// no single was found for, are warnings.
fn metadata(
    host: &Host,
    album: &Album,
    groups: Vec<Group>,
    score: f64,
    lookups: &mut Fetched<Result<Lookup, u16>>,
    warnings: &mut Vec<String>,
) -> Result<Vec<Candidate>, RpcError> {
    // A track without an album was looked up by its title: only a single of that name is its
    // record, as an album of that name is its title track's.
    let fits = |t: &Track, g: &Group| !t.single || g.group.primary_type.as_deref() == Some("Single");
    let lookup = match groups.as_slice() {
        [one] if album.tracks.iter().any(|t| fits(t, one)) => {
            let looked_up = once(lookups, &one.group.id, || musicbrainz::look_up(host, &one.group.id))?;
            looked_up
                .map_err(|status| warnings.push(format!("{album}: no genre, as MusicBrainz answered {status}")))
                .ok()
        }
        _ => None,
    };
    for t in album.tracks.iter().filter(|t| !groups.iter().any(|g| fits(t, g))) {
        warnings.push(format!("{album}: track #{} has no album, and MusicBrainz has no single of that name", t.id));
    }
    let mut found = Vec::new();
    for g in &groups {
        let label = label(&g.group);
        for t in album.tracks.iter().filter(|t| fits(t, g)) {
            // The recording an ISRC found is the track with that ISRC alone: the album's other
            // tracks are other recordings.
            let recording = g.recording.as_ref().filter(|_| t.isrc == album.isrc);
            let tags = TrackMetadata {
                title: recording.map(|(title, _)| title.clone()),
                artist: recording.map(|(_, artist)| artist.clone()),
                album: Some(g.album.clone()),
                // Its lookup's when it alone was found, else the earliest on the search's page.
                year: lookup.as_ref().and_then(|l| l.year).or(g.year),
                genre: lookup.as_ref().and_then(|l| l.genre.clone()),
                label: g.label.clone(),
                // An ISRC isn't offered: only a track with it found its recording.
                isrc: None,
            };
            found.push(Candidate {
                track: t.id,
                score,
                source: TAGS_SOURCE.into(),
                label: label.clone(),
                artwork: None,
                metadata: Some(tags),
            });
        }
    }
    Ok(found)
}

/// What was fetched for each release group, by its id.
type Fetched<T> = IndexMap<String, T>;

/// Release group `id`'s entry in `fetched`, fetched the first time it's asked for: two albums can
/// be one release group.
fn once<T: Clone>(
    fetched: &mut Fetched<T>,
    id: &str,
    fetch: impl FnOnce() -> Result<T, RpcError>,
) -> Result<T, RpcError> {
    if let Some(had) = fetched.get(id) {
        return Ok(had.clone());
    }
    let new = fetch()?;
    fetched.insert(id.into(), new.clone());
    Ok(new)
}

/// What a release group's candidates are shown as: its title and its kind (`Discovery, album`).
fn label(group: &ReleaseGroup) -> String {
    let kind = group.primary_type.as_deref().unwrap_or("release").to_lowercase();
    format!("{}, {kind}", group.title)
}

/// `tracks` by album (by artist and name, ignoring letter case and spacing), in the order they
/// come. A track with neither album nor title is warned about.
fn albums(tracks: &[TrackTags], warnings: &mut Vec<String>) -> Vec<Album> {
    let mut albums: IndexMap<(String, String), Album> = IndexMap::new();
    for t in tracks {
        let (artist, name) = (first([&t.album_artist, &t.artist]), first([&t.album, &t.title]));
        if name.is_empty() {
            warnings.push(format!("track #{} has no album or title to look up", t.id));
            continue;
        }
        let album = albums.entry((normalize(artist), normalize(name))).or_insert_with(|| Album {
            artist: artist.into(),
            name: name.into(),
            isrc: None,
            tracks: Vec::new(),
        });
        let isrc = isrc(&t.isrc);
        album.isrc = album.isrc.take().or_else(|| isrc.clone());
        album.tracks.push(Track { id: t.id, isrc, single: t.album.trim().is_empty() });
    }
    albums.into_values().collect()
}

/// The first of `tags` that isn't blank, trimmed.
fn first(tags: [&str; 2]) -> &str {
    tags.into_iter().map(str::trim).find(|s| !s.is_empty()).unwrap_or_default()
}

/// `tag` as an ISRC: twelve letters and digits, without the hyphens and spaces some tags have.
fn isrc(tag: &str) -> Option<String> {
    let isrc: String = tag.chars().filter(|c| !matches!(c, '-' | ' ')).collect::<String>().to_ascii_uppercase();
    (isrc.len() == 12 && isrc.chars().all(|c| c.is_ascii_alphanumeric())).then_some(isrc)
}

#[cfg(test)]
mod tests;
