use std::sync::atomic::{AtomicUsize, Ordering};

use hegel::TestCase;
use hegel::generators as gs;
use loclib_model::analysis::{Cue, hot_cue_palette};

use super::*;
use crate::masterdb::{DEFAULT_KEY, open};
use crate::test_util::{ANALYSIS_DIR, Fixture, UPDATE_COUNT, analysed, codec, scratch_db, writable};

/// Panics should a test's write be pointed at rekordbox's own collection, or anywhere in its
/// folders: tests write fixtures in temporary folders only.
pub(super) fn not_rekordboxs_own(path: &Path) {
    let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()).unwrap_or_default();
    let config = directories::BaseDirs::new().map(|b| b.config_dir().to_path_buf()).unwrap_or_default();
    for theirs in [home.join("Library/Pioneer"), config.join("Pioneer")] {
        assert!(!path.starts_with(&theirs), "a test tried to write {}", path.display());
    }
    if let Some(own) = default_path().filter(|_| std::env::var_os(super::super::PATH_ENV).is_none()) {
        assert_ne!(path, own, "a test tried to write rekordbox's own collection");
    }
}

fn closed() -> Running {
    Running::default()
}

/// A collection in a temporary folder of its own, with somewhere for backups and working copies,
/// and an agent log that says sync is off.
struct Lib {
    dir: tempfile::TempDir,
    target: Target,
}

impl Lib {
    /// `f`'s database as `master.db`, encrypted unless `plain`.
    fn new(f: &Fixture, plain: bool) -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let rekordbox = dir.path().join("rekordbox");
        std::fs::create_dir(&rekordbox).unwrap();
        let mut bytes = f.plain();
        // rekordbox keeps master.db in WAL mode.
        bytes[18..20].copy_from_slice(&[2, 2]);
        let bytes = if plain { bytes } else { codec(DEFAULT_KEY).encrypt(&bytes).unwrap() };
        std::fs::write(rekordbox.join("master.db"), bytes).unwrap();
        let log = dir.path().join("log.log");
        std::fs::write(&log, "[261126T012213.828-0600] I/sync mode: null => standalone\n").unwrap();
        std::fs::create_dir(dir.path().join("work")).unwrap();
        let target = Target {
            master_db: rekordbox.join("master.db"),
            key: DEFAULT_KEY.into(),
            backups: dir.path().join("backups"),
            agent_log: Some(log),
            work: Some(dir.path().join("work")),
            running: closed,
        };
        Lib { dir, target }
    }

    fn bytes(&self) -> Vec<u8> {
        std::fs::read(&self.target.master_db).unwrap()
    }

    /// The database, decrypted, opened to look at.
    fn sql(&self) -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plain.db");
        let mut plain = crate::test_util::plain(&self.target.master_db);
        plain[18..20].copy_from_slice(&[1, 1]);
        std::fs::write(&path, plain).unwrap();
        let conn = scratch_db(&path);
        (dir, conn)
    }

    fn query<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        let (_dir, conn) = self.sql();
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// Files left anywhere a write puts them but in `master.db` and the backups (when `backups`).
    fn leftovers(&self, backups: bool) -> Vec<String> {
        let mut out = Vec::new();
        let rekordbox = self.target.master_db.parent().unwrap();
        for dir in [rekordbox, &self.dir.path().join("work"), &self.target.backups] {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let keep = e.path() == self.target.master_db || (backups && dir == self.target.backups);
                if !keep {
                    out.push(e.path().display().to_string());
                }
            }
        }
        out
    }

    fn cues(&self, track: u32) -> Vec<Cue> {
        open(&self.target.master_db, DEFAULT_KEY).unwrap().cues.get(&track).cloned().unwrap_or_default()
    }
}

fn hot(h: u8, ms: u32) -> CueEntry {
    CueEntry { hot_cue: Some(h), start_ms: ms, ..Default::default() }
}

const TRACK: &str = "84935471";

#[test]
fn cues_set_read_back_as_rekordbox_lays_them_out() {
    let lib = Lib::new(&writable(), false);
    let original = lib.bytes();
    let cues = [
        CueEntry { color: Some(22), comment: "Drop".into(), ..hot(0, 16_335) },
        CueEntry { end_ms: Some(118_100), color: Some(36), ..hot(3, 110_383) },
        CueEntry { start_ms: 154, ..Default::default() },
        CueEntry { start_ms: 2000, end_ms: Some(4000), ..Default::default() },
    ];
    let written = write(&lib.target, |e| e.set_cues(TRACK, &cues)).unwrap();

    // What was there before comes back for undo.
    let before: Vec<String> = written.value.rows.iter().map(|r| r[0].1.as_str().unwrap().to_string()).collect();
    assert_eq!(before, ["c1", "c2", "c3"]);
    assert_eq!(written.value.content_cue, None);
    // The backup is the file as it was, byte for byte.
    assert_eq!(std::fs::read(&written.backup).unwrap(), original);
    // Encrypted again with the same salt, and back in WAL mode.
    let now = lib.bytes();
    assert_eq!(now[..16], original[..16]);
    assert_eq!(crate::test_util::plain(&lib.target.master_db)[18..20], [2, 2]);
    assert_eq!(lib.leftovers(true), Vec::<String>::new());

    // The reader reads them back.
    let read = lib.cues(84935471);
    let summary: Vec<_> = read.iter().map(|c| (c.hot_cue, c.start_ms, c.end_ms, c.comment.as_str(), c.color)).collect();
    assert_eq!(
        summary,
        [
            (None, 154, None, "", None),
            (None, 2000, Some(4000), "", None),
            (Some(0), 16_335, None, "Drop", hot_cue_palette(22)),
            (Some(3), 110_383, Some(118_100), "", hot_cue_palette(36)),
        ]
    );

    // Stamps: the list first, then the track; the counter ends at the last.
    assert_eq!(written.count, (UPDATE_COUNT, UPDATE_COUNT + 2));
    assert_eq!(
        lib.query::<i64>("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'"),
        UPDATE_COUNT + 2
    );
    assert_eq!(lib.query::<i64>("SELECT rb_local_usn FROM contentCue"), UPDATE_COUNT + 1);
    let track = format!("SELECT rb_local_usn || ' ' || CueUpdated FROM djmdContent WHERE ID = '{TRACK}'");
    assert_eq!(lib.query::<String>(&track), format!("{} 2", UPDATE_COUNT + 2));
    let at: String = lib.query(&format!("SELECT updated_at FROM djmdContent WHERE ID = '{TRACK}'"));
    assert!(chrono::DateTime::parse_from_str(&at, "%Y-%m-%d %H:%M:%S%.3f %:z").is_ok(), "{at}");
    assert_eq!(at.len(), "2026-11-26 07:24:06.375 +00:00".len(), "{at}");
    // Cue rows aren't stamped, as rekordbox leaves them.
    assert_eq!(lib.query::<i64>("SELECT count(*) FROM djmdCue WHERE rb_local_usn IS NOT NULL OR usn IS NOT NULL"), 0);

    // The rows, as rekordbox writes them.
    let (_d, conn) = lib.sql();
    let mut stmt = conn
        .prepare(
            "SELECT InMsec, InFrame, OutMsec, OutFrame, Kind, Color, ColorTableIndex, Comment, ContentUUID, \
             length(ID) <= 10 AND CAST(ID AS INTEGER) > 0, length(UUID) FROM djmdCue WHERE ContentID = ?1 ORDER BY rowid",
        )
        .unwrap();
    type Row = (i64, i64, i64, i64, i64, i64, Option<i64>, Option<String>, String, bool, i64);
    let rows: Vec<Row> = stmt
        .query_map([TRACK], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
                r.get(10)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let uuid = || "uuid-84935471".to_string();
    assert_eq!(
        rows,
        [
            (16_335, 2450, -1, 0, 1, -1, Some(22), Some("Drop".into()), uuid(), true, 36),
            (110_383, 16_557, 118_100, 17_715, 5, -1, Some(36), None, uuid(), true, 36),
            (154, 23, -1, 0, 0, -1, None, None, uuid(), true, 36),
            (2000, 300, 4000, 600, 4, -1, None, None, uuid(), true, 36),
        ]
    );

    // The list: one row, keyed by the track's UUID, the rows' columns that aren't NULL in
    // rekordbox's order and spelling.
    let (id, cues_json, count): (String, String, i64) = conn
        .query_row("SELECT ID, Cues, rb_cue_count FROM contentCue", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap();
    assert_eq!((id.as_str(), count), ("uuid-84935471", 4));
    let list: Vec<serde_json::Map<String, serde_json::Value>> = serde_json::from_str(&cues_json).unwrap();
    let keys: Vec<&str> = list[0].keys().map(String::as_str).collect();
    // serde_json's map sorts its keys, so the order is checked on the text.
    let order: Vec<usize> = [
        "\"ID\"",
        "\"ContentID\"",
        "\"ContentUUID\"",
        "\"InMsec\"",
        "\"InFrame\"",
        "\"InMpegFrame\"",
        "\"InMpegAbs\"",
        "\"OutMsec\"",
        "\"OutFrame\"",
        "\"OutMpegFrame\"",
        "\"OutMpegAbs\"",
        "\"Kind\"",
        "\"Color\"",
        "\"ColorTableIndex\"",
        "\"Comment\"",
        "\"UUID\"",
        "\"created_at\"",
        "\"updated_at\"",
    ]
    .iter()
    .map(|k| cues_json.find(k).unwrap())
    .collect();
    assert!(order.is_sorted(), "{cues_json}");
    assert_eq!(keys.len(), 18, "{keys:?}");
    assert_eq!(list[0]["InMsec"], 16_335);
    assert_eq!(list[0]["ContentID"], TRACK);
    assert!(list[0]["ID"].is_string());
    assert!(!list[2].contains_key("ColorTableIndex") && !list[2].contains_key("Comment"));
    let created = list[0]["created_at"].as_str().unwrap();
    assert!(chrono::DateTime::parse_from_str(created, "%Y-%m-%dT%H:%M:%S%.3f%:z").is_ok(), "{created}");
    assert!(!created.contains(' '), "{created}");
}

/// Undo moves forward: putting back what a write replaced is a new write with new stamps, and
/// the cues read as they did.
#[test]
fn undo_puts_the_old_cues_back_with_new_stamps() {
    let lib = Lib::new(&writable(), false);
    let original = lib.cues(84935471);
    let first = write(&lib.target, |e| e.set_cues(TRACK, &[hot(1, 5000)])).unwrap();
    assert_ne!(lib.cues(84935471), original);
    let undone = write(&lib.target, |e| e.restore_cues(&first.value)).unwrap();
    assert_eq!(lib.cues(84935471), original);
    assert!(undone.count.1 > first.count.1 && undone.count.0 == first.count.1);
    let ids = lib.query::<String>(&format!("SELECT group_concat(ID) FROM djmdCue WHERE ContentID = '{TRACK}'"));
    assert_eq!(ids, "c1,c2,c3");
    // And undoing the undo is the first write again.
    write(&lib.target, |e| e.restore_cues(&undone.value)).unwrap();
    assert_eq!(lib.cues(84935471).iter().map(|c| (c.hot_cue, c.start_ms)).collect::<Vec<_>>(), [(Some(1), 5000)]);
    assert_eq!(std::fs::read_dir(&lib.target.backups).unwrap().count(), 3);
}

/// A drawn run of writes, each a whole cue list for a drawn track: each reads back through the
/// collection's reader, and the counter only ever rises, past every stamp.
#[hegel::test(test_cases = 15, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_writes_read_back_and_the_counter_only_rises(tc: TestCase) {
    let lib = Lib::new(&writable(), true);
    let mut count = UPDATE_COUNT;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let track = tc.draw(gs::sampled_from(vec!["84935471", "205356547", "4"]));
        let n = tc.draw(gs::integers::<usize>().max_value(5));
        let mut cues = Vec::new();
        let mut letters: Vec<u8> = (0..8).collect();
        for _ in 0..n {
            let start = tc.draw(gs::integers::<u32>().max_value(600_000));
            let hot_cue = if tc.draw(gs::booleans()) && !letters.is_empty() {
                Some(letters.remove(tc.draw(gs::integers::<usize>().max_value(letters.len() - 1))))
            } else {
                None
            };
            let end =
                tc.draw(gs::booleans()).then(|| start + tc.draw(gs::integers::<u32>().min_value(1).max_value(9999)));
            let color = hot_cue.and_then(|_| {
                tc.draw(gs::booleans()).then(|| tc.draw(gs::integers::<u8>().min_value(1).max_value(62)))
            });
            let comment = tc.draw(gs::sampled_from(vec!["", "drop", "ü \"q\""])).to_string();
            cues.push(CueEntry { hot_cue, start_ms: start, end_ms: end, comment, color, active_loop: false });
        }
        let written = write(&lib.target, |e| e.set_cues(track, &cues)).unwrap();
        assert_eq!(written.count.0, count);
        assert!(written.count.1 > count);
        count = written.count.1;
        let highest: i64 = lib.query(
            "SELECT max(n) FROM (SELECT max(rb_local_usn) n FROM djmdContent UNION ALL SELECT max(rb_local_usn) FROM contentCue)",
        );
        assert_eq!(highest, count);

        let mut want: Vec<_> = cues
            .iter()
            .map(|c| (c.hot_cue, c.start_ms, c.end_ms, c.comment.clone(), c.color.and_then(hot_cue_palette)))
            .collect();
        want.sort_by_key(|c| (c.0.map_or(0, |h| h as u32 + 1), c.1));
        let got: Vec<_> = lib
            .cues(track.parse().unwrap())
            .into_iter()
            .map(|c| (c.hot_cue, c.start_ms, c.end_ms, c.comment, c.color))
            .collect();
        // Same order for equal keys isn't promised: compare as sets of sorted lists.
        let mut got_sorted = got.clone();
        got_sorted.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        want.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        assert_eq!(got_sorted, want);
    }
    let mismatched = cue_list_mismatches(&lib.target.master_db, DEFAULT_KEY, false).unwrap();
    assert_eq!(mismatched, Vec::<String>::new());
}

/// Each refusal leaves master.db as it was, byte for byte, and nothing behind.
#[test]
fn refusals_leave_master_db_untouched() {
    fn rekordbox() -> Running {
        Running { rekordbox: true, agent: true }
    }
    fn agent() -> Running {
        Running { rekordbox: false, agent: true }
    }
    type Setup = fn(&mut Lib);
    type Expected = fn(&Refusal) -> bool;
    let cases: &[(&str, Setup, Expected)] = &[
        ("rekordbox open", |l| l.target.running = rekordbox, |r| *r == Refusal::RekordboxRunning),
        ("agent running", |l| l.target.running = agent, |r| *r == Refusal::AgentRunning),
        ("a WAL with changes", |l| wal(l, "-wal", b"x"), |r| matches!(r, Refusal::Unsaved(_))),
        ("shared memory in use", |l| wal(l, "-shm", b"x"), |r| matches!(r, Refusal::Unsaved(_))),
        ("a journal", |l| wal(l, "-journal", b"x"), |r| matches!(r, Refusal::Unsaved(_))),
        ("no agent log", |l| l.target.agent_log = None, |r| matches!(r, Refusal::CloudSync(_))),
        ("a log saying sync", |l| log(l, "I/sync mode: standalone => cloud\n"), |r| matches!(r, Refusal::CloudSync(_))),
        ("a log saying nothing", |l| log(l, "I/started\n"), |r| matches!(r, Refusal::CloudSync(_))),
    ];
    fn wal(l: &mut Lib, end: &str, bytes: &[u8]) {
        std::fs::write(format!("{}{end}", l.target.master_db.display()), bytes).unwrap();
    }
    fn log(l: &mut Lib, text: &str) {
        std::fs::write(l.target.agent_log.as_ref().unwrap(), text).unwrap();
    }
    for (what, setup, expected) in cases {
        let mut lib = Lib::new(&writable(), false);
        setup(&mut lib);
        let before = lib.bytes();
        let Err(e) = write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])) else { panic!("{what} was written") };
        let Error::Refused(why) = &e else { panic!("{what}: {e}") };
        assert!(expected(why), "{what}: {e}");
        assert_eq!(lib.bytes(), before, "{what}");
        let left: Vec<String> = lib.leftovers(false).into_iter().filter(|p| !p.contains("master.db-")).collect();
        assert_eq!(left, Vec::<String>::new(), "{what}");
        assert!(!e.to_string().is_empty());
    }
}

/// An empty WAL or shared memory file, which a clean close can leave, doesn't stop a write.
#[test]
fn empty_wal_files_are_no_changes() {
    let lib = Lib::new(&writable(), false);
    for end in ["-wal", "-shm"] {
        std::fs::write(format!("{}{end}", lib.target.master_db.display()), b"").unwrap();
    }
    write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap();
}

/// What in the database says sync is (or was) on refuses the write.
#[test]
fn cloud_library_sync_in_the_database_refuses() {
    for (what, sql) in [
        ("usn", "UPDATE djmdContent SET usn = 5 WHERE ID = '4'"),
        ("synced", "UPDATE djmdPlaylist SET rb_local_synced = 1 WHERE ID = '10'"),
        ("cloud agent", "INSERT INTO cloudAgentRegistry (ID, int_1) VALUES ('x', 1)"),
        ("last count", "INSERT INTO agentRegistry (registry_id, int_1) VALUES ('lastUpdateCount', 7)"),
    ] {
        let f = writable();
        f.exec(sql);
        let lib = Lib::new(&f, false);
        let before = lib.bytes();
        let e = write(&lib.target, |e| e.set_cues(TRACK, &[])).unwrap_err();
        assert!(matches!(e, Error::Refused(Refusal::CloudSync(_))), "{what}: {e}");
        assert!(e.to_string().contains("Cloud Library Sync"), "{e}");
        assert_eq!(lib.bytes(), before, "{what}");
        assert_eq!(lib.leftovers(false), Vec::<String>::new(), "{what}");
    }
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

/// rekordbox starting while the edit is made stops the swap: nothing changes, nothing's left.
#[test]
fn a_failed_swap_leaves_nothing_behind() {
    fn starts_later() -> Running {
        Running { rekordbox: CALLS.fetch_add(1, Ordering::SeqCst) > 0, agent: false }
    }
    let mut lib = Lib::new(&writable(), false);
    lib.target.running = starts_later;
    let before = lib.bytes();
    let e = write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::RekordboxRunning)), "{e}");
    assert_eq!(lib.bytes(), before);
    assert_eq!(lib.leftovers(false), Vec::<String>::new());
}

/// master.db changing underneath (rekordbox saved) stops the swap, and the change stays.
#[test]
fn a_change_underneath_is_kept() {
    let lib = Lib::new(&writable(), false);
    let master = lib.target.master_db.clone();
    let e = write(&lib.target, |e| {
        let mut bytes = std::fs::read(&master).unwrap();
        bytes.extend_from_slice(&[0; 4096]);
        std::fs::write(&master, bytes).unwrap();
        e.set_cues(TRACK, &[hot(0, 1)])
    })
    .unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::Changed)), "{e}");
    assert_eq!(lib.bytes().len() % 4096, 0);
    assert_eq!(lib.leftovers(false), Vec::<String>::new());
}

/// An edit that fails changes nothing, and so does one given cues rekordbox can't hold.
#[test]
fn a_failed_edit_changes_nothing() {
    let lib = Lib::new(&writable(), false);
    let before = lib.bytes();
    let bad: &[&[CueEntry]] = &[
        &[hot(8, 1)],
        &[hot(0, 1), hot(0, 2)],
        &[CueEntry { end_ms: Some(5), ..hot(0, 10) }],
        &[CueEntry { color: Some(63), ..hot(0, 10) }],
    ];
    for cues in bad {
        let e = write(&lib.target, |e| e.set_cues(TRACK, cues)).unwrap_err();
        assert!(matches!(e, Error::Invalid(_)), "{e}");
    }
    let e = write(&lib.target, |e| e.set_cues("3", &[])).unwrap_err();
    assert!(matches!(e, Error::NotFound(_)), "a deleted track: {e}");
    let e = write(&lib.target, |e| {
        e.set_cues(TRACK, &[hot(0, 1)])?;
        Err::<(), _>(Error::Invalid("changed my mind".into()))
    })
    .unwrap_err();
    assert_eq!(e.to_string(), "changed my mind");
    assert_eq!(lib.bytes(), before);
    assert_eq!(lib.leftovers(false), Vec::<String>::new());
}

/// Lists that disagree with their rows are made to agree; with `create_missing`, tracks without
/// a list get one.
#[test]
fn repair_makes_the_lists_agree() {
    let f = writable();
    // A list from before a script added c3 (and a count that agrees with it), and one for a
    // track with no cues left.
    let stale = r#"[{"ID":"c1","InMsec":1000,"OutMsec":-1,"Kind":1},{"ID":"c2","InMsec":500,"OutMsec":-1,"Kind":0}]"#;
    f.exec(&format!(
        "INSERT INTO contentCue (ID, ContentID, Cues, rb_cue_count) VALUES
             ('uuid-84935471', '84935471', '{stale}', 2), ('uuid-4', '4', '{stale}', 2);
         INSERT INTO djmdCue (ID, ContentID, ContentUUID, InMsec, OutMsec, Kind) VALUES ('c9', '205356547', 'uuid-205356547', 9, -1, 0);"
    ));
    let lib = Lib::new(&f, false);
    let mut found = cue_list_mismatches(&lib.target.master_db, DEFAULT_KEY, false).unwrap();
    found.sort();
    assert_eq!(found, ["4", TRACK]);
    let all = cue_list_mismatches(&lib.target.master_db, DEFAULT_KEY, true).unwrap();
    assert_eq!(all.len(), 3, "{all:?}");

    let repaired = write(&lib.target, |e| e.repair_cue_lists(false)).unwrap();
    assert_eq!(repaired.value.len(), 2);
    assert!(repaired.value.iter().all(|b| b.content_cue.is_some()));
    assert_eq!(repaired.count.1, UPDATE_COUNT + 4);
    assert_eq!(cue_list_mismatches(&lib.target.master_db, DEFAULT_KEY, false).unwrap(), Vec::<String>::new());
    assert_eq!(lib.query::<String>("SELECT Cues || rb_cue_count FROM contentCue WHERE ContentID = '4'"), "[]0");
    assert_eq!(lib.query::<i64>(&format!("SELECT rb_cue_count FROM contentCue WHERE ContentID = '{TRACK}'")), 3);
    // The rows themselves are as they were.
    assert_eq!(lib.query::<i64>("SELECT count(*) FROM djmdCue"), 5);

    let created = write(&lib.target, |e| e.repair_cue_lists(true)).unwrap();
    assert_eq!(created.value.iter().map(|b| b.track_id.as_str()).collect::<Vec<_>>(), ["205356547"]);
    assert_eq!(cue_list_mismatches(&lib.target.master_db, DEFAULT_KEY, true).unwrap(), Vec::<String>::new());
}

/// A track's columns set through the write path are stamped, and put back the same way.
#[test]
fn track_columns_are_set_stamped_and_put_back() {
    let lib = Lib::new(&writable(), false);
    let set = [("BPM", Value::Integer(12400)), ("KeyID", Value::Text("2".into()))];
    let first = write(&lib.target, |e| e.update_track(TRACK, &set)).unwrap();
    assert_eq!(first.value.values, [("BPM".to_string(), 12280.into()), ("KeyID".to_string(), "1".into())]);
    let row = format!("SELECT BPM || ' ' || KeyID || ' ' || rb_local_usn FROM djmdContent WHERE ID = '{TRACK}'");
    assert_eq!(lib.query::<String>(&row), format!("12400 2 {}", UPDATE_COUNT + 1));
    write(&lib.target, |e| e.restore_track(&first.value)).unwrap();
    assert_eq!(lib.query::<String>(&row), format!("12280 1 {}", UPDATE_COUNT + 2));
    for kept in ["ID", "rb_local_usn", "updated_at", "Nope"] {
        let e = write(&lib.target, |e| e.update_track(TRACK, &[(kept, Value::Null)])).unwrap_err();
        assert!(matches!(e, Error::Invalid(_)), "{kept}: {e}");
    }
}

/// A collection without a localUpdateCount isn't one this writes.
#[test]
fn no_update_count_is_refused() {
    let f = writable();
    f.exec("DELETE FROM agentRegistry WHERE registry_id = 'localUpdateCount'");
    let lib = Lib::new(&f, false);
    let e = write(&lib.target, |e| e.set_cues(TRACK, &[])).unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::Odd(_))), "{e}");
}

/// A hot cue from the model takes the palette code nearest its colour; a memory cue none.
#[test]
fn model_cues_become_entries() {
    let cue = |hot_cue, start_ms, end_ms, color| Cue {
        hot_cue,
        start_ms,
        end_ms,
        comment: String::new(),
        color,
        color_id: 0,
        loop_beats: None,
        active_loop: false,
    };
    let c = cue(Some(2), 10, Some(10), hot_cue_palette(36));
    assert_eq!(CueEntry::from(&c), CueEntry { color: Some(36), ..hot(2, 10) });
    let m = cue(None, 5, Some(9), Some([1, 2, 3]));
    assert_eq!(CueEntry::from(&m), CueEntry { start_ms: 5, end_ms: Some(9), ..Default::default() });
}

#[test]
fn rekordboxs_agent_and_its_helpers_are_the_agent() {
    use std::ffi::OsStr;
    for (name, is) in [
        ("rekordboxAgent", true),
        ("rekordboxAgent.exe", true),
        ("rekordboxAgent Helper (Renderer)", true),
        ("rekordbox", false),
    ] {
        assert_eq!(super::super::files::is_agent(OsStr::new(name)), is, "{name}");
    }
}

/// A counter some script left below a stamp is raised to it, even by an edit that stamps nothing,
/// and a count kept as text in `str_1` rises with `int_1`.
#[test]
fn the_counter_is_raised_past_every_stamp() {
    let f = writable();
    f.exec(&format!(
        "UPDATE djmdContent SET rb_local_usn = {} WHERE ID = '4';
         UPDATE agentRegistry SET int_1 = NULL, str_1 = '{UPDATE_COUNT}' WHERE registry_id = 'localUpdateCount';",
        UPDATE_COUNT + 50
    ));
    let lib = Lib::new(&f, false);
    let written = write(&lib.target, |_| Ok(())).unwrap();
    assert_eq!(written.count, (UPDATE_COUNT, UPDATE_COUNT + 50));
    let stored = "SELECT int_1 || ' ' || str_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'";
    assert_eq!(lib.query::<String>(stored), format!("{0} {0}", UPDATE_COUNT + 50));
    let written = write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap();
    assert_eq!(written.count, (UPDATE_COUNT + 50, UPDATE_COUNT + 52));

    // Text that isn't a number is no count.
    let f = writable();
    f.exec("UPDATE agentRegistry SET int_1 = NULL, str_1 = 'many' WHERE registry_id = 'localUpdateCount'");
    let lib = Lib::new(&f, false);
    let e = write(&lib.target, |e| e.set_cues(TRACK, &[])).unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::Odd(_))), "{e}");
}

/// Rows that already point nowhere (rekordbox leaves some) don't stop a write; an edit that would
/// leave a new one fails (SQLite enforces foreign keys on the working copy), and nothing changes.
#[test]
fn only_new_rows_pointing_nowhere_are_refused() {
    let f = writable();
    f.exec(
        "PRAGMA foreign_keys = OFF;
         CREATE TABLE pointer (ID VARCHAR(255) PRIMARY KEY, CueID VARCHAR(255) REFERENCES djmdCue (ID));
         INSERT INTO pointer VALUES ('old', 'gone');",
    );
    let lib = Lib::new(&f, false);
    write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap();
    let before = lib.bytes();
    let e = write(&lib.target, |e| {
        e.conn()
            .execute("INSERT INTO pointer VALUES ('new', 'also gone')", [])
            .map_err(|e| Error::Invalid(e.to_string()))
    })
    .unwrap_err();
    assert!(e.to_string().contains("FOREIGN KEY"), "{e}");
    assert_eq!(lib.bytes(), before);
    assert_eq!(lib.leftovers(true), Vec::<String>::new());
}

/// This computer's collection is the one the environment names, with what finds rekordbox
/// running.
#[test]
fn this_computers_collection_is_the_one_the_environment_names() {
    let dir = tempfile::tempdir().unwrap();
    let master = dir.path().join("master.db");
    // SAFETY: nextest runs each test in a process of its own.
    unsafe { std::env::set_var(super::super::PATH_ENV, &master) };
    let target = Target::this_computer(dir.path().join("backups")).unwrap();
    assert_eq!(target.master_db, master);
    assert_eq!(target.key, resolve_key(None));
    assert_eq!((target.agent_log, target.work), (agent_log_path(), None));
    let _ = (target.running)();
}

/// Without a folder of its own for working copies, a write uses the system's temporary folder.
#[test]
fn working_copies_go_in_the_systems_temporary_folder_by_default() {
    let mut lib = Lib::new(&writable(), false);
    lib.target.work = None;
    write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap();
    assert_eq!(lib.cues(84935471).len(), 1);
}

/// Only refusals that pass once rekordbox has quit are worth trying again.
#[test]
fn only_a_running_rekordbox_or_a_change_waits() {
    for waits in [Refusal::RekordboxRunning, Refusal::AgentRunning, Refusal::Changed] {
        assert!(waits.waits(), "{waits}");
    }
    let path = PathBuf::from("master.db-wal");
    for not in [Refusal::Unsaved(path), Refusal::CloudSync("x".into()), Refusal::Odd("x".into())] {
        assert!(!not.waits(), "{not}");
    }
}

/// A collection with what rekordbox never makes, a `master.db` that isn't a file, a track without
/// a UUID or with two cue lists: none is written, and none of it is worth waiting out.
#[test]
fn collections_a_write_cant_vouch_for_are_refused() {
    let odd = |e: &Error| matches!(e, Error::Refused(r @ Refusal::Odd(_)) if !r.waits());
    let f = writable();
    f.exec("CREATE VIEW tracks AS SELECT ID FROM djmdContent");
    let e = write(&Lib::new(&f, false).target, |_| Ok(())).unwrap_err();
    assert!(odd(&e), "a view: {e}");

    let mut lib = Lib::new(&writable(), false);
    lib.target.master_db = lib.dir.path().join("work");
    let e = write(&lib.target, |_| Ok(())).unwrap_err();
    assert!(odd(&e), "a folder: {e}");

    let f = writable();
    f.exec(&format!("UPDATE djmdContent SET UUID = NULL WHERE ID = '{TRACK}'"));
    let e = write(&Lib::new(&f, false).target, |e| e.set_cues(TRACK, &[])).unwrap_err();
    assert!(matches!(e, Error::Invalid(_)), "no UUID: {e}");

    let f = writable();
    f.exec(&format!(
        "INSERT INTO contentCue (ID, ContentID, Cues, rb_cue_count) VALUES ('a', '{TRACK}', '[]', 0), ('b', '{TRACK}', '[]', 0)"
    ));
    let lib = Lib::new(&f, false);
    let before = lib.bytes();
    let e = write(&lib.target, |e| e.set_cues(TRACK, &[])).unwrap_err();
    assert!(odd(&e), "two lists: {e}");
    assert_eq!(lib.bytes(), before);
}

/// A WAL beside `master.db` that can't be looked at (its name is too long) stops a write.
#[cfg(unix)]
#[test]
fn a_wal_that_cant_be_looked_at_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let master = dir.path().join("m".repeat(250));
    std::fs::write(&master, b"").unwrap();
    let e = alone(&master).unwrap_err();
    assert!(matches!(e, Error::Io { .. }), "{e}");
}

/// A backup is the file as it was, under a name nothing has yet, or there's none.
#[test]
fn a_backup_is_whole_and_new_or_isnt_made() {
    let lib = Lib::new(&writable(), false);
    let master = &lib.target.master_db;
    let before = Fingerprint::of(master).unwrap();
    let to = lib.dir.path().join("backup.db");
    let other = Fingerprint { hash: blake3::hash(b""), ..before.clone() };
    let e = backup_as(master, &to, &other).unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::Changed)), "{e}");
    let e = unchanged(master, &other).unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::Changed)), "{e}");
    std::fs::write(&to, b"older").unwrap();
    let e = backup_as(master, &to, &before).unwrap_err();
    assert!(matches!(e, Error::Exists(_)), "{e}");
    assert_eq!(std::fs::read(&to).unwrap(), b"older");
    assert!(!beside(&to, "partial").exists());
}

/// [`writable`] with an index SQLite's quick check passes and its integrity check doesn't: it says
/// it indexes one column and holds another's values.
fn misindexed() -> (tempfile::TempDir, PathBuf) {
    let f = writable();
    f.exec(
        "CREATE INDEX by_title ON djmdContent (Title);
         PRAGMA writable_schema = ON;
         UPDATE sqlite_master SET sql = 'CREATE INDEX by_title ON djmdContent (FolderPath)' WHERE name = 'by_title';
         PRAGMA writable_schema = OFF;",
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("misindexed.db");
    std::fs::write(&path, f.plain()).unwrap();
    (dir, path)
}

/// The file written is read back whole, with the count the edit ended at.
#[test]
fn the_file_written_is_read_back_whole_with_its_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.db");
    std::fs::write(&path, writable().plain()).unwrap();
    read_back(&path, DEFAULT_KEY, dir.path(), UPDATE_COUNT).unwrap();
    let e = read_back(&path, DEFAULT_KEY, dir.path(), UPDATE_COUNT + 1).unwrap_err();
    assert!(e.to_string().contains("localUpdateCount"), "{e}");
    let (_damaged, path) = misindexed();
    let e = read_back(&path, DEFAULT_KEY, dir.path(), UPDATE_COUNT).unwrap_err();
    assert!(e.to_string().contains("damaged"), "{e}");
}

/// The edited copy isn't written when it's damaged, points somewhere new at nothing, counts below
/// a stamp, lost a track it edited, or has a cue list disagreeing with its rows.
#[test]
fn the_edited_copy_is_checked_before_its_written() {
    let f = writable();
    let none = FxHashSet::default();
    let count = (UPDATE_COUNT, UPDATE_COUNT);
    let before = dangling(&f.conn).unwrap();
    check(&f.conn, count, &before, &none, &none).unwrap();
    let e = check(&f.conn, (UPDATE_COUNT, UPDATE_COUNT + 1), &before, &none, &none).unwrap_err();
    assert!(e.contains("localUpdateCount"), "{e}");
    let gone: FxHashSet<String> = ["gone".to_string()].into_iter().collect();
    let e = check(&f.conn, count, &before, &gone, &none).unwrap_err();
    assert!(e.contains("no track gone"), "{e}");
    f.exec(&format!("INSERT INTO contentCue (ID, ContentID, Cues, rb_cue_count) VALUES ('l', '{TRACK}', '[]', 9)"));
    let track: FxHashSet<String> = [TRACK.to_string()].into_iter().collect();
    let e = check(&f.conn, count, &before, &none, &track).unwrap_err();
    assert!(e.contains("contentCue disagreeing"), "{e}");
    f.exec(
        "PRAGMA foreign_keys = OFF;
         CREATE TABLE pointer (ID VARCHAR(255) PRIMARY KEY, CueID VARCHAR(255) REFERENCES djmdCue (ID));
         INSERT INTO pointer VALUES ('p', 'gone');",
    );
    let e = check(&f.conn, count, &before, &none, &none).unwrap_err();
    assert!(e.contains("pointing at"), "{e}");

    let (_dir, path) = misindexed();
    let conn = scratch_db(&path);
    let e = check(&conn, count, &before, &none, &none).unwrap_err();
    assert!(e.contains("damaged"), "{e}");
}

/// An edit can see its time and the lists that disagree; one that stamps nothing leaves the count;
/// a cue row put back whose id is taken gets a new one; and SQL that fails says so.
#[test]
fn edits_see_their_time_and_put_rows_back_under_free_ids() {
    let lib = Lib::new(&writable(), false);
    let looked = write(&lib.target, |e| {
        assert_eq!(e.now().len(), "2026-11-26 07:24:06.375 +00:00".len(), "{}", e.now());
        e.cue_list_mismatches(true)
    })
    .unwrap();
    assert_eq!(looked.value, [TRACK]);
    assert_eq!(looked.count, (UPDATE_COUNT, UPDATE_COUNT));

    let first = write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap();
    let mut twice = first.value.clone();
    twice.rows.push(twice.rows[0].clone());
    write(&lib.target, |e| e.restore_cues(&twice)).unwrap();
    let ids = format!("SELECT count(DISTINCT ID) FROM djmdCue WHERE ContentID = '{TRACK}'");
    assert_eq!(lib.query::<i64>(&ids), 4);

    let before = lib.bytes();
    let e = write(&lib.target, |e| {
        e.conn().execute_batch("ALTER TABLE djmdContent RENAME TO gone").unwrap();
        e.update_track(TRACK, &[])
    })
    .unwrap_err();
    assert!(e.to_string().contains("djmdContent"), "{e}");
    assert_eq!(lib.bytes(), before);
}

/// A collection without cue lists or `CueUpdated` (rekordbox 6) has its cues written without them.
#[test]
fn a_collection_without_cue_lists_has_cues_written_without() {
    let f = writable();
    f.exec("DROP TABLE contentCue; ALTER TABLE djmdContent DROP COLUMN CueUpdated;");
    let lib = Lib::new(&f, false);
    let written = write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap();
    assert_eq!(written.value.content_cue, None);
    assert_eq!(written.count, (UPDATE_COUNT, UPDATE_COUNT + 1));
    assert_eq!(lib.cues(84935471).len(), 1);
}

/// [`analysed`], encrypted, with [`TRACK`]'s `.DAT` and `.EXT` in the share folder; and that
/// folder.
fn analysed_lib() -> (Lib, PathBuf) {
    let lib = Lib::new(&analysed(b"dat", b"ext"), false);
    let dir = lib.target.master_db.parent().unwrap().join("share").join(ANALYSIS_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ANLZ0000.DAT"), b"dat").unwrap();
    std::fs::write(dir.join("ANLZ0000.EXT"), b"ext").unwrap();
    (lib, dir)
}

/// The files in `dir`, by name, with what they hold.
fn files_in(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap_or_default()))
        .collect();
    out.sort();
    out
}

fn named(files: &[(&str, &[u8])]) -> Vec<(String, Vec<u8>)> {
    files.iter().map(|(n, b)| (n.to_string(), b.to_vec())).collect()
}

#[test]
fn analysis_files_are_backed_up_replaced_and_listed() {
    let (lib, dir) = analysed_lib();
    let written = write(&lib.target, |e| {
        assert_eq!(e.read_analysis_file(TRACK, "ext")?, Some(b"ext".to_vec()));
        assert_eq!(e.read_analysis_file(TRACK, "2EX")?, None);
        let ext = e.replace_analysis_file(TRACK, "EXT", b"new ext")?;
        let dat = e.replace_analysis_file(TRACK, "DAT", b"dat")?;
        Ok((ext, dat))
    })
    .unwrap();
    let (ext, dat) = written.value;
    // A file holding the bytes already isn't replaced.
    assert_eq!(dat, None);
    let ext = ext.unwrap();
    assert_eq!(ext.path, "/PIONEER/USBANLZ/P016/0000875E/ANLZ0000.EXT");
    assert_eq!(ext.md5, share::md5_hex(b"ext"));
    // The backup sits beside master.db's, holding what the file held.
    assert_eq!(ext.backup, PathBuf::from(format!("{}.share{}", written.backup.display(), ext.path)));
    assert_eq!(std::fs::read(&ext.backup).unwrap(), b"ext");
    assert_eq!(files_in(&dir), named(&[("ANLZ0000.DAT", b"dat"), ("ANLZ0000.EXT", b"new ext")]));

    // The file's row has its new hash and size, then the track is stamped, its AnalysisUpdated up
    // by one; the counter ends at the last stamp.
    assert_eq!(written.count, (UPDATE_COUNT, UPDATE_COUNT + 2));
    let row = lib.query::<String>("SELECT Hash || ' ' || Size || ' ' || rb_local_usn FROM contentFile WHERE ID = 'f2'");
    assert_eq!(row, format!("{} 7 {}", share::md5_hex(b"new ext"), UPDATE_COUNT + 1));
    assert_eq!(lib.query::<i64>("SELECT rb_local_usn FROM contentFile WHERE ID = 'f1'"), 74001);
    let track = format!("SELECT AnalysisUpdated || ' ' || rb_local_usn FROM djmdContent WHERE ID = '{TRACK}'");
    assert_eq!(lib.query::<String>(&track), format!("2 {}", UPDATE_COUNT + 2));
    // The reader doesn't take the file to be still syncing.
    let c = open(&lib.target.master_db, DEFAULT_KEY).unwrap();
    assert!(!c.still_syncing(&ext.path));

    // Undo puts the old bytes back as a new change.
    let undone = write(&lib.target, |e| e.restore_analysis_file(&ext)).unwrap();
    assert_eq!(files_in(&dir), named(&[("ANLZ0000.DAT", b"dat"), ("ANLZ0000.EXT", b"ext")]));
    assert_eq!(undone.count, (UPDATE_COUNT + 2, UPDATE_COUNT + 4));
    assert_eq!(lib.query::<String>(&track), format!("3 {}", UPDATE_COUNT + 4));
    let hash = lib.query::<String>("SELECT Hash FROM contentFile WHERE ID = 'f2'");
    assert_eq!(hash, share::md5_hex(b"ext"));
    assert_eq!(std::fs::read(&undone.value.unwrap().backup).unwrap(), b"new ext");
}

/// A write refused at the last moment leaves the files as they were, and nothing beside them or
/// in the backups.
#[test]
fn a_refused_write_leaves_the_files() {
    fn starts_later() -> Running {
        Running { rekordbox: SHARE_CALLS.fetch_add(1, Ordering::SeqCst) > 0, agent: false }
    }
    static SHARE_CALLS: AtomicUsize = AtomicUsize::new(0);
    let (mut lib, dir) = analysed_lib();
    lib.target.running = starts_later;
    let before = lib.bytes();
    let e = write(&lib.target, |e| e.replace_analysis_file(TRACK, "EXT", b"new")).unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::RekordboxRunning)), "{e}");
    assert_eq!(lib.bytes(), before);
    assert_eq!(files_in(&dir), named(&[("ANLZ0000.DAT", b"dat"), ("ANLZ0000.EXT", b"ext")]));
    assert_eq!(std::fs::read_dir(&lib.target.backups).unwrap().count(), 0);
}

/// A file changed underneath (rekordbox analysed it again) stops the swap, and the change stays.
#[test]
fn a_file_changed_underneath_is_kept() {
    let (lib, dir) = analysed_lib();
    let before = lib.bytes();
    let e = write(&lib.target, |e| {
        let replaced = e.replace_analysis_file(TRACK, "EXT", b"new");
        std::fs::write(dir.join("ANLZ0000.EXT"), b"rekordbox's").unwrap();
        replaced
    })
    .unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::Changed)), "{e}");
    assert_eq!(lib.bytes(), before);
    assert_eq!(files_in(&dir), named(&[("ANLZ0000.DAT", b"dat"), ("ANLZ0000.EXT", b"rekordbox's")]));
    assert_eq!(std::fs::read_dir(&lib.target.backups).unwrap().count(), 0);
}

/// Only a track's own analysis files, that are there and listed as rekordbox lists them, are
/// replaced.
#[test]
fn analysis_files_that_cant_be_replaced() {
    let (lib, dir) = analysed_lib();
    let before = lib.bytes();
    let invalid: &[(&str, &str)] = &[
        // No analysis path: rekordbox hasn't analysed it.
        ("205356547", "DAT"),
        // Not a file replaced here.
        (TRACK, "3EX"),
        (TRACK, "../x"),
        // Not there.
        (TRACK, "2EX"),
    ];
    for (track, ext) in invalid {
        let e = write(&lib.target, |e| e.replace_analysis_file(track, ext, b"x")).unwrap_err();
        assert!(matches!(e, Error::Invalid(_)), "{track} {ext}: {e}");
    }
    let e = write(&lib.target, |e| {
        e.replace_analysis_file(TRACK, "EXT", b"x")?;
        e.replace_analysis_file(TRACK, "ext", b"y")
    })
    .unwrap_err();
    assert!(matches!(e, Error::Invalid(_)), "twice: {e}");
    // A file without its contentFile row.
    let e = write(&lib.target, |e| {
        e.conn().execute("DELETE FROM contentFile WHERE ID = 'f2'", []).unwrap();
        e.replace_analysis_file(TRACK, "EXT", b"x")
    })
    .unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::Odd(_))), "{e}");
    // A path out of the share folder.
    let e = write(&lib.target, |e| {
        let sql =
            format!("UPDATE djmdContent SET AnalysisDataPath = '/PIONEER/USBANLZ/../../x.DAT' WHERE ID = '{TRACK}'");
        e.conn().execute(&sql, []).unwrap();
        e.replace_analysis_file(TRACK, "DAT", b"x")
    })
    .unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::Odd(_))), "{e}");
    assert_eq!(lib.bytes(), before);
    assert_eq!(files_in(&dir), named(&[("ANLZ0000.DAT", b"dat"), ("ANLZ0000.EXT", b"ext")]));
    assert_eq!(std::fs::read_dir(&lib.target.backups).unwrap().count(), 0);
}

/// Undo puts back only a backup from the collection's own backups folder, into the file it was
/// taken from: not after rekordbox has analysed the track again into other files.
#[test]
fn undo_takes_only_its_own_backups_into_the_same_file() {
    let (lib, dir) = analysed_lib();
    let ext = write(&lib.target, |e| e.replace_analysis_file(TRACK, "EXT", b"new ext")).unwrap().value.unwrap();
    let before = lib.bytes();

    let elsewhere = lib.target.master_db.parent().unwrap().join("elsewhere.EXT");
    std::fs::write(&elsewhere, b"ext").unwrap();
    let forged = FileBefore { backup: elsewhere, ..ext.clone() };
    let e = write(&lib.target, |e| e.restore_analysis_file(&forged)).unwrap_err();
    assert!(matches!(e, Error::Invalid(_)), "{e}");

    let moved = format!(
        "UPDATE djmdContent SET AnalysisDataPath = '/PIONEER/USBANLZ/P099/X/ANLZ0000.DAT' WHERE ID = '{TRACK}'"
    );
    let e = write(&lib.target, |e| {
        e.conn().execute(&moved, []).unwrap();
        e.restore_analysis_file(&ext)
    })
    .unwrap_err();
    assert!(matches!(e, Error::Invalid(_)), "{e}");

    assert_eq!(lib.bytes(), before);
    assert_eq!(files_in(&dir), named(&[("ANLZ0000.DAT", b"dat"), ("ANLZ0000.EXT", b"new ext")]));
}

/// A new file left beside one (by a crash, or another write) refuses the write, and stays.
#[test]
fn a_new_file_there_already_is_left_alone() {
    let (lib, dir) = analysed_lib();
    let before = lib.bytes();
    std::fs::write(dir.join("ANLZ0000.EXT.locallibrary-new"), b"theirs").unwrap();
    let e = write(&lib.target, |e| e.replace_analysis_file(TRACK, "EXT", b"new")).unwrap_err();
    assert!(matches!(e, Error::Io { .. }), "{e}");
    assert_eq!(lib.bytes(), before);
    assert_eq!(
        files_in(&dir),
        named(&[("ANLZ0000.DAT", b"dat"), ("ANLZ0000.EXT", b"ext"), ("ANLZ0000.EXT.locallibrary-new", b"theirs")])
    );
}

/// A file that can't be renamed in once master.db has been swapped in fails the write, and keeps
/// master.db's backup and the files' backups, which are all that can put it back.
#[cfg(unix)]
#[test]
fn a_file_that_cant_be_swapped_in_keeps_the_backups() {
    use std::os::unix::fs::PermissionsExt;
    let (lib, dir) = analysed_lib();
    let mode = |m: u32| std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(m)).unwrap();
    let result = write(&lib.target, |e| {
        let replaced = e.replace_analysis_file(TRACK, "EXT", b"new ext");
        mode(0o555);
        replaced
    });
    mode(0o755);
    // Where the folder can be written anyway (as root), the write goes through.
    let Err(e) = result else { return };
    assert!(matches!(e, Error::Io { .. }), "{e}");
    assert_eq!(std::fs::read(dir.join("ANLZ0000.EXT")).unwrap(), b"ext");
    let mut kept: Vec<String> = std::fs::read_dir(&lib.target.backups)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    kept.sort();
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert!(kept[0].ends_with(".db") && kept[1] == format!("{}.share", kept[0]), "{kept:?}");
    let share_backup = lib.target.backups.join(&kept[1]).join(ANALYSIS_DIR).join("ANLZ0000.EXT");
    assert_eq!(std::fs::read(share_backup).unwrap(), b"ext");
}

/// A file that isn't what its contentFile row lists (half synced, or changed outside rekordbox),
/// or whose row says it's somewhere else on this computer, isn't replaced.
#[test]
fn a_file_its_row_doesnt_list_is_refused() {
    let (lib, dir) = analysed_lib();
    let before = lib.bytes();
    let other = lib.target.master_db.parent().unwrap().join("other.EXT");
    std::fs::write(&other, b"ext").unwrap();
    let here = dir.join("ANLZ0000.EXT");
    let odd = [
        "UPDATE contentFile SET Hash = 'd41d8cd98f00b204e9800998ecf8427e' WHERE ID = 'f2'".to_string(),
        "UPDATE contentFile SET Size = 4 WHERE ID = 'f2'".to_string(),
        format!("UPDATE contentFile SET rb_local_path = '{}' WHERE ID = 'f2'", other.display()),
        format!("UPDATE contentFile SET rb_local_path = '{}' WHERE ID = 'f2'", dir.join("gone.EXT").display()),
    ];
    for sql in &odd {
        let e = write(&lib.target, |e| {
            e.conn().execute(sql, []).unwrap();
            e.replace_analysis_file(TRACK, "EXT", b"new")
        })
        .unwrap_err();
        assert!(matches!(&e, Error::Refused(Refusal::Odd(why)) if why.contains("ANLZ0000.EXT")), "{sql}: {e}");
    }
    assert_eq!(lib.bytes(), before);
    assert_eq!(files_in(&dir), named(&[("ANLZ0000.DAT", b"dat"), ("ANLZ0000.EXT", b"ext")]));
    assert_eq!(std::fs::read_dir(&lib.target.backups).unwrap().count(), 0);

    // Its own path, as rekordbox gives it, is fine.
    let sql = format!("UPDATE contentFile SET rb_local_path = '{}' WHERE ID = 'f2'", here.display());
    write(&lib.target, |e| {
        e.conn().execute(&sql, []).unwrap();
        e.replace_analysis_file(TRACK, "EXT", b"new")
    })
    .unwrap();
    assert_eq!(std::fs::read(&here).unwrap(), b"new");
}

/// Undo leaves a file rekordbox analysed again since the save, which keeps its path and row and
/// gets new bytes and a new hash: putting the backup back would throw that analysis away.
#[test]
fn undo_leaves_a_file_analysed_again_in_place() {
    let (lib, dir) = analysed_lib();
    let ext = write(&lib.target, |e| e.replace_analysis_file(TRACK, "EXT", b"new ext")).unwrap().value.unwrap();
    assert_eq!(ext.after_md5, share::md5_hex(b"new ext"));
    // rekordbox analyses it again: the file and its row change, the path doesn't.
    std::fs::write(dir.join("ANLZ0000.EXT"), b"rekordbox's").unwrap();
    let again =
        format!("UPDATE contentFile SET Hash = '{}', Size = 11 WHERE ID = 'f2'", share::md5_hex(b"rekordbox's"));
    let e = write(&lib.target, |e| {
        e.conn().execute(&again, []).unwrap();
        e.restore_analysis_file(&ext)
    })
    .unwrap_err();
    assert!(matches!(&e, Error::Refused(Refusal::Odd(why)) if why.contains("analysed it again")), "{e}");
    assert_eq!(std::fs::read(dir.join("ANLZ0000.EXT")).unwrap(), b"rekordbox's");
}

/// Analysis paths that name a file only by Windows rewriting them (a trailing dot or space) are
/// refused, and so is one file named twice in one write by two tracks spelling it in other cases.
#[test]
fn analysis_paths_windows_rewrites_or_case_folds_together() {
    let (lib, dir) = analysed_lib();
    let before = lib.bytes();
    for odd in ["/PIONEER/USBANLZ/P016./0000875E/ANLZ0000.DAT", "/PIONEER/USBANLZ/P016/0000875E /ANLZ0000.DAT"] {
        let e = write(&lib.target, |e| {
            let sql = format!("UPDATE djmdContent SET AnalysisDataPath = '{odd}' WHERE ID = '{TRACK}'");
            e.conn().execute(&sql, []).unwrap();
            e.read_analysis_file(TRACK, "DAT")
        })
        .unwrap_err();
        assert!(matches!(e, Error::Refused(Refusal::Odd(_))), "{odd}: {e}");
    }
    let e = write(&lib.target, |e| {
        let folded = format!("/{}/ANLZ0000.DAT", ANALYSIS_DIR.to_lowercase());
        let sql = format!("UPDATE djmdContent SET AnalysisDataPath = '{folded}' WHERE ID = '205356547'");
        e.conn().execute(&sql, []).unwrap();
        e.replace_analysis_file(TRACK, "EXT", b"x")?;
        e.replace_analysis_file("205356547", "EXT", b"y")
    })
    .unwrap_err();
    assert!(matches!(&e, Error::Invalid(why) if why.contains("twice")), "{e}");
    assert_eq!(lib.bytes(), before);
    assert_eq!(files_in(&dir), named(&[("ANLZ0000.DAT", b"dat"), ("ANLZ0000.EXT", b"ext")]));
}

/// Reading a track's analysis file doesn't follow a folder linked out of the share folder.
#[cfg(unix)]
#[test]
fn analysis_files_are_read_only_from_the_share_folder() {
    let (lib, dir) = analysed_lib();
    let outside = lib.target.master_db.parent().unwrap().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("ANLZ0000.DAT"), b"not rekordbox's").unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    std::os::unix::fs::symlink(&outside, &dir).unwrap();
    let e = write(&lib.target, |e| e.read_analysis_file(TRACK, "DAT")).unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::Odd(_))), "{e}");
}

/// A backup of a file that doesn't read back as what the file held stops the swap: it would be
/// all that could put the file back.
#[test]
fn a_backup_that_doesnt_read_back_stops_the_write() {
    let (lib, dir) = analysed_lib();
    let before = lib.bytes();
    let e = write(&lib.target, |e| {
        let replaced = e.replace_analysis_file(TRACK, "EXT", b"new")?.unwrap();
        std::fs::write(&replaced.backup, b"exx").unwrap();
        Ok(replaced)
    })
    .unwrap_err();
    assert!(matches!(&e, Error::Refused(Refusal::Odd(why)) if why.contains("backup")), "{e}");
    assert_eq!(lib.bytes(), before);
    assert_eq!(files_in(&dir), named(&[("ANLZ0000.DAT", b"dat"), ("ANLZ0000.EXT", b"ext")]));
    assert_eq!(std::fs::read_dir(&lib.target.backups).unwrap().count(), 0);
}
