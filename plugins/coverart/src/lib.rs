//! The coverart plugin: cover art and tags found online for the tracks locallibrary asks about (an
//! artwork provider and a metadata one). Each album is looked up once on MusicBrainz (see
//! `musicbrainz`), as either provider asks. For artwork, the front cover of each release group it
//! could be is downloaded from the Cover Art Archive (see `covers`), a candidate for each of the
//! album's tracks. For metadata, each release group's album title, year and label are a candidate
//! for each of its tracks, with its genre when it's surely the album, and the title, artist and
//! ISRC of the recording an ISRC found for the track with that ISRC. locallibrary offers them,
//! makes the edits chosen, and journals them. The host makes its requests (`net/fetch`), a second
//! apart for MusicBrainz as the manifest asks, and the covers go to it as files in the request's
//! scratch folder, which it removes once it has read them: nothing downloaded is kept, as it's
//! cheap to fetch again.

mod covers;
mod musicbrainz;

use std::fmt;
use std::io::{Read, Write};
use std::path::Path;

use indexmap::IndexMap;
use loclib_model::model::normalize;
use loclib_plugin::{
    Blob, Candidate, Candidates, Hooks, Host, ProvideCandidates, ProviderKind, RpcError, TrackMetadata, TrackTags,
};
use loclib_progress::{Event, Phase};

use musicbrainz::{Found, Group, ReleaseGroup};

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(provide), input, output);
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
    /// Its tracks' ids, each with its ISRC.
    tracks: Vec<(u32, Option<String>)>,
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
    // Each release group's cover, by its id: its file, or why there's none; and its genre, or
    // the status MusicBrainz refused it with.
    let mut covers: Fetched<Result<String, String>> = IndexMap::new();
    let mut genres: Fetched<Result<Option<String>, u16>> = IndexMap::new();
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
            None => metadata(host, album, groups, score, &mut genres, &mut warnings)?,
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
            Ok(blob) => found.extend(album.tracks.iter().map(|&(track, _)| Candidate {
                track,
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

/// The candidates for `album`'s tracks of `groups`' tags. The genre is asked for (once for each
/// release group, kept in `genres`) only when one release group alone was found: another's may
/// not be the album's. Why it can't be had is a warning.
fn metadata(
    host: &Host,
    album: &Album,
    groups: Vec<Group>,
    score: f64,
    genres: &mut Fetched<Result<Option<String>, u16>>,
    warnings: &mut Vec<String>,
) -> Result<Vec<Candidate>, RpcError> {
    let genre = match groups.as_slice() {
        [one] => once(genres, &one.group.id, || musicbrainz::genre(host, &one.group.id))?.unwrap_or_else(|status| {
            warnings.push(format!("{album}: no genre, as MusicBrainz answered {status}"));
            None
        }),
        _ => None,
    };
    let mut found = Vec::new();
    for g in &groups {
        let label = label(&g.group);
        for (track, isrc) in &album.tracks {
            // The recording an ISRC found is the track with that ISRC alone: the album's other
            // tracks are other recordings.
            let recording = g.recording.as_ref().filter(|_| *isrc == album.isrc);
            let tags = TrackMetadata {
                title: recording.map(|(title, _)| title.clone()),
                artist: recording.map(|(_, artist)| artist.clone()),
                album: Some(g.album.clone()),
                year: g.year,
                genre: genre.clone(),
                label: g.label.clone(),
                isrc: recording.and(isrc.clone()),
            };
            found.push(Candidate {
                track: *track,
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
        album.tracks.push((t.id, isrc));
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
