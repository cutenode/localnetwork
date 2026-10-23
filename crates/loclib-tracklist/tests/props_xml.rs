//! Properties of reading rekordbox XML.

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use loclib_tracklist::{Entry, Error, Format, MAX_BEATS, Mark, Tempo, XmlTrack};

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
    let cut = tc.draw(gs::sampled_from(&ends));
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

/// One collection track as locallibrary's XML writer writes one: its tags, rating, key and
/// tempo, a TEMPO at each tempo change and its cues, with `{:.3}` seconds and `{:.2}` BPMs.
/// What it should read as, but for its entry.
#[hegel::test_helper]
fn library_track(tc: &TestCase, doc: &mut Doc) -> XmlTrack {
    let tags: Vec<String> = (0..6).map(|_| tc.draw(text())).collect();
    let (year, stars) = (tc.draw(gs::integers::<u16>()), tc.draw(gs::integers::<u8>().max_value(5)));
    let hundredths = tc.draw(gs::integers::<u16>());
    let mut attrs: Vec<(&str, String)> = vec![("TrackID", "1".into()), ("Name", "Glue".into())];
    let names = ["Genre", "Label", "Remixer", "Composer", "Comments", "Tonality"];
    attrs.extend(names.into_iter().zip(tags.iter().cloned()));
    attrs.push(("Year", year.to_string()));
    attrs.push(("Rating", [0, 51, 102, 153, 204, 255][usize::from(stars)].to_string()));
    attrs.push(("AverageBpm", format!("{:.2}", f64::from(hundredths) / 100.0)));
    let mut starts: Vec<i32> =
        tc.draw(gs::vecs(gs::integers::<i32>().min_value(-5_000).max_value(900_000)).max_size(4).unique(true));
    starts.sort();
    let tempos: Vec<Tempo> = starts
        .into_iter()
        .map(|start| {
            let beats_per_bar = tc.draw(gs::integers::<u8>().min_value(1).max_value(4));
            let beat = tc.draw(gs::integers::<u8>().min_value(1).max_value(beats_per_bar));
            let bpm = f64::from(tc.draw(gs::integers::<u16>().min_value(1))) / 100.0;
            Tempo { start_ms: f64::from(start), bpm, beats_per_bar, beat }
        })
        .collect();
    let marks: Vec<Mark> = (0..tc.draw(gs::integers::<usize>().max_value(4)))
        .map(|_| {
            let hot_cue = tc.draw(gs::optional(gs::integers::<u8>().max_value(7)));
            let start_ms = tc.draw(gs::integers::<u32>().max_value(86_000_000));
            let end_ms =
                tc.draw(gs::optional(gs::integers::<u32>().min_value(1).max_value(400_000))).map(|n| start_ms + n);
            let rgb = hot_cue.and(tc.draw(gs::optional(gs::vecs(gs::integers::<u8>()).min_size(3).max_size(3))));
            Mark { hot_cue, start_ms, end_ms, name: tc.draw(text()), rgb: rgb.map(|c| [c[0], c[1], c[2]]) }
        })
        .collect();
    doc.push("    ");
    doc.tag(tc, "TRACK", &attrs, false);
    doc.push("\n");
    for t in &tempos {
        let attrs = [
            ("Inizio", format!("{:.3}", t.start_ms / 1000.0)),
            ("Bpm", format!("{:.2}", t.bpm)),
            ("Metro", format!("{}/4", t.beats_per_bar)),
            ("Battito", t.beat.to_string()),
        ];
        doc.tag(tc, "TEMPO", &attrs, true);
        doc.push("\n");
    }
    for m in &marks {
        let kind = if m.end_ms.is_some() { "4" } else { "0" };
        let mut attrs = vec![("Name", m.name.clone()), ("Type", kind.to_string())];
        attrs.push(("Start", format!("{:.3}", f64::from(m.start_ms) / 1000.0)));
        attrs.extend(m.end_ms.map(|end| ("End", format!("{:.3}", f64::from(end) / 1000.0))));
        attrs.push(("Num", m.hot_cue.map_or(-1, i32::from).to_string()));
        if let Some([r, g, b]) = m.rgb {
            attrs.extend([("Red", r.to_string()), ("Green", g.to_string()), ("Blue", b.to_string())]);
        }
        doc.tag(tc, "POSITION_MARK", &attrs, true);
        doc.push("\n");
    }
    doc.push("    </TRACK>\n");
    let tag = |i: usize| tags[i].trim().to_string();
    XmlTrack {
        genre: tag(0),
        label: tag(1),
        remixer: tag(2),
        composer: tag(3),
        comment: tag(4),
        key: Some(tag(5)).filter(|k| !k.is_empty()),
        year,
        rating: stars,
        bpm: (hundredths > 0).then(|| f64::from(hundredths) / 100.0),
        tempos,
        marks,
        ..XmlTrack::default()
    }
}

/// A collection track in the shape locallibrary's XML writer gives it reads back with its tags,
/// rating, key and tempo, its grid's tempos and its cues as written, whatever the attribute
/// quoting, escaping and layout.
#[hegel::test(test_cases = 200)]
fn a_library_track_reads_back(tc: TestCase) {
    let mut doc = Doc("<DJ_PLAYLISTS Version=\"1.0.0\">\n  <COLLECTION Entries=\"1\">\n".into());
    let want = library_track(&tc, &mut doc);
    doc.push("  </COLLECTION>\n</DJ_PLAYLISTS>\n");
    tc.note(&doc.0);
    let read = loclib_tracklist::read_xml_library(doc.0.as_bytes()).unwrap();
    assert!(read.warnings.is_empty(), "{:?}", read.warnings);
    let got = &read.tracks[0];
    assert_eq!(*got, XmlTrack { entry: got.entry.clone(), ..want });
}

/// A number as a file might spell one: in range or not, not finite, or not a number at all.
fn number() -> impl hegel::PrintableGenerator<String> {
    hegel::one_of!(
        gs::floats::<f64>().map(|f| f.to_string()),
        gs::floats::<f64>().min_value(-100.0).max_value(1_000.0).map(|f| format!("{f:.3}")),
        gs::integers::<i64>().map(|n| n.to_string()),
        gs::sampled_from(vec!["NaN", "inf", "-inf", "1e308", "-0", "", "x", "4/4", "0/0", "255/1"]).map(String::from)
    )
}

/// Attributes, each left out, one of its values (at the edges of its range and past them) or
/// any [`number`].
#[hegel::test_helper]
fn any_attrs<'a>(tc: &TestCase, attrs: &[(&'a str, &[&str])]) -> Vec<(&'a str, String)> {
    let mut drawn = Vec::new();
    for (name, values) in attrs {
        let value = match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => continue,
            1 => tc.draw(number()),
            _ => tc.draw(gs::sampled_from(values.to_vec())).to_string(),
        };
        drawn.push((*name, value));
    }
    drawn
}

/// A TRACK's numbers, a TEMPO's and a POSITION_MARK's, with values at the edges of their ranges.
const TRACK_NUMBERS: &[(&str, &[&str])] = &[
    ("TotalTime", &["0", "3", "600", "36000"]),
    ("AverageBpm", &["0.00", "128", "655.35", "655.36", "700"]),
    ("Rating", &["0", "255", "256"]),
];
const TEMPO_NUMBERS: &[(&str, &[&str])] = &[
    ("Inizio", &["0", "-0.5", "1.000", "86400", "86400.001", "-86400.001"]),
    ("Bpm", &["120", "0.01", "0.009", "655.35", "655.36", "700"]),
    ("Metro", &["4/4", "3/4", "1/4", "5/4", "0/4"]),
    ("Battito", &["1", "3", "4", "5", "0"]),
];
const MARK_NUMBERS: &[(&str, &[&str])] = &[
    ("Type", &["0", "4", "1"]),
    ("Start", &["0", "1.5", "86400", "86400.001", "-0.001"]),
    ("End", &["2", "1.5", "86400", "86400.001"]),
    ("Num", &["-1", "0", "7", "8", "-2"]),
    ("Red", &["0", "255", "256"]),
    ("Green", &["0", "255"]),
    ("Blue", &["0", "255"]),
];

/// The number at the start of the warning about `what`, or 0 with none.
fn counted(warnings: &[String], what: &str) -> usize {
    let line = warnings.iter().find(|w| w.contains(what));
    line.map_or(0, |w| w.split(' ').next().unwrap().parse().unwrap())
}

/// Whatever numbers the file holds, reading it finishes with bounded output: each tempo, cue and
/// track tempo read is in range, the others are counted in its warnings, and a grid is at most
/// MAX_BEATS beats, in order, at or after 0 and before the track's end, each 1-4 in its bar.
/// Playlists nested too deep are left out, counted.
#[hegel::test(test_cases = 200)]
fn any_numbers_read_in_range_with_bounded_grids(tc: TestCase) {
    let mut doc = Doc("<DJ_PLAYLISTS>\n<COLLECTION>\n".into());
    let (mut tempos, mut marks) = (0, 0);
    for id in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        let mut attrs = vec![("TrackID", id.to_string())];
        attrs.extend(any_attrs(&tc, TRACK_NUMBERS));
        doc.tag(&tc, "TRACK", &attrs, false);
        for _ in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
            tempos += 1;
            doc.tag(&tc, "TEMPO", &any_attrs(&tc, TEMPO_NUMBERS), true);
        }
        for _ in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
            marks += 1;
            doc.tag(&tc, "POSITION_MARK", &any_attrs(&tc, MARK_NUMBERS), true);
        }
        doc.push("</TRACK>\n");
    }
    let depth = tc.draw(hegel::one_of!(
        gs::integers::<usize>().min_value(58).max_value(66),
        gs::integers::<usize>().max_value(200)
    ));
    doc.push("</COLLECTION>\n<PLAYLISTS>");
    doc.push(&"<NODE Type=\"0\" Name=\"f\">".repeat(depth));
    doc.push("<NODE Type=\"1\" Name=\"deep\"/>");
    doc.push(&"</NODE>".repeat(depth));
    doc.push("</PLAYLISTS>\n</DJ_PLAYLISTS>\n");
    tc.note(&doc.0);
    let read = loclib_tracklist::read_xml_library(doc.0.as_bytes()).unwrap();
    let w = &read.warnings;
    let kept = read.tracks.iter().fold((0, 0), |(t, m), x| (t + x.tempos.len(), m + x.marks.len()));
    assert_eq!((kept.0 + counted(w, "(TEMPO)"), kept.1 + counted(w, "(POSITION_MARK)")), (tempos, marks));
    for t in &read.tracks {
        assert!(t.bpm.is_none_or(|b| (0.01..=655.35).contains(&b)) && t.rating <= 5);
        for tempo in &t.tempos {
            assert!((0.01..=655.35).contains(&tempo.bpm) && tempo.start_ms.abs() <= 86_400_000.0, "{tempo:?}");
            assert!((1..=4).contains(&tempo.beats_per_bar) && (1..=tempo.beats_per_bar).contains(&tempo.beat));
        }
        for m in &t.marks {
            assert!(m.start_ms <= 86_400_000 && m.end_ms.is_none_or(|e| e > m.start_ms && e <= 86_400_000));
            assert!(m.hot_cue.is_none_or(|h| h <= 7));
        }
        let (beats, cut) = t.beats();
        let end = t.entry.duration_secs.map_or(0, |s| u64::from(s) * 1000);
        assert!(beats.len() <= MAX_BEATS && (!cut || beats.len() == MAX_BEATS));
        assert!(beats.windows(2).all(|w| w[0].time_ms < w[1].time_ms));
        let bar = t.tempos.iter().map(|x| u16::from(x.beats_per_bar)).max().unwrap_or(4);
        assert!(beats.iter().all(|b| u64::from(b.time_ms) < end && (1..=bar).contains(&b.beat_number)));
    }
    // DJ_PLAYLISTS and PLAYLISTS hold the folders: 62 of them leave room for the playlist.
    let lists: Vec<&str> = read.playlists.iter().map(|(p, _)| p.as_str()).collect();
    match depth <= 61 {
        true => assert_eq!((lists.len(), lists[0].matches('/').count()), (1, depth)),
        false => assert!(lists.is_empty() && counted(w, "nested") == 1, "{w:?}"),
    }
}
