//! A recording's WAV header, which the `wav` fuzz target reads.

use std::io::{Read, Seek, SeekFrom};

use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct WavFormat {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits: u16,
}

impl WavFormat {
    pub(super) fn bytes_per_second(&self) -> u64 {
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
