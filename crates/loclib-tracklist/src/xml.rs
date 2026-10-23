//! rekordbox XML (`DJ_PLAYLISTS`).

mod library;

use rustc_hash::FxHashMap;

pub use library::{MAX_BEATS, Mark, Tempo, XmlLibrary, XmlTrack};

use crate::detection::file_path;
use crate::m3u::location_entry;
use crate::table::parse_duration;
use crate::{Entry, Error, Result};
use library::{MAX_DEPTH, Skipped};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum TagKind {
    Start,
    End,
    Empty,
}

struct Tag<'a> {
    name: &'a str,
    kind: TagKind,
    attrs: Vec<(&'a str, String)>,
    line: usize,
    raw: &'a str,
}

impl Tag<'_> {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_str())
    }
}

/// A minimal XML tag scanner: elements and their attributes (entities decoded), skipping text,
/// comments, CDATA, processing instructions and doctypes. Enough for rekordbox's regular format.
struct Scanner<'a> {
    text: &'a str,
    pos: usize,
    line: usize,
    counted: usize,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        Scanner { text, pos: 0, line: 1, counted: 0 }
    }

    fn next_tag(&mut self) -> Result<Option<Tag<'a>>> {
        let text = self.text;
        loop {
            let Some(off) = text[self.pos..].find('<') else { return Ok(None) };
            let start = self.pos + off;
            let rest = &text[start..];
            let skip = [("<!--", "-->"), ("<![CDATA[", "]]>"), ("<?", "?>"), ("<!", ">")]
                .iter()
                .find(|(open, _)| rest.starts_with(open))
                .map(|(_, close)| rest.find(close).map_or(text.len(), |i| start + i + close.len()));
            if let Some(next) = skip {
                self.pos = next;
                continue;
            }
            self.line += text[self.counted..start].matches('\n').count();
            self.counted = start;
            let line = self.line;
            let unterminated = || Error::UnterminatedTag { line };
            let b = rest.as_bytes();
            let end_tag = b.get(1) == Some(&b'/');
            let mut i = if end_tag { 2 } else { 1 };
            let name_start = i;
            while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
                i += 1;
            }
            let name = &rest[name_start..i];
            let mut attrs = Vec::new();
            let kind = loop {
                while i < b.len() && b[i].is_ascii_whitespace() {
                    i += 1;
                }
                match b.get(i) {
                    None => return Err(unterminated()),
                    Some(b'>') => {
                        i += 1;
                        break if end_tag { TagKind::End } else { TagKind::Start };
                    }
                    Some(b'/') if b.get(i + 1) == Some(&b'>') => {
                        i += 2;
                        break TagKind::Empty;
                    }
                    Some(_) => {
                        let attr_start = i;
                        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
                            i += 1;
                        }
                        let attr = &rest[attr_start..i];
                        while i < b.len() && b[i].is_ascii_whitespace() {
                            i += 1;
                        }
                        if b.get(i) != Some(&b'=') {
                            // Valueless attribute (not XML, but don't choke on it).
                            if attr.is_empty() {
                                i += 1;
                            }
                            attrs.push((attr, String::new()));
                            continue;
                        }
                        i += 1;
                        while i < b.len() && b[i].is_ascii_whitespace() {
                            i += 1;
                        }
                        let quote = *b.get(i).ok_or_else(unterminated)?;
                        if quote != b'"' && quote != b'\'' {
                            return Err(Error::UnquotedAttribute { attr: attr.to_string(), line });
                        }
                        let len = rest[i + 1..].find(quote as char).ok_or_else(unterminated)?;
                        attrs.push((attr, unescape(&rest[i + 1..i + 1 + len])));
                        i += len + 2;
                    }
                }
            };
            self.pos = start + i;
            return Ok(Some(Tag { name, kind, attrs, line, raw: &rest[..i] }));
        }
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let entity = rest.find(';').filter(|&j| j <= 10).and_then(|j| {
            let name = &rest[1..j];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => match name.strip_prefix('#') {
                    Some(n) => match n.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => n.parse().ok(),
                    }
                    .and_then(char::from_u32),
                    None => None,
                },
            };
            c.map(|c| (c, j))
        });
        match entity {
            Some((c, j)) => {
                out.push(c);
                rest = &rest[j + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

struct XmlNode {
    name: String,
    playlist: bool,
    /// `KeyType="1"`: the playlist's TRACK keys are Locations rather than TrackIDs.
    by_location: bool,
    /// (key, line) for each TRACK.
    keys: Vec<(String, usize)>,
}

pub(crate) fn parse_xml(text: &str) -> Result<XmlLibrary> {
    let mut scanner = Scanner::new(text);
    let mut collection: Vec<XmlTrack> = Vec::new();
    let mut by_id: FxHashMap<String, usize> = FxHashMap::default();
    let mut by_location: FxHashMap<String, usize> = FxHashMap::default();
    let mut stack: Vec<&str> = Vec::new();
    let mut nodes: Vec<XmlNode> = Vec::new();
    let mut lists: Vec<(String, XmlNode)> = Vec::new();
    let mut saw_root = false;
    let mut skipped = Skipped::default();
    // How deep inside an element nested past MAX_DEPTH the scanner is: everything there is left
    // out, so no loop below runs over more than MAX_DEPTH names.
    let mut too_deep = 0usize;

    // Folder path of the innermost node, without rekordbox's ROOT.
    let node_path = |nodes: &[XmlNode]| {
        let skip = usize::from(nodes.first().is_some_and(|n| n.name == "ROOT"));
        nodes[skip..].iter().map(|n| n.name.as_str()).collect::<Vec<_>>().join("/")
    };

    while let Some(tag) = scanner.next_tag()? {
        if too_deep > 0 {
            match tag.kind {
                TagKind::Start => too_deep += 1,
                TagKind::End => too_deep -= 1,
                TagKind::Empty => {}
            }
            continue;
        }
        // Nodes too: an end tag that skips theirs leaves them open.
        if tag.kind != TagKind::End && stack.len().max(nodes.len()) >= MAX_DEPTH {
            skipped.nested += 1;
            too_deep = usize::from(tag.kind == TagKind::Start);
            continue;
        }
        if tag.kind == TagKind::End {
            if tag.name == "NODE" && stack.contains(&"PLAYLISTS") && !nodes.is_empty() {
                let path = node_path(&nodes);
                if let Some(n) = nodes.pop().filter(|n| n.playlist) {
                    lists.push((path, n));
                }
            }
            if let Some(i) = stack.iter().rposition(|&s| s == tag.name) {
                stack.truncate(i);
            }
            continue;
        }
        let parent = stack.last().copied();
        let in_collection = stack.len() >= 2 && stack[stack.len() - 2] == "COLLECTION";
        match (tag.name, parent) {
            ("DJ_PLAYLISTS", _) => saw_root = true,
            ("TRACK", Some("COLLECTION")) => {
                let get = |name| tag.attr(name).map(str::trim).filter(|s| !s.is_empty());
                let e = Entry {
                    line: tag.line,
                    raw: tag.raw.to_string(),
                    title: get("Name").unwrap_or_default().to_string(),
                    artist: get("Artist").unwrap_or_default().to_string(),
                    album: get("Album").map(String::from),
                    mix: get("Mix").map(String::from),
                    duration_secs: get("TotalTime").and_then(|v| parse_duration(v, false)),
                    path: get("Location").and_then(file_path),
                    isrc: None,
                };
                if let Some(id) = get("TrackID") {
                    by_id.insert(id.to_string(), collection.len());
                }
                if let Some(loc) = get("Location") {
                    by_location.insert(loc.to_string(), collection.len());
                }
                collection.push(library::track(&tag, e, &mut skipped));
            }
            // The track they're in is the last one read.
            ("TEMPO", Some("TRACK")) if in_collection => match library::tempo(&tag) {
                Ok(tempo) => collection.last_mut().expect("the TRACK it's in").tempos.push(tempo),
                Err(why) => skipped.tempo(why),
            },
            ("POSITION_MARK", Some("TRACK")) if in_collection => match library::mark(&tag) {
                Ok(mark) => collection.last_mut().expect("the TRACK it's in").marks.push(mark),
                Err(why) => skipped.mark(why),
            },
            ("NODE", _) if stack.contains(&"PLAYLISTS") => {
                nodes.push(XmlNode {
                    name: tag.attr("Name").unwrap_or_default().to_string(),
                    playlist: tag.attr("Type") == Some("1"),
                    by_location: tag.attr("KeyType") == Some("1"),
                    keys: Vec::new(),
                });
                if tag.kind == TagKind::Empty {
                    let path = node_path(&nodes);
                    if let Some(n) = nodes.pop().filter(|n| n.playlist) {
                        lists.push((path, n));
                    }
                }
            }
            ("TRACK", Some("NODE")) => {
                if let (Some(n), Some(key)) = (nodes.last_mut(), tag.attr("Key")) {
                    n.keys.push((key.to_string(), tag.line));
                }
            }
            _ => {}
        }
        if tag.kind == TagKind::Start {
            stack.push(tag.name);
        }
    }
    if !saw_root {
        return Err(Error::NotRekordboxXml);
    }

    // Playlist entries are the collection's, at the playlist's line; keys the collection lacks
    // stay as entries (with only a path, for location keys) so they're reported missing.
    let playlists = lists
        .into_iter()
        .map(|(path, node)| {
            let entries = node
                .keys
                .into_iter()
                .map(|(key, line)| {
                    let found = if node.by_location { by_location.get(&key) } else { by_id.get(&key) };
                    match found {
                        Some(&i) => Entry { line, ..collection[i].entry.clone() },
                        None if node.by_location => {
                            Entry { line, raw: format!("Key=\"{key}\""), ..location_entry(&key) }
                        }
                        None => Entry { line, raw: format!("Key=\"{key}\""), ..Default::default() },
                    }
                })
                .collect();
            (path, entries)
        })
        .collect();
    for t in &mut collection {
        t.tempos.sort_by(|a, b| a.start_ms.total_cmp(&b.start_ms));
    }
    Ok(XmlLibrary { tracks: collection, playlists, warnings: skipped.warnings() })
}

#[cfg(test)]
mod tests;
