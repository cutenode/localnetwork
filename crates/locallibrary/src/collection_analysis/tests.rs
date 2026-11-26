use loclib_rekordbox_db::masterdb::write::Target;
use loclib_rekordbox_db::masterdb::{DEFAULT_KEY, files::Running};
use loclib_rekordbox_db::test_util::{ANALYSIS_DIR, Fixture, UPDATE_COUNT, analysed, codec, plain, scratch_db};
use loclib_rekordbox_db::{Error, Refusal};

use super::*;
use crate::anlz::Beat;
use crate::cue_edit::CueStyle;

const TRACK: &str = "84935471";

fn closed() -> Running {
    Running::default()
}

/// [`analysed`]'s collection, encrypted, in a temporary folder, with its track's `.DAT` and `.EXT`
/// (made as for a track without any) in the share folder, and an agent log saying sync is off.
struct Lib {
    _dir: tempfile::TempDir,
    target: Target,
    share: PathBuf,
}

impl Lib {
    fn new(f: &Fixture) -> Lib {
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
        let (dat, ext) = anlz::new_analysis_files("/Users/dj/Music/one.mp3").unwrap();
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
    let lib = Lib::new(&analysed());
    let (dat, ext) = (lib.file("ANLZ0000.DAT"), lib.file("ANLZ0000.EXT"));
    let edit = CollectionAnalysis {
        analysis: NewAnalysis { beats: Some(grid()), ..NewAnalysis::default() },
        key: Some("Am".into()),
        cues: Some(AutoCues { style: CueStyle::FirstCue, overwrite: false }),
    };
    let saved = save(&lib.target, TRACK, &edit).unwrap();

    // The grid is in the .DAT, which the collection lists with its new hash.
    assert_eq!(anlz::beat_grid(&lib.file("ANLZ0000.DAT")).unwrap(), grid());
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
    assert_eq!(cues, "0:500: 0:1000:1.1Bars 1:1000:Drop 2:2000:Loop");
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
    let lib = Lib::new(&analysed());
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
    let mut lib = Lib::new(&analysed());
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
    let lib = Lib::new(&analysed());
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
