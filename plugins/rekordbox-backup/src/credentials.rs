//! Sign-in tokens taken out of the settings files a backup carries, when the user asks
//! (`credentials = "strip"`). `rekordbox3.settings` holds rekordbox's and Dropbox's tokens as
//! plain values and `activations.xml` the AlphaTheta account; each element holding them is cut out
//! where the parser found it, so every other byte of the file stays as rekordbox wrote it.

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;

/// `rekordbox3.settings` without any `VALUE` whose name has "token" in it, in any case
/// (`RekordboxToken`, `DropboxRefreshToken`).
pub(crate) fn strip_tokens(settings: &[u8]) -> Result<Vec<u8>, String> {
    strip(settings, |e| {
        if e.name().as_ref() != "VALUE" {
            return Ok(false);
        }
        let name = e.attributes().flatten().find(|a| a.key.as_ref() == "name");
        let name = name.map(|a| a.normalized_value(XmlVersion::Implicit1_0).map(|v| v.to_lowercase()));
        Ok(name.transpose().map_err(|e| e.to_string())?.is_some_and(|n| n.contains("token")))
    })
}

/// `activations.xml` without its `rekordbox_account` elements (the account, its details and an
/// encrypted secret).
pub(crate) fn strip_accounts(activations: &[u8]) -> Result<Vec<u8>, String> {
    strip(activations, |e| Ok(e.name().as_ref() == "rekordbox_account"))
}

/// `xml` without the elements `cut` picks, each with its line when it has one to itself. A file
/// that doesn't parse is an error: what it holds can't be told, so it isn't backed up as it is.
fn strip(xml: &[u8], cut: impl Fn(&BytesStart<'_>) -> Result<bool, String>) -> Result<Vec<u8>, String> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut spans = Vec::new();
    // quick-xml stops at the end of the file whatever's open: one cut short is no settings file.
    // Nor is one with no element at all (text alone).
    let (mut open, mut any) = (0usize, false);
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|e| format!("not XML: {e}"))?;
        any |= matches!(event, Event::Start(_) | Event::Empty(_));
        match event {
            Event::Eof if open > 0 => return Err("not XML: it ends inside an element".into()),
            Event::Eof if !any => return Err("not XML: it has no element".into()),
            Event::Eof => break,
            Event::End(_) => open = open.saturating_sub(1),
            Event::Empty(e) if cut(&e)? => spans.push((start, reader.buffer_position() as usize)),
            Event::Start(e) if cut(&e)? => {
                let name = e.name().as_ref().to_owned();
                reader.read_to_end(QName(&name)).map_err(|e| format!("not XML: {e}"))?;
                spans.push((start, reader.buffer_position() as usize));
            }
            Event::Start(_) => open += 1,
            _ => {}
        }
    }
    let mut out = Vec::with_capacity(xml.len());
    let mut at = 0;
    for (start, end) in spans {
        let (start, end) = with_line(xml, start, end);
        out.extend_from_slice(&xml[at..start]);
        at = end;
    }
    out.extend_from_slice(&xml[at..]);
    Ok(out)
}

/// `start..end` widened to its whole line, line break included, when nothing but spaces share
/// the line with it.
fn with_line(xml: &[u8], start: usize, end: usize) -> (usize, usize) {
    let blank = |b: &u8| *b == b' ' || *b == b'\t';
    let line_start = xml[..start].iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    let rest = &xml[end..];
    let line_end = rest.iter().position(|b| *b == b'\n').map_or(xml.len(), |i| end + i + 1);
    let before = xml[line_start..start].iter().all(blank);
    let after = xml[end..line_end].iter().all(|b| blank(b) || *b == b'\r' || *b == b'\n');
    if before && after { (line_start, line_end) } else { (start, end) }
}

#[cfg(test)]
mod tests;
