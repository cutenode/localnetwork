use super::*;
use crate::testing::utf16le;

#[test]
fn decodes_bom_utf16_and_legacy_encodings() {
    assert_eq!(text_of(&utf16le("a\r\nb")), "a\nb");
    let mut be = vec![0xFE, 0xFF];
    be.extend("Café".encode_utf16().flat_map(u16::to_be_bytes));
    assert_eq!(text_of(&be), "Café");
    // UTF-16 without a BOM
    let bare: Vec<u8> = "Artist - Title\n".encode_utf16().flat_map(u16::to_le_bytes).collect();
    assert_eq!(text_of(&bare), "Artist - Title\n");
    assert_eq!(text_of(b"\xEF\xBB\xBFx"), "x");
    // Windows-1252: é, en dash, curly apostrophe
    assert_eq!(text_of(b"Caf\xe9 \x96 Don\x92t"), "Café – Don’t");
    assert_eq!(text_of(b"a\rb"), "a\nb");
}
