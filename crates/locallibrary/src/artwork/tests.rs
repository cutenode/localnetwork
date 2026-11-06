use super::*;

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]));
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img).write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

#[test]
fn renders_square_jpegs() {
    let art = Artwork::from_bytes(&png(300, 200)).unwrap();
    for size in [THUMB_SIZE, MEDIUM_SIZE] {
        let jpg = art.jpeg(size).unwrap();
        let back = image::load_from_memory(&jpg).unwrap();
        assert_eq!((back.width(), back.height()), (size, size));
    }
    assert!(Artwork::from_bytes(b"not an image").is_err());
    assert!(Artwork::from_bytes(&png(4, 4)).is_err());
}

#[test]
fn variants() {
    assert_eq!(medium_variant("PIONEER/Artwork/00001/a1.jpg"), "PIONEER/Artwork/00001/a1_m.jpg");
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(free_artwork_key(dir.path(), 7, "jpg"), "PIONEER/Artwork/00001/a7.jpg");
    // rekordbox's own exports: a333 in 00017, a1852 in 00093.
    assert_eq!(free_artwork_key(dir.path(), 333, "jpg"), "PIONEER/Artwork/00017/a333.jpg");
    assert_eq!(free_artwork_key(dir.path(), 1852, "jpg"), "PIONEER/Artwork/00093/a1852.jpg");
    let taken = paths::to_fs(dir.path(), "PIONEER/Artwork/00001/a7.jpg");
    std::fs::create_dir_all(taken.parent().unwrap()).unwrap();
    std::fs::write(&taken, b"x").unwrap();
    assert_eq!(free_artwork_key(dir.path(), 7, "jpg"), "PIONEER/Artwork/00002/a7.jpg");
}

/// Without an extension, or with a dot only in a folder's name, the suffix goes at the end.
#[test]
fn medium_variants_of_names_without_an_extension() {
    assert_eq!(medium_variant("PIONEER/Artwork/00001/a1"), "PIONEER/Artwork/00001/a1_m");
    assert_eq!(medium_variant("PIONEER/Art.work/a1"), "PIONEER/Art.work/a1_m");
}

#[test]
fn previews_are_lossless_pngs() {
    let jpeg = Artwork::from_bytes(&png(40, 30)).unwrap().jpeg(THUMB_SIZE).unwrap();
    let back = image::load_from_memory(&to_png(&jpeg).unwrap()).unwrap();
    assert_eq!((back.width(), back.height()), (THUMB_SIZE, THUMB_SIZE));
    assert_eq!(image::guess_format(&to_png(&jpeg).unwrap()).unwrap(), ImageFormat::Png);
    assert!(matches!(to_png(b"not an image"), Err(Error::Image(_))));
}

use hegel::TestCase;
use hegel::generators as gs;

/// An image an artwork edit gives, as a file or as base64, is staged as the JPEG of it at the
/// size players show, named by that JPEG's hash, the same either way; given staged, it's taken
/// by its hash while the drive keeps it and refused, naming it, once it doesn't. Text that isn't
/// base64, or bytes that aren't an image, are refused as images.
#[hegel::test(test_cases = 30)]
fn images_are_staged_by_the_hash_of_their_jpeg(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    let (w, h) = (
        tc.draw(gs::integers::<u32>().min_value(16).max_value(80)),
        tc.draw(gs::integers::<u32>().min_value(16).max_value(80)),
    );
    let bytes = png(w, h);
    let file = dir.path().join("cover.png");
    std::fs::write(&file, &bytes).unwrap();
    let (hash, jpeg) = stage(&device, &ImageSource::Base64(BASE64_STANDARD.encode(&bytes))).unwrap();
    let jpeg = jpeg.unwrap();
    assert_eq!(hash, Hash::of(&jpeg));
    let back = image::load_from_memory(&jpeg).unwrap();
    assert_eq!(
        (back.width(), back.height(), image::guess_format(&jpeg).unwrap()),
        (MEDIUM_SIZE, MEDIUM_SIZE, ImageFormat::Jpeg)
    );
    assert_eq!(stage(&device, &ImageSource::Path(file)).unwrap(), (hash, Some(jpeg.clone())));

    let kept = tc.draw(gs::booleans());
    if kept {
        let staged = crate::staged::IMAGES.path(&device, &hash);
        std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
        std::fs::write(&staged, &jpeg).unwrap();
    }
    match stage(&device, &ImageSource::Staged(hash)) {
        Ok(got) => assert!(kept && got == (hash, None), "{got:?}"),
        Err(Error::NotFound(m)) => assert!(!kept && m.contains(&hash.short()), "{m}"),
        Err(e) => panic!("{e}"),
    }

    // Short text is never an image, whether or not it's base64.
    let junk = tc.draw(gs::text().max_size(12));
    let e = stage(&device, &ImageSource::Base64(junk.clone())).unwrap_err();
    let base64 = BASE64_STANDARD.decode(&junk).is_ok();
    assert!(matches!(&e, Error::Image(m) if m.starts_with("the image isn't base64") != base64), "{junk:?}: {e}");
}

/// A preview keeps the image's shape, shrunk to fit [`PREVIEW_SIZE`] when it's bigger (never
/// grown), and one wider than 8192 pixels is refused. An audio file that doesn't read, or an image
/// that isn't there, gives no artwork, and an image given as a plugin's blob isn't staged outside
/// a plugin's plan.
#[hegel::test(test_cases = 20)]
fn prop_previews_keep_their_shape_within_the_preview_size(tc: TestCase) {
    let long = tc.draw(gs::integers::<u32>().min_value(16).max_value(1100));
    let short = tc.draw(gs::integers::<u32>().min_value(16).max_value(40)).min(long);
    let (w, h) = if tc.draw(gs::booleans()) { (long, short) } else { (short, long) };
    let back = image::load_from_memory(&to_png(&png(w, h)).unwrap()).unwrap();
    let side = long.min(PREVIEW_SIZE);
    assert_eq!(back.width().max(back.height()), side, "{w}x{h}");
    let scaled = f64::from(short) * f64::from(side) / f64::from(long);
    assert!(
        (f64::from(back.width().min(back.height())) - scaled).abs() <= 1.0,
        "{w}x{h}: {:?}",
        (back.width(), back.height())
    );
    assert!(matches!(to_png(&png(8193, 1)), Err(Error::Image(_))));

    let dir = tempfile::tempdir().unwrap();
    let not_audio = dir.path().join("song.mp3");
    std::fs::write(&not_audio, png(w, h)).unwrap();
    assert!(matches!(Artwork::from_path(&not_audio), Err(Error::Tags(_))));
    assert!(matches!(Artwork::from_path(&dir.path().join("gone.png")), Err(Error::Io { .. })));
    let device = Device::open(dir.path()).unwrap();
    let name = tc.draw(gs::text().max_size(8));
    assert!(matches!(stage(&device, &ImageSource::Blob(name)), Err(Error::Invalid(m)) if m.contains("blob")));
}
