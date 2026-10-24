//! Properties of reading tracklists.

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use loclib_tracklist::{Entry, Format};

const HINTS: &[Option<&str>] = &[None, Some("list.m3u8"), Some("list.CSV"), Some("rekordbox.xml"), Some("set.txt")];
const FORMATS: [Format; 5] = [Format::M3u, Format::Csv, Format::RekordboxText, Format::RekordboxXml, Format::Text];

/// Fragments that steer the parsers into their less travelled paths.
const FRAGMENTS: &[&str] = &[
    "#EXTM3U",
    "#EXTINF:",
    "#EXTINF:-1 a=\"x,y\",",
    "#EXTALB:",
    "<?xml ?>",
    "<DJ_PLAYLISTS>",
    "<COLLECTION>",
    "<TRACK ",
    "Name=\"",
    "Location=\"file://localhost/",
    "/>",
    "<!--",
    "&#x",
    "&amp;",
    "<PLAYLISTS><NODE Type=\"1\" ",
    "Key=\"",
    "#,Track Title\tArtist",
    "Title,Artist,Duration\n",
    "sep=;",
    "\"",
    "\t",
    "01. ",
    "[1:02:03] ",
    "(5:23)",
    " - ",
    " – ",
    " by ",
    "file:///C:/x.mp3",
    "C:\\Music\\",
    "\u{a0}",
    "\u{3000}",
    "\u{feff}",
    "•",
    "12:34",
    "Disk:Folder:",
];

fn messy_line() -> impl hegel::PrintableGenerator<String> {
    gs::vecs(hegel::one_of!(
        gs::sampled_from(FRAGMENTS.to_vec()).map(String::from),
        gs::text().max_size(6),
        gs::text().alphabet("0123456789:.-()[]# \u{a0}\u{2003}aé,;\t\"").max_size(8),
    ))
    .max_size(6)
    .map(|parts| parts.concat())
}

/// Reading never panics, whatever the bytes, file name or format.
#[hegel::test(test_cases = 500)]
#[hegel::explicit_test_case(bytes = "a\u{a0}b".as_bytes().to_vec(), hint = None::<&str>)]
#[hegel::explicit_test_case(bytes = "x\u{3000}1:23".as_bytes().to_vec(), hint = Some("x.txt"))]
fn parse_never_panics_on_bytes(tc: TestCase) {
    let bytes = tc.draw(hegel::one_of!(
        gs::binary().max_size(200),
        gs::vecs(messy_line()).max_size(6).map(|l| l.join("\n").into_bytes()),
        gs::vecs(messy_line()).max_size(4).map(|l| l
            .join("\r\n")
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<u8>>()),
    ));
    let hint = tc.draw(gs::sampled_from(HINTS.to_vec()));
    let _ = loclib_tracklist::parse(&bytes, hint);
    let _ = loclib_tracklist::detect(&bytes, hint);
    let _ = loclib_tracklist::read_xml_library(&bytes);
    for f in FORMATS {
        let _ = loclib_tracklist::parse_as(&bytes, f);
    }
}

/// `messy_line` as a comment's text: on one line, without control characters or tabs (a tab in
/// the first line can make it a table header).
fn comment_text() -> impl hegel::PrintableGenerator<String> {
    messy_line().map(|s| s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect())
}

/// An artist or title a plain list can hold unambiguously: it starts with a letter and has no
/// dashes, colons (they'd read as cue times), path separators or a lone "by".
fn plain_name() -> impl hegel::PrintableGenerator<String> {
    gs::from_regex("[A-Za-zé][A-Za-z0-9é&'.,!()]{0,7}( [A-Za-z0-9é&'.,!()]{1,8}){0,2}")
        .fullmatch(true)
        .filter(|s: &String| !s.split(' ').any(|w| w.eq_ignore_ascii_case("by")))
}

/// `secs` as lists write a time: "m:ss", "mm:ss" or "h:mm:ss".
#[hegel::test_helper]
fn clock(tc: &TestCase, secs: u32) -> String {
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => format!("{}:{:02}", secs / 60, secs % 60),
        1 => format!("{:02}:{:02}", secs / 60, secs % 60),
        _ => format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60),
    }
}

/// How a list numbers its `i`th line. Style 8, a bare number, only reads as numbering when the
/// whole list has one.
fn numbering(style: u8, i: usize) -> String {
    match style {
        0 => String::new(),
        1 => format!("{i:02}. "),
        2 => format!("{i}) "),
        3 => format!("#{i} "),
        4 => format!("[{i:02}] "),
        5 => format!("{i:03} "),
        6 => format!("{i} - "),
        7 => format!("({i}) "),
        _ => format!("{i} "),
    }
}

/// `lines` as bytes in an encoding tracklists come in, with `\n` or `\r\n` line ends.
#[hegel::test_helper]
fn encoded(tc: &TestCase, lines: &[String]) -> Vec<u8> {
    let eol = tc.draw(gs::sampled_from(vec!["\n", "\r\n"]));
    let mut text = lines.join(eol);
    if tc.draw(gs::booleans()) {
        text.push_str(eol);
    }
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => text.into_bytes(),
        1 => [&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat(),
        _ => [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect(),
    }
}

/// A pasted set list reads back as exactly the artists, titles and lengths it lists, on their
/// lines, whatever the numbering, cue times, bullets, separators, comments and headings around
/// them.
#[hegel::test(test_cases = 300)]
fn plain_lists_read_back(tc: TestCase) {
    let tracks: Vec<(String, String)> =
        tc.draw(gs::vecs(gs::tuples2(plain_name(), plain_name())).min_size(1).max_size(8));
    let style = tc.draw(gs::integers::<u8>().max_value(8));
    // A bare number before every line, and nothing else that isn't a comment.
    let bare = style == 8 && tracks.len() >= 2;
    let style = if style == 8 && !bare { 0 } else { style };
    let mut lines: Vec<String> = Vec::new();
    let mut want = Vec::new();
    for (i, (artist, title)) in tracks.iter().enumerate() {
        match tc.draw(gs::integers::<u8>().max_value(if bare { 4 } else { 7 })) {
            0 => {}
            1 => lines.push(String::new()),
            2 => lines.push(format!("// {}", tc.draw(comment_text()))),
            3 => lines.push(format!("; {}", tc.draw(comment_text()))),
            4 => lines.push(format!("# {}", tc.draw(comment_text()))),
            5 => lines.push("Tracklist:".into()),
            6 => lines.push("-----".into()),
            _ => lines.push("* * *".into()),
        }
        let bullet =
            if bare { "" } else { tc.draw(gs::sampled_from(vec!["", "• ", "- ", "* ", "\u{25ba} ", "\u{2192}"])) };
        let cue = match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 => String::new(),
            1 => format!("[{}] ", clock(&tc, tc.draw(gs::integers::<u32>().max_value(7200)))),
            2 => format!("({}) ", clock(&tc, tc.draw(gs::integers::<u32>().max_value(7200)))),
            3 => format!("{} ", clock(&tc, tc.draw(gs::integers::<u32>().max_value(7200)))),
            _ => format!("{} - ", clock(&tc, tc.draw(gs::integers::<u32>().max_value(7200)))),
        };
        let (before, after) =
            if !bare && tc.draw(gs::booleans()) { (cue, String::new()) } else { (String::new(), cue) };
        // "1 - Title by Artist" reads the number as the artist.
        let sep = tc.draw(gs::integers::<u8>().max_value(if style == 6 { 4 } else { 5 }));
        let text = match sep {
            0 => format!("{artist} - {title}"),
            1 => format!("{artist} \u{2013} {title}"),
            2 => format!("{artist} \u{2014} {title}"),
            3 => format!("{artist} -- {title}"),
            4 => format!("{artist}\u{2013}{title}"),
            _ => format!("{title} by {artist}"),
        };
        let length = tc.draw(gs::optional(gs::integers::<u32>().max_value(7200)));
        let (mut artist, mut title) = (artist.clone(), title.clone());
        let tail = match length {
            // Not a time (no minute has 75 seconds), so part of whatever ends the line.
            None if tc.draw(gs::weighted_booleans(0.2)) => {
                let fake = format!(
                    " ({}:{})",
                    tc.draw(gs::integers::<u8>().max_value(99)),
                    tc.draw(gs::integers::<u8>().min_value(60).max_value(99))
                );
                if sep == 5 {
                    artist.push_str(&fake)
                } else {
                    title.push_str(&fake)
                }
                fake
            }
            None => String::new(),
            Some(secs) => {
                let c = clock(&tc, secs);
                match tc.draw(gs::integers::<u8>().max_value(4)) {
                    0 => format!(" ({c})"),
                    1 => format!(" [{c}]"),
                    2 => format!(" {c}"),
                    3 => format!(" - {c}"),
                    _ => format!("\u{a0}{c}"),
                }
            }
        };
        let indent = tc.draw(gs::sampled_from(vec!["", "  ", "\t"]));
        let line = format!("{bullet}{before}{}{after}{text}{tail}", numbering(style, i + 1));
        lines.push(format!("{indent}{line}  "));
        want.push(Entry { line: lines.len(), raw: line, title, artist, duration_secs: length, ..Default::default() });
    }
    let bytes = encoded(&tc, &lines);
    assert_eq!(loclib_tracklist::parse_as(&bytes, Format::Text).unwrap(), want);
}

/// Lines of "Artist - Title" read as text with or without a `.txt` hint, unless most of the
/// lines are audio file paths: then the list is an M3U, and either way each path line keeps
/// its path.
#[hegel::test(test_cases = 200)]
fn plain_lists_and_path_lists_are_told_apart(tc: TestCase) {
    let name = || gs::from_regex("[A-Za-z][A-Za-z0-9é&' ]{0,10}[A-Za-z0-9]").fullmatch(true);
    let lines: Vec<(bool, String)> = tc.draw(
        gs::vecs(hegel::one_of!(
            gs::tuples2(name(), name()).map(|(a, t)| (false, format!("{a} - {t}"))),
            gs::tuples3(name(), name(), gs::sampled_from(vec!["mp3", "FLAC", "aiff", "m4a", "wav"]))
                .map(|(d, f, e)| (true, format!("/Users/me/Music/{d}/{f}.{e}"))),
            gs::tuples2(name(), name()).map(|(d, f)| (true, format!("C:\\Music\\{d}\\{f}.mp3"))),
        ))
        .min_size(1)
        .max_size(10),
    );
    let hint = tc.draw(gs::sampled_from(vec![None, Some("set.txt"), Some("Set List")]));
    let text: Vec<String> = lines.iter().map(|(_, l)| l.clone()).collect();
    let bytes = text.join("\n").into_bytes();
    let paths = lines.iter().filter(|(p, _)| *p).count();
    let format = loclib_tracklist::detect(&bytes, hint);
    // "Most" is three in five.
    assert_eq!(format, if paths * 5 >= lines.len() * 3 { Format::M3u } else { Format::Text });
    let entries = loclib_tracklist::parse(&bytes, hint).unwrap();
    assert_eq!(entries.len(), lines.len());
    for (e, (path, line)) in entries.iter().zip(&lines) {
        if *path {
            assert_eq!(e.path.as_deref(), Some(line.as_str()));
        } else if format == Format::Text {
            assert_eq!(&format!("{} - {}", e.artist, e.title), line);
        }
    }
}

/// A line whose dash has nothing after it ("Title–", an unspaced en or em dash at the end) has
/// no artist to split off: it's all title, dash and all.
#[hegel::test(test_cases = 100)]
fn a_dash_with_nothing_after_it_is_part_of_the_title(tc: TestCase) {
    let names: Vec<String> = tc.draw(gs::vecs(plain_name()).min_size(1).max_size(5));
    let lines: Vec<String> =
        names.iter().map(|n| format!("{n}{}", tc.draw(gs::sampled_from(vec!['\u{2013}', '\u{2014}'])))).collect();
    let entries = loclib_tracklist::parse_as(lines.join("\n").as_bytes(), Format::Text).unwrap();
    let read: Vec<(&str, &str)> = entries.iter().map(|e| (e.artist.as_str(), e.title.as_str())).collect();
    let want: Vec<(&str, &str)> = lines.iter().map(|l| ("", l.as_str())).collect();
    assert_eq!(read, want);
}

/// An `#EXTINF` with only a length (no comma, so no display name) gives its file's entry that
/// length, and the file name its artist and title.
#[hegel::test(test_cases = 100)]
fn extinf_lengths_without_a_name_keep_the_file_name(tc: TestCase) {
    let (artist, title) = (tc.draw(plain_name()), tc.draw(plain_name()));
    let secs = tc.draw(gs::integers::<u32>().min_value(1).max_value(36_000));
    let file = format!("/Music/{artist} - {title}.mp3");
    let m3u = format!("#EXTM3U\n#EXTINF:{secs}\n{file}\n");
    let entries = loclib_tracklist::parse_as(m3u.as_bytes(), Format::M3u).unwrap();
    let read: Vec<(&str, &str, Option<u32>, Option<&str>)> =
        entries.iter().map(|e| (e.artist.as_str(), e.title.as_str(), e.duration_secs, e.path.as_deref())).collect();
    assert_eq!(read, [(artist.as_str(), title.as_str(), Some(secs), Some(file.as_str()))]);
}

/// Characters a `file://` URL may leave as they are.
const URL_SAFE: &percent_encoding::AsciiSet =
    &percent_encoding::NON_ALPHANUMERIC.remove(b'/').remove(b'-').remove(b'.');

/// A file the way playlists name it: POSIX, Windows or relative paths, or `file://` URLs
/// (percent-encoded, with or without `localhost`, with a drive letter or not). Returns (as
/// written, the path it names).
#[hegel::test_helper]
fn location(tc: &TestCase, dirs: &[String], file: &str) -> (String, String) {
    let mut parts = dirs.to_vec();
    parts.push(file.to_string());
    let posix = format!("/{}", parts.join("/"));
    let enc = |p: &str| percent_encoding::utf8_percent_encode(p, URL_SAFE).to_string();
    match tc.draw(gs::integers::<u8>().max_value(6)) {
        0 => (posix.clone(), posix),
        1 => {
            let p = format!("C:{}", posix.replace('/', "\\"));
            (p.clone(), p)
        }
        2 => (format!("file://{}", enc(&posix)), posix),
        3 => (format!("file://localhost{}", enc(&posix)), posix),
        4 => (format!("file:///C:{}", enc(&posix)), format!("C:{posix}")),
        5 => (format!("FILE://LOCALHOST/D:{}", enc(&posix)), format!("D:{posix}")),
        _ => (format!("..{posix}"), format!("..{posix}")),
    }
}

/// An M3U playlist reads back as the files it lists, on their lines, with each one's artist
/// and title from its `#EXTINF` (or, without one, its file name), its length, and the album
/// and album artist of `#EXTALB` and `#EXTART`. It's detected as M3U.
#[hegel::test(test_cases = 300)]
fn m3u_playlists_read_back(tc: TestCase) {
    let tracks: Vec<(String, String)> =
        tc.draw(gs::vecs(gs::tuples2(plain_name(), plain_name())).min_size(1).max_size(6));
    let header = tc.draw(gs::booleans());
    let album = tc.draw(gs::optional(plain_name()));
    let album_artist = tc.draw(gs::optional(plain_name()));
    let mut lines: Vec<String> = Vec::new();
    if header {
        lines.push("#EXTM3U".into());
    }
    if let Some(a) = &album {
        lines.push(format!("#EXTALB:{a}"));
    }
    if let Some(a) = &album_artist {
        lines.push(format!("#extart: {a}"));
    }
    let mut want = Vec::new();
    let mut extinf = false;
    for (artist, title) in &tracks {
        match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => lines.push(String::new()),
            1 => lines.push(format!("# {}", tc.draw(comment_text()))),
            _ => {}
        }
        // (artist, title) as the entry should read: from the #EXTINF, else the file name.
        let mut got = (artist.clone(), title.clone());
        let mut duration_secs = None;
        if tc.draw(gs::booleans()) {
            extinf = true;
            let secs = tc.draw(gs::integers::<u32>().max_value(7200));
            // Lengths under a second (and -1, unknown) aren't lengths.
            let head = match tc.draw(gs::integers::<u8>().max_value(3)) {
                0 => secs.to_string(),
                1 => format!("{secs}.4"),
                2 => format!("-1 tvg-name=\"{artist},{title}\" group-title=\"a,b\""),
                _ => "0".into(),
            };
            duration_secs = Some(secs).filter(|&s| s >= 1 && !head.starts_with(['-', '0']));
            let display = match tc.draw(gs::integers::<u8>().max_value(2)) {
                0 => format!("{artist} - {title}"),
                1 => {
                    got = (album_artist.clone().unwrap_or_default(), title.clone());
                    title.clone()
                }
                _ => String::new(),
            };
            lines.push(format!("#EXTINF:{head},{display}"));
        }
        let number = match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => String::new(),
            1 => format!("{:02} ", tc.draw(gs::integers::<u8>().min_value(1).max_value(9))),
            _ => format!("{:02} - ", tc.draw(gs::integers::<u8>().max_value(99))),
        };
        let ext = tc.draw(gs::sampled_from(vec!["mp3", "flac", "wav", "aiff", "m4a"]));
        let dirs: Vec<String> = tc.draw(gs::vecs(plain_name()).max_size(3));
        let (written, path) = location(&tc, &dirs, &format!("{number}{artist} - {title}.{ext}"));
        lines.push(written.clone());
        want.push(Entry {
            line: lines.len(),
            raw: written,
            title: got.1,
            artist: got.0,
            album: album.clone(),
            duration_secs,
            path: Some(path),
            ..Default::default()
        });
    }
    let bytes = encoded(&tc, &lines);
    assert_eq!(loclib_tracklist::parse_as(&bytes, Format::M3u).unwrap(), want);
    // Without "#EXTM3U" or "#EXTINF", the extension tells.
    let hint = if header || extinf { tc.draw(gs::sampled_from(HINTS.to_vec())) } else { Some("Set.M3U") };
    assert_eq!(loclib_tracklist::detect(&bytes, hint), Format::M3u);
    assert_eq!(loclib_tracklist::parse(&bytes, hint).unwrap(), want);
}

/// Windows-1252's characters for 0x80-0x9F, where it differs from Latin-1 (from the code page's
/// published table).
const CP1252_HIGH: &[(char, u8)] = &[
    ('€', 0x80),
    ('‚', 0x82),
    ('ƒ', 0x83),
    ('„', 0x84),
    ('…', 0x85),
    ('†', 0x86),
    ('‡', 0x87),
    ('ˆ', 0x88),
    ('‰', 0x89),
    ('Š', 0x8A),
    ('‹', 0x8B),
    ('Œ', 0x8C),
    ('Ž', 0x8E),
    ('‘', 0x91),
    ('’', 0x92),
    ('“', 0x93),
    ('”', 0x94),
    ('•', 0x95),
    ('–', 0x96),
    ('\u{2014}', 0x97),
    ('˜', 0x98),
    ('™', 0x99),
    ('š', 0x9A),
    ('›', 0x9B),
    ('œ', 0x9C),
    ('ž', 0x9E),
    ('Ÿ', 0x9F),
];

/// Text survives every encoding tracklists come in, with its line ends made `\n`: UTF-8 with or
/// without a BOM, UTF-16 either way round with a BOM or (when it's ASCII) without one, and
/// Windows-1252 when the bytes aren't UTF-8.
#[hegel::test(test_cases = 400)]
fn text_survives_every_encoding(tc: TestCase) {
    let lf = |s: &str| s.replace("\r\n", "\n").replace('\r', "\n");
    // A leading U+FEFF is a BOM, and NULs make UTF-8 look like UTF-16.
    let s: String =
        tc.draw(gs::text().max_size(40).map(|s| s.replace('\0', "").trim_start_matches('\u{feff}').to_string()));
    let utf16 = |le: bool| -> Vec<u8> {
        s.encode_utf16().flat_map(|u| if le { u.to_le_bytes() } else { u.to_be_bytes() }).collect()
    };
    let text_of = loclib_tracklist::text_of;
    assert_eq!(text_of(s.as_bytes()), lf(&s));
    assert_eq!(text_of(&[&[0xEF, 0xBB, 0xBF][..], s.as_bytes()].concat()), lf(&s));
    assert_eq!(text_of(&[&[0xFF, 0xFE][..], &utf16(true)].concat()), lf(&s));
    assert_eq!(text_of(&[&[0xFE, 0xFF][..], &utf16(false)].concat()), lf(&s));
    if s.len() >= 2 && s.is_ascii() {
        assert_eq!(text_of(&utf16(true)), lf(&s));
        assert_eq!(text_of(&utf16(false)), lf(&s));
    }

    let alphabet: String =
        (1u8..=0x7F).chain(0xA0..=0xFF).map(char::from).chain(CP1252_HIGH.iter().map(|&(c, _)| c)).collect();
    let latin: String = tc.draw(gs::text().alphabet(&alphabet).max_size(40));
    let bytes: Vec<u8> =
        latin.chars().map(|c| CP1252_HIGH.iter().find(|&&(h, _)| h == c).map_or(c as u8, |&(_, b)| b)).collect();
    let bom = [&[0xEF, 0xBB, 0xBF][..], &[0xFF, 0xFE], &[0xFE, 0xFF]].iter().any(|b| bytes.starts_with(b));
    if std::str::from_utf8(&bytes).is_err() && !bom {
        assert_eq!(text_of(&bytes), lf(&latin));
    }
}
