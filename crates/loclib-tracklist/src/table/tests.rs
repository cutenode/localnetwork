use super::*;
use crate::parse;
use crate::testing::{at, pairs, utf16le};

#[test]
fn exportify_csv() {
    let csv = "\u{feff}\"Track URI\",\"Track Name\",\"Album Name\",\"Artist Name(s)\",\"Release Date\",\"Duration (ms)\",\"Popularity\",\"ISRC\"\r\n\
                   \"spotify:track:1\",\"One More Time\",\"Discovery\",\"Daft Punk\",\"2001-03-12\",\"320357\",\"80\",\"GBDUW0000053\"\r\n\
                   \"spotify:track:2\",\"Get Lucky (feat. Pharrell Williams & Nile Rodgers) - Radio Edit\",\"Get Lucky\",\"Daft Punk,Pharrell Williams,Nile Rodgers\",\"2013\",\"248413\",\"85\",\"USQX91300108\"\r\n";
    let e = parse(csv.as_bytes(), Some("Liked.csv")).unwrap();
    assert_eq!(e.len(), 2);
    assert_eq!((e[0].line, e[0].title.as_str(), e[0].artist.as_str()), (2, "One More Time", "Daft Punk"));
    assert_eq!((e[0].album.as_deref(), e[0].duration_secs), (Some("Discovery"), Some(320)));
    assert_eq!(e[0].isrc.as_deref(), Some("GBDUW0000053"));
    assert_eq!(e[1].artist, "Daft Punk,Pharrell Williams,Nile Rodgers");
    assert_eq!(e[1].duration_secs, Some(248));
    assert!(e[1].raw.starts_with("\"spotify:track:2\""));
}

#[test]
fn other_csv_dialects() {
    // TuneMyMusic
    let t =
        "Track name,Artist name,Album,Playlist name,Type,ISRC\nStrobe,deadmau5,For Lack,Set,Playlist,CA6D20900085\n";
    let e = parse(t.as_bytes(), Some("x.csv")).unwrap();
    assert_eq!((e[0].title.as_str(), e[0].artist.as_str()), ("Strobe", "deadmau5"));
    assert_eq!(e[0].isrc.as_deref(), Some("CA6D20900085"));
    // Beatport-style, semicolons, a mix column and a m:ss length
    let b = "Track;Artists;Mix;Label;Length\nOpus;Eric Prydz;Original Mix;Virgin;9:03\n";
    let e = parse(b.as_bytes(), None).unwrap();
    assert_eq!(
        (e[0].title.as_str(), e[0].mix.as_deref(), e[0].duration_secs),
        ("Opus", Some("Original Mix"), Some(543))
    );
    // Soundiiz-style lower case, tab separated, plus a file column
    let s = "title\tartist\talbum\tisrc\tlocation\nOpus\tEric Prydz\tOpus\t\tfile:///M/opus.mp3\n";
    let e = parse(s.as_bytes(), Some("x.tsv")).unwrap();
    assert_eq!((e[0].isrc.as_deref(), e[0].path.as_deref()), (None, Some("/M/opus.mp3")));
    // Excel's sep= line, and a header under a title row
    let x = "sep=;\nMy playlist\nTitle;Artist\nA;B\n";
    let e = parse(x.as_bytes(), Some("x.csv")).unwrap();
    assert_eq!((e[0].line, e[0].title.as_str(), e[0].artist.as_str()), (4, "A", "B"));
    // No title column at all
    let err = parse(b"foo,bar\n1,2\n", Some("x.csv")).unwrap_err();
    assert!(err.to_string().contains("foo, bar"), "{err}");
}

#[test]
fn csv_quoting() {
    let csv = "Title,Artist,Album\n\
                   \"Hello, Goodbye\",\"The Beatles\",\"Magical \"\"Mystery\"\" Tour\"\n\
                   \"Two\nLines\",X, \"spaced\" \n\
                   \"half\"open,Y,\n\
                   \n\
                   ,,\n\
                   Last,Z";
    let e = parse(csv.as_bytes(), Some("x.csv")).unwrap();
    assert_eq!(e.len(), 4);
    assert_eq!(e[0].title, "Hello, Goodbye");
    assert_eq!(e[0].album.as_deref(), Some("Magical \"Mystery\" Tour"));
    assert_eq!((e[1].line, e[1].title.as_str(), e[1].album.as_deref()), (3, "Two\nLines", Some("spaced")));
    assert_eq!(e[1].raw, "\"Two\nLines\",X, \"spaced\"");
    assert_eq!((e[2].line, e[2].title.as_str()), (5, "halfopen"));
    assert_eq!((e[3].line, e[3].title.as_str(), e[3].artist.as_str()), (8, "Last", "Z"));
}

#[test]
fn csv_without_artist_column_splits_titles() {
    let e = parse(b"Name\nDaft Punk - Da Funk\nIntro\n", Some("x.csv")).unwrap();
    assert_eq!(at(&e), pairs(&[("Daft Punk", "Da Funk"), ("", "Intro")]));
}

#[test]
fn apple_music_text_export() {
    // Apple Music / iTunes "Export Playlist" text: UTF-16, tabs, Time in seconds, Location.
    let txt = "Name\tArtist\tComposer\tAlbum\tTime\tLocation\n\
                   Porcelain\tMoby\t\tPlay\t241\tMacintosh HD:Users:me:Music:Moby:Play:Porcelain.mp3\n";
    let e = parse(&utf16le(txt), Some("Playlist.txt")).unwrap();
    assert_eq!((e[0].title.as_str(), e[0].duration_secs), ("Porcelain", Some(241)));
    assert_eq!(e[0].path.as_deref(), Some("/Macintosh HD/Users/me/Music/Moby/Play/Porcelain.mp3"));
}

#[test]
fn rekordbox_text_export() {
    let txt = "#\tArtwork\tTrack Title\tArtist\tAlbum\tGenre\tBPM\tRating\tTime\tKey\tDate Added\r\n\
                   1\t\t\"Heroes\" (Extended)\tDavid Bowie\tHeroes\tRock\t112.00\t***\t06:07\t4A\t2024-01-01\r\n\
                   2\t\tOpus\tEric Prydz\tOpus\tProgressive House\t126.00\t\t09:03\t8A\t2024-01-02\r\n";
    let e = parse(&utf16le(txt), Some("set.txt")).unwrap();
    assert_eq!(e.len(), 2);
    // Quotes are literal in rekordbox's export.
    assert_eq!(e[0].title, "\"Heroes\" (Extended)");
    assert_eq!((e[0].artist.as_str(), e[0].duration_secs, e[0].line), ("David Bowie", Some(367), 2));
    assert_eq!((e[1].title.as_str(), e[1].album.as_deref()), ("Opus", Some("Opus")));
}

#[test]
fn durations() {
    assert_eq!(parse_duration("5:23", false), Some(323));
    assert_eq!(parse_duration("1:02:03", false), Some(3723));
    assert_eq!(parse_duration("05:23.4", false), Some(323));
    assert_eq!(parse_duration("320357", true), Some(320));
    assert_eq!(parse_duration("320357", false), Some(320)); // too long to be seconds
    assert_eq!(parse_duration("323.6", false), Some(324));
    assert_eq!(parse_duration("0", false), None);
    assert_eq!(parse_duration("abc", false), None);
    assert_eq!(parse_duration("5:61", false), None);
}
