use std::path::PathBuf;

/// locallibrary's test fixtures, whose demo export (`rekordcrate/demo_tracks`) holds the analysis
/// files of two tracks as rekordbox wrote them.
pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../locallibrary/tests/fixtures")
}

/// A minimal analysis file (`PMAI` and `PPTH`) for `device_path`.
pub fn synthetic_anlz(device_path: &str) -> Vec<u8> {
    let mut path_bytes: Vec<u8> = device_path.encode_utf16().flat_map(u16::to_be_bytes).collect();
    path_bytes.extend_from_slice(&[0, 0]);
    let mut f = Vec::new();
    f.extend_from_slice(b"PMAI");
    f.extend_from_slice(&0x1cu32.to_be_bytes());
    f.extend_from_slice(&((0x1c + 16 + path_bytes.len()) as u32).to_be_bytes());
    f.extend_from_slice(&[0u8; 0x1c - 12]);
    f.extend_from_slice(b"PPTH");
    f.extend_from_slice(&16u32.to_be_bytes());
    f.extend_from_slice(&((16 + path_bytes.len()) as u32).to_be_bytes());
    f.extend_from_slice(&(path_bytes.len() as u32).to_be_bytes());
    f.extend_from_slice(&path_bytes);
    f
}
