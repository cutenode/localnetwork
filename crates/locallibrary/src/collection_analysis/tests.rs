use loclib_rekordbox_db::masterdb::write::Target;
use loclib_rekordbox_db::masterdb::{DEFAULT_KEY, files::Running};
use loclib_rekordbox_db::test_util::{ANALYSIS_DIR, UPDATE_COUNT, analysed, codec, plain, scratch_db};
use loclib_rekordbox_db::{Error, Refusal};

use super::*;
use crate::anlz::Beat;
use crate::cue_edit::CueStyle;

const TRACK: &str = "84935471";

fn closed() -> Running {
    Running::default()
}

/// [`analysed`]'s collection, encrypted, in a temporary folder, with its track's `.DAT` and `.EXT`
/// (made as for a track without any, and listed) in the share folder, and an agent log saying sync
/// is off.
struct Lib {
    _dir: tempfile::TempDir,
    target: Target,
    share: PathBuf,
}

impl Lib {
    fn new() -> Lib {
        Lib::with("", None)
    }

    /// [`Lib::new`], its collection changed by `sql` first, its `.EXT` holding `phrases`.
    fn with(sql: &str, phrases: Option<&anlz::SongStructure>) -> Lib {
        let (dat, ext) = anlz::new_analysis_files("/Users/dj/Music/one.mp3").unwrap();
        let ext = match phrases {
            Some(p) => anlz::write_song_structure(&ext, Some(p)).unwrap(),
            None => ext,
        };
        let f = analysed(&dat, &ext);
        f.exec(sql);
        let dir = tempfile::tempdir().unwrap();
        let rekordbox = dir.path().join("rekordbox");
        std::fs::create_dir(&rekordbox).unwrap();
        let mut bytes = f.plain();
        bytes[18..20].copy_from_slice(&[2, 2]);
        std::fs::write(rekordbox.join("master.db"), codec(DEFAULT_KEY).encrypt(&bytes).unwrap()).unwrap();
        let log = dir.path().join("log.log");
        std::fs::write(&log, "[261126T012213.828-0600] I/sync mode: null => standalone\n").unwrap();
        let share = rekordbox.join("share").join(ANALYSIS_DIR);
        std::fs::create_dir_all(&share).unwrap();
        std::fs::write(share.join("ANLZ0000.DAT"), dat).unwrap();
        std::fs::write(share.join("ANLZ0000.EXT"), ext).unwrap();
        let target = Target {
            master_db: rekordbox.join("master.db"),
            key: DEFAULT_KEY.into(),
            backups: dir.path().join("backups"),
            agent_log: Some(log),
            work: None,
            running: closed,
        };
        Lib { _dir: dir, target, share }
    }

    fn file(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.share.join(name)).unwrap()
    }

    fn query<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plain.db");
        let mut bytes = plain(&self.target.master_db);
        bytes[18..20].copy_from_slice(&[1, 1]);
        std::fs::write(&path, bytes).unwrap();
        scratch_db(&path).query_row(sql, [], |r| r.get(0)).unwrap()
    }
}

/// 64 beats at 120.5 BPM from 1 s.
fn grid() -> Vec<Beat> {
    (0..64).map(|i| Beat { beat_number: (i % 4 + 1) as u16, bpm: 120.5, time_ms: 1000 + i * 498 }).collect()
}

#[test]
fn a_grid_key_and_cues_are_saved_and_undone() {
    let lib = Lib::new();
    // Its first downbeat over a bar past the track's memory cue at 0.5 s, which would be its first cue.
    let later: Vec<Beat> = grid().into_iter().map(|b| Beat { time_ms: b.time_ms + 2000, ..b }).collect();
    let (dat, ext) = (lib.file("ANLZ0000.DAT"), lib.file("ANLZ0000.EXT"));
    let edit = CollectionAnalysis {
        analysis: NewAnalysis { beats: Some(later.clone()), ..NewAnalysis::default() },
        key: Some("Am".into()),
        cues: Some(AutoCues { style: CueStyle::FirstCue, overwrite: false }),
    };
    let saved = save(&lib.target, TRACK, &edit).unwrap();

    // The grid is in the .DAT, which the collection lists with its new hash.
    assert_eq!(anlz::beat_grid(&lib.file("ANLZ0000.DAT")).unwrap(), later);
    let files: Vec<&str> = saved.before.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(files, [format!("/{ANALYSIS_DIR}/ANLZ0000.DAT")]);
    assert_eq!(std::fs::read(&saved.before.files[0].backup).unwrap(), dat);
    let size: i64 = lib.query("SELECT Size FROM contentFile WHERE ID = 'f1'");
    assert_eq!(size, lib.file("ANLZ0000.DAT").len() as i64);
    // Its tempo and key, and AnalysisUpdated up by one.
    let row: String = lib
        .query(&format!("SELECT BPM || ' ' || KeyID || ' ' || AnalysisUpdated FROM djmdContent WHERE ID = '{TRACK}'"));
    assert_eq!(row, "12050 1 2");
    // The memory cue on the first downbeat joins the track's cues, as the cue writer writes them.
    let cues: String = lib.query(&format!(
        "SELECT group_concat(Kind || ':' || InMsec || ':' || IFNULL(Comment, ''), ' ') FROM \
         (SELECT * FROM djmdCue WHERE ContentID = '{TRACK}' ORDER BY Kind, InMsec)"
    ));
    assert_eq!(cues, "0:500: 0:3000:1.1Bars 1:1000:Drop 2:2000:Loop");
    assert_eq!(lib.query::<i64>(&format!("SELECT rb_cue_count FROM contentCue WHERE ContentID = '{TRACK}'")), 4);
    assert!(saved.count.0 == UPDATE_COUNT && saved.count.1 > UPDATE_COUNT);

    // Undo puts it all back, with new stamps.
    let undone = undo(&lib.target, &saved.before).unwrap();
    assert_eq!(lib.file("ANLZ0000.DAT"), dat);
    assert_eq!(lib.file("ANLZ0000.EXT"), ext);
    let row: String = lib
        .query(&format!("SELECT BPM || ' ' || KeyID || ' ' || AnalysisUpdated FROM djmdContent WHERE ID = '{TRACK}'"));
    assert_eq!(row, "12280 1 3");
    assert_eq!(lib.query::<i64>(&format!("SELECT count(*) FROM djmdCue WHERE ContentID = '{TRACK}'")), 3);
    assert!(undone.count.0 == saved.count.1 && undone.count.1 > saved.count.1);
}

#[test]
fn what_cant_be_saved_changes_nothing() {
    let lib = Lib::new();
    let before = std::fs::read(&lib.target.master_db).unwrap();
    let dat = lib.file("ANLZ0000.DAT");
    let bad = [
        (
            "205356547",
            CollectionAnalysis {
                analysis: NewAnalysis { beats: Some(grid()), ..Default::default() },
                ..Default::default()
            },
        ),
        (TRACK, CollectionAnalysis { key: Some("H#m".into()), ..Default::default() }),
        (
            TRACK,
            CollectionAnalysis {
                analysis: NewAnalysis {
                    waveform: Some(analysis_edit::WaveSource::Base64("!!".into())),
                    ..Default::default()
                },
                ..Default::default()
            },
        ),
    ];
    for (track, edit) in bad {
        let e = save(&lib.target, track, &edit).unwrap_err();
        assert!(matches!(e, Error::Invalid(_)), "{e}");
    }
    assert_eq!(std::fs::read(&lib.target.master_db).unwrap(), before);
    assert_eq!(lib.file("ANLZ0000.DAT"), dat);
}

#[test]
fn rekordbox_open_is_a_refusal_that_waits() {
    fn open() -> Running {
        Running { rekordbox: true, agent: false }
    }
    let mut lib = Lib::new();
    lib.target.running = open;
    let edit = CollectionAnalysis { key: Some("Am".into()), ..Default::default() };
    match save(&lib.target, TRACK, &edit).unwrap_err() {
        Error::Refused(r) => assert!(r.waits() && r == Refusal::RekordboxRunning),
        e => panic!("{e}"),
    }
}

#[test]
fn a_waveform_goes_into_both_files() {
    use base64::prelude::*;
    let lib = Lib::new();
    let (dat, ext) = (lib.file("ANLZ0000.DAT"), lib.file("ANLZ0000.EXT"));
    let bands = crate::anlz::Bands((0..320 * 150).map(|i| [(i % 200) as u8, 40, 90]).collect());
    let edit = CollectionAnalysis {
        analysis: NewAnalysis {
            waveform: Some(analysis_edit::WaveSource::Base64(BASE64_STANDARD.encode(bands.pack()))),
            ..NewAnalysis::default()
        },
        ..CollectionAnalysis::default()
    };
    let saved = save(&lib.target, TRACK, &edit).unwrap();
    assert_eq!(saved.before.files.len(), 2);
    assert!(lib.file("ANLZ0000.DAT") != dat && lib.file("ANLZ0000.EXT") != ext);
    assert!(anlz::has_section(&lib.file("ANLZ0000.EXT"), b"PWV5").unwrap());
    // No grid given: the tempo stays.
    assert_eq!(lib.query::<i64>(&format!("SELECT BPM FROM djmdContent WHERE ID = '{TRACK}'")), 12280);
    let sizes: i64 = lib.query("SELECT sum(Size) FROM contentFile");
    assert_eq!(sizes, (lib.file("ANLZ0000.DAT").len() + lib.file("ANLZ0000.EXT").len()) as i64);
}

/// A grid moved from under the collection's own cues (in djmdCue, not the analysis files) says so.
#[test]
fn a_moved_grid_warns_of_the_collections_cues() {
    let lib = Lib::new();
    let with = |beats: Vec<Beat>| CollectionAnalysis {
        analysis: NewAnalysis { beats: Some(beats), ..NewAnalysis::default() },
        ..CollectionAnalysis::default()
    };
    save(&lib.target, TRACK, &with(grid())).unwrap();
    let later: Vec<Beat> = grid().into_iter().map(|b| Beat { time_ms: b.time_ms + 10, ..b }).collect();
    let saved = save(&lib.target, TRACK, &with(later)).unwrap();
    // The hot cue at 1 s sat on the first beat of the old grid.
    assert!(saved.warnings.iter().any(|w| w.starts_with("1 cue(s) of")), "{:?}", saved.warnings);
}

/// A key is saved as the collection spells it: the spelling the track's key has, else the one most
/// of the collection's tracks have, else any; refused only when no spelling of it is there.
#[test]
fn a_key_is_saved_as_the_collection_spells_it() {
    // Camelot on two tracks, a note name on this one, and min/maj on none.
    let keys = "INSERT INTO djmdKey (ID, ScaleName, Seq) VALUES ('2', '8A', 2), ('3', '12A', 3), ('4', 'Dbm', 4),
                    ('5', 'C#min', 5), ('6', '1B', 6), ('7', '8B', 7), ('8', 'Cmaj', 8);
                UPDATE djmdContent SET KeyID = '2' WHERE ID = '205356547';
                UPDATE djmdContent SET KeyID = '3' WHERE ID = '4';";
    let key_id = |lib: &Lib| lib.query::<String>(&format!("SELECT KeyID FROM djmdContent WHERE ID = '{TRACK}'"));
    let set = |lib: &Lib, key: &str| {
        save(&lib.target, TRACK, &CollectionAnalysis { key: Some(key.into()), ..Default::default() })
    };

    let lib = Lib::with(keys, None);
    // The track's key is a note name (Am): C#m and 12A are Dbm.
    for key in ["C#m", "12A", "c# minor"] {
        set(&lib, key).unwrap();
        assert_eq!(key_id(&lib), "4", "{key}");
    }
    // A key with no row in any spelling, and one that isn't a key.
    for key in ["Bbm", "H#m"] {
        let e = set(&lib, key).unwrap_err();
        assert!(matches!(&e, Error::Invalid(why) if why.contains(key)), "{key}: {e}");
    }
    assert_eq!(key_id(&lib), "4");
    // No note name for C major: the collection's most used spelling, Camelot, over Cmaj.
    set(&lib, "C").unwrap();
    assert_eq!(key_id(&lib), "7");

    // A Camelot track takes Camelot.
    let lib = Lib::with(&format!("{keys} UPDATE djmdContent SET KeyID = '6' WHERE ID = '{TRACK}';"), None);
    set(&lib, "Dbm").unwrap();
    assert_eq!(key_id(&lib), "3");
    // A track whose key has no row takes the collection's most used spelling.
    let lib = Lib::with(&format!("{keys} UPDATE djmdContent SET KeyID = '0' WHERE ID = '{TRACK}';"), None);
    set(&lib, "Dbm").unwrap();
    assert_eq!(key_id(&lib), "3");
}

/// A new grid moves the collection's phrases with it, by time, and undo puts them back.
#[test]
fn phrases_move_with_a_new_grid() {
    use anlz::{Mood, Phrase, PhraseKind, SongStructure};
    let phrase = |kind, start_beat, end_beat| Phrase { kind, start_beat, end_beat, fill_beat: None };
    let s = |p| SongStructure { mood: Mood::High, phrases: p };
    let theirs = s(vec![phrase(PhraseKind::Intro, 3, 17), phrase(PhraseKind::Chorus, 17, 65)]);
    let lib = Lib::with("", Some(&theirs));
    let with = |beats: Vec<Beat>| CollectionAnalysis {
        analysis: NewAnalysis { beats: Some(beats), ..NewAnalysis::default() },
        ..CollectionAnalysis::default()
    };
    let phrases = || anlz::song_structure(&lib.file("ANLZ0000.EXT")).unwrap();
    save(&lib.target, TRACK, &with(grid())).unwrap();
    assert_eq!(phrases().as_ref(), Some(&theirs));

    // Two beats shorter at the front: each phrase starts two beats earlier.
    let saved = save(&lib.target, TRACK, &with(grid()[2..].to_vec())).unwrap();
    assert_eq!(phrases(), Some(s(vec![phrase(PhraseKind::Intro, 1, 15), phrase(PhraseKind::Chorus, 15, 63)])));
    undo(&lib.target, &saved.before).unwrap();
    assert_eq!(phrases(), Some(theirs));
}
