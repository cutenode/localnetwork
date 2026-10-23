use crate::testing::{at, pairs};
use crate::{parse, read_xml_library, xml_playlist};

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

/// Playlists by folder path, their entries the collection's tracks at the playlist's lines, each
/// with its key as the text it came from rather than the track's whole tag.
#[test]
fn xml_playlists_by_path() {
    let library = read_xml_library(XML.as_bytes()).unwrap();
    let names: Vec<&str> = library.playlists.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(names, ["Shows/Warmup", "Shows/Empty", "By location"]);
    let warmup = xml_playlist(XML.as_bytes(), "Shows/Warmup").unwrap().unwrap();
    assert_eq!(at(&warmup), pairs(&[("deadmau5", "Strobe"), ("Led Zeppelin", "Rock & Roll – Part 2"), ("", "")]));
    assert_eq!(warmup[0].line, 18);
    let raws: Vec<&str> = warmup.iter().map(|e| e.raw.as_str()).collect();
    assert_eq!(raws, ["Key=\"3\"", "Key=\"1\"", "Key=\"42\""]);
    assert_eq!(xml_playlist(XML.as_bytes(), "shows/empty").unwrap(), Some(Vec::new()), "by path, ignoring case");
    assert_eq!(xml_playlist(XML.as_bytes(), "Nowhere").unwrap(), None);
    let by_loc = library.playlist("By location").unwrap();
    assert_eq!(by_loc[0].title, "Opus");
    assert_eq!((by_loc[1].artist.as_str(), by_loc[1].title.as_str()), ("Artist", "Gone"));
    assert_eq!(by_loc[1].path.as_deref(), Some("/gone/Artist - Gone.mp3"));
}
