//! Making a copy: decode at the source's own rate, resample if the recipe's differs, round to
//! its depth, write the container, carry the tags over.

use std::fs::File;
use std::io::Read;
use std::ops::ControlFlow;
use std::path::Path;

use loclib_mixid::audio::{Damaged, decode_native, open};
use loclib_progress::{Ctx, Event, Phase};
use lofty::config::ParseOptions;
use lofty::file::TaggedFileExt;
use lofty::probe::Probe;
use lofty::tag::{Tag, TagType};

use crate::dither::Quantizer;
use crate::error::IoContext;
use crate::resample::Resampler;
use crate::write::{Spec, Writer};
use crate::{Container, Error, Recipe, Result};

/// Bytes of the source between progress events.
const PROGRESS_BYTES: u64 = 1 << 20;

/// Write a copy of `source` made by `recipe` to `out`, creating or replacing it: name it as a
/// temp file and rename it into place once this returns, so a cancelled or failed copy never
/// takes a track's place.
///
/// Progress comes as [`Event::Bytes`] adding up to the source's size, in [`Phase::Copy`], as a
/// plain copy reports it. Cancelling the `ctx` stops it with [`Error::Cancelled`].
///
/// The copy starts at the source's first sample and is as long as the source at the new rate,
/// to the nearest frame, so cue points and beat grids measured from the start still land. A
/// source under one frame at the new rate is refused with [`Error::TooShort`]. Tags
/// and artwork come along: ID3v2.3 in AIFF and WAV, Vorbis comments and pictures in FLAC. A
/// source whose tags can't be read is copied without them, with a warning through `ctx`.
pub fn transcode(source: &Path, recipe: &Recipe, out: &Path, ctx: &Ctx<'_>) -> Result<Transcoded> {
    if !matches!(recipe.bit_depth, 16 | 24) || recipe.sample_rate == 0 {
        return Err(Error::Invalid(format!("players take 16 or 24 bit at a known rate, not {recipe}")));
    }
    run(source, recipe, out, ctx)
}

/// What a copy holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transcoded {
    /// Frames (samples per channel) at the recipe's rate.
    pub frames: u64,
    pub channels: u16,
}

fn parse(path: &Path, message: &str) -> Error {
    Error::Decode(loclib_mixid::Error::Parse { path: path.to_path_buf(), message: message.into() })
}

fn run(source: &Path, recipe: &Recipe, out: &Path, ctx: &Ctx<'_>) -> Result<Transcoded> {
    // Readable too: lofty reads the FLAC header back to add the tags.
    let file = File::options().read(true).write(true).create(true).truncate(true).open(out);
    let mut file = Some(file.at(out)?);
    let mut tag = read_tag(source, recipe.container, ctx);
    let mut frames_left = aiff_frames(source).unwrap_or(u64::MAX);
    // Progress is the source's size in step with the audio decoded, when its header says how
    // much there is; the rest when it's done.
    let size = std::fs::metadata(source).map_or(0, |m| m.len());
    // (symphonia can panic on a malformed header; the decode below reports that file's error.)
    let header = std::panic::catch_unwind(|| open(source).ok().and_then(|(_, track)| track.codec_params.n_frames));
    let total = header.ok().flatten().filter(|n| *n > 0);
    let (mut decoded, mut reported, mut source_rate) = (0u64, 0u64, 0);
    let mut pipeline: Option<Pipeline> = None;
    // A damaged packet is an error rather than skipped: leaving it out would move everything after
    // it earlier, and the cues with it.
    decode_native(source, Damaged::Fail, ctx, |samples: &[i32], rate, channels| -> Result<_> {
        let frames = (samples.len() / channels).min(frames_left.try_into().unwrap_or(usize::MAX));
        frames_left -= frames as u64;
        let p = match &mut pipeline {
            Some(p) => p,
            None => {
                source_rate = rate;
                let file = file.take().expect("the pipeline is made once");
                pipeline.insert(Pipeline::new(file, out, recipe, rate, channels, tag.take())?)
            }
        };
        p.push(&samples[..frames * channels])?;
        decoded += frames as u64;
        if let Some(total) = total {
            let at = size * decoded.min(total) / total;
            if at - reported >= PROGRESS_BYTES {
                ctx.emit(Event::Bytes { phase: Phase::Copy, bytes: at - reported });
                reported = at;
            }
        }
        Ok(if frames_left == 0 { ControlFlow::Break(()) } else { ControlFlow::Continue(()) })
    })?;
    ctx.emit(Event::Bytes { phase: Phase::Copy, bytes: size - reported });
    let Some(pipeline) = pipeline else { return Err(parse(source, "it holds no audio")) };
    let channels = pipeline.channels;
    let frames = pipeline.finish()?;
    // No frame at the new rate is a copy of nothing: a FLAC of only its header, an AIFF whose
    // SSND symphonia reads as damaged.
    if frames == 0 {
        return Err(Error::TooShort {
            path: source.to_path_buf(),
            frames: decoded,
            source_rate,
            rate: recipe.sample_rate,
        });
    }
    Ok(Transcoded { frames, channels })
}

/// The frames an AIFF file's COMM chunk says it holds, when `path` is one. symphonia 0.5 takes
/// the SSND chunk's offset and block size fields for audio, so it decodes 8 bytes past the end
/// of the samples when a chunk follows them (as an ID3 tag usually does): noise at the end of
/// the copy, and a frame or more too many, unless it stops here.
pub(crate) fn aiff_frames(path: &Path) -> Option<u64> {
    let mut file = std::io::BufReader::new(File::open(path).ok()?);
    let mut header = [0; 12];
    file.read_exact(&mut header).ok()?;
    if &header[..4] != b"FORM" || !matches!(&header[8..], b"AIFF" | b"AIFC") {
        return None;
    }
    loop {
        let mut chunk = [0; 8];
        file.read_exact(&mut chunk).ok()?;
        let len = u32::from_be_bytes(chunk[4..].try_into().expect("4 bytes"));
        if &chunk[..4] == b"COMM" {
            let mut comm = [0; 6];
            file.read_exact(&mut comm).ok()?;
            return Some(u32::from_be_bytes(comm[2..].try_into().expect("4 bytes")).into());
        }
        // Chunks start on even offsets.
        file.seek_relative(i64::from(len + len % 2)).ok()?;
    }
}

/// The source's tags, in the form `container` holds them. `None` when it has none or they can't
/// be read (with a warning: the copy still plays).
fn read_tag(source: &Path, container: Container, ctx: &Ctx<'_>) -> Option<Tag> {
    let tag_type = match container {
        Container::Flac => TagType::VorbisComments,
        Container::Aiff | Container::Wav => TagType::Id3v2,
    };
    let read = Probe::open(source).and_then(|p| p.options(ParseOptions::new().read_properties(false)).read());
    let tagged = match read {
        Ok(tagged) => tagged,
        Err(e) => {
            ctx.warn(format!("{}: its tags weren't copied: {e}", source.display()));
            return None;
        }
    };
    let mut tag = tagged.primary_tag().or_else(|| tagged.first_tag())?.clone();
    // Re-mapping drops what only the source's own tag format holds (ID3v2's binary frames), so
    // a tag already in the right format is kept whole.
    if tag.tag_type() != tag_type {
        tag.re_map(tag_type);
    }
    Some(tag)
}

/// Samples on their way from the decoder to the file.
struct Pipeline {
    channels: u16,
    resampler: Option<Resampler>,
    quantizer: Quantizer,
    writer: Writer,
    /// Scratch space, kept between packets.
    floats: Vec<f64>,
    ints: Vec<i32>,
}

impl Pipeline {
    fn new(file: File, out: &Path, recipe: &Recipe, rate: u32, channels: usize, tag: Option<Tag>) -> Result<Pipeline> {
        let spec = Spec {
            container: recipe.container,
            sample_rate: recipe.sample_rate,
            channels: channels as u16,
            bit_depth: recipe.bit_depth,
        };
        Ok(Pipeline {
            channels: spec.channels,
            resampler: (rate != recipe.sample_rate).then(|| Resampler::new(rate, recipe.sample_rate, channels)),
            quantizer: Quantizer::new(recipe.bit_depth, recipe.dither),
            writer: Writer::new(file, out, spec, tag)?,
            floats: Vec::new(),
            ints: Vec::new(),
        })
    }

    /// Take interleaved samples at full 32-bit scale. Through f64 they stay exact, so a copy
    /// that changes nothing but the container is bit for bit the source.
    fn push(&mut self, samples: &[i32]) -> Result<()> {
        self.floats.clear();
        self.floats.extend(samples.iter().map(|&s| f64::from(s) / 2_147_483_648.0));
        let Pipeline { resampler, quantizer, writer, floats, ints, .. } = self;
        let mut sink = |x: &[f64]| write(quantizer, writer, ints, x);
        match resampler {
            Some(r) => r.push(floats, &mut sink),
            None => sink(floats),
        }
    }

    fn finish(self) -> Result<u64> {
        let Pipeline { resampler, mut quantizer, mut writer, mut ints, .. } = self;
        if let Some(r) = resampler {
            r.finish(&mut |x: &[f64]| write(&mut quantizer, &mut writer, &mut ints, x))?;
        }
        writer.finish()
    }
}

fn write(quantizer: &mut Quantizer, writer: &mut Writer, ints: &mut Vec<i32>, samples: &[f64]) -> Result<()> {
    ints.clear();
    ints.extend(samples.iter().map(|&s| quantizer.quantize(s)));
    writer.write(ints)
}

#[cfg(test)]
mod tests;
