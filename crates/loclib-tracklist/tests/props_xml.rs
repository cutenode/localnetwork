//! Properties of reading rekordbox XML.

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use loclib_tracklist::{Entry, Error, Format};

/// Characters a `file://` URL may leave as they are.
const URL_SAFE: &percent_encoding::AsciiSet =
    &percent_encoding::NON_ALPHANUMERIC.remove(b'/').remove(b'-').remove(b'.');

/// Attribute text, often heavy on what XML escapes: anything but CR (reading makes it a line
/// end) and NUL.
fn text() -> impl hegel::PrintableGenerator<String> {
    hegel::one_of!(gs::text().max_size(10), gs::text().alphabet("ab &<>\"';#x\n\t é").max_size(10))
        .map(|s| s.replace(['\r', '\0'], ""))
}

/// A name that also works as a file name "Artist - Title.mp3".
fn file_name() -> impl hegel::PrintableGenerator<String> {
    gs::from_regex("[A-Za-zé][A-Za-z0-9é&'.!()<>\"]{0,7}( [A-Za-z0-9é&'.!()]{1,8}){0,2}").fullmatch(true)
}

fn file_url(path: &str) -> String {
    format!("file://localhost{}", percent_encoding::utf8_percent_encode(path, URL_SAFE))
}

/// `v` escaped for an attribute quoted with `quote`, the ways XML writers do it: `<` and the
/// quote always, `&` unless nothing after it could be read as a reference, anything else
/// sometimes as a named or numeric character reference.
#[hegel::test_helper]
fn escaped(tc: &TestCase, v: &str, quote: char) -> String {
    let mut out = String::new();
    for (i, c) in v.char_indices() {
        let how = tc.draw(gs::integers::<u8>().max_value(4));
        match c {
            '&' if how == 0 && !v[i..].contains(';') => out.push('&'),
            '&' => out.push_str("&amp;"),
            '<' if how == 0 => out.push_str("&#60;"),
            '<' => out.push_str("&lt;"),
            '>' if how == 0 => out.push_str("&gt;"),
            '"' if quote == '"' || how == 0 => out.push_str("&quot;"),
            '\'' if quote == '\'' || how == 0 => out.push_str("&apos;"),
            c if how == 1 => out.push_str(&format!("&#{};", u32::from(c))),
            c if how == 2 => out.push_str(&format!("&#x{:X};", u32::from(c))),
            c => out.push(c),
        }
    }
    out
}

/// Playlists as `xml_playlists` reads them: folder path and entries.
type Playlists = Vec<(String, Vec<Entry>)>;

/// A document being written, which knows the line it's on.
struct Doc(String);

impl Doc {
    fn line(&self) -> usize {
        1 + self.0.matches('\n').count()
    }

    /// Writes `<name attrs...>` (or `/>` when `empty`) and returns its text.
    fn tag(&mut self, tc: &TestCase, name: &str, attrs: &[(&str, String)], empty: bool) -> String {
        let mut t = format!("<{name}");
        for (attr, value) in attrs {
            let quote = tc.draw(gs::sampled_from(vec!['"', '\'']));
            let space = tc.draw(gs::sampled_from(vec![" ", "\n      ", "  "]));
            let eq = tc.draw(gs::sampled_from(vec!["=", " = "]));
            t.push_str(&format!("{space}{attr}{eq}{quote}{}{quote}", escaped(tc, value, quote)));
        }
        t.push_str(if empty { "/>" } else { ">" });
        self.0.push_str(&t);
        t
    }

    fn push(&mut self, s: &str) {
        self.0.push_str(s);
    }
}

/// Something that isn't an element, which the reader skips.
#[hegel::test_helper]
fn aside(tc: &TestCase) -> String {
    let s = tc.draw(gs::sampled_from(vec![
        "",
        "<!-- <TRACK TrackID=\"99\" Name=\"commented out\"/> -->\n",
        "<![CDATA[ <TRACK TrackID=\"98\"/> ]]>\n",
        "<?rekordbox note?>\n",
        "<!DOCTYPE DJ_PLAYLISTS>\n",
        "text between tags\n",
    ]));
    s.to_string()
}

/// A track of the collection, and the entry it should read as.
#[derive(Debug, Clone)]
struct Track {
    id: u32,
    location: Option<String>,
    entry: Entry,
}

#[hegel::test_helper]
fn write_track(tc: &TestCase, doc: &mut Doc, id: u32) -> Track {
    let (artist, title) = (tc.draw(text()), tc.draw(text()));
    let album = tc.draw(gs::optional(text()));
    let mix = tc.draw(gs::optional(text()));
    let secs = tc.draw(gs::optional(gs::integers::<u32>().max_value(36_000)));
    // Each track in its own folder, so no two share a location.
    let path = tc.draw(gs::optional(file_name())).map(|n| format!("/Users/me/Music/{id}/{n}.mp3"));
    let location = path.as_deref().map(file_url);
    let mut attrs: Vec<(&str, String)> =
        vec![("TrackID", id.to_string()), ("Name", title.clone()), ("Artist", artist.clone())];
    attrs.extend(album.clone().map(|a| ("Album", a)));
    attrs.extend(mix.clone().map(|m| ("Mix", m)));
    attrs.extend(secs.map(|s| ("TotalTime", s.to_string())));
    attrs.extend(location.clone().map(|l| ("Location", l)));
    attrs.push(("Kind", "MP3 File".into()));
    attrs.push(("AverageBpm", "128.00".into()));
    let order: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>()).min_size(attrs.len()).max_size(attrs.len()));
    let mut keyed: Vec<(u32, (&str, String))> = order.into_iter().zip(attrs).collect();
    keyed.sort_by_key(|(k, _)| *k);
    let attrs: Vec<(&str, String)> = keyed.into_iter().map(|(_, a)| a).collect();

    doc.push("    ");
    let line = doc.line();
    let children = tc.draw(gs::booleans());
    let raw = doc.tag(tc, "TRACK", &attrs, !children);
    if children {
        doc.push("\n      <TEMPO Inizio=\"0.025\" Bpm=\"128.00\" Metro=\"4/4\" Battito=\"1\"/>");
        doc.push(
            "\n      <POSITION_MARK Name=\"\" Type=\"0\" Start=\"0.025\" Num=\"-1\"></POSITION_MARK>\n    </TRACK>",
        );
    }
    doc.push("\n");
    let some = |s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let entry = Entry {
        line,
        raw,
        title: title.trim().to_string(),
        artist: artist.trim().to_string(),
        album: some(album),
        mix: some(mix),
        duration_secs: secs.filter(|&s| s > 0),
        path,
        isrc: None,
    };
    Track { id, location, entry }
}

/// Writes the playlist tree below the current node, returning each playlist's folder path and
/// expected entries in document order. `path` is the names of the nodes above.
#[hegel::test_helper]
fn write_nodes(tc: &TestCase, doc: &mut Doc, tracks: &[Track], path: &[String], depth: usize) -> Playlists {
    let mut lists = Vec::new();
    let n = tc.draw(gs::integers::<usize>().max_value(3));
    for _ in 0..n {
        let name = tc.draw(text());
        let mut here = path.to_vec();
        here.push(name.clone());
        let indent = "  ".repeat(depth + 3);
        doc.push(&indent);
        if depth < 2 && tc.draw(gs::booleans()) {
            // A folder.
            let attrs = [("Type", "0".to_string()), ("Name", name)];
            if tc.draw(gs::booleans()) {
                doc.tag(tc, "NODE", &attrs, true);
                doc.push("\n");
            } else {
                doc.tag(tc, "NODE", &attrs, false);
                doc.push("\n");
                lists.extend(write_nodes(tc, doc, tracks, &here, depth + 1));
                doc.push(&format!("{indent}</NODE>\n"));
            }
            continue;
        }
        let by_location = tc.draw(gs::booleans());
        let attrs = [("Name", name), ("Type", "1".to_string()), ("KeyType", u8::from(by_location).to_string())];
        let keys = tc.draw(gs::integers::<usize>().max_value(5));
        let mut entries = Vec::new();
        if keys == 0 && tc.draw(gs::booleans()) {
            doc.tag(tc, "NODE", &attrs, true);
            doc.push("\n");
        } else {
            doc.tag(tc, "NODE", &attrs, false);
            doc.push("\n");
            for _ in 0..keys {
                let pick = tc.draw(gs::integers::<usize>().max_value(tracks.len()));
                let found = tracks.get(pick).filter(|t| !by_location || t.location.is_some());
                let key = match (found, by_location) {
                    (Some(t), false) => t.id.to_string(),
                    (Some(t), true) => t.location.clone().unwrap_or_default(),
                    (None, false) => format!("{}", 100_000 + pick),
                    (None, true) => {
                        let (a, t) = (tc.draw(file_name()), tc.draw(file_name()));
                        file_url(&format!("/gone/{a} - {t}.mp3"))
                    }
                };
                doc.push(&format!("{indent}  "));
                let line = doc.line();
                doc.tag(tc, "TRACK", &[("Key", key.clone())], true);
                doc.push("\n");
                entries.push(match found {
                    Some(t) => Entry { line, ..t.entry.clone() },
                    None if by_location => {
                        // Only the file is known: its name says what it is.
                        let file = key.rsplit('/').next().unwrap_or_default();
                        let file = percent_encoding::percent_decode_str(file).decode_utf8_lossy();
                        let (artist, title) = file.strip_suffix(".mp3").unwrap().split_once(" - ").unwrap();
                        let path =
                            percent_encoding::percent_decode_str(&key["file://localhost".len()..]).decode_utf8_lossy();
                        Entry {
                            line,
                            raw: format!("Key=\"{key}\""),
                            title: title.to_string(),
                            artist: artist.to_string(),
                            path: Some(path.into_owned()),
                            ..Default::default()
                        }
                    }
                    None => Entry { line, raw: format!("Key=\"{key}\""), ..Default::default() },
                });
            }
            doc.push(&format!("{indent}</NODE>\n"));
        }
        // rekordbox's ROOT folder isn't part of the path.
        let shown = if here.first().is_some_and(|n| n == "ROOT") { &here[1..] } else { &here[..] };
        lists.push((shown.join("/"), entries));
    }
    lists
}

/// A rekordbox XML file as rekordbox and other writers produce it, with its collection and a
/// playlist tree.
#[hegel::test_helper]
fn library_xml(tc: &TestCase) -> (String, Vec<Track>, Playlists) {
    let mut doc = Doc(String::new());
    let prolog = tc.draw(gs::booleans());
    if prolog {
        doc.push("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        doc.push(&aside(tc));
    }
    doc.push(
        "<DJ_PLAYLISTS Version=\"1.0.0\">\n  <PRODUCT Name=\"rekordbox\" Version=\"6.8.5\" Company=\"AlphaTheta\"/>\n",
    );
    let ids: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().max_value(99_999)).max_size(6).unique(true));
    doc.push(&format!("  <COLLECTION Entries=\"{}\">\n", ids.len()));
    let mut tracks = Vec::new();
    for id in ids {
        doc.push(&aside(tc));
        tracks.push(write_track(tc, &mut doc, id));
    }
    doc.push("  </COLLECTION>\n  <PLAYLISTS>\n");
    let root = tc.draw(gs::booleans());
    if root {
        doc.push("    <NODE Type=\"0\" Name=\"ROOT\" Count=\"1\">\n");
    }
    let above = if root { vec!["ROOT".to_string()] } else { vec![] };
    let lists = write_nodes(tc, &mut doc, &tracks, &above, usize::from(root));
    if root {
        doc.push("    </NODE>\n");
    }
    doc.push("  </PLAYLISTS>\n</DJ_PLAYLISTS>\n");
    (doc.0, tracks, lists)
}

/// A rekordbox XML file reads back as its collection (every attribute decoded, on the track's
/// line, with its tag as the raw text), whatever the attribute order, quoting, escaping and
/// layout, comments and other markup around it. Its playlists read back with their folder
/// paths and tracks in order, by TrackID or by location, and the keys the collection lacks are
/// kept as entries to report missing. It's detected as rekordbox XML whatever it's called.
#[hegel::test(test_cases = 200)]
fn rekordbox_xml_reads_back(tc: TestCase) {
    let (xml, tracks, lists) = library_xml(&tc);
    tc.note(&xml);
    let bytes = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => xml.clone().into_bytes(),
        1 => [&[0xEF, 0xBB, 0xBF][..], xml.as_bytes()].concat(),
        _ => [0xFF, 0xFE].into_iter().chain(xml.encode_utf16().flat_map(u16::to_le_bytes)).collect(),
    };
    let hint = tc.draw(gs::sampled_from(vec![None, Some("rekordbox.xml"), Some("export.txt")]));
    assert_eq!(loclib_tracklist::detect(&bytes, hint), Format::RekordboxXml);
    let want: Vec<Entry> = tracks.iter().map(|t| t.entry.clone()).collect();
    assert_eq!(loclib_tracklist::parse(&bytes, hint).unwrap(), want);
    assert_eq!(loclib_tracklist::xml_playlists(&bytes).unwrap(), lists);
}

/// An XML file cut off anywhere reads as the tracks whose tags it still holds whole, or fails
/// naming the line of the tag it cuts through, or (cut before the root element) isn't
/// rekordbox XML at all.
#[hegel::test(test_cases = 100)]
fn truncated_xml_reads_a_prefix_or_fails_cleanly(tc: TestCase) {
    let (xml, tracks, _) = library_xml(&tc);
    let ends: Vec<usize> = xml.char_indices().map(|(i, _)| i).chain([xml.len()]).collect();
    let cut = ends[tc.draw(gs::integers::<usize>().max_value(ends.len() - 1))];
    let prefix = &xml[..cut];
    tc.note(prefix);
    match loclib_tracklist::parse_as(prefix.as_bytes(), Format::RekordboxXml) {
        Ok(entries) => {
            let whole = tracks.iter().filter(|t| xml[..cut].contains(&t.entry.raw)).count();
            let want: Vec<Entry> = tracks[..whole].iter().map(|t| t.entry.clone()).collect();
            assert_eq!(entries, want);
        }
        Err(Error::UnterminatedTag { line }) => {
            let start = prefix.rfind('<').unwrap();
            assert_eq!(line, 1 + prefix[..start].matches('\n').count());
        }
        Err(Error::NotRekordboxXml) => {
            let root = prefix.find("<DJ_PLAYLISTS");
            assert!(root.is_none_or(|i| !prefix[i..].contains('>')), "{prefix}");
        }
        Err(e) => panic!("{e}"),
    }
}

/// A tag with an attribute whose value isn't quoted is refused, naming the attribute and the
/// tag's line, whatever quoted attributes come before it.
#[hegel::test(test_cases = 100)]
fn unquoted_attribute_values_are_refused(tc: TestCase) {
    let (xml, _, _) = library_xml(&tc);
    let at = xml.find("</COLLECTION>\n").unwrap() + "</COLLECTION>\n".len();
    let quoted: Vec<String> = tc.draw(gs::vecs(text()).max_size(3));
    let unquoted = tc.draw(gs::from_regex("[A-Za-z][A-Za-z_]{0,7}").fullmatch(true));
    let value = tc.draw(gs::from_regex("[0-9A-Za-z.#&]{1,8}").fullmatch(true));
    let mut tag = String::from("  <EXTRA");
    for (i, v) in quoted.iter().enumerate() {
        tag += &format!(" q{i}=\"{}\"", escaped(&tc, v, '"'));
    }
    tag += &format!(" {unquoted}={}{value}/>\n", if tc.draw(gs::booleans()) { " " } else { "" });
    let doc = format!("{}{tag}{}", &xml[..at], &xml[at..]);
    // The line the tag starts on (a quoted value may hold line ends of its own).
    let line = 1 + xml[..at].matches('\n').count();
    match loclib_tracklist::parse_as(doc.as_bytes(), Format::RekordboxXml) {
        Err(Error::UnquotedAttribute { attr, line: l }) => assert_eq!((attr, l), (unquoted, line)),
        other => panic!("{other:?}"),
    }
}

/// A stray `/` or a valueless attribute (neither of them XML) in a track's tag doesn't stop it
/// reading: it reads as it did without, its raw text aside.
#[hegel::test(test_cases = 100)]
fn stray_markup_in_a_tag_is_skipped(tc: TestCase) {
    let (xml, tracks, _) = library_xml(&tc);
    tc.assume(!tracks.is_empty());
    let i = tc.draw(gs::integers::<usize>().max_value(tracks.len() - 1));
    let raw = &tracks[i].entry.raw;
    let stray = tc.draw(gs::sampled_from(vec![" /", " / ", " Bare", " Bare "]));
    let messy = format!("<TRACK{stray}{}", &raw["<TRACK".len()..]);
    let at = xml.find(raw.as_str()).unwrap();
    let doc = format!("{}{messy}{}", &xml[..at], &xml[at + raw.len()..]);
    let mut want: Vec<Entry> = tracks.iter().map(|t| t.entry.clone()).collect();
    want[i].raw = messy;
    assert_eq!(loclib_tracklist::parse_as(doc.as_bytes(), Format::RekordboxXml).unwrap(), want);
}
