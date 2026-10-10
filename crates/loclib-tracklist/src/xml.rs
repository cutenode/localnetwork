//! rekordbox XML (`DJ_PLAYLISTS`).

use rustc_hash::FxHashMap;

use crate::detection::file_path;
use crate::m3u::location_entry;
use crate::table::parse_duration;
use crate::{Entry, Error, Result};

pub(crate) struct XmlDoc {
    pub collection: Vec<Entry>,
    pub playlists: Vec<(String, Vec<Entry>)>,
}

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

pub(crate) fn parse_xml(text: &str) -> Result<XmlDoc> {
    let mut scanner = Scanner::new(text);
    let mut collection: Vec<Entry> = Vec::new();
    let mut by_id: FxHashMap<String, usize> = FxHashMap::default();
    let mut by_location: FxHashMap<String, usize> = FxHashMap::default();
    let mut stack: Vec<&str> = Vec::new();
    let mut nodes: Vec<XmlNode> = Vec::new();
    let mut lists: Vec<(String, XmlNode)> = Vec::new();
    let mut saw_root = false;

    // Folder path of the innermost node, without rekordbox's ROOT.
    let node_path = |nodes: &[XmlNode]| {
        let skip = usize::from(nodes.first().is_some_and(|n| n.name == "ROOT"));
        nodes[skip..].iter().map(|n| n.name.as_str()).collect::<Vec<_>>().join("/")
    };

    while let Some(tag) = scanner.next_tag()? {
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
                collection.push(e);
            }
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
                        Some(&i) => Entry { line, ..collection[i].clone() },
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
    Ok(XmlDoc { collection, playlists })
}

#[cfg(test)]
mod tests {
    use crate::testing::{at, pairs};
    use crate::{parse, xml_playlists};

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <PRODUCT Name="rekordbox" Version="6.8.5" Company="AlphaTheta"/>
  <!-- <TRACK TrackID="99" Name="commented out"/> -->
  <COLLECTION Entries="3">
    <TRACK TrackID="1" Name="Rock &amp; Roll &#x2013; Part &#50;" Artist="Led Zeppelin" Album="IV" TotalTime="220"
      Location="file://localhost/Users/me/Music/Rock%20&amp;%20Roll.mp3" Mix="">
      <TEMPO Inizio="0.025" Bpm="170.00" Metro="4/4" Battito="1"/>
      <POSITION_MARK Name="" Type="0" Start="0.025" Num="-1"/>
    </TRACK>
    <TRACK TrackID="2" Name='Opus' Artist="Eric Prydz" TotalTime="543" Mix="Original Mix" Location="file://localhost/C:/Music/opus.mp3"/>
    <TRACK TrackID="3" Name="Strobe" Artist="deadmau5" TotalTime="637" Location="file://localhost/Volumes/USB/Contents/deadmau5/strobe.mp3"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="2">
      <NODE Type="0" Name="Shows" Count="2">
        <NODE Name="Warmup" Type="1" KeyType="0" Entries="3">
          <TRACK Key="3"/>
          <TRACK Key="1"/>
          <TRACK Key="42"/>
        </NODE>
        <NODE Name="Empty" Type="1" KeyType="0" Entries="0"/>
      </NODE>
      <NODE Name="By location" Type="1" KeyType="1" Entries="2">
        <TRACK Key="file://localhost/C:/Music/opus.mp3"/>
        <TRACK Key="file://localhost/gone/Artist%20-%20Gone.mp3"/>
      </NODE>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>
"#;

    #[test]
    fn xml_collection() {
        let e = parse(XML.as_bytes(), None).unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].title, "Rock & Roll – Part 2");
        assert_eq!(e[0].path.as_deref(), Some("/Users/me/Music/Rock & Roll.mp3"));
        assert_eq!(
            (e[0].duration_secs, e[0].line, e[0].album.as_deref(), e[0].mix.as_deref()),
            (Some(220), 6, Some("IV"), None)
        );
        assert!(e[0].raw.starts_with("<TRACK TrackID=\"1\"") && e[0].raw.ends_with('>'));
        assert_eq!((e[1].title.as_str(), e[1].mix.as_deref()), ("Opus", Some("Original Mix")));
        assert_eq!(e[1].path.as_deref(), Some("C:/Music/opus.mp3"));
        assert!(parse(b"<?xml version=\"1.0\"?><plist/>", None).is_err());
        assert!(parse(b"<?xml version=\"1.0\"?><DJ_PLAYLISTS><COLLECTION><TRACK Name=\"x", None).is_err());
    }

    #[test]
    fn xml_playlists_by_path() {
        let lists = xml_playlists(XML.as_bytes()).unwrap();
        let names: Vec<&str> = lists.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(names, ["Shows/Warmup", "Shows/Empty", "By location"]);
        let warmup = &lists[0].1;
        assert_eq!(at(warmup), pairs(&[("deadmau5", "Strobe"), ("Led Zeppelin", "Rock & Roll – Part 2"), ("", "")]));
        assert_eq!(warmup[0].line, 18);
        assert_eq!(warmup[2].raw, "Key=\"42\"");
        assert!(lists[1].1.is_empty());
        let by_loc = &lists[2].1;
        assert_eq!(by_loc[0].title, "Opus");
        assert_eq!((by_loc[1].artist.as_str(), by_loc[1].title.as_str()), ("Artist", "Gone"));
        assert_eq!(by_loc[1].path.as_deref(), Some("/gone/Artist - Gone.mp3"));
    }
}
