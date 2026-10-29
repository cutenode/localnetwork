//! A set's recording, uploaded to the DJ's Atmosphere account and published as a recording record
//! ([`record`]), which the set's record then points at.
//!
//! It goes up as it is when browsers play it (MP3, or AAC alone or in an M4A file) and it fits the
//! data server. Otherwise ffmpeg (from `PATH`) encodes it again as AAC in an M4A file, with its
//! index at the front so a player can start before it has the whole file: at [`KBPS`], or less to
//! fit, down to [`LEAST_KBPS`] ([`encoding`]). No data server says how big a file it takes: the
//! reference one takes 5 MiB unless whoever runs it sets `PDS_BLOB_UPLOAD_LIMIT` (its installer
//! allows 300 MiB), and Bluesky's don't say. So the limit is the recording Lexicon's,
//! [`MAX_BYTES`], unless the DJ names a smaller one.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{DateTime, Utc};
use lofty::file::FileType;
use lofty::mp4::{Mp4Codec, Mp4File};
use lofty::prelude::*;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use crate::atmosphere::lexicon;
use crate::error::IoContext;
use crate::export::one_line;
use crate::op::bytes;
use crate::progress::{Ctx, Event, Phase};
use crate::{Error, Result};

/// The most a recording may be: the recording Lexicon's `maxSize`, 300 MiB.
pub const MAX_BYTES: u64 = 300 * 1024 * 1024;

/// The bitrate a recording is encoded at when it fits, in kbit/s: plenty for a mix.
pub const KBPS: u32 = 128;

/// The least it's encoded at to fit, in kbit/s: below that, a mix sounds worse than people expect.
pub const LEAST_KBPS: u32 = 64;

/// The share of the limit an encoding aims for, leaving room for the M4A file's index and an
/// encoder that runs a little over its bitrate.
const HEADROOM: f64 = 0.97;

/// A recording on this computer, as uploading it needs it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Audio {
    pub path: PathBuf,
    /// What it is, for people: `WAV`, `MP3`, `M4A (ALAC)`.
    pub format: String,
    pub bytes: u64,
    /// How long it plays, to the nearest second.
    pub duration_secs: u64,
    /// Its type, when browsers play it as it is: `audio/mpeg`, `audio/aac` or `audio/mp4`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playable: Option<String>,
}

/// Read what kind of audio the file at `path` is, how big and how long.
pub fn probe(path: &Path) -> Result<Audio> {
    let unreadable = |e: &dyn std::fmt::Display| {
        Error::invalid(one_line(&format!(
            "{} can't be uploaded: it isn't audio locallibrary can read ({e}); export the set as WAV, FLAC or MP3 \
             and attach that (`sets edit SET --recording FILE`)",
            path.display()
        )))
    };
    let bytes = std::fs::metadata(path).at(path)?.len();
    let options = lofty::config::ParseOptions::new().read_tags(false);
    let tagged = lofty::probe::Probe::open(path)
        .map_err(|e| unreadable(&e))?
        .options(options)
        .guess_file_type()
        .at(path)?
        .read()
        .map_err(|e| unreadable(&e))?;
    let (format, playable) = match tagged.file_type() {
        FileType::Mpeg => ("MP3".to_string(), Some("audio/mpeg")),
        FileType::Aac => ("AAC".to_string(), Some("audio/aac")),
        FileType::Mp4 => {
            let mut file = std::fs::File::open(path).at(path)?;
            let codec = Mp4File::read_from(&mut file, options).map_err(|e| unreadable(&e))?.properties().codec();
            match codec {
                Some(Mp4Codec::AAC) => ("M4A".to_string(), Some("audio/mp4")),
                other => (format!("M4A ({})", other.map_or("unknown".into(), |c| format!("{c:?}"))), None),
            }
        }
        other => (format!("{other:?}").to_uppercase(), None),
    };
    let duration_secs = tagged.properties().duration().as_secs_f64().round() as u64;
    Ok(Audio { path: path.to_path_buf(), format, bytes, duration_secs, playable: playable.map(String::from) })
}

/// How a recording goes up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Encoding {
    /// As it is.
    AsIs,
    /// Encoded again by ffmpeg as AAC in an M4A file at `kbps` kbit/s, about `bytes` long.
    Aac { kbps: u32, bytes: u64 },
}

/// How a recording of `bytes`, playing for `duration_secs`, goes up to a data server that takes
/// `limit` bytes at most: as it is when browsers play it (`playable`) and it fits; otherwise as AAC
/// at [`KBPS`], or the most that fits, down to [`LEAST_KBPS`]. Longer than fits even then: an
/// error.
pub fn encoding(playable: bool, bytes: u64, duration_secs: u64, limit: u64) -> Result<Encoding> {
    if playable && bytes <= limit {
        return Ok(Encoding::AsIs);
    }
    let secs = duration_secs.max(1);
    let fits = (limit as f64 * HEADROOM * 8.0 / 1000.0 / secs as f64).floor();
    let kbps = fits.min(f64::from(KBPS)) as u32;
    if kbps < LEAST_KBPS {
        return Err(Error::invalid(format!(
            "the recording is too long to upload: even at {LEAST_KBPS} kbit/s it would be about {}, more than {}",
            self::bytes(aac_bytes(LEAST_KBPS, secs)),
            self::bytes(limit)
        )));
    }
    Ok(Encoding::Aac { kbps, bytes: aac_bytes(kbps, secs) })
}

/// About how big `secs` of AAC at `kbps` is.
fn aac_bytes(kbps: u32, secs: u64) -> u64 {
    u64::from(kbps) * 125 * secs
}

/// The file that goes up, and its type: the recording itself, or ffmpeg's encoding of it in a
/// temporary file that goes when this does.
pub struct Upload {
    path: PathBuf,
    pub mime: String,
    _encoded: Option<tempfile::TempPath>,
}

impl Upload {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The type a recording goes up as, encoded as `encoding` says.
pub fn mime(audio: &Audio, encoding: Encoding) -> &str {
    match (encoding, &audio.playable) {
        (Encoding::AsIs, Some(mime)) => mime,
        _ => "audio/mp4",
    }
}

/// The file to upload for `audio`: itself, or encoded by ffmpeg as `encoding` says (reported in
/// the write phase), and no bigger than `limit`.
pub fn prepare(audio: &Audio, encoding: Encoding, limit: u64, ctx: &Ctx<'_>) -> Result<Upload> {
    let mime = mime(audio, encoding).to_string();
    let Encoding::Aac { kbps, bytes: about } = encoding else {
        return Ok(Upload { path: audio.path.clone(), mime, _encoded: None });
    };
    let encoded = tempfile::Builder::new()
        .prefix("locallibrary-recording-")
        .suffix(".m4a")
        .tempfile()
        .map_err(|e| Error::invalid(format!("a temporary file for the encoded recording: {e}")))?
        .into_temp_path();
    ctx.phase_start(Phase::Write, 1, about);
    let name = audio.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    ctx.emit(Event::Item { phase: Phase::Write, path: name, bytes: about });
    let ran = ffmpeg(audio, kbps, &encoded, ctx);
    ctx.phase_end(Phase::Write);
    ran?;
    let size = std::fs::metadata(&encoded).at(&encoded)?.len();
    if size > limit {
        return Err(Error::invalid(format!(
            "ffmpeg's encoding of the recording came to {}, more than {}",
            bytes(size),
            bytes(limit)
        )));
    }
    let path = encoded.to_path_buf();
    Ok(Upload { path, mime, _encoded: Some(encoded) })
}

/// Encode `audio` with ffmpeg as AAC at `kbps` in an M4A file at `to`, its index at the front,
/// reporting what it has written as it goes (its `-progress` lines' `total_size`).
fn ffmpeg(audio: &Audio, kbps: u32, to: &Path, ctx: &Ctx<'_>) -> Result<()> {
    let mut child = Command::new("ffmpeg")
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-i"])
        .arg(&audio.path)
        .args(["-vn", "-c:a", "aac", "-b:a", &format!("{kbps}k"), "-movflags", "+faststart"])
        .args(["-progress", "pipe:1", "-nostats"])
        .arg(to)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound(format!(
                "ffmpeg, which encodes the recording ({}) as AAC for browsers to play and to fit the data server: \
                 install it (https://ffmpeg.org/download.html) and run this again",
                audio.format
            )),
            _ => Error::External { command: "ffmpeg".into(), message: e.to_string() },
        })?;
    let (stdout, mut stderr) = (child.stdout.take(), child.stderr.take());
    // Read on its own while the progress is read here, so neither pipe fills up.
    let said = std::thread::spawn(move || {
        let mut said = String::new();
        if let Some(e) = stderr.as_mut() {
            let _ = e.read_to_string(&mut said);
        }
        said
    });
    let mut written = 0;
    for line in stdout.map(BufReader::new).into_iter().flat_map(BufRead::lines) {
        if ctx.check().is_err() {
            let _ = child.kill();
            break;
        }
        let Ok(line) = line else { break };
        if let Some(total) = line.strip_prefix("total_size=").and_then(|n| n.trim().parse::<u64>().ok())
            && total > written
        {
            ctx.emit(Event::Bytes { phase: Phase::Write, bytes: total - written });
            written = total;
        }
    }
    let status = child.wait().map_err(|e| Error::External { command: "ffmpeg".into(), message: e.to_string() })?;
    let said = said.join().unwrap_or_default();
    ctx.check()?;
    if !status.success() {
        let why = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it gave no reason");
        return Err(Error::External {
            command: "ffmpeg".into(),
            message: one_line(&format!("encoding {} ({status}): {why}", audio.path.display())),
        });
    }
    Ok(())
}

/// The recording record for an uploaded file (`blob`, as the data server answered it) that plays
/// for `duration_secs`.
pub fn record(blob: Value, duration_secs: u64, now: DateTime<Utc>) -> Value {
    let mut r = json!({ "$type": lexicon::RECORDING, "audio": blob, "createdAt": crate::profile::timestamp(now) });
    if duration_secs >= 1 {
        r["durationSeconds"] = duration_secs.into();
    }
    r
}

#[cfg(test)]
mod tests;
