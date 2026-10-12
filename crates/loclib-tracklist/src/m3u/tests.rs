use super::*;
use crate::parse;

#[test]
fn m3u_extinf_and_every_kind_of_path() {
    let m3u = "#EXTM3U\n\
                   #EXTINF:323,Daft Punk - One More Time\n\
                   /Users/me/Music/Daft Punk/Discovery/01 One More Time.mp3\n\
                   \n\
                   # a comment\n\
                   #EXTINF:-1 tvg-name=\"a,b\",Stream\n\
                   https://radio.example/stream.mp3?x=1\n\
                   file:///Users/me/Music/Caf%C3%A9%20del%20Mar.mp3\n\
                   file://localhost/C:/Music/Track%20Two.flac\n\
                   file://nas/share/x.mp3\n\
                   C:\\Music\\Artist - Song.mp3\n\
                   ../Music/03 - Moby - Porcelain.mp3\n\
                   Macintosh HD:Users:me:x.mp3\n";
    let e = parse(m3u.as_bytes(), Some("list.m3u8")).unwrap();
    assert_eq!(e.len(), 8);
    assert_eq!((e[0].line, e[0].duration_secs), (3, Some(323)));
    assert_eq!((e[0].artist.as_str(), e[0].title.as_str()), ("Daft Punk", "One More Time"));
    assert_eq!(e[0].path.as_deref(), Some("/Users/me/Music/Daft Punk/Discovery/01 One More Time.mp3"));
    assert_eq!(e[0].raw, "/Users/me/Music/Daft Punk/Discovery/01 One More Time.mp3");
    // A stream isn't a file; its EXTINF names it, and the quoted comma didn't split it.
    assert_eq!((e[1].path.as_deref(), e[1].title.as_str(), e[1].duration_secs), (None, "Stream", None));
    assert_eq!(e[2].path.as_deref(), Some("/Users/me/Music/Café del Mar.mp3"));
    assert_eq!(e[2].title, "Café del Mar");
    assert_eq!(e[3].path.as_deref(), Some("C:/Music/Track Two.flac"));
    assert_eq!(e[4].path.as_deref(), Some("//nas/share/x.mp3"));
    assert_eq!(e[5].path.as_deref(), Some("C:\\Music\\Artist - Song.mp3"));
    assert_eq!((e[5].artist.as_str(), e[5].title.as_str()), ("Artist", "Song"));
    // Track number dropped from the file name.
    assert_eq!((e[6].artist.as_str(), e[6].title.as_str()), ("Moby", "Porcelain"));
    assert_eq!(e[7].path.as_deref(), Some("/Macintosh HD/Users/me/x.mp3"));
    // One colon in a relative name is just a colon; other schemes aren't files.
    assert_eq!(file_path("Artist - Title: Remix.mp3").as_deref(), Some("Artist - Title: Remix.mp3"));
    assert_eq!(file_path("spotify:track:4uLU6hMCjMI75M1A2tKUQC"), None);
    assert_eq!(file_path("file:/Users/me/x.mp3").as_deref(), Some("/Users/me/x.mp3"));
}

#[test]
fn m3u_album_tags_and_latin1() {
    let m3u = b"#EXTM3U\n#EXTALB:Discovery\n#EXTART:Daft Punk\n#EXTINF:200,Aerodynamic\nx.mp3\n\
                    #EXTINF:200,Caf\xe9 - Del Mar\ny.mp3\n";
    let e = parse(m3u, Some("x.m3u")).unwrap();
    assert_eq!(e[0].album.as_deref(), Some("Discovery"));
    assert_eq!((e[0].artist.as_str(), e[0].title.as_str()), ("Daft Punk", "Aerodynamic"));
    assert_eq!((e[1].artist.as_str(), e[1].title.as_str()), ("Café", "Del Mar"));
}
