use crate::parse;
use crate::testing::{at, pairs};

#[test]
fn plain_text_lines() {
    let txt = "Tracklist:\n\
                   \n\
                   // made by hand\n\
                   # comment\n\
                   01. Daft Punk - One More Time\n\
                   2) Moby – Porcelain\n\
                   #3 Eric Prydz — Opus\n\
                   [04] deadmau5 - Strobe (10:37)\n\
                   [00:12:34] Bicep - Glue\n\
                   1:02:03 Four Tet - Baby\n\
                   05 [00:20:00] Floating Points - Silhouettes\n\
                   • Caribou - Sun\n\
                   - Burial - Archangel\n\
                   808 State - Pacific State\n\
                   10cc - I'm Not In Love\n\
                   Strobe by deadmau5\n\
                   Artist–Title\n\
                   Only A Title 4:05\n\
                   -----\n";
    let e = parse(txt.as_bytes(), Some("set.txt")).unwrap();
    assert_eq!(
        at(&e),
        pairs(&[
            ("Daft Punk", "One More Time"),
            ("Moby", "Porcelain"),
            ("Eric Prydz", "Opus"),
            ("deadmau5", "Strobe"),
            ("Bicep", "Glue"),
            ("Four Tet", "Baby"),
            ("Floating Points", "Silhouettes"),
            ("Caribou", "Sun"),
            ("Burial", "Archangel"),
            ("808 State", "Pacific State"),
            ("10cc", "I'm Not In Love"),
            ("deadmau5", "Strobe"),
            ("Artist", "Title"),
            ("", "Only A Title"),
        ])
    );
    assert_eq!((e[0].line, e[0].raw.as_str()), (5, "01. Daft Punk - One More Time"));
    assert_eq!(e[3].duration_secs, Some(637));
    assert_eq!(e[4].duration_secs, None); // a cue time, not a length
    assert_eq!(e[13].duration_secs, Some(245));
}

#[test]
fn plain_text_bare_numbering() {
    // Every line numbered: the number goes, even before "2 Unlimited".
    let e = parse(b"1 Daft Punk - Da Funk\n2 2 Unlimited - Get Ready\n3 Moby - Go\n", Some("x.txt")).unwrap();
    assert_eq!(at(&e), pairs(&[("Daft Punk", "Da Funk"), ("2 Unlimited", "Get Ready"), ("Moby", "Go")]));
    // Not a numbered list: "2 Unlimited" keeps its 2.
    let e = parse(b"2 Unlimited - Get Ready\nMoby - Go\nBicep - Glue\n", Some("x.txt")).unwrap();
    assert_eq!(e[0].artist, "2 Unlimited");
    // "1 - Artist - Title"
    let e = parse(b"1 - Moby - Go\n", Some("x.txt")).unwrap();
    assert_eq!(at(&e), pairs(&[("Moby", "Go")]));
}

#[test]
fn plain_text_paths() {
    let e = parse(b"Moby - Go\n/Users/me/Music/Bicep - Glue.mp3\n", Some("x.txt")).unwrap();
    assert_eq!(e[1].path.as_deref(), Some("/Users/me/Music/Bicep - Glue.mp3"));
    assert_eq!((e[1].artist.as_str(), e[1].title.as_str(), e[1].line), ("Bicep", "Glue", 2));
}
