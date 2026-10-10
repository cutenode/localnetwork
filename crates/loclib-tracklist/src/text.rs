//! Bytes to text: UTF-8, UTF-16 (with or without a BOM) or Windows-1252, with `\n` line ends.

/// Bytes to text with `\n` line ends: UTF-8 or UTF-16 (with or without a BOM), falling back to
/// Windows-1252 for the legacy `.m3u` files some software still writes.
pub fn text_of(bytes: &[u8]) -> String {
    let text = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(rest).into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        utf16(rest, true)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        utf16(rest, false)
    } else if let Some(le) = sniff_utf16(bytes) {
        utf16(bytes, le)
    } else {
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => bytes.iter().map(|&b| cp1252(b)).collect(),
        }
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    if text.contains('\r') { text.replace("\r\n", "\n").replace('\r', "\n") } else { text.to_string() }
}

fn utf16(bytes: &[u8], le: bool) -> String {
    let units = bytes
        .chunks_exact(2)
        .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) });
    char::decode_utf16(units).map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
}

/// UTF-16 without a BOM: mostly-ASCII text has a zero in every other byte. `Some(true)` for
/// little-endian.
fn sniff_utf16(bytes: &[u8]) -> Option<bool> {
    let pairs: Vec<&[u8]> = bytes[..bytes.len().min(1024)].chunks_exact(2).collect();
    if pairs.len() < 2 {
        return None;
    }
    let le = pairs.iter().filter(|p| p[0] != 0 && p[1] == 0).count();
    let be = pairs.iter().filter(|p| p[0] == 0 && p[1] != 0).count();
    if le * 10 >= pairs.len() * 4 && be * 10 < pairs.len() {
        Some(true)
    } else if be * 10 >= pairs.len() * 4 && le * 10 < pairs.len() {
        Some(false)
    } else {
        None
    }
}

fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘',
        '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

#[cfg(test)]
mod tests {
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
}
