//! Audacity projects: `.aup3`, and Audacity 4's `.aup4` (the same container).
//!
//! A project is a SQLite database. Table `project` holds the document as Audacity's binary XML
//! (a `dict` of element and attribute names, and a `doc` of tokens referring to them), and
//! `sampleblocks` holds the audio, up to a few hundred thousand samples per row. Each wave track
//! is one channel, made of clips placed on the timeline; a clip plays a trimmed window of its
//! sequence of blocks.
//!
//! [`Project::render`] mixes every audible channel to mono, which is all fingerprinting needs.
//! Gain and mute/solo are honoured; pan, envelopes, effects and pitch shifts are not.

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use super::audio::{Resampler, SourceFormat};
use crate::progress::Ctx;
use crate::{Error, Result};

/// A label from a label track (DJs sometimes mark where tracks change).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct Label {
    pub start: f64,
    pub end: f64,
    pub title: String,
}

#[derive(Debug)]
pub struct Project {
    path: PathBuf,
    pub rate: f64,
    /// Metadata tags (TITLE, ARTIST, GENRE...), in project order.
    pub tags: Vec<(String, String)>,
    pub labels: Vec<Label>,
    channels: Vec<Channel>,
}

#[derive(Debug)]
struct Channel {
    gain: f32,
    clips: Vec<Clip>,
}

#[derive(Debug)]
struct Clip {
    /// Timeline position (seconds) of the first audible sample.
    play_start: f64,
    /// First audible sample of the sequence.
    first: u64,
    /// Audible samples.
    len: u64,
    /// Playback stretch (Audacity 3.4+ clip speed): > 1 plays slower.
    stretch: f64,
    blocks: Vec<Block>,
}

#[derive(Debug, Clone, Copy)]
struct Block {
    start: u64,
    /// Row in `sampleblocks`; negative ids are silence of that many samples.
    id: i64,
}

// Token types of Audacity's ProjectSerializer.
const FT_CHAR_SIZE: u8 = 0;
const FT_START_TAG: u8 = 1;
const FT_END_TAG: u8 = 2;
const FT_STRING: u8 = 3;
const FT_INT: u8 = 4;
const FT_BOOL: u8 = 5;
const FT_LONG: u8 = 6;
const FT_LONG_LONG: u8 = 7;
const FT_SIZE_T: u8 = 8;
const FT_FLOAT: u8 = 9;
const FT_DOUBLE: u8 = 10;
const FT_DATA: u8 = 11;
const FT_RAW: u8 = 12;
const FT_PUSH: u8 = 13;
const FT_POP: u8 = 14;
const FT_NAME: u8 = 15;

/// How deep elements nest at most. Audacity's go about seven deep; a file nesting more is
/// damaged, and each level holds memory (and stack, to drop the tree).
const MAX_DEPTH: usize = 64;

/// Project sample rates read as such: the resampler steps by whole hertz, so one rounding to 0
/// would never advance. Audacity offers up to 384 kHz.
const RATES: std::ops::RangeInclusive<f64> = 1.0..=1_000_000.0;

// Sample formats.
const INT16: i64 = 0x0002_0001;
const INT24: i64 = 0x0004_0001;
const FLOAT: i64 = 0x0004_000F;

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl Value {
    fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            Value::Str(s) => s.parse().ok(),
            Value::Bool(b) => Some(f64::from(u8::from(*b))),
        }
    }

    fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            Value::Str(s) => s.parse().ok(),
            Value::Bool(b) => Some(i64::from(*b)),
        }
    }

    fn as_str(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Bool(b) => b.to_string(),
        }
    }
}

#[derive(Debug, Default)]
struct Element {
    name: String,
    attrs: Vec<(String, Value)>,
    children: Vec<Element>,
}

impl Element {
    fn attr(&self, name: &str) -> Option<&Value> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    fn f64(&self, name: &str) -> Option<f64> {
        self.attr(name).and_then(Value::as_f64)
    }

    fn bool(&self, name: &str) -> bool {
        self.attr(name).and_then(Value::as_i64).is_some_and(|v| v != 0)
    }

    fn str(&self, name: &str) -> String {
        self.attr(name).map(Value::as_str).unwrap_or_default()
    }

    fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
}

/// Reads Audacity's binary XML. Names are shared between the dictionary and the document, as is
/// the character size of strings (1, 2 or 4 bytes).
struct Reader {
    names: FxHashMap<u16, String>,
    char_size: u8,
}

struct Cursor<'a> {
    buf: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn bytes(&mut self, n: usize) -> std::result::Result<&'a [u8], String> {
        let s = self.buf.get(self.at..self.at + n).ok_or_else(|| format!("truncated at byte {}", self.at))?;
        self.at += n;
        Ok(s)
    }

    fn array<const N: usize>(&mut self) -> std::result::Result<[u8; N], String> {
        Ok(self.bytes(N)?.try_into().expect("N bytes"))
    }

    fn u8(&mut self) -> std::result::Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> std::result::Result<u16, String> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    fn i32(&mut self) -> std::result::Result<i32, String> {
        Ok(i32::from_le_bytes(self.array()?))
    }
}

impl Reader {
    fn read(&mut self, buf: &[u8], stack: &mut Vec<Element>) -> std::result::Result<(), String> {
        let mut c = Cursor { buf, at: 0 };
        while c.at < buf.len() {
            let op = c.u8()?;
            match op {
                FT_CHAR_SIZE => self.char_size = c.u8()?,
                FT_NAME => {
                    let id = c.u16()?;
                    let len = c.u16()? as usize;
                    let name = self.text(c.bytes(len)?);
                    self.names.insert(id, name);
                }
                FT_START_TAG => {
                    let id = c.u16()?;
                    if stack.len() > MAX_DEPTH {
                        return Err(format!("elements nested more than {MAX_DEPTH} deep"));
                    }
                    stack.push(Element { name: self.name(id), ..Default::default() });
                }
                FT_END_TAG => {
                    c.u16()?;
                    if stack.len() > 1 {
                        let done = stack.pop().expect("len > 1");
                        stack.last_mut().expect("len > 1").children.push(done);
                    }
                }
                FT_STRING => {
                    let id = c.u16()?;
                    let len = c.i32()?.max(0) as usize;
                    let v = Value::Str(self.text(c.bytes(len)?));
                    self.attr(stack, id, v);
                }
                FT_INT | FT_LONG => {
                    let id = c.u16()?;
                    let v = c.i32()? as i64;
                    self.attr(stack, id, Value::Int(v));
                }
                FT_SIZE_T => {
                    let id = c.u16()?;
                    let v = u32::from_le_bytes(c.array()?) as i64;
                    self.attr(stack, id, Value::Int(v));
                }
                FT_BOOL => {
                    let id = c.u16()?;
                    let v = c.u8()? != 0;
                    self.attr(stack, id, Value::Bool(v));
                }
                FT_LONG_LONG => {
                    let id = c.u16()?;
                    let v = i64::from_le_bytes(c.array()?);
                    self.attr(stack, id, Value::Int(v));
                }
                FT_FLOAT => {
                    let id = c.u16()?;
                    let v = f32::from_le_bytes(c.array()?) as f64;
                    c.i32()?; // digits
                    self.attr(stack, id, Value::Float(v));
                }
                FT_DOUBLE => {
                    let id = c.u16()?;
                    let v = f64::from_le_bytes(c.array()?);
                    c.i32()?; // digits
                    self.attr(stack, id, Value::Float(v));
                }
                FT_DATA | FT_RAW => {
                    let len = c.i32()?.max(0) as usize;
                    c.bytes(len)?;
                }
                FT_PUSH | FT_POP => {}
                other => return Err(format!("unknown token {other} at byte {}", c.at - 1)),
            }
        }
        Ok(())
    }

    fn name(&self, id: u16) -> String {
        self.names.get(&id).cloned().unwrap_or_else(|| format!("#{id}"))
    }

    fn attr(&self, stack: &mut [Element], id: u16, v: Value) {
        if let Some(e) = stack.last_mut() {
            e.attrs.push((self.name(id), v));
        }
    }

    fn text(&self, raw: &[u8]) -> String {
        match self.char_size {
            4 => raw
                .chunks_exact(4)
                .filter_map(|c| char::from_u32(u32::from_le_bytes([c[0], c[1], c[2], c[3]])))
                .collect(),
            2 => String::from_utf16_lossy(
                &raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>(),
            ),
            _ => String::from_utf8_lossy(raw).into_owned(),
        }
    }
}

fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| Error::parse(path, e))?;
    // A project is whatever file was named.
    match crate::onelibrary::distrust(&conn, &["view"], "Audacity").map_err(|e| Error::parse(path, e))? {
        Some(why) => Err(Error::parse(path, why)),
        None => Ok(conn),
    }
}

/// Whether `path` looks like an Audacity project.
pub fn is_project(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "aup3" | "aup4")
}

impl Project {
    pub fn open(path: &Path) -> Result<Project> {
        let db = open(path)?;
        let row = |table: &str| {
            db.query_row(&format!("SELECT dict, doc FROM {table} WHERE id = 1"), [], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
            })
            .optional()
        };
        // A project never saved has only its autosave.
        let (dict, doc) = match row("project").map_err(|e| Error::parse(path, e))? {
            Some(r) => r,
            None => row("autosave")
                .map_err(|e| Error::parse(path, e))?
                .ok_or_else(|| Error::parse(path, "the project is empty"))?,
        };
        let mut reader = Reader { names: FxHashMap::default(), char_size: 1 };
        let mut stack = vec![Element::default()];
        reader.read(&dict, &mut stack).map_err(|e| Error::parse(path, format!("project dictionary: {e}")))?;
        reader.read(&doc, &mut stack).map_err(|e| Error::parse(path, format!("project document: {e}")))?;
        // Close anything left open (a truncated document) and take the root.
        while stack.len() > 1 {
            let done = stack.pop().expect("len > 1");
            stack.last_mut().expect("len > 1").children.push(done);
        }
        let root = stack.pop().expect("root");
        let project = root.children("project").next().ok_or_else(|| Error::parse(path, "no <project> element"))?;
        Self::from_element(path, project)
    }

    fn from_element(path: &Path, project: &Element) -> Result<Project> {
        let rate = project.f64("rate").filter(|r| *r > 0.0).unwrap_or(44_100.0);
        if !RATES.contains(&rate) {
            return Err(Error::parse(path, format!("a sample rate of {rate} Hz")));
        }
        let tags = project
            .children("tags")
            .flat_map(|t| t.children("tag"))
            .map(|t| (t.str("name"), t.str("value")))
            .filter(|(n, v)| !n.is_empty() && !v.is_empty())
            .collect();
        let labels = project
            .children("labeltrack")
            .flat_map(|t| t.children("label"))
            .map(|l| {
                let start = l.f64("t").unwrap_or(0.0);
                Label { start, end: l.f64("t1").unwrap_or(start), title: l.str("title") }
            })
            .collect();

        let tracks: Vec<&Element> = project.children("wavetrack").collect();
        let any_solo = tracks.iter().any(|t| t.bool("solo"));
        let mut channels = Vec::new();
        for t in tracks {
            if t.bool("mute") || (any_solo && !t.bool("solo")) {
                continue;
            }
            let track_rate = t.f64("rate").filter(|r| *r > 0.0).unwrap_or(rate);
            let mut clips = Vec::new();
            for c in t.children("waveclip") {
                clips.extend(clip(c, track_rate));
                // Audacity 3.4+ nests a stereo clip's second channel inside the first.
                for inner in c.children("waveclip") {
                    clips.extend(clip(inner, track_rate));
                }
            }
            channels.push(Channel { gain: t.f64("gain").unwrap_or(1.0) as f32, clips });
        }
        if channels.iter().all(|c| c.clips.is_empty()) {
            return Err(Error::parse(path, "the project has no audible audio"));
        }
        Ok(Project { path: path.to_path_buf(), rate, tags, labels, channels })
    }

    pub fn tag(&self, name: &str) -> Option<&str> {
        self.tags.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// Seconds from the start of the timeline to the end of the last clip.
    pub fn duration_secs(&self) -> f64 {
        self.channels
            .iter()
            .flat_map(|c| &c.clips)
            .map(|c| c.play_start + c.len as f64 * c.stretch / self.rate)
            .fold(0.0, f64::max)
    }

    /// Mix the audible channels to mono at `rate` and hand the result to `sink` in chunks.
    pub fn render(&self, rate: u32, ctx: &Ctx<'_>, sink: &mut dyn FnMut(&[f32])) -> Result<SourceFormat> {
        let db = open(&self.path)?;
        let mut stmt = db
            .prepare("SELECT sampleformat, samples FROM sampleblocks WHERE blockid = ?1")
            .map_err(|e| Error::parse(&self.path, e))?;
        let mut cache: FxHashMap<i64, Vec<f32>> = FxHashMap::default();
        let mut load = |id: i64| -> Result<Vec<f32>> {
            let (format, bytes): (i64, Vec<u8>) = stmt
                .query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| Error::parse(&self.path, format!("sample block {id}: {e}")))?;
            Ok(match format {
                INT16 => bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32_768.0).collect(),
                INT24 => bytes
                    .chunks_exact(4)
                    .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 8_388_608.0)
                    .collect(),
                FLOAT => bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect(),
                other => return Err(Error::parse(&self.path, format!("unknown sample format {other:#x}"))),
            })
        };

        let total = (self.duration_secs() * self.rate).ceil() as u64;
        let audible = self.channels.iter().filter(|c| !c.clips.is_empty()).count().max(1) as f32;
        let mut resampler = Resampler::new(self.rate.round() as u32, rate);
        const CHUNK: u64 = 1 << 16;
        let mut mix = vec![0.0f32; CHUNK as usize];
        let mut pos = 0u64;
        while pos < total {
            ctx.check()?;
            let n = CHUNK.min(total - pos) as usize;
            mix[..n].fill(0.0);
            for ch in &self.channels {
                let gain = ch.gain / audible;
                for clip in &ch.clips {
                    let start = (clip.play_start * self.rate).round() as i64;
                    let end = start + (clip.len as f64 * clip.stretch).round() as i64;
                    let lo = start.max(pos as i64);
                    let hi = end.min((pos + n as u64) as i64);
                    for t in lo..hi {
                        let src = clip.first + ((t - start) as f64 / clip.stretch) as u64;
                        let Some(b) = clip.block_at(src) else { continue };
                        if b.id <= 0 {
                            continue; // silence
                        }
                        if !cache.contains_key(&b.id) {
                            // Clips read their blocks in order: one cached block per channel is plenty.
                            if cache.len() > 2 * self.channels.len() + 2 {
                                cache.clear();
                            }
                            let samples = load(b.id)?;
                            cache.insert(b.id, samples);
                        }
                        let s = cache[&b.id].get((src - b.start) as usize).copied().unwrap_or(0.0);
                        mix[(t - pos as i64) as usize] += s * gain;
                    }
                }
            }
            resampler.push(&mix[..n], sink);
            pos += n as u64;
        }
        resampler.finish(sink);
        Ok(SourceFormat {
            sample_rate: self.rate.round() as u32,
            channels: self.channels.len() as u16,
            duration_secs: total as f64 / self.rate,
        })
    }
}

fn clip(c: &Element, rate: f64) -> Option<Clip> {
    let seq = c.children("sequence").next()?;
    let samples = seq.attr("numsamples").and_then(Value::as_i64)?.max(0) as u64;
    let trim_left = c.f64("trimLeft").unwrap_or(0.0).max(0.0);
    let trim_right = c.f64("trimRight").unwrap_or(0.0).max(0.0);
    let first = ((trim_left * rate).round() as u64).min(samples);
    let last = samples.saturating_sub((trim_right * rate).round() as u64).max(first);
    let stretch = c.f64("clipStretchRatio").filter(|r| *r > 0.0).unwrap_or(1.0);
    let blocks = seq
        .children("waveblock")
        .filter_map(|b| {
            Some(Block { start: b.attr("start")?.as_i64()?.max(0) as u64, id: b.attr("blockid")?.as_i64()? })
        })
        .collect();
    Some(Clip {
        // `offset` is where the untrimmed sequence would start.
        play_start: c.f64("offset").unwrap_or(0.0) + trim_left * stretch,
        first,
        len: last - first,
        stretch,
        blocks,
    })
}

impl Clip {
    fn block_at(&self, sample: u64) -> Option<Block> {
        let i = self.blocks.partition_point(|b| b.start <= sample);
        i.checked_sub(1).map(|i| self.blocks[i])
    }
}

#[cfg(test)]
pub(crate) mod tests;
