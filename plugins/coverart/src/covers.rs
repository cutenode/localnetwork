//! A release group's front cover from the Cover Art Archive, made the image a `set_artwork` edit
//! gives: what locallibrary stages for players (a square JPEG, centre-cropped) at the larger size
//! they show, in the request's scratch folder for the host to read.

use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use loclib_plugin::{Fetch, Host, RpcError};

/// Where the Cover Art Archive is.
const COVER_ART_ARCHIVE: &str = "https://coverartarchive.org";
/// The Cover Art Archive's thumbnail asked for: twice the size players show, so a cover that isn't
/// square still fills it once cropped.
const THUMBNAIL: &str = "front-500";
/// The larger size players show, which locallibrary scales artwork to (its `MEDIUM_SIZE`).
pub(crate) const SIZE: u32 = 240;
/// The JPEG quality locallibrary writes artwork at.
const QUALITY: u8 = 90;
/// The smallest image locallibrary takes as artwork, either way.
const SMALLEST: u32 = 16;

/// Release group `id`'s front cover as a JPEG of [`SIZE`] in `scratch` (its name there), or what
/// the plan's warning says instead. Fails when the operation is cancelled, or the JPEG can't be
/// written.
pub(crate) fn front(host: &Host, scratch: &Path, id: &str) -> Result<Result<String, String>, RpcError> {
    let url = format!("{COVER_ART_ARCHIVE}/release-group/{id}/{THUMBNAIL}");
    let download = scratch.join(format!("{THUMBNAIL}-{id}"));
    let fetched = match host.fetch(&Fetch { into_blob: Some(download.clone()), ..Fetch::get(url) }) {
        Err(e) if e.kind() == Some("cancelled") => return Err(e),
        Err(e) => return Ok(Err(format!("its cover didn't download: {}", e.message))),
        Ok(fetched) => fetched,
    };
    // Gone once read: the scratch folder only has so much room, which the covers take up.
    let body = std::fs::read(&download).unwrap_or_default();
    let _ = std::fs::remove_file(&download);
    eprintln!("coverart: Cover Art Archive {id}: {}, {} bytes", fetched.status, body.len());
    match fetched.status {
        200 => {}
        404 => return Ok(Err("the Cover Art Archive has no front cover for it".into())),
        status => return Ok(Err(format!("its cover didn't download: the Cover Art Archive answered {status}"))),
    }
    let image = match image::load_from_memory(&body) {
        Ok(image) => image,
        Err(e) => return Ok(Err(format!("its cover isn't an image: {e}"))),
    };
    if image.width() < SMALLEST || image.height() < SMALLEST {
        return Ok(Err(format!("its cover is too small ({} x {} px)", image.width(), image.height())));
    }
    let square = image.resize_to_fill(SIZE, SIZE, FilterType::Lanczos3).to_rgb8();
    let mut jpeg = Vec::new();
    square.write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, QUALITY)).expect("an image encodes into memory");
    let name = format!("cover-{id}.jpg");
    std::fs::write(scratch.join(&name), jpeg).map_err(|e| RpcError::failed("io", format!("{name}: {e}")))?;
    Ok(Ok(name))
}
