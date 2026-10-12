//! Properties of reading tables: CSV in its dialects and rekordbox's text export.

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use loclib_tracklist::{Entry, Error, Format};

/// Any text a cell holds, trimmed, with no NUL (which makes UTF-8 look like UTF-16) or CR
/// (which reading turns into a line end).
fn cell() -> impl hegel::PrintableGenerator<String> {
    gs::text().max_size(12).map(|s| s.replace(['\0', '\r'], "").trim().to_string())
}

/// A name that also works in a file name and as "Artist - Title".
fn name() -> impl hegel::PrintableGenerator<String> {
    gs::from_regex("[A-Za-zé][A-Za-z0-9é&'.!()]{0,7}( [A-Za-z0-9é&'.!()]{1,8}){0,2}").fullmatch(true)
}

/// `secs` as a clock, "m:ss" or "h:mm:ss", sometimes with a fraction.
#[hegel::test_helper]
fn clock(tc: &TestCase, secs: u32) -> String {
    let c = if tc.draw(gs::booleans()) {
        format!("{}:{:02}", secs / 60, secs % 60)
    } else {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    };
    if tc.draw(gs::booleans()) { format!("{c}.4") } else { c }
}

/// `v` as a field of a table delimited by `delim`, quoted when it must be and sometimes when
/// it needn't.
#[hegel::test_helper]
fn quoted(tc: &TestCase, v: &str, delim: char) -> String {
    let must = v.contains([delim, '\n']) || v.trim_start().starts_with('"');
    if must || tc.draw(gs::booleans()) {
        let pad = if tc.draw(gs::booleans()) { " " } else { "" };
        format!("{pad}\"{}\"", v.replace('"', "\"\""))
    } else {
        v.to_string()
    }
}

/// A random order for `n` things.
#[hegel::test_helper]
fn shuffled(tc: &TestCase, n: usize) -> Vec<usize> {
    let keys: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>()).min_size(n).max_size(n));
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| keys[i]);
    order
}

/// Text as bytes: UTF-8, with a BOM, or UTF-16 as Excel and Apple Music write it.
#[hegel::test_helper]
fn encoded(tc: &TestCase, text: &str) -> Vec<u8> {
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => text.as_bytes().to_vec(),
        1 => [&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat(),
        _ => [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect(),
    }
}

/// Every record a CSV writer puts out reads back field for field, on the line it starts on and
/// with its own text, quoted fields spanning lines included; blank records are dropped. Without
/// quoting (rekordbox's text), quotes are kept as they are.
#[hegel::test(test_cases = 300)]
fn tables_read_back_record_for_record(tc: TestCase) {
    let delim = tc.draw(gs::sampled_from(vec![',', ';', '\t', '|']));
    let quoting = tc.draw(gs::booleans());
    // Often made of what quoting is about.
    let field = hegel::one_of!(gs::text().max_size(8), gs::text().alphabet("a \"\n,;\t|").max_size(8))
        .map(|s| s.replace('\r', ""));
    let mut rows: Vec<Vec<String>> = tc.draw(gs::vecs(gs::vecs(field).min_size(1).max_size(5)).max_size(6));
    if !quoting {
        // Nothing can hide a delimiter or a line end.
        for f in rows.iter_mut().flatten() {
            *f = f.replace([delim, '\n'], "");
        }
    }
    let mut text = String::new();
    let mut want = Vec::new();
    for row in &rows {
        let line = 1 + text.matches('\n').count();
        let fields: Vec<String> = row.iter().map(|f| if quoting { quoted(&tc, f, delim) } else { f.clone() }).collect();
        let raw = fields.join(&delim.to_string());
        if row.iter().any(|f| !f.trim().is_empty()) {
            want.push((line, raw.clone(), row.clone()));
        }
        text.push_str(&raw);
        text.push('\n');
    }
    if tc.draw(gs::booleans()) {
        text.pop();
    }
    let got: Vec<(usize, String, Vec<String>)> = loclib_tracklist::read_table(&text, delim, quoting)
        .into_iter()
        .map(|r| (r.line, r.raw.to_string(), r.fields))
        .collect();
    assert_eq!(got, want);
}

/// What a column holds.
#[derive(Debug, Clone, Copy, PartialEq, hegel::PrettyPrintable)]
enum Role {
    Title,
    Artist,
    Album,
    Mix,
    Duration,
    DurationMs,
    Isrc,
    Path,
    Other,
}

/// Header names exporters use for each role (Exportify, TuneMyMusic, Soundiiz, Beatport...).
fn header(role: Role) -> Vec<&'static str> {
    match role {
        Role::Title => vec!["Track Name", "Title", "Song", "name", "Track"],
        Role::Artist => vec!["Artist Name(s)", "Artist", "Artists", "Performer"],
        Role::Album => vec!["Album Name", "Album", "Release"],
        Role::Mix => vec!["Mix", "Version", "Mix Name"],
        Role::Duration => vec!["Length", "Time", "Duration"],
        Role::DurationMs => vec!["Duration (ms)", "Length ms"],
        Role::Isrc => vec!["ISRC", "isrc code"],
        Role::Path => vec!["Location", "File Path", "path"],
        Role::Other => vec!["Popularity", "Label", "Genre", "BPM", "Comment"],
    }
}

/// A CSV export (any delimiter, quoting as a writer would, optionally Excel's "sep=" line and
/// a playlist name above the header, columns in any order) reads back as exactly the tracks in
/// it, on their lines: titles, artists (split from the title when there's no artist column),
/// albums, mixes, lengths in seconds or milliseconds, ISRCs and file paths, with the title from
/// the file name when there's no title column. Records with neither a title nor a file are
/// skipped. It's detected as CSV.
#[hegel::test(test_cases = 300)]
fn csv_exports_read_back(tc: TestCase) {
    let title_column = tc.draw(gs::weighted_booleans(0.85));
    let mut roles = vec![];
    if title_column {
        roles.push(Role::Title);
    }
    for role in [Role::Artist, Role::Album, Role::Mix, Role::Isrc, Role::Other] {
        if tc.draw(gs::booleans()) {
            roles.push(role);
        }
    }
    if let Some(d) = tc.draw(gs::optional(gs::sampled_from(vec![Role::Duration, Role::DurationMs]))) {
        roles.push(d);
    }
    if !title_column || tc.draw(gs::booleans()) {
        roles.push(Role::Path);
    }
    let roles: Vec<Role> = shuffled(&tc, roles.len()).into_iter().map(|i| roles[i]).collect();
    // One column has no delimiter to sniff, so it must be the default.
    let delim = if roles.len() == 1 { ',' } else { tc.draw(gs::sampled_from(vec![',', ';', '\t'])) };
    let sep_line = tc.draw(gs::booleans());
    let preamble = (sep_line || delim == ',') && tc.draw(gs::booleans());
    let has = |r: Role| roles.contains(&r);

    let mut text = String::new();
    if sep_line {
        text.push_str(&format!("sep={delim}\n"));
    }
    if preamble {
        text.push_str(tc.draw(gs::sampled_from(vec!["My playlist", "Exported from Soundiiz", "Set 2026"])));
        text.push('\n');
    }
    let names: Vec<String> = roles.iter().map(|&r| tc.draw(gs::sampled_from(header(r))).to_string()).collect();
    let header_line: Vec<String> = names.iter().map(|n| quoted(&tc, n, delim)).collect();
    text.push_str(&header_line.join(&delim.to_string()));
    text.push('\n');

    let n = tc.draw(gs::integers::<usize>().max_value(5));
    let mut want = Vec::new();
    for _ in 0..n {
        let (artist, title) = (tc.draw(name()), tc.draw(name()));
        let mut e = Entry { line: 1 + text.matches('\n').count(), ..Default::default() };
        let mut cells = Vec::new();
        // A record naming neither a title nor a file isn't a track.
        let untitled = tc.draw(gs::weighted_booleans(0.1));
        for &role in &roles {
            let v = match role {
                Role::Title | Role::Path if untitled => String::new(),
                Role::Title if has(Role::Artist) => {
                    // Anything at all, when a column says who it's by.
                    let t = tc.draw(cell().filter(|s: &String| !s.is_empty()));
                    e.title = t.clone();
                    t
                }
                Role::Title => {
                    (e.artist, e.title) = (artist.clone(), title.clone());
                    format!("{artist} - {title}")
                }
                Role::Artist => {
                    let a = tc.draw(cell());
                    if !has(Role::Title) && a.is_empty() {
                        // The file name says who it's by.
                        e.artist = artist.clone();
                    } else {
                        e.artist = a.clone();
                    }
                    a
                }
                Role::Album | Role::Mix | Role::Other => {
                    let v = tc.draw(cell());
                    let v_opt = Some(v.clone()).filter(|v| !v.is_empty());
                    match role {
                        Role::Album => e.album = v_opt,
                        Role::Mix => e.mix = v_opt,
                        _ => {}
                    }
                    v
                }
                Role::Isrc => {
                    let v = tc.draw(gs::from_regex("([A-Z]{2}[A-Z0-9]{3}[0-9]{7})?").fullmatch(true));
                    e.isrc = Some(v.clone()).filter(|v| !v.is_empty());
                    v
                }
                Role::Duration => {
                    let secs = tc.draw(gs::integers::<u32>().min_value(1).max_value(35_999));
                    e.duration_secs = Some(secs);
                    match tc.draw(gs::integers::<u8>().max_value(2)) {
                        0 => clock(&tc, secs),
                        1 => secs.to_string(),
                        _ => format!("{secs}.4"),
                    }
                }
                Role::DurationMs => {
                    let secs = tc.draw(gs::integers::<u32>().min_value(1).max_value(35_999));
                    e.duration_secs = Some(secs);
                    (u64::from(secs) * 1000 + tc.draw(gs::integers::<u64>().max_value(499))).to_string()
                }
                Role::Path => {
                    let file = format!("/Music/{artist}/{artist} - {title}.mp3");
                    e.path = Some(file.clone());
                    if !has(Role::Title) {
                        e.title = title.clone();
                        if !has(Role::Artist) {
                            e.artist = artist.clone();
                        }
                    }
                    if tc.draw(gs::booleans()) { format!("file://localhost{}", file.replace(' ', "%20")) } else { file }
                }
            };
            cells.push(quoted(&tc, &v, delim));
        }
        let raw = cells.join(&delim.to_string());
        e.raw = raw.trim_end().to_string();
        text.push_str(&raw);
        text.push('\n');
        if !untitled {
            want.push(e);
        }
    }
    tc.note(&text);
    let bytes = encoded(&tc, &text);
    assert_eq!(loclib_tracklist::parse_as(&bytes, Format::Csv).unwrap(), want);
    let recognised = roles.iter().filter(|&&r| r != Role::Other).count();
    let sniffable = sep_line || (!preamble && has(Role::Title) && recognised >= 2);
    let hint = tc.draw(gs::sampled_from(if sniffable { vec![None, Some("x.csv")] } else { vec![Some("Liked.CSV")] }));
    assert_eq!(loclib_tracklist::detect(&bytes, hint), Format::Csv);
    assert_eq!(loclib_tracklist::parse(&bytes, hint).unwrap(), want);
}

/// A table whose first records name no title or file column is refused, quoting its first
/// record.
#[hegel::test(test_cases = 100)]
fn tables_without_a_title_column_are_refused(tc: TestCase) {
    let names: Vec<&str> =
        tc.draw(gs::vecs(gs::sampled_from(header(Role::Other))).min_size(1).max_size(4).unique(true));
    let rows: Vec<Vec<u32>> = tc.draw(gs::vecs(gs::vecs(gs::integers::<u32>()).min_size(1).max_size(4)).max_size(6));
    let mut text = names.join(",");
    for row in &rows {
        text.push('\n');
        text.push_str(&row.iter().map(u32::to_string).collect::<Vec<_>>().join(","));
    }
    assert_eq!(
        loclib_tracklist::parse_as(text.as_bytes(), Format::Csv),
        Err(Error::NoTitleColumn { found: names.join(", ") })
    );
}

/// rekordbox's "Export a playlist to a file" text (UTF-16, tabs, CRLF, a `#` column) reads
/// back as its tracks with quotes kept literally, and is recognised whatever the file is called.
#[hegel::test(test_cases = 200)]
fn rekordbox_text_exports_read_back(tc: TestCase) {
    let field = || cell().map(|s| s.replace(['\t', '\n'], " ").trim().to_string());
    let n = tc.draw(gs::integers::<usize>().max_value(6));
    // Its title column is "Track Title", or "Title" in some languages' exports.
    let title_column = tc.draw(gs::sampled_from(vec!["Track Title", "Title"]));
    let mut text = format!("#\tArtwork\t{title_column}\tArtist\tAlbum\tGenre\tBPM\tRating\tTime\tKey\tDate Added\r\n");
    let mut want = Vec::new();
    for i in 1..=n {
        let title = tc.draw(field().filter(|s: &String| !s.is_empty()));
        let (artist, album) = (tc.draw(field()), tc.draw(field()));
        let secs = tc.draw(gs::optional(gs::integers::<u32>().min_value(1).max_value(5999)));
        let time = secs.map(|s| format!("{:02}:{:02}", s / 60, s % 60)).unwrap_or_default();
        let row = format!("{i}\t\t{title}\t{artist}\t{album}\tHouse\t126.00\t***\t{time}\t8A\t2026-01-0{}", i % 9 + 1);
        want.push(Entry {
            line: i + 1,
            raw: row.clone(),
            title,
            artist,
            album: Some(album).filter(|a| !a.is_empty()),
            duration_secs: secs,
            ..Default::default()
        });
        text.push_str(&row);
        text.push_str("\r\n");
    }
    let bytes = encoded(&tc, &text);
    let hint = tc.draw(gs::sampled_from(vec![None, Some("set.txt"), Some("set.csv")]));
    assert_eq!(loclib_tracklist::detect(&bytes, hint), Format::RekordboxText);
    assert_eq!(loclib_tracklist::parse(&bytes, hint).unwrap(), want);
}
