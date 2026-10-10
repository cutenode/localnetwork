//! Pictures and audio to give tracks, drives and the computer's files: a PNG, silent WAV files and
//! a few MP3 frames, all made on the spot.

use std::io::Cursor;
use std::path::Path;

use locallibrary::artwork::{self, Artwork};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;

/// A `w` x `h` PNG, a gradient tinted by `shade` (so two shades are two pictures).
pub fn png(w: u32, h: u32, shade: u8) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, shade]));
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// One picture set on the tracks `ids` of the drive at `root`, as `art set` sets it.
pub fn set_art(root: &Path, ids: &[u32]) {
    let mut d = Device::open_export(root).unwrap();
    artwork::set_on_drive(&mut d, ids, &Artwork::from_bytes(&png(300, 300, 99)).unwrap(), &Ctx::silent()).unwrap();
}

/// A tenth of a second of silence, 16-bit mono PCM at 44.1 kHz: audio art can be embedded in.
pub fn wav() -> Vec<u8> {
    wav_of(44_100, 16, 1, 0.1)
}

/// `secs` of silence as a PCM WAV file.
pub fn wav_of(rate: u32, bits: u16, channels: u16, secs: f64) -> Vec<u8> {
    let frame = u32::from(bits / 8 * channels);
    let data = (f64::from(rate) * secs) as u32 * frame;
    let mut w = b"RIFF".to_vec();
    w.extend((36 + data).to_le_bytes());
    w.extend(b"WAVEfmt ");
    w.extend(16u32.to_le_bytes());
    w.extend(1u16.to_le_bytes());
    w.extend(channels.to_le_bytes());
    w.extend(rate.to_le_bytes());
    w.extend((rate * frame).to_le_bytes());
    w.extend((frame as u16).to_le_bytes());
    w.extend(bits.to_le_bytes());
    w.extend(b"data");
    w.extend(data.to_le_bytes());
    w.resize(w.len() + data as usize, 0);
    w
}

/// Ten silent MPEG-1 Layer III frames (128 kbit/s, 44.1 kHz): enough of an MP3 to tag.
pub fn mp3() -> Vec<u8> {
    [&[0xff, 0xfb, 0x90, 0x64][..], &[0; 413]].concat().repeat(10)
}
