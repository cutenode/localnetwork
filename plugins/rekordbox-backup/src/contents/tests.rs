use hegel::TestCase;
use hegel::generators as gs;

use super::*;

/// A Mac account's folders, as rekordbox 7.2.14 recorded them.
fn mac() -> Folders {
    let home = "/Users/dj";
    Folders {
        common_app_data: "/Library".into(),
        app_data: format!("{home}/Library"),
        music: format!("{home}/Music"),
        movies: format!("{home}/Movies"),
        documents: format!("{home}/Documents"),
        desktop: format!("{home}/Desktop"),
        home: home.into(),
    }
}

/// Byte for byte what rekordbox writes: CRLF after every line, ASCII with each other character as
/// one decimal reference to its code point (a non-BMP one too), `&` named, apostrophes as they are.
#[test]
fn contents_xml_is_written_as_rekordbox_writes_it() {
    let contents = Contents {
        roots: vec![("/Users/dj/Music".into(), "-1193679842".into())],
        tracks: vec![
            ("026EE51D".into(), "/TikTok/UnknownAlbum/026EE51D_Lizard (Once).mp3".into()),
            ("00000002".into(), "/Łukasz&Mø💀/Don't/00000002_\"<a^b`>\".mp3".into()),
        ],
        db_id: "2354715273".into(),
        content_num: 2050,
        rb_version: 70214,
        directories: vec!["/Users/dj/Music".into(), "/Users/dj".into()],
        folders: mac(),
    };
    let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\
\r\n\
<PROPERTIES>\r\n  \
<VALUE name=\"/Users/dj/Music\" val=\"-1193679842\"/>\r\n  \
<VALUE name=\"026EE51D\" val=\"/TikTok/UnknownAlbum/026EE51D_Lizard (Once).mp3\"/>\r\n  \
<VALUE name=\"00000002\" val=\"/&#321;ukasz&amp;M&#248;&#128128;/Don't/00000002_&quot;&lt;a&#94;b&#96;&gt;&quot;.mp3\"/>\r\n  \
<VALUE name=\"DB_ID\" val=\"2354715273\"/>\r\n  \
<VALUE name=\"CONTENT_ID_MAX\" val=\"0\"/>\r\n  \
<VALUE name=\"CONTENT_NUM\" val=\"2050\"/>\r\n  \
<VALUE name=\"BACKUP_RB_VER\" val=\"70214\"/>\r\n  \
<VALUE name=\"BACKUP_FILE_VER\" val=\"4\"/>\r\n  \
<VALUE name=\"DIRECTORY_ARRAY\" val=\"/Users/dj/Music,/Users/dj\"/>\r\n  \
<VALUE name=\"commonApplicationDataDirectory\" val=\"/Library\"/>\r\n  \
<VALUE name=\"userApplicationDataDirectory\" val=\"/Users/dj/Library\"/>\r\n  \
<VALUE name=\"userMusicDirectory\" val=\"/Users/dj/Music\"/>\r\n  \
<VALUE name=\"userMoviesDirectory\" val=\"/Users/dj/Movies\"/>\r\n  \
<VALUE name=\"userDocumentsDirectory\" val=\"/Users/dj/Documents\"/>\r\n  \
<VALUE name=\"userDesktopDirectory\" val=\"/Users/dj/Desktop\"/>\r\n  \
<VALUE name=\"userHomeDirectory\" val=\"/Users/dj\"/>\r\n\
</PROPERTIES>\r\n";
    assert_eq!(String::from_utf8(contents.to_xml()).unwrap(), expected);
    assert_eq!(parse(expected.as_bytes()).unwrap(), contents.values());
}

/// Line breaks and other control characters are references too, and a NUL ends a value, as a
/// JUCE string ends at one.
#[test]
fn controls_are_references_and_nul_ends_a_value() {
    let mut out = String::new();
    escape("a\r\nb\tc\u{7f}d\0never", &mut out);
    assert_eq!(out, "a&#13;&#10;b&#9;c&#127;d");
}

/// A name longer than JUCE's line puts the value on a line of its own, lined up past `<VALUE`.
#[test]
fn a_long_name_puts_its_value_on_the_next_line() {
    let long = format!("/Volumes/{}", "x".repeat(44));
    let values = [(long.clone(), "1".to_string()), ("/Volumes/short".into(), "2".into())];
    let xml = String::from_utf8(write(&values)).unwrap();
    let wrapped =
        format!("  <VALUE name=\"{long}\"\r\n         val=\"1\"/>\r\n  <VALUE name=\"/Volumes/short\" val=\"2\"/>");
    assert!(xml.contains(&wrapped), "{xml}");
    assert_eq!(parse(xml.as_bytes()).unwrap(), values);
}

#[test]
fn versions_read_as_rekordbox_records_them() {
    for (version, number) in [("7.2.14.0323", Some(70214)), ("7.2.14", Some(70214)), ("6.8", Some(60800))] {
        assert_eq!(rb_version(version), number, "{version}");
    }
    for odd in ["", "7", "seven.2.1", "7.200.1"] {
        assert_eq!(rb_version(odd), None, "{odd}");
    }
    assert_eq!(version_text(70214), "7.2.14");
    assert_eq!(version_text(60805), "6.8.5");
}

/// Each track's root is the longest user folder holding it, else its volume, drive or `/`, each
/// once in the order first met; a streaming track has none.
#[test]
fn directory_array_is_where_the_music_is() {
    let paths = [
        "/Users/dj/Music/Storage/a.mp3",
        "/Users/dj/Downloads/b.mp3",
        "/Users/dj/Music/c.mp3",
        "/Users/dj/Musical/d.mp3",
        "/Volumes/USB STICK/e.mp3",
        "/opt/f.mp3",
        "tidal:tracks:1",
        "relative/g.mp3",
    ];
    assert_eq!(directories(paths, &mac()), ["/Users/dj/Music", "/Users/dj", "/Volumes/USB STICK", "/"]);
    let windows = Folders {
        common_app_data: r"C:\ProgramData".into(),
        app_data: r"C:\Users\dj\AppData\Roaming".into(),
        music: r"C:\Users\dj\Music".into(),
        movies: r"C:\Users\dj\Videos".into(),
        documents: r"C:\Users\dj\Documents".into(),
        desktop: r"C:\Users\dj\Desktop".into(),
        home: r"C:\Users\dj".into(),
    };
    let paths = ["c:/users/DJ/music/a.mp3", "F:/Contents/b.mp3", r"C:\Other\c.mp3"];
    assert_eq!(directories(paths, &windows), [r"C:\Users\dj\Music", "F:", "C:"]);
}

/// Any value (but a NUL, which a JUCE string can't hold) comes back from the file as it went in,
/// and the file is ASCII, a line to an entry.
#[hegel::test(test_cases = 200)]
fn prop_escaping_round_trips(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().max_value(6));
    let values: Vec<(String, String)> = (0..n)
        .map(|_| {
            let name = tc.draw(gs::text().max_size(80)).replace('\0', "");
            let val = tc.draw(gs::text().max_size(80)).replace('\0', "");
            (name, val)
        })
        .collect();
    let xml = write(&values);
    assert!(xml.is_ascii());
    let lines = xml.split(|b| *b == b'\n').filter(|l| l.starts_with(b"  <VALUE")).count();
    assert_eq!(lines, values.len());
    assert!(xml.windows(2).all(|w| w[1] != b'\n' || w[0] == b'\r'), "CRLF only");
    assert_eq!(parse(&xml).unwrap(), values);
}
