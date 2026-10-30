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
//!
//! The set's timed tracks go into the file as chapters, for podcast apps and players to show and
//! skip between: MP4 chapters in what ffmpeg encodes, and ID3 chapters (CHAP frames, listed in
//! order by a CTOC) in a copy of an MP3 that goes up as it is. An AAC or M4A file that goes up as it
//! is keeps no chapters: adding them would need ffmpeg, which it otherwise doesn't.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{DateTime, Utc};
use lofty::TextEncoding;
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::FileType;
use lofty::id3::v2::{
    ChapterFrame, ChapterTableOfContentsFrame, CtocFlags, Frame, FrameId, FrameList, Id3v2Tag, TextInformationFrame,
};
use lofty::mp4::{Mp4Codec, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::prelude::*;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use crate::atmosphere::lexicon;
use crate::error::IoContext;
use crate::export::one_line;
use crate::op::bytes;
use crate::progress::{Ctx, Event, Phase};
use crate::sets::chapters::{Chapter, ends};
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

/// The file that goes up, and its type: the recording itself, or ffmpeg's encoding of it or a copy
/// with chapters, in a temporary file that goes when this does.
pub struct Upload {
    path: PathBuf,
    pub mime: String,
    _made: Option<tempfile::TempPath>,
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

/// Whether `chapters` go into the file that goes up for `audio`, as `encoding` says: into what
/// ffmpeg encodes, or a copy of an MP3. Otherwise it goes up as it is, without them.
pub fn takes_chapters(audio: &Audio, encoding: Encoding) -> bool {
    matches!(encoding, Encoding::Aac { .. }) || audio.playable.as_deref() == Some("audio/mpeg")
}

/// The file to upload for `audio`: itself, encoded by ffmpeg as `encoding` says (reported in the
/// write phase), or for an MP3 with `chapters` a copy with them, and no bigger than `limit`.
/// `chapters` are for a file [`takes_chapters`] says takes them.
pub fn prepare(audio: &Audio, encoding: Encoding, limit: u64, chapters: &[Chapter], ctx: &Ctx<'_>) -> Result<Upload> {
    let upload = |path: PathBuf, made| Upload { path, mime: mime(audio, encoding).to_string(), _made: made };
    let as_it_is = || Ok(upload(audio.path.clone(), None));
    let (ffmpeg_kbps, about) = match encoding {
        Encoding::Aac { kbps, bytes } => (Some(kbps), bytes),
        Encoding::AsIs if chapters.is_empty() => return as_it_is(),
        Encoding::AsIs => (None, audio.bytes),
    };
    let made = temporary(if ffmpeg_kbps.is_some() { ".m4a" } else { ".mp3" })?;
    ctx.phase_start(Phase::Write, 1, about);
    let name = audio.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    ctx.emit(Event::Item { phase: Phase::Write, path: name, bytes: about });
    let ran = match ffmpeg_kbps {
        Some(kbps) => ffmpeg(audio, kbps, chapters, &made, ctx),
        None => tag_chapters(audio, chapters, &made)
            .inspect(|_| ctx.emit(Event::Bytes { phase: Phase::Write, bytes: about })),
    };
    ctx.phase_end(Phase::Write);
    ran?;
    let size = std::fs::metadata(&made).at(&made)?.len();
    match ffmpeg_kbps {
        Some(_) if size > limit => Err(Error::invalid(format!(
            "ffmpeg's encoding of the recording came to {}, more than {}",
            bytes(size),
            bytes(limit)
        ))),
        // Chapters tip it over: it goes up without them, as it would have.
        None if size > limit => {
            ctx.emit(Event::Warning {
                message: format!("the recording goes up without chapters: with them it's more than {}", bytes(limit)),
            });
            as_it_is()
        }
        _ => Ok(upload(made.to_path_buf(), Some(made))),
    }
}

/// A temporary file for what goes up, or what ffmpeg's told to put in it, named with `suffix`.
fn temporary(suffix: &str) -> Result<tempfile::TempPath> {
    let file = tempfile::Builder::new().prefix("locallibrary-recording-").suffix(suffix).tempfile();
    Ok(file.map_err(|e| Error::invalid(format!("a temporary file for the recording: {e}")))?.into_temp_path())
}

/// A copy at `to` of the MP3 `audio` with `chapters` in its ID3v2 tag, in place of any it had: a
/// CHAP frame each, titled, and a CTOC listing them in order. Its other tags stay.
fn tag_chapters(audio: &Audio, chapters: &[Chapter], to: &Path) -> Result<()> {
    let failed = |e: &dyn std::fmt::Display| {
        Error::invalid(one_line(&format!("adding chapters to a copy of {}: {e}", audio.path.display())))
    };
    std::fs::copy(&audio.path, to).at(to)?;
    let mut file = std::fs::File::open(to).at(to)?;
    let read = MpegFile::read_from(&mut file, ParseOptions::new().read_properties(false)).map_err(|e| failed(&e))?;
    let mut tag = read.id3v2().cloned().unwrap_or_else(Id3v2Tag::new);
    tag.retain(|f| !matches!(f, Frame::Chapter(_) | Frame::TableOfContents(_)));
    let title = FrameId::Valid("TIT2".into());
    let ms = |secs: u64| u32::try_from(secs.saturating_mul(1000)).unwrap_or(u32::MAX);
    let mut ids: Vec<Cow<'static, str>> = Vec::new();
    for (n, (c, end)) in (1..).zip(ends(chapters, audio.duration_secs)) {
        let id = format!("chp{n}");
        let mut named = FrameList::new();
        named.insert(Frame::Text(TextInformationFrame::new(title.clone(), TextEncoding::UTF8, c.title.clone())));
        // Times only: u32::MAX for both offsets says there are no byte offsets.
        let chapter = ChapterFrame::new(id.clone(), ms(c.start.into())..ms(end), u32::MAX..u32::MAX, named);
        tag.insert(Frame::Chapter(chapter));
        ids.push(id.into());
    }
    let order = CtocFlags { top_level: true, ordered: true };
    tag.insert(Frame::TableOfContents(ChapterTableOfContentsFrame::new("toc", order, ids, FrameList::new())));
    // No padding: nothing edits the copy after this.
    tag.save_to_path(to, WriteOptions::new().preferred_padding(0)).map_err(|e| failed(&e))
}

/// `chapters` of a recording `length` seconds long as an FFMETADATA file
/// (<https://ffmpeg.org/ffmpeg-formats.html#Metadata-2>), for ffmpeg to write into what it encodes:
/// times in milliseconds, each chapter ending where the next starts.
fn ffmetadata(chapters: &[Chapter], length: u64) -> String {
    let mut out = String::from(";FFMETADATA1\n");
    for (c, end) in ends(chapters, length) {
        let (start, title) = (u64::from(c.start) * 1000, escape(&c.title));
        let _ = write!(out, "[CHAPTER]\nTIMEBASE=1/1000\nSTART={start}\nEND={}\ntitle={title}\n", end * 1000);
    }
    out
}

/// `s` as an FFMETADATA value: a backslash before each `=`, `;`, `#`, `\` and newline.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '=' | ';' | '#' | '\\' | '\n') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Encode `audio` with ffmpeg as AAC at `kbps` in an M4A file at `to`, its index at the front and
/// `chapters` in it, reporting what it has written as it goes (its `-progress` lines' `total_size`).
fn ffmpeg(audio: &Audio, kbps: u32, chapters: &[Chapter], to: &Path, ctx: &Ctx<'_>) -> Result<()> {
    let metadata = match chapters {
        [] => None,
        _ => {
            let file = temporary(".ffmetadata")?;
            std::fs::write(&file, ffmetadata(chapters, audio.duration_secs)).at(&file)?;
            Some(file)
        }
    };
    let mut command = Command::new("ffmpeg");
    command.args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-i"]).arg(&audio.path);
    if let Some(m) = &metadata {
        command.args(["-f", "ffmetadata", "-i"]).arg(m);
    }
    command.args(["-vn", "-c:a", "aac", "-b:a", &format!("{kbps}k"), "-movflags", "+faststart"]);
    if metadata.is_some() {
        // The chapters come from the metadata file, and the recording's own tags from the
        // recording, as ffmpeg maps them by default.
        command.args(["-map_chapters", "1"]);
    }
    let mut child = command
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
