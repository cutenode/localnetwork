use super::*;

/// 44.1 kHz as AIFF writers spell it (Apple's own files hold these bytes).
#[test]
fn sample_rates_as_extended_floats() {
    assert_eq!(extended(44_100), [0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
    assert_eq!(extended(48_000), [0x40, 0x0E, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]);
}

use hegel::generators as gs;
use lofty::tag::TagType;

/// WAV and AIFF hold their sizes in 32 bits: audio that would take the file past 4 GiB is
/// refused, naming the container, and anything up to that is written with its sizes, a pad
/// byte after audio of an odd length included.
#[hegel::test(test_cases = 100)]
fn pcm_files_stop_at_4_gib(tc: hegel::TestCase) {
    let container = tc.draw(gs::sampled_from(vec![Container::Aiff, Container::Wav]));
    let (header, name) = if container == Container::Aiff { (54u64, "AIFF") } else { (44, "WAV") };
    let limit = u64::from(u32::MAX) - header;
    let data_len = tc.draw(hegel::one_of!(
        gs::integers::<u64>().min_value(limit - 3).max_value(limit + 3),
        gs::integers::<u64>().max_value(1 << 34)
    ));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("copy.{}", container.extension()));
    let file = File::options().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
    let spec = Spec { container, sample_rate: 44_100, channels: 1, bit_depth: 16 };
    let mut pcm = Pcm::new(file, &path, spec, None).unwrap();
    // As if that much audio had been written: what's on disk is only the header and the end.
    pcm.data_len = data_len;
    let total = header + data_len + data_len % 2;
    match pcm.finish() {
        Ok(frames) => {
            assert!(total <= u64::from(u32::MAX), "{total} bytes");
            assert_eq!(frames, data_len / 2);
            let bytes = std::fs::read(&path).unwrap();
            let size: [u8; 4] = bytes[4..8].try_into().unwrap();
            let size = if container == Container::Aiff { u32::from_be_bytes(size) } else { u32::from_le_bytes(size) };
            assert_eq!(u64::from(size), total - 8);
        }
        Err(Error::Encode { path: at, message }) => {
            assert!(total > u64::from(u32::MAX), "{total} bytes: {message}");
            assert!(at == path && message.contains(name), "{message}");
        }
        Err(e) => panic!("{e}"),
    }
}

/// `samples` written to a copy made with `file`, and the copy finished: what failed first.
fn write_all(file: File, path: &Path, spec: Spec, tag: Option<Tag>, samples: &[i32]) -> Result<u64> {
    let mut w = Writer::new(file, path, spec, tag)?;
    w.write(samples)?;
    w.finish()
}

/// A tag of `kind` titled `title`.
fn titled(kind: lofty::tag::TagType, title: String) -> Tag {
    use lofty::tag::Accessor;
    let mut tag = Tag::new(kind);
    tag.set_title(title);
    tag
}

/// FLAC frames take samples in their depth's range only: one past it is the copy's encoding
/// error, when its block is encoded as it fills or when the file is finished short of one.
#[hegel::test(test_cases = 60)]
fn prop_samples_past_the_depth_are_an_encoding_error(tc: hegel::TestCase) {
    let depth = tc.draw(gs::sampled_from(vec![16u16, 24]));
    let channels = tc.draw(gs::integers::<u16>().min_value(1).max_value(2));
    let frames = tc.draw(gs::integers::<usize>().min_value(1).max_value(2 * BLOCK + 10));
    let ch = usize::from(channels);
    let over = tc.draw(gs::integers::<usize>().max_value(frames * ch - 1));
    let mut samples = vec![0; frames * ch];
    let limit = 1i32 << (depth - 1);
    samples[over] = if tc.draw(gs::booleans()) { limit } else { -limit - 1 };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("copy.flac");
    let file = File::options().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
    let spec = Spec { container: Container::Flac, sample_rate: 44_100, channels, bit_depth: depth };
    let mut w = Writer::new(file, &path, spec, None).unwrap();
    // The block it's in is encoded as the samples come when they fill it.
    let block = over / (BLOCK * ch);
    let as_written = (block + 1) * BLOCK <= frames;
    let err = match w.write(&samples) {
        Err(e) => {
            assert!(as_written, "frame {} of {frames} failed the write", over / ch);
            e
        }
        Ok(()) => {
            assert!(!as_written, "frame {} of {frames} was written", over / ch);
            w.finish().unwrap_err()
        }
    };
    assert!(matches!(&err, Error::Encode { path: at, message } if *at == path && message.contains("range")), "{err}");
}

/// A copy into a file that can't be written (opened read-only) fails as that file's I/O error,
/// wherever writing first reaches the file: FLAC's header at once, an AIFF's or WAV's samples,
/// tag or header once they no longer fit the buffer before it, or at the end. The file is left
/// as it was.
#[hegel::test(test_cases = 60)]
fn prop_a_copy_into_a_file_that_cant_be_written_is_its_io_error(tc: hegel::TestCase) {
    let container = tc.draw(gs::sampled_from(Container::ALL.to_vec()));
    // Up to 20 KB of 16-bit mono samples, and a title of up to 12 KB.
    let frames = tc.draw(gs::integers::<usize>().max_value(10_000));
    let title =
        tc.draw(hegel::one_of!(gs::integers::<usize>().max_value(10), gs::integers::<usize>().max_value(12_000)));
    let kind = if container == Container::Flac { TagType::VorbisComments } else { TagType::Id3v2 };
    let tag = (title > 0).then(|| titled(kind, "t".repeat(title)));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("copy.{}", container.extension()));
    std::fs::write(&path, b"").unwrap();
    let file = File::open(&path).unwrap();
    let spec = Spec { container, sample_rate: 44_100, channels: 1, bit_depth: 16 };
    let result = write_all(file, &path, spec, tag, &vec![7; frames]);
    assert!(matches!(&result, Err(Error::Io { path: at, .. }) if *at == path), "{result:?}");
    assert_eq!(std::fs::read(&path).unwrap(), b"");
}

/// A tag the copy can't hold is the copy's tag error: an ID3v2 comment in a language that isn't
/// three letters, in an AIFF or WAV; ID3v2 at all in a FLAC, which takes Vorbis comments.
#[hegel::test(test_cases = 20)]
fn prop_tags_the_copy_cant_hold_are_its_tag_error(tc: hegel::TestCase) {
    use lofty::tag::{ItemKey, ItemValue, TagItem};

    let container = tc.draw(gs::sampled_from(Container::ALL.to_vec()));
    let language: [u8; 3] = tc.draw(gs::sampled_from(vec![*b"en1", *b"   ", *b"\0\0\0", *b"e-n"]));
    let mut tag = titled(TagType::Id3v2, "Night Drive".into());
    let mut comment = TagItem::new(ItemKey::Comment, ItemValue::Text("hi".into()));
    comment.set_lang(language);
    tag.insert(comment);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("copy.{}", container.extension()));
    let file = File::options().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
    let spec = Spec { container, sample_rate: 44_100, channels: 1, bit_depth: 16 };
    let result = write_all(file, &path, spec, Some(tag), &[1, 2, 3]);
    assert!(matches!(&result, Err(Error::Tags { path: at, .. }) if *at == path), "{result:?}");
}

/// A pipe can't be gone back over: a FLAC copy into one fails as its I/O error before any
/// samples, going back to its start for its tags or to the end of its header; an AIFF or WAV
/// copy when it goes back to fill in its header.
#[cfg(unix)]
#[hegel::test(test_cases = 20)]
fn prop_a_copy_into_a_pipe_is_its_io_error(tc: hegel::TestCase) {
    let container = tc.draw(gs::sampled_from(Container::ALL.to_vec()));
    let tagged = tc.draw(gs::booleans());
    let kind = if container == Container::Flac { TagType::VorbisComments } else { TagType::Id3v2 };
    let (reader, writer) = std::io::pipe().unwrap();
    let file = File::from(std::os::fd::OwnedFd::from(writer));
    let path = Path::new("pipe");
    let spec = Spec { container, sample_rate: 44_100, channels: 1, bit_depth: 16 };
    let tag = tagged.then(|| titled(kind, "Night Drive".into()));
    let result = write_all(file, path, spec, tag, &[1, 2, 3]);
    assert!(matches!(&result, Err(Error::Io { path: at, .. }) if at == path), "{result:?}");
    drop(reader);
}
