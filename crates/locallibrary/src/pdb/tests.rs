use super::*;

#[test]
fn rejects_garbage_without_panicking() {
    let p = Path::new("x.pdb");
    assert!(parse_database(vec![], p).is_err());
    assert!(parse_database(vec![0u8; 100], p).is_err());
    // Valid-looking header, garbage pages.
    let mut buf = vec![0xffu8; 4096 * 4];
    buf[0..4].copy_from_slice(&0u32.to_le_bytes());
    buf[4..8].copy_from_slice(&4096u32.to_le_bytes());
    buf[8..12].copy_from_slice(&3u32.to_le_bytes());
    assert!(parse_database(buf, p).is_err());
}
