use super::*;

/// A collection in the shape Traktor Pro 3 and 4 write one: a track on the startup disk with a
/// grid marker on a pad (holding its own tempo), a hot cue, a looping hot cue and a coloured load
/// marker, a track on a Windows drive, a stream with no file, a marker of a type nobody
/// documents, and playlists in a folder beside a smart playlist, one listing a track as a stem
/// (as Traktor 4 lists a stem file) and a track the collection hasn't.
const NML: &str = r##"<?xml version="1.0" encoding="UTF-8" standalone="no" ?>
<NML VERSION="19"><HEAD COMPANY="www.native-instruments.com" PROGRAM="Traktor"></HEAD>
<MUSICFOLDERS></MUSICFOLDERS>
<COLLECTION ENTRIES="3"><ENTRY MODIFIED_DATE="2024/3/1" MODIFIED_TIME="40000" AUDIO_ID="AWEhIzMiNEQ=" TITLE="Glue" ARTIST="Bicep">
<LOCATION DIR="/:Users/:dj/:Music/:Bicep/:" FILE="Glue.mp3" VOLUME="Macintosh HD" VOLUMEID="Macintosh HD"></LOCATION>
<ALBUM TRACK="3" TITLE="Bicep"></ALBUM>
<MODIFICATION_INFO AUTHOR_TYPE="user"></MODIFICATION_INFO>
<INFO BITRATE="320000" GENRE="Electronic" LABEL="Ninja Tune" COMMENT="peak &amp; after" KEY="8A" PLAYCOUNT="4" PLAYTIME="269" PLAYTIME_FLOAT="268.538000" RANKING="204" IMPORT_DATE="2023/1/15" RELEASE_DATE="2017/9/1" FLAGS="14" FILESIZE="10514"></INFO>
<TEMPO BPM="130.000061" BPM_QUALITY="100.000000"></TEMPO>
<LOUDNESS PEAK_DB="-0.5" PERCEIVED_DB="0.2" ANALYZED_DB="0.2"></LOUDNESS>
<MUSICAL_KEY VALUE="21"></MUSICAL_KEY>
<CUE_V2 NAME="AutoGrid" DISPL_ORDER="0" TYPE="4" START="49.5" LEN="0.000000" REPEATS="-1" HOTCUE="0"><GRID BPM="130.000000"></GRID></CUE_V2>
<CUE_V2 NAME="n.n." DISPL_ORDER="0" TYPE="0" START="29588.6" LEN="0.000000" REPEATS="-1" HOTCUE="1"></CUE_V2>
<CUE_V2 NAME="Drop" DISPL_ORDER="0" TYPE="5" START="59127.7" LEN="7384.6" REPEATS="-1" HOTCUE="2"></CUE_V2>
<CUE_V2 NAME="n.n." DISPL_ORDER="0" TYPE="3" START="1000.000000" LEN="0.000000" REPEATS="-1" HOTCUE="-1" COLOR="#FF8800"></CUE_V2>
<CUE_V2 NAME="Odd" DISPL_ORDER="0" TYPE="9" START="0.000000" LEN="0.000000" REPEATS="-1" HOTCUE="-1"></CUE_V2>
</ENTRY>
<ENTRY MODIFIED_DATE="2024/3/1" MODIFIED_TIME="40000" TITLE="b"><LOCATION DIR="/:Music/:" FILE="b.wav" VOLUME="E:" VOLUMEID="5e3a"/><INFO KEY="Fm" COLOR="2" RANKING="999"/><MUSICAL_KEY VALUE="24"/></ENTRY>
<ENTRY TITLE="Radio"><LOCATION DIR="" FILE="" VOLUME=""></LOCATION></ENTRY>
</COLLECTION>
<SETS ENTRIES="0"></SETS>
<PLAYLISTS><NODE TYPE="FOLDER" NAME="$ROOT"><SUBNODES COUNT="3">
<NODE TYPE="PLAYLIST" NAME="Preparation"><PLAYLIST ENTRIES="0" TYPE="LIST" UUID="1f0e"></PLAYLIST></NODE>
<NODE TYPE="FOLDER" NAME="Gigs"><SUBNODES COUNT="2">
<NODE TYPE="PLAYLIST" NAME="Friday"><PLAYLIST ENTRIES="2" TYPE="LIST" UUID="2a9b"><ENTRY><PRIMARYKEY TYPE="TRACK" KEY="E:/:Music/:b.wav"></PRIMARYKEY></ENTRY><ENTRY><PRIMARYKEY TYPE="TRACK" KEY="Macintosh HD/:Users/:dj/:Music/:Bicep/:Glue.mp3"></PRIMARYKEY></ENTRY><ENTRY><PRIMARYKEY TYPE="STEM" KEY="E:/:Music/:b.wav"></PRIMARYKEY></ENTRY><ENTRY><PRIMARYKEY TYPE="TRACK" KEY="E:/:Music/:gone.wav"></PRIMARYKEY></ENTRY></PLAYLIST></NODE>
<NODE TYPE="SMARTLIST" NAME="Recent"><SMARTLIST UUID="3c7d"><SEARCH_EXPRESSION VERSION="1" QUERY="$IMPORTDATE &gt; 2024/1/1"></SEARCH_EXPRESSION></SMARTLIST></NODE>
</SUBNODES></NODE>
</SUBNODES></NODE></PLAYLISTS>
<INDEXING></INDEXING>
</NML>
"##;

fn marker(kind: MarkerKind, name: &str, start_ms: f64, len_ms: f64, hot_cue: Option<u8>) -> Marker {
    Marker { kind, name: name.into(), start_ms, len_ms, hot_cue, color: None, bpm: None }
}

#[test]
fn a_collection_traktor_wrote_is_read() {
    let c = read(NML.as_bytes()).unwrap();
    let [glue, b] = &c.entries[..] else { panic!("two tracks with files: {:?}", c.entries) };
    let location =
        Location { volume: "Macintosh HD".into(), dir: "/:Users/:dj/:Music/:Bicep/:".into(), file: "Glue.mp3".into() };
    assert_eq!(glue.location, location);
    let tags = [&glue.title, &glue.artist, &glue.album, &glue.genre, &glue.label, &glue.comment, &glue.key_text];
    assert_eq!(tags, ["Glue", "Bicep", "Bicep", "Electronic", "Ninja Tune", "peak & after", "8A"]);
    assert_eq!(glue.key, Some(Camelot { number: 8, major: false }));
    assert_eq!((glue.bpm, glue.duration_secs, glue.rating, glue.year), (Some(130.000061), Some(268.538), 4, 2017));
    assert_eq!(
        glue.markers,
        [
            Marker { bpm: Some(130.0), ..marker(MarkerKind::Grid, "AutoGrid", 49.5, 0.0, Some(0)) },
            marker(MarkerKind::Cue, "", 29588.6, 0.0, Some(1)),
            marker(MarkerKind::Loop, "Drop", 59127.7, 7384.6, Some(2)),
            Marker { color: Some("#ff8800".into()), ..marker(MarkerKind::Load, "", 1000.0, 0.0, None) },
        ]
    );
    // What it doesn't say is left empty; a key value past 23 and a ranking past five stars aren't
    // taken as they are.
    assert_eq!((b.location.key(), b.title.as_str(), b.key_text.as_str()), ("E:/:Music/:b.wav".into(), "b", "Fm"));
    assert_eq!((b.key, b.bpm, b.duration_secs, b.rating, b.year), (None, None, None, 5, 0));
    assert!(b.markers.is_empty() && b.artist.is_empty());

    let list = |path: &[&str], keys: &[&str]| List {
        path: path.iter().map(|p| p.to_string()).collect(),
        keys: keys.iter().map(|k| k.to_string()).collect(),
    };
    assert_eq!(
        c.lists,
        [
            list(&["Preparation"], &[]),
            list(
                &["Gigs", "Friday"],
                &["E:/:Music/:b.wav", "Macintosh HD/:Users/:dj/:Music/:Bicep/:Glue.mp3", "E:/:Music/:b.wav"]
            ),
        ]
    );
    assert_eq!(
        c.warnings,
        [
            "1 entr(ies) without a file left out",
            "1 marker(s) of a type Traktor doesn't document left out",
            "1 smart playlist(s), whose rules aren't read, left out",
            "1 playlist entr(ies) naming no track in the collection left out",
        ]
    );
}

/// A playlist tree without Traktor's `$ROOT` keeps its outermost folder, a playlist named
/// `$ROOT` keeps its name, and a playlist's own node closed at once is an empty playlist.
#[test]
fn playlists_without_a_root_folder() {
    let nml = r#"<NML><PLAYLISTS><NODE TYPE="FOLDER" NAME="Top"><SUBNODES><NODE TYPE="PLAYLIST" NAME="A"/></SUBNODES></NODE><NODE TYPE="PLAYLIST" NAME="$ROOT"/></PLAYLISTS></NML>"#;
    let c = read(nml.as_bytes()).unwrap();
    let list = |path: &[&str]| List { path: path.iter().map(|p| p.to_string()).collect(), keys: Vec::new() };
    assert_eq!(c.lists, [list(&["Top", "A"]), list(&["$ROOT"])]);
    assert!(c.entries.is_empty() && c.warnings.is_empty());
}

#[test]
fn what_isnt_a_collection_is_refused() {
    let refused = |bytes: &[u8]| read(bytes).unwrap_err().to_string();
    assert_eq!(refused(b""), "not a Traktor collection: there's no NML element");
    assert_eq!(
        refused(b"<?xml version=\"1.0\"?><plist/>"),
        "not a Traktor collection: its root element is plist, not NML"
    );
    assert_eq!(refused(b"<NML><COLLECTION>"), "not a Traktor collection: it ends before its NML element does");
    assert!(refused(b"<NML></COLLECTION>").starts_with("not a Traktor collection: "), "a mismatched end tag");
    assert!(refused(b"<NML A=\"1\" A=\"2\"/>").starts_with("not a Traktor collection: "), "a repeated attribute");
    assert!(refused(b"<NML A=\"&nope;\"/>").starts_with("not a Traktor collection: "), "an unknown entity");
    assert!(refused(b"<NML>\xff</NML>").contains("utf-8"), "{}", refused(b"<NML>\xff</NML>"));
}

fn grid(start_ms: f64, bpm: Option<f64>) -> Marker {
    Marker { bpm, ..marker(MarkerKind::Grid, "", start_ms, 0.0, None) }
}

fn entry(bpm: Option<f64>, duration_secs: Option<f64>, markers: Vec<Marker>) -> Entry {
    Entry { bpm, duration_secs, markers, ..Entry::default() }
}

fn beat(beat_number: u16, bpm: f64, time_ms: u32) -> Beat {
    Beat { beat_number, bpm, time_ms }
}

/// The grid runs back from the first marker to the start of the track and on to its end, each
/// marker a downbeat at its own tempo or the track's.
#[test]
fn grid_markers_lay_down_beats() {
    let one = entry(Some(120.0), Some(2.0), vec![grid(1100.0, None), marker(MarkerKind::Cue, "", 0.0, 0.0, None)]);
    assert_eq!(
        one.beats().unwrap(),
        [beat(3, 120.0, 100), beat(4, 120.0, 600), beat(1, 120.0, 1100), beat(2, 120.0, 1600)]
    );
    // Listed out of order; the second at its own tempo, from where it is.
    let two = entry(Some(120.0), Some(2.5), vec![grid(1500.0, Some(60.0)), grid(-200.0, None)]);
    assert_eq!(
        two.beats().unwrap(),
        [beat(2, 120.0, 300), beat(3, 120.0, 800), beat(4, 120.0, 1300), beat(1, 60.0, 1500)]
    );
    // A marker within a millisecond of a beat: the beat already there stays.
    let close = entry(Some(120.0), Some(1.2), vec![grid(0.0, None), grid(1000.3, None)]);
    assert_eq!(close.beats().unwrap(), [beat(1, 120.0, 0), beat(2, 120.0, 500), beat(3, 120.0, 1000)]);
    assert_eq!(entry(Some(120.0), Some(1.0), Vec::new()).beats(), Ok(Vec::new()));
    let unknown = "the track's length isn't known, or is over a day";
    assert_eq!(entry(Some(120.0), None, vec![grid(0.0, None)]).beats(), Err(unknown));
    assert_eq!(entry(Some(120.0), Some(86_400.5), vec![grid(0.0, None)]).beats(), Err(unknown));
    let too_fast = entry(Some(700.0), Some(1.0), vec![grid(0.0, None)]);
    assert_eq!(too_fast.beats(), Err("its tempo isn't known, or isn't 0.01 to 655.35 BPM"));
    assert!(entry(None, Some(1.0), vec![grid(0.0, None)]).beats().is_err(), "no tempo");
    for far in [-1e300, 4_294_967_296.0, f64::NAN] {
        let out_of_reach = entry(Some(120.0), Some(1.0), vec![grid(far, None)]);
        assert_eq!(out_of_reach.beats(), Err("a grid marker is further from the start than a track reaches"));
    }
    // A day at the fastest tempo, and from a marker as far before the start as a track reaches.
    let longest = entry(Some(655.35), Some(86_400.0), vec![grid(-4_294_967_295.0, None)]);
    let beats = longest.beats().unwrap();
    assert_eq!((beats.len(), beats.last().map(|b| b.time_ms < 86_400_000)), (943_704, Some(true)));
}

/// Numbers out of what a track or a grid holds are left out as they're read: a tempo, a length,
/// a marker (counted in a warning).
#[test]
fn numbers_out_of_reach_are_left_out() {
    let nml = r#"<NML><COLLECTION><ENTRY><LOCATION FILE="x.mp3"/><INFO PLAYTIME_FLOAT="5e6" PLAYTIME="inf"/><TEMPO BPM="0.001"/>
        <CUE_V2 TYPE="4" START="-1e300"/><CUE_V2 TYPE="5" START="0" LEN="1e10"/><CUE_V2 TYPE="0" START="NaN" LEN="-4"><GRID BPM="900"/></CUE_V2>
        </ENTRY></COLLECTION></NML>"#;
    let c = read(nml.as_bytes()).unwrap();
    let e = &c.entries[0];
    assert_eq!((e.bpm, e.duration_secs), (None, None));
    assert_eq!(
        e.markers,
        [marker(MarkerKind::Cue, "", 0.0, 0.0, None)],
        "a NaN start isn't a number; the GRID's tempo is too fast"
    );
    assert_eq!(c.warnings, ["2 marker(s) further from the start than a track reaches left out"]);
}

/// Playlists whose paths come to more than 16 MiB are refused: each holds its folders' names, so a
/// long folder name over many playlists comes to far more than the file. Folders nested deep are
/// read as any are.
#[test]
fn playlists_past_16_mib_of_paths_are_refused() {
    let lists = |n: usize| {
        let playlists = "<NODE TYPE=\"PLAYLIST\" NAME=\"p\"/>".repeat(n);
        format!(
            "<NML><PLAYLISTS><NODE TYPE=\"FOLDER\" NAME=\"{}\">{playlists}</NODE></PLAYLISTS></NML>",
            "x".repeat(1 << 20)
        )
    };
    assert_eq!(read(lists(15).as_bytes()).unwrap().lists.len(), 15);
    let refused = read(lists(17).as_bytes()).unwrap_err().to_string();
    assert_eq!(refused, "not a Traktor collection: its playlists' paths come to more than 16 MiB");
    let deep =
        format!("<NML><PLAYLISTS>{}{}</PLAYLISTS></NML>", "<NODE TYPE=\"FOLDER\">".repeat(500), "</NODE>".repeat(500));
    assert!(read(deep.as_bytes()).unwrap().lists.is_empty());
}

#[test]
fn colours_are_traktors_for_each_kind() {
    use MarkerKind::*;
    let colours = [Cue, FadeIn, FadeOut, Load, Grid, Loop].map(MarkerKind::colour);
    assert_eq!(colours, [Some("blue"), Some("orange"), Some("orange"), Some("yellow"), None, Some("green")]);
    assert_eq!(
        (0..7).map(MarkerKind::of).collect::<Vec<_>>(),
        [Some(Cue), Some(FadeIn), Some(FadeOut), Some(Load), Some(Grid), Some(Loop), None]
    );
}
