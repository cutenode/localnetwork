//! Set recordings made on the player.
//!
//! All-in-one units record the master output to the drive (MASTER REC): 44.1 kHz, 16-bit WAV
//! files, about 635 MB an hour, split every 3 hours, in an `ALPHATHETA REC` (XDJ-AZ) or
//! `PIONEER REC` (OPUS-QUAD) folder at the drive's root. They live outside the library, so sync
//! and pull never see them. [`list`] finds them, [`collect`] copies them to the computer
//! (verifying every byte), writes a tracklist from a history list next to them, and can delete
//! them from the drive afterwards.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::device::Device;
use crate::error::IoContext;
use crate::error::Reason;
use crate::fsutil;
use crate::hash;
use crate::local_time;
use crate::model::Library;
use crate::progress::{Ctx, Event, Phase};
use crate::schema::Failure;
use crate::space::is_recordings_dir;
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Recording {
    /// Relative to the drive root.
    pub path: String,
    pub size: u64,
    pub format: Option<WavFormat>,
    /// Length of the audio, from the WAV header (or the file size when recording was cut off
    /// before the header was finalised).
    pub duration_secs: Option<f64>,
    /// When the file was last written: the end of the recording.
    pub modified: DateTime<Utc>,
    pub started: Option<DateTime<Utc>>,
}

impl Recording {
    /// When it started (or, if its length is unknown, ended), in this computer's time zone.
    pub fn local_time(&self, format: &str) -> String {
        local_time(self.started.unwrap_or(self.modified), format).to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WavFormat {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits: u16,
}

impl WavFormat {
    fn bytes_per_second(&self) -> u64 {
        u64::from(self.channels) * u64::from(self.sample_rate) * u64::from(self.bits.div_ceil(8))
    }
}

/// Read a WAV header: the format and the length of the `data` chunk.
pub fn read_wav_header(mut r: impl Read + Seek, file_len: u64) -> Option<(WavFormat, u64)> {
    let mut riff = [0u8; 12];
    r.read_exact(&mut riff).ok()?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return None;
    }
    let mut format = None;
    let mut pos = 12u64;
    // Chunks; give up after a few dozen (a header never has that many).
    for _ in 0..64 {
        let mut head = [0u8; 8];
        r.read_exact(&mut head).ok()?;
        let len = u64::from(u32::from_le_bytes(head[4..8].try_into().unwrap()));
        pos += 8;
        match &head[0..4] {
            b"fmt " => {
                let mut fmt = [0u8; 16];
                r.read_exact(&mut fmt).ok()?;
                format = Some(WavFormat {
                    channels: u16::from_le_bytes([fmt[2], fmt[3]]),
                    sample_rate: u32::from_le_bytes(fmt[4..8].try_into().unwrap()),
                    bits: u16::from_le_bytes([fmt[14], fmt[15]]),
                });
                r.seek(SeekFrom::Current(len as i64 - 16 + (len & 1) as i64)).ok()?;
            }
            b"data" => {
                let available = file_len.saturating_sub(pos);
                // 0 or 0xFFFFFFFF: the recorder never went back to fill the size in.
                let data = if len == 0 || len == u64::from(u32::MAX) || len > available { available } else { len };
                return Some((format?, data));
            }
            _ => {
                r.seek(SeekFrom::Current(len as i64 + (len & 1) as i64)).ok()?;
            }
        }
        pos += len + (len & 1);
    }
    None
}

fn recording(root: &Path, path: &Path) -> Result<Recording> {
    let meta = std::fs::metadata(path).at(path)?;
    let modified: DateTime<Utc> = meta.modified().at(path)?.into();
    let header = std::fs::File::open(path).ok().and_then(|f| read_wav_header(std::io::BufReader::new(f), meta.len()));
    let (format, duration_secs) = match header {
        Some((f, data)) if f.bytes_per_second() > 0 => (Some(f), Some(data as f64 / f.bytes_per_second() as f64)),
        Some((f, _)) => (Some(f), None),
        None => (None, None),
    };
    Ok(Recording {
        path: path.strip_prefix(root).expect("below root").to_string_lossy().replace('\\', "/"),
        size: meta.len(),
        format,
        duration_secs,
        modified,
        started: duration_secs.map(|d| modified - Duration::milliseconds((d * 1000.0) as i64)),
    })
}

/// Every WAV file in the drive's recording folders, oldest first.
pub fn list(device: &Device) -> Result<Vec<Recording>> {
    let root = device.root();
    let mut out = Vec::new();
    let Ok(top) = std::fs::read_dir(root) else { return Ok(out) };
    for dir in top.flatten() {
        let name = dir.file_name().to_string_lossy().to_string();
        if !dir.file_type().is_ok_and(|t| t.is_dir()) || !is_recordings_dir(&name) {
            continue;
        }
        for e in WalkDir::new(dir.path()).follow_links(false).into_iter().flatten() {
            let n = e.file_name().to_string_lossy();
            if e.file_type().is_file()
                && !n.starts_with("._")
                && n.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("wav"))
            {
                out.push(recording(root, e.path())?);
            }
        }
    }
    out.sort_by(|a, b| (a.modified, &a.path).cmp(&(b.modified, &b.path)));
    Ok(out)
}

/// Which history list becomes a recording's tracklist.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Tracklist {
    #[default]
    None,
    /// The newest history list, for the newest recording.
    Latest,
    /// This history list (by name), for every recording copied.
    Named(String),
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CollectOptions {
    /// Folder to copy into; a subfolder named after the drive is created.
    pub to: PathBuf,
    /// Only these recordings (paths as [`list`] gives them) [default: all].
    #[serde(default)]
    pub only: Vec<String>,
    #[serde(default)]
    pub tracklist: Tracklist,
    /// Delete each recording from the drive once its copy is verified.
    #[serde(default)]
    pub delete: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Collected {
    pub recording: String,
    pub copied_to: PathBuf,
    pub tracklist: Option<PathBuf>,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CollectReport {
    pub collected: Vec<Collected>,
    pub bytes: u64,
    pub failed: Vec<Failure>,
}

/// "01. Artist - Title" lines for a history list.
pub fn tracklist_text(lib: &Library, history: &crate::model::Playlist) -> String {
    let mut out = format!("{}\n\n", history.name);
    for (i, id) in history.track_ids.iter().enumerate() {
        let name = lib.track(*id).map(|t| t.display_name()).unwrap_or_else(|| format!("(track {id})"));
        out.push_str(&format!("{:02}. {name}\n", i + 1));
    }
    out
}

/// Copy recordings off the drive (with `delete`, then delete those whose copy was verified, through
/// the drive's write routine; nothing is backed up, as the copies are).
pub fn collect(device: &mut Device, opts: &CollectOptions, ctx: &Ctx<'_>) -> Result<CollectReport> {
    let all = list(device)?;
    let chosen: Vec<&Recording> = if opts.only.is_empty() {
        all.iter().collect()
    } else {
        let mut v = Vec::new();
        for want in &opts.only {
            v.push(
                all.iter()
                    .find(|r| r.path.eq_ignore_ascii_case(want.trim_start_matches('/')))
                    .ok_or_else(|| Error::NotFound(format!("recording {want:?}")))?,
            );
        }
        v
    };
    let library = device.library().ok();
    let history = |name: &str| {
        let l = library.as_ref()?;
        crate::model::pick(l.history.iter().filter(|h| !h.is_folder).map(|h| (&h.name, h)), name).map(|h| (l, h))
    };
    let named = match &opts.tracklist {
        Tracklist::Named(n) => Some(history(n).ok_or_else(|| Error::NotFound(format!("history list {n:?}")))?),
        _ => None,
    };
    let latest = match opts.tracklist {
        Tracklist::Latest => library.as_ref().and_then(|l| l.history.iter().max_by_key(|h| h.id).map(|h| (l, h))),
        _ => None,
    };
    let newest = chosen.iter().max_by_key(|r| r.modified).map(|r| r.path.clone());

    let dest = opts.to.join(crate::paths::sanitize_component(&device.name()));
    std::fs::create_dir_all(&dest).at(&dest)?;
    let mut report = CollectReport { collected: Vec::new(), bytes: 0, failed: Vec::new() };
    ctx.phase_start(Phase::Copy, chosen.len() as u64, chosen.iter().map(|r| r.size).sum());
    for r in chosen {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Copy, path: r.path.clone(), bytes: r.size });
        let src = device.root().join(&r.path);
        let file_name = Path::new(&r.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let stamp = r.local_time("%Y-%m-%d %H%M");
        let target = dest.join(format!("{stamp} {file_name}"));
        let result = (|| -> Result<Collected> {
            if target.exists() {
                return Err(Error::conflict(Reason::Exists, format!("{} already exists", target.display()))
                    .with("path", &target));
            }
            let mtime = fsutil::mtime_secs(&std::fs::metadata(&src).at(&src)?);
            let h = fsutil::copy_file(&src, &target, Some(mtime), None, true, ctx)?;
            // Read the copy back before trusting it.
            if hash::hash_file(&target, &Ctx::silent(), Phase::Verify)? != h {
                let _ = std::fs::remove_file(&target);
                return Err(Error::Corrupt {
                    path: target.clone(),
                    expected: h.to_hex(),
                    actual: "a different copy".into(),
                });
            }
            let pair = named.or_else(|| latest.filter(|_| newest.as_deref() == Some(r.path.as_str())));
            let tracklist = match pair {
                Some((lib, h)) => {
                    let t = target.with_extension("txt");
                    fsutil::write_atomic(&t, tracklist_text(lib, h).as_bytes())?;
                    Some(t)
                }
                None => None,
            };
            Ok(Collected { recording: r.path.clone(), copied_to: target.clone(), tracklist, deleted: false })
        })();
        match result {
            Ok(c) => {
                report.bytes += r.size;
                report.collected.push(c);
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => report.failed.push(Failure::new(r.path.clone(), e)),
        }
    }
    ctx.phase_end(Phase::Copy);
    if opts.delete && !report.collected.is_empty() {
        let _lock = crate::commit::lock(device, None)?;
        let deletes = report.collected.iter().map(|c| crate::paths::key(&c.recording)).collect();
        let changes = crate::commit::Changes { deletes, ..Default::default() };
        let mut o = crate::commit::Options::new(crate::device::LineageOp::Edit, "delete collected recordings");
        o.lineage = false;
        crate::commit::commit(device, changes, o, ctx)?;
        for c in &mut report.collected {
            c.deleted = true;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests;
