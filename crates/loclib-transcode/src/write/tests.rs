use super::*;

/// 44.1 kHz as AIFF writers spell it (Apple's own files hold these bytes).
#[test]
fn sample_rates_as_extended_floats() {
    assert_eq!(extended(44_100), [0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
    assert_eq!(extended(48_000), [0x40, 0x0E, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]);
}

use hegel::generators as gs;

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
