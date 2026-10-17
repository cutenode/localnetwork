//! A release group's front cover from the Cover Art Archive, made the image a `set_artwork` edit
//! gives: what locallibrary stages for players (a square JPEG, centre-cropped) at the larger size
//! they show. Sent at that size, a plan's images stay small enough for one message.

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;

use crate::web::Web;

/// The Cover Art Archive's thumbnail asked for: twice the size players show, so a cover that isn't
/// square still fills it once cropped.
const THUMBNAIL: &str = "front-500";
/// The larger size players show, which locallibrary scales artwork to (its `MEDIUM_SIZE`).
pub(crate) const SIZE: u32 = 240;
/// The JPEG quality locallibrary writes artwork at.
const QUALITY: u8 = 90;
/// The smallest image locallibrary takes as artwork, either way.
const SMALLEST: u32 = 16;

/// Release group `id`'s front cover as a JPEG of [`SIZE`], or what the plan's warning says
/// instead.
pub(crate) fn front(web: &Web, base: &str, id: &str) -> Result<Vec<u8>, String> {
    let url = format!("{base}/release-group/{id}/{THUMBNAIL}");
    let (status, body) = web.get(&url, &[]).map_err(|e| format!("its cover didn't download: {e}"))?;
    eprintln!("coverart: Cover Art Archive {id}: {status}, {} bytes", body.len());
    match status {
        200 => {}
        404 => return Err("the Cover Art Archive has no front cover for it".into()),
        status => return Err(format!("its cover didn't download: the Cover Art Archive answered {status}")),
    }
    let image = image::load_from_memory(&body).map_err(|e| format!("its cover isn't an image: {e}"))?;
    if image.width() < SMALLEST || image.height() < SMALLEST {
        return Err(format!("its cover is too small ({} x {} px)", image.width(), image.height()));
    }
    let square = image.resize_to_fill(SIZE, SIZE, FilterType::Lanczos3).to_rgb8();
    let mut jpeg = Vec::new();
    square.write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, QUALITY)).expect("an image encodes into memory");
    Ok(jpeg)
}
