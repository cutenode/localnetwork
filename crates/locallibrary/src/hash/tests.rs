use super::*;

/// A writer that takes `room` bytes and then fails.
struct Limited {
    room: usize,
    written: Vec<u8>,
}

impl Write for Limited {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.written.len() + buf.len() > self.room {
            return Err(std::io::Error::other("the drive is full"));
        }
        self.written.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A copy holds the source's bytes and hashes to what they do, however big the file is next to
/// the buffers; progress adds up to its size; a write that fails is the error, and leaves the
/// reader stopped rather than waiting to be heard.
#[hegel::test(test_cases = 30)]
fn a_copy_is_the_file_and_its_hash(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let size =
        tc.draw(gs::sampled_from(vec![0, 1, BUF_SIZE - 1, BUF_SIZE, BUF_SIZE + 1, BUF_SIZE * 3 + 7, BUF_SIZE * 8]));
    let fails = tc.draw(gs::booleans()) && size > 0;
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.mp3");
    let bytes: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    std::fs::write(&src, &bytes).unwrap();
    let copied = std::sync::atomic::AtomicU64::new(0);
    let progress = |e: &Event| {
        if let Event::Bytes { bytes, .. } = e {
            copied.fetch_add(*bytes, std::sync::atomic::Ordering::Relaxed);
        }
    };
    let ctx = Ctx::new(&progress, Default::default());
    let mut dst = Limited { room: if fails { size - 1 } else { size }, written: Vec::new() };
    match copy_hashing(&src, &mut dst, Path::new("dst"), &ctx, Phase::Copy) {
        Ok((hash, total)) => {
            assert!(!fails);
            assert_eq!((hash, total), (Hash::of(&bytes), size as u64));
            assert_eq!(dst.written, bytes);
            assert_eq!(copied.into_inner(), size as u64);
        }
        Err(e) => {
            assert!(fails, "{e}");
            assert!(matches!(e, Error::Io { ref path, .. } if path == Path::new("dst")), "{e}");
        }
    }
}

#[test]
fn a_copy_of_a_big_file_stops_when_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.mp3");
    std::fs::write(&src, vec![7; BUF_SIZE * 3]).unwrap();
    let cancel = crate::progress::CancelToken::new();
    let ctx = Ctx::new(&crate::progress::NoProgress, cancel.clone());
    cancel.cancel();
    let copied = copy_hashing(&src, &mut std::io::sink(), Path::new("dst"), &ctx, Phase::Copy);
    assert!(matches!(copied, Err(Error::Cancelled)), "{copied:?}");
}

/// The JSON schema says what a hash serialises to, and logs show its short form.
#[test]
fn the_schema_matches_the_serialised_form() {
    let h = Hash::of(b"audio");
    let json = serde_json::to_value(h).unwrap();
    let hex = json.as_str().unwrap();
    assert!(hex.len() == 64 && hex.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')), "{hex}");
    let schema = schemars::schema_for!(Hash);
    assert_eq!(schema.get("pattern").and_then(|p| p.as_str()), Some("^[0-9a-f]{64}$"));
    assert_eq!(format!("{h:?}"), format!("Hash({})", &hex[..12]));
    assert!(serde_json::from_str::<Hash>("\"not a hash\"").is_err());
}

/// A hash reads back from its hex in either case, its short form starts it, and it shows as
/// `Hash(short)` when debugged.
#[hegel::test(test_cases = 100)]
fn a_hash_reads_back_from_its_hex(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let bytes: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>()).max_size(64));
    let h = Hash::of(&bytes);
    let hex = h.to_hex();
    assert_eq!(hex.len(), 64);
    assert!(hex.starts_with(&h.short()));
    let upper = tc.draw(gs::booleans());
    let text = if upper { hex.to_uppercase() } else { hex.clone() };
    assert_eq!(text.parse::<Hash>().ok(), Some(h));
    assert_eq!(format!("{h:?}"), format!("Hash({})", h.short()));
    assert_eq!(h.to_string(), hex);
}
