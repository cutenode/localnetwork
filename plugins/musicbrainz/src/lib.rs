//! The musicbrainz plugin: one analyzer, `ids`, answering a track's MusicBrainz recording,
//! release and artist IDs, which `sets publish` reads by their roles (`recording`, `release`,
//! `artists`) and publishes with a set's tracks. rekordbox keeps no MusicBrainz IDs, but most
//! tracks from a store carry an ISRC, and MusicBrainz knows which recording most ISRCs are: the
//! track's is looked up (see `lookup`), and the IDs its answer gives it kept (see `matching`).
//! The host keeps them by the track's audio file, so each is looked up once: a field that's null
//! was worked out, with nothing found. With them go the ISRC they were looked up by (`isrc`) and
//! the names the artists' IDs pair with (`credits`), so the host takes them only for a track
//! still tagged that way.

mod lookup;
mod matching;

use std::io::{Read, Write};

use loclib_model::model::{artist_credits, isrc};
use loclib_plugin::{Analysis, AnalyzeTrack, FieldValue, Hooks, Host, RpcError};
use uuid::Uuid;

use matching::Ids;

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(analyze_track), input, output);
}

/// `ids`: the track's tags, read from the library (the host hands an analyzer only its files),
/// and the IDs its ISRC gives it; none without one.
fn analyze_track(req: AnalyzeTrack, host: &Host) -> Result<Analysis, RpcError> {
    let drive = req.context.drive.as_deref().ok_or_else(|| RpcError::invalid_params("no drive for the track"))?;
    let id = req.track.id;
    let Some(track) = host.drive(drive).tracks(&[id])?.tracks.into_iter().next() else {
        return Err(RpcError::failed("not_found", format!("the library has no track #{id}")));
    };
    let isrc = isrc(&track.isrc);
    let ids = match &isrc {
        Some(isrc) => matching::ids(&track, &lookup::recordings(host, isrc)?),
        None => Ids::default(),
    };
    Ok(fields(ids, isrc, artist_credits(&track.artist)))
}

/// `ids` as the analyzer's fields, each a bare lowercase UUID or null, with the `isrc` they were
/// looked up by and the `credits` the artists' pair with.
fn fields(ids: Ids, isrc: Option<String>, credits: Vec<String>) -> Analysis {
    let id = |id: Option<Uuid>| id.map_or(FieldValue::Null, |id| FieldValue::String(id.to_string()));
    let artists = match ids.artists.is_empty() {
        true => FieldValue::Null,
        false => FieldValue::Strings(ids.artists.iter().map(Uuid::to_string).collect()),
    };
    let fields = [
        ("recording", id(ids.recording)),
        ("release", id(ids.release)),
        ("artists", artists),
        ("isrc", isrc.map_or(FieldValue::Null, FieldValue::String)),
        ("credits", FieldValue::Strings(credits)),
    ];
    Analysis { fields: fields.into_iter().map(|(name, v)| (name.into(), v)).collect() }
}

#[cfg(test)]
mod tests;
