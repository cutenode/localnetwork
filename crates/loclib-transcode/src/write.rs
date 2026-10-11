//! Writing samples as AIFF, WAV or FLAC, with the source's tags.
//!
//! Samples stream to the file; sizes are patched into the headers at the end. WAV and AIFF get
//! their ID3v2 tag in a chunk after the audio, written here from lofty's encoding, so no file is
//! ever read back whole. WAV uses the plain PCM format code even at 24 bit, the form every
//! player reads, rather than WAVE_FORMAT_EXTENSIBLE. FLAC gets its tags from lofty while the
//! file is still only its header, and its frames follow.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use flacenc::bitsink::{BitSink, ByteSink};
use flacenc::component::{BitRepr, StreamInfo};
use flacenc::config;
use flacenc::error::Verify;
use flacenc::source::{Fill, FrameBuf};
use lofty::config::WriteOptions;
use lofty::tag::{Tag, TagExt};

use crate::error::{Context as _, IoContext};
use crate::{Container, Error, Result};

/// FLAC's usual block size, in frames.
const BLOCK: usize = 4096;

/// What every copy needs to know to be written.
pub(crate) struct Spec {
    pub container: Container,
    pub sample_rate: u32,
    pub channels: u16,
    pub bit_depth: u16,
}

pub(crate) enum Writer {
    Pcm(Pcm),
    Flac(Box<Flac>),
}

impl Writer {
    pub(crate) fn new(file: File, path: &Path, spec: Spec, tag: Option<Tag>) -> Result<Writer> {
        Ok(match spec.container {
            Container::Aiff | Container::Wav => Writer::Pcm(Pcm::new(file, path, spec, tag)?),
            Container::Flac => Writer::Flac(Box::new(Flac::new(file, path, spec, tag)?)),
        })
    }

    /// Write interleaved samples, already in the bit depth's range.
    pub(crate) fn write(&mut self, samples: &[i32]) -> Result<()> {
        match self {
            Writer::Pcm(w) => w.write(samples),
            Writer::Flac(w) => w.write(samples),
        }
    }

    /// Finish the file. Returns the frames written.
    pub(crate) fn finish(self) -> Result<u64> {
        match self {
            Writer::Pcm(w) => w.finish(),
            Writer::Flac(w) => w.finish(),
        }
    }
}

/// ID3v2.3, which more players and tools read than 2.4.
fn tag_options() -> WriteOptions {
    WriteOptions::new().use_id3v23(true)
}

pub(crate) struct Pcm {
    file: BufWriter<File>,
    path: PathBuf,
    spec: Spec,
    tag: Option<Tag>,
    bytes: Vec<u8>,
    data_len: u64,
}

impl Pcm {
    fn new(file: File, path: &Path, spec: Spec, tag: Option<Tag>) -> Result<Pcm> {
        let mut pcm =
            Pcm { file: BufWriter::new(file), path: path.to_path_buf(), spec, tag, bytes: Vec::new(), data_len: 0 };
        // Sizes are 0 until `finish`.
        let header = pcm.header(0, 0);
        pcm.file.write_all(&header).at(path)?;
        Ok(pcm)
    }

    /// Everything before the samples, for `frames` frames in a file of `total` bytes.
    fn header(&self, frames: u32, total: u32) -> Vec<u8> {
        let Spec { container, sample_rate, channels, bit_depth } = self.spec;
        let data_len = frames * u32::from(channels) * u32::from(bit_depth / 8);
        let mut h = Vec::with_capacity(54);
        match container {
            Container::Aiff => {
                h.extend_from_slice(b"FORM");
                h.extend_from_slice(&total.saturating_sub(8).to_be_bytes());
                h.extend_from_slice(b"AIFFCOMM");
                h.extend_from_slice(&18u32.to_be_bytes());
                h.extend_from_slice(&channels.to_be_bytes());
                h.extend_from_slice(&frames.to_be_bytes());
                h.extend_from_slice(&bit_depth.to_be_bytes());
                h.extend_from_slice(&extended(sample_rate));
                h.extend_from_slice(b"SSND");
                h.extend_from_slice(&(data_len + 8).to_be_bytes());
                // Offset and block size, both 0.
                h.extend_from_slice(&[0; 8]);
            }
            _ => {
                let block_align = channels * (bit_depth / 8);
                h.extend_from_slice(b"RIFF");
                h.extend_from_slice(&total.saturating_sub(8).to_le_bytes());
                h.extend_from_slice(b"WAVEfmt ");
                h.extend_from_slice(&16u32.to_le_bytes());
                h.extend_from_slice(&1u16.to_le_bytes());
                h.extend_from_slice(&channels.to_le_bytes());
                h.extend_from_slice(&sample_rate.to_le_bytes());
                h.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
                h.extend_from_slice(&block_align.to_le_bytes());
                h.extend_from_slice(&bit_depth.to_le_bytes());
                h.extend_from_slice(b"data");
                h.extend_from_slice(&data_len.to_le_bytes());
            }
        }
        h
    }

    fn write(&mut self, samples: &[i32]) -> Result<()> {
        let width = usize::from(self.spec.bit_depth / 8);
        self.bytes.clear();
        for s in samples {
            match self.spec.container {
                Container::Aiff => self.bytes.extend_from_slice(&s.to_be_bytes()[4 - width..]),
                _ => self.bytes.extend_from_slice(&s.to_le_bytes()[..width]),
            }
        }
        self.data_len += self.bytes.len() as u64;
        self.file.write_all(&self.bytes).at(&self.path)
    }

    fn finish(mut self) -> Result<u64> {
        let frame_len = u64::from(self.spec.channels) * u64::from(self.spec.bit_depth / 8);
        let frames = self.data_len / frame_len;
        let mut tail = Vec::new();
        // Chunks start on even offsets.
        if self.data_len % 2 == 1 {
            tail.push(0);
        }
        if let Some(tag) = self.tag.as_ref().filter(|t| !t.is_empty()) {
            let mut id3 = Vec::new();
            tag.dump_to(&mut id3, tag_options()).tagging(&self.path)?;
            let (id, len) = match self.spec.container {
                Container::Aiff => (b"ID3 ", (id3.len() as u32).to_be_bytes()),
                _ => (b"id3 ", (id3.len() as u32).to_le_bytes()),
            };
            tail.extend_from_slice(id);
            tail.extend_from_slice(&len);
            tail.extend_from_slice(&id3);
            if id3.len() % 2 == 1 {
                tail.push(0);
            }
        }
        let header_len = self.header(0, 0).len() as u64;
        let Ok(total) = u32::try_from(header_len + self.data_len + tail.len() as u64) else {
            let message = format!("too long for {} (4 GiB at most)", self.spec.container.extension().to_uppercase());
            return Err(Error::Encode { path: self.path, message });
        };
        let header = self.header(frames as u32, total);
        self.file.write_all(&tail).at(&self.path)?;
        self.file.seek(SeekFrom::Start(0)).at(&self.path)?;
        self.file.write_all(&header).at(&self.path)?;
        self.file.flush().at(&self.path)?;
        Ok(frames)
    }
}

/// `rate` (not 0) as the 80-bit extended float AIFF's COMM chunk holds it.
fn extended(rate: u32) -> [u8; 10] {
    let mut out = [0; 10];
    let shift = u64::from(rate).leading_zeros();
    out[..2].copy_from_slice(&((16_383 + 63 - shift) as u16).to_be_bytes());
    out[2..].copy_from_slice(&(u64::from(rate) << shift).to_be_bytes());
    out
}

pub(crate) struct Flac {
    file: BufWriter<File>,
    path: PathBuf,
    channels: usize,
    config: flacenc::error::Verified<config::Encoder>,
    info: StreamInfo,
    /// The MD5 of the samples, which STREAMINFO carries.
    md5: flacenc::source::Context,
    block: FrameBuf,
    /// Interleaved samples short of a block.
    pending: Vec<i32>,
    frames: usize,
    sink: ByteSink,
}

/// Where STREAMINFO's content starts: after `fLaC` and its block header.
const STREAMINFO_AT: u64 = 8;

impl Flac {
    fn new(mut file: File, path: &Path, spec: Spec, tag: Option<Tag>) -> Result<Flac> {
        let (channels, depth) = (usize::from(spec.channels), usize::from(spec.bit_depth));
        let mut info = StreamInfo::new(spec.sample_rate as usize, channels, depth).encoding(path)?;
        info.set_block_sizes(BLOCK, BLOCK).expect("valid block sizes");
        let config = config::Encoder::default().into_verified().expect("the default configuration is valid");
        let block = FrameBuf::with_size(channels, BLOCK).encoding(path)?;
        let mut sink = ByteSink::new();
        sink.write_bytes_aligned(b"fLaC").expect("memory");
        // STREAMINFO, the last metadata block until lofty adds the tags.
        sink.write_bytes_aligned(&[0x80, 0, 0, 34]).expect("memory");
        info.write(&mut sink).expect("memory");
        file.write_all(sink.as_slice()).at(path)?;
        if let Some(tag) = tag.filter(|t| !t.is_empty()) {
            // lofty tells the format from the start of the file.
            file.rewind().at(path)?;
            TagExt::save_to(&tag, &mut file, tag_options()).tagging(path)?;
        }
        file.seek(SeekFrom::End(0)).at(path)?;
        Ok(Flac {
            file: BufWriter::new(file),
            path: path.to_path_buf(),
            channels,
            config,
            info,
            md5: flacenc::source::Context::new(depth, channels),
            block,
            pending: Vec::with_capacity(BLOCK * channels),
            frames: 0,
            sink,
        })
    }

    fn write(&mut self, samples: &[i32]) -> Result<()> {
        let block_len = BLOCK * self.channels;
        let mut samples = samples;
        while !samples.is_empty() {
            let take = (block_len - self.pending.len()).min(samples.len());
            self.pending.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.pending.len() == block_len {
                self.encode()?;
            }
        }
        Ok(())
    }

    /// Encode what's pending as one frame.
    fn encode(&mut self) -> Result<()> {
        self.block.fill_interleaved(&self.pending).encoding(&self.path)?;
        self.md5.fill_interleaved(&self.pending).encoding(&self.path)?;
        let frame = flacenc::encode_fixed_size_frame(&self.config, &self.block, self.frames, &self.info)
            .encoding(&self.path)?;
        self.info.update_frame_info(&frame);
        self.sink.clear();
        frame.write(&mut self.sink).expect("memory");
        self.file.write_all(self.sink.as_slice()).at(&self.path)?;
        self.frames += 1;
        self.pending.clear();
        Ok(())
    }

    fn finish(mut self) -> Result<u64> {
        if !self.pending.is_empty() {
            self.encode()?;
        }
        self.info.set_md5_digest(&self.md5.md5_digest());
        // The last block may be shorter, but the minimum leaves it out, as the format says:
        // counting it would make readers (symphonia) take the frame numbers for sample numbers.
        self.info.set_block_sizes(BLOCK, BLOCK).expect("valid block sizes");
        self.sink.clear();
        self.info.write(&mut self.sink).expect("memory");
        self.file.seek(SeekFrom::Start(STREAMINFO_AT)).at(&self.path)?;
        self.file.write_all(self.sink.as_slice()).at(&self.path)?;
        self.file.flush().at(&self.path)?;
        Ok(self.info.total_samples() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 44.1 kHz as AIFF writers spell it (Apple's own files hold these bytes).
    #[test]
    fn sample_rates_as_extended_floats() {
        assert_eq!(extended(44_100), [0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
        assert_eq!(extended(48_000), [0x40, 0x0E, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]);
    }
}
