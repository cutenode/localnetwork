use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

use hegel::TestCase;
use hegel::generators as gs;
use loclib_model::analysis::{Cue, hot_cue_palette};

use super::*;
use crate::masterdb::{DEFAULT_KEY, open};
use crate::test_util::{Fixture, UPDATE_COUNT, codec, scratch_db, writable};

thread_local! {
    /// Run on the new file just before it's read back.
    pub(super) static BEFORE_READ_BACK: Cell<Option<fn(&Path)>> = const { Cell::new(None) };
    /// Whether the rename that swaps the new file in fails.
    pub(super) static SWAP_FAILS: Cell<bool> = const { Cell::new(false) };
}

/// Panics should a test's write be pointed anywhere but the temporary folder, or anywhere in
/// rekordbox's folders: tests write fixtures in temporary folders only. Links are followed, and
/// case doesn't matter where the filesystem ignores it.
pub(super) fn not_rekordboxs_own(path: &Path) {
    let real = |p: &Path| {
        let parent = || Some(p.parent()?.canonicalize().ok()?.join(p.file_name()?));
        p.canonicalize().ok().or_else(parent).unwrap_or_else(|| p.to_path_buf())
    };
    let fold = |p: PathBuf| {
        if cfg!(any(target_os = "macos", windows)) { p.to_string_lossy().to_lowercase().into() } else { p }
    };
    let path = fold(real(path));
    let temp = fold(real(&std::env::temp_dir()));
    assert!(path.starts_with(&temp), "a test tried to write {}, outside {}", path.display(), temp.display());
    let base = directories::BaseDirs::new();
    for theirs in base.iter().flat_map(|b| [b.home_dir().join("Library/Pioneer"), b.config_dir().join("Pioneer")]) {
        assert!(!path.starts_with(fold(real(&theirs))), "a test tried to write {}", path.display());
    }
}

fn closed() -> Option<Running> {
    Some(Running::default())
}

fn measured_version() -> Option<String> {
    Some("7.2.18".into())
}

/// An agent log whose last run says sync is off.
const STANDALONE: &str = "[261126T012213.000-0600] I/main \n###\n  rekordboxAgent/3.1.4 (darwin_arm64; 27.0.0)\n###\n\
                          [261126T012213.828-0600] I/sync mode: null => standalone\n";

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
        std::fs::write(&log, STANDALONE).unwrap();
        std::fs::create_dir(dir.path().join("work")).unwrap();
        let target = Target {
            master_db: rekordbox.join("master.db"),
            key: DEFAULT_KEY.into(),
            backups: dir.path().join("backups"),
            agent_log: Some(log),
            work: Some(dir.path().join("work")),
            running: closed,
            version: measured_version,
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

    /// Track `track`'s `djmdCue` rows, every column, by `ID`.
    fn cue_rows(&self, track: &str) -> indexmap::IndexMap<String, Vec<(String, Value)>> {
        let (_dir, conn) = self.sql();
        let mut stmt = conn.prepare("SELECT * FROM djmdCue WHERE ContentID = ?1 ORDER BY ID").unwrap();
        let names: Vec<String> = stmt.column_names().map(|n| n.to_string()).collect();
        let rows = stmt.query_map([track], |r| {
            let row: Vec<(String, Value)> =
                names.iter().enumerate().map(|(i, n)| (n.clone(), r.get(i).unwrap())).collect();
            Ok((row[0].1.clone(), row))
        });
        rows.unwrap().map(Result::unwrap).map(|(id, row)| (crate::masterdb::text(&id), row)).collect()
    }

    /// Files left anywhere a write puts them but in `master.db`, its lock and the backups (when
    /// `backups`).
    fn leftovers(&self, backups: bool) -> Vec<String> {
        let mut out = Vec::new();
        let rekordbox = self.target.master_db.parent().unwrap();
        let lock = beside(&self.target.master_db, "locallibrary-lock");
        for dir in [rekordbox, &self.dir.path().join("work"), &self.target.backups] {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let keep =
                    e.path() == self.target.master_db || e.path() == lock || (backups && dir == self.target.backups);
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

    /// Track `track`'s cues as the reader gives them, as a player passes them back.
    fn entries(&self, track: u32) -> Vec<CueEntry> {
        self.cues(track).iter().map(CueEntry::from).collect()
    }
}

/// The columns two versions of a row differ in.
fn differ(a: &[(String, Value)], b: &[(String, Value)]) -> Vec<String> {
    a.iter().zip(b).filter(|(x, y)| x != y).map(|(x, _)| x.0.clone()).collect()
}

fn hot(h: u8, ms: u32) -> CueEntry {
    CueEntry { hot_cue: Some(h), start_ms: ms, ..Default::default() }
}

const TRACK: &str = "84935471";

/// A track with no cues.
const EMPTY: &str = "205356547";

/// New rows are laid out as rekordbox's newest are, and read back; the list follows them.
#[test]
fn cues_set_read_back_as_rekordbox_lays_them_out() {
    let lib = Lib::new(&writable(), false);
    let original = lib.bytes();
    let cues = [
        CueEntry { color: Some(22), comment: "Drop".into(), ..hot(0, 16_335) },
        CueEntry { end_ms: Some(118_100), color: Some(36), active_loop: true, ..hot(3, 110_383) },
        CueEntry { start_ms: 154, ..Default::default() },
        CueEntry { start_ms: 2000, end_ms: Some(4000), ..Default::default() },
        hot(8, 3000),
    ];
    let written = write(&lib.target, |e| e.set_cues(EMPTY, &cues)).unwrap();

    // What was there before comes back for undo: nothing.
    assert_eq!((written.value.rows.len(), written.value.content_cue), (0, None));
    // The backup is the file as it was, byte for byte, readable by its owner only.
    assert_eq!(std::fs::read(written.backup.as_ref().unwrap()).unwrap(), original);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!((mode(written.backup.as_ref().unwrap()), mode(&lib.target.backups)), (0o600, 0o700));
    }
    // Encrypted again with the same salt, and back in WAL mode.
    let now = lib.bytes();
    assert_eq!(now[..16], original[..16]);
    assert_eq!(crate::test_util::plain(&lib.target.master_db)[18..20], [2, 2]);
    assert_eq!(lib.leftovers(true), Vec::<String>::new());

    // The reader reads them back.
    let read = lib.cues(205356547);
    let summary: Vec<_> = read.iter().map(|c| (c.hot_cue, c.start_ms, c.end_ms, c.comment.as_str(), c.color)).collect();
    assert_eq!(
        summary,
        [
            (None, 154, None, "", None),
            (None, 2000, Some(4000), "", None),
            (Some(0), 16_335, None, "Drop", hot_cue_palette(22)),
            (Some(3), 110_383, Some(118_100), "", hot_cue_palette(36)),
            (Some(8), 3000, None, "", None),
        ]
    );

    // Stamps: the list first, then the track; the counter ends at the last.
    assert_eq!(written.count, (UPDATE_COUNT, UPDATE_COUNT + 2));
    assert_eq!(
        lib.query::<i64>("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'"),
        UPDATE_COUNT + 2
    );
    assert_eq!(lib.query::<i64>("SELECT rb_local_usn FROM contentCue"), UPDATE_COUNT + 1);
    let track = format!("SELECT rb_local_usn || ' ' || CueUpdated FROM djmdContent WHERE ID = '{EMPTY}'");
    assert_eq!(lib.query::<String>(&track), format!("{} 2", UPDATE_COUNT + 2));
    let at: String = lib.query(&format!("SELECT updated_at FROM djmdContent WHERE ID = '{EMPTY}'"));
    assert!(chrono::DateTime::parse_from_str(&at, "%Y-%m-%d %H:%M:%S%.3f %:z").is_ok(), "{at}");
    assert_eq!(at.len(), "2026-11-26 07:24:06.375 +00:00".len(), "{at}");
    // Cue rows aren't stamped, as rekordbox leaves them.
    assert_eq!(lib.query::<i64>("SELECT count(*) FROM djmdCue WHERE rb_local_usn IS NOT NULL OR usn IS NOT NULL"), 0);

    // The rows, as rekordbox writes them.
    let (_d, conn) = lib.sql();
    let mut stmt = conn
        .prepare(
            "SELECT InMsec, InFrame, OutMsec, OutFrame, Kind, Color, ColorTableIndex, ActiveLoop, Comment, \
             ContentUUID, length(ID) <= 10 AND CAST(ID AS INTEGER) > 0, length(UUID) \
             FROM djmdCue WHERE ContentID = ?1 ORDER BY rowid",
        )
        .unwrap();
    type Row = (i64, i64, i64, i64, i64, i64, Option<i64>, Option<i64>, Option<String>, String, bool, i64);
    let rows: Vec<Row> = stmt
        .query_map([EMPTY], |r| {
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
                r.get(11)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let uuid = || "uuid-205356547".to_string();
    assert_eq!(
        rows,
        [
            (16_335, 2450, -1, 0, 1, -1, Some(22), None, Some("Drop".into()), uuid(), true, 36),
            (110_383, 16_557, 118_100, 17_715, 5, -1, Some(36), Some(1), None, uuid(), true, 36),
            (154, 23, -1, 0, 0, -1, None, None, None, uuid(), true, 36),
            (2000, 300, 4000, 600, 4, -1, None, None, None, uuid(), true, 36),
            (3000, 450, -1, 0, 10, -1, None, None, None, uuid(), true, 36),
        ]
    );

    // The list: one row, keyed by the track's UUID, the rows' columns that aren't NULL in
    // rekordbox's order and spelling.
    let (id, cues_json, count): (String, String, i64) = conn
        .query_row("SELECT ID, Cues, rb_cue_count FROM contentCue", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap();
    assert_eq!((id.as_str(), count), ("uuid-205356547", 5));
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
    assert_eq!(list[0]["ContentID"], EMPTY);
    assert!(list[0]["ID"].is_string());
    assert_eq!(list[1]["ActiveLoop"], 1);
    assert!(!list[2].contains_key("ColorTableIndex") && !list[2].contains_key("Comment"));
    let created = list[0]["created_at"].as_str().unwrap();
    assert!(chrono::DateTime::parse_from_str(created, "%Y-%m-%dT%H:%M:%S%.3f%:z").is_ok(), "{created}");
    assert!(!created.contains(' '), "{created}");
}

/// [`writable`] with track [`TRACK`]'s cues as rekordbox leaves them: rows in its older shape (every
/// column filled: `Color` 255, a sub-millisecond `CueMicrosec`, MPEG seek columns, a loop's
/// `BeatLoopSize`, an empty `Comment`) and its newer one (NULLs), a memory loop kept as `Kind` 0,
/// and two first-bar memory cues a few ms apart from two analysis passes, both named. Its list is
/// from before the second pass.
fn kept() -> Fixture {
    let f = writable();
    f.exec(&format!(
        "UPDATE djmdCue SET Color = 255, ActiveLoop = 0, BeatLoopSize = 0, CueMicrosec = 0, InMpegFrame = 0,
             InMpegAbs = 0, OutMpegFrame = 0, OutMpegAbs = 0, InFrame = InMsec * 150 / 1000,
             OutFrame = max(OutMsec, 0) * 150 / 1000, UUID = 'cue-' || ID,
             created_at = '2026-01-02 03:04:05.678 +00:00', updated_at = '2026-01-02 03:04:05.679 +00:00'
             WHERE ContentID = '{TRACK}';
         UPDATE djmdCue SET CueMicrosec = 927 WHERE ID = 'c1';
         UPDATE djmdCue SET InMpegFrame = 12, InMpegAbs = 34 WHERE ID = 'c2';
         UPDATE djmdCue SET BeatLoopSize = 524289 WHERE ID = 'c3';
         INSERT INTO djmdCue (ID, ContentID, ContentUUID, UUID, InMsec, InFrame, InMpegFrame, InMpegAbs, OutMsec,
             OutFrame, OutMpegFrame, OutMpegAbs, Kind, Color, ColorTableIndex, ActiveLoop, Comment, BeatLoopSize,
             CueMicrosec, created_at, updated_at) VALUES
             ('c5', '{TRACK}', 'uuid-{TRACK}', 'cue-c5', 6000, 900, 0, 0, 9000, 1350, 0, 0, 0, -1, NULL, NULL, NULL,
                 NULL, NULL, '2026-02-03 01:02:03.004 +00:00', '2026-02-03 01:02:03.004 +00:00'),
             ('c6', '{TRACK}', 'uuid-{TRACK}', 'cue-c6', 15, 2, 0, 0, -1, 0, 0, 0, 0, 255, 0, 0, '1.1Bars', 0, 0,
                 '2026-01-02 03:04:05.678 +00:00', '2026-01-02 03:04:05.678 +00:00'),
             ('c7', '{TRACK}', 'uuid-{TRACK}', 'cue-c7', 24, 3, 0, 0, -1, 0, 0, 0, 0, -1, NULL, NULL, '1.1Bars', NULL,
                 NULL, '2026-03-04 05:06:07.080 +00:00', '2026-03-04 05:06:07.080 +00:00');
         INSERT INTO contentCue (ID, ContentID, Cues, rb_cue_count, UUID) VALUES
             ('uuid-{TRACK}', '{TRACK}', '[{{\"ID\":\"c6\",\"InMsec\":15}}]', 1, 'list-{TRACK}');"
    ));
    f
}

/// An edit writes only what it changes: a moved hot cue is the same row with its position and
/// what seeks to it changed, and every other row stays as stored, column for column (rekordbox's
/// leftovers, the old shape and the new, a loop kept as `Kind` 0). The list is made from the rows.
#[test]
fn an_edit_keeps_every_row_it_doesnt_change() {
    let lib = Lib::new(&kept(), false);
    let before = lib.cue_rows(TRACK);
    let mut cues = lib.entries(84935471);
    let a = cues.iter_mut().find(|c| c.hot_cue == Some(0)).unwrap();
    a.start_ms = 1500;
    let written = write(&lib.target, |e| e.set_cues(TRACK, &cues)).unwrap();
    let after = lib.cue_rows(TRACK);
    assert_eq!(after.keys().collect::<Vec<_>>(), before.keys().collect::<Vec<_>>());
    for (id, row) in &before {
        let changed = differ(row, &after[id]);
        let want: &[&str] = if id == "c1" { &["InMsec", "InFrame", "CueMicrosec", "updated_at"] } else { &[] };
        assert_eq!(changed, want, "{id}");
    }
    let moved = &after["c1"];
    let get = |name: &str| moved.iter().find(|(n, _)| n == name).unwrap().1.clone();
    assert_eq!((get("InMsec"), get("InFrame"), get("CueMicrosec")), (1500.into(), 225.into(), 0.into()));
    assert_eq!(written.value.rows.len(), 6);
    // The list follows the rows: both first bars named, the loop kept as Kind 0.
    let list: String = lib.query(&format!("SELECT Cues FROM contentCue WHERE ContentID = '{TRACK}'"));
    assert_eq!(list.matches("1.1Bars").count(), 2, "{list}");
    assert!(list.contains(r#""ID":"c5","ContentID":"84935471","ContentUUID":"uuid-84935471","InMsec":6000,"InFrame":900,"InMpegFrame":0,"InMpegAbs":0,"OutMsec":9000,"OutFrame":1350,"OutMpegFrame":0,"OutMpegAbs":0,"Kind":0,"Color":-1,"UUID":"cue-c5""#), "{list}");
}

/// Changing a cue in place writes the columns whose meaning changed, in the row's shape; a cue
/// added is one new row; a cue taken away is that row gone.
#[test]
fn cues_changed_in_place_added_and_taken_away() {
    let lib = Lib::new(&kept(), false);
    let before = lib.cue_rows(TRACK);
    let mut cues = lib.entries(84935471);
    let b = cues.iter_mut().find(|c| c.hot_cue == Some(1)).unwrap();
    b.end_ms = Some(3000);
    b.comment = String::new();
    let memory = cues.iter_mut().find(|c| c.start_ms == 500).unwrap();
    memory.comment = "Intro".into();
    let loop_kept_as_0 = cues.iter_mut().find(|c| c.start_ms == 6000).unwrap();
    loop_kept_as_0.end_ms = None;
    cues.retain(|c| c.start_ms != 15);
    cues.push(CueEntry { start_ms: 7000, ..Default::default() });
    write(&lib.target, |e| e.set_cues(TRACK, &cues)).unwrap();
    let after = lib.cue_rows(TRACK);
    let get = |id: &str, name: &str| after[id].iter().find(|(n, _)| n == name).unwrap().1.clone();

    // A loop of another length loses its loop size; an emptied name keeps the row's empty text.
    assert_eq!(differ(&before["c3"], &after["c3"]), ["OutMsec", "OutFrame", "Comment", "BeatLoopSize", "updated_at"]);
    assert_eq!((get("c3", "OutMsec"), get("c3", "OutFrame")), (3000.into(), 450.into()));
    assert_eq!((get("c3", "Comment"), get("c3", "BeatLoopSize")), (Value::Text(String::new()), 0.into()));
    // A memory cue renamed is the same row; its seek columns stay, since it didn't move.
    assert_eq!(differ(&before["c2"], &after["c2"]), ["Comment", "updated_at"]);
    // A loop that's a cue now: Kind 0 already, its end gone, its NULLs kept NULL.
    assert_eq!(differ(&before["c5"], &after["c5"]), ["OutMsec", "OutFrame", "updated_at"]);
    assert_eq!((get("c5", "OutMsec"), get("c5", "BeatLoopSize")), ((-1).into(), Value::Null));
    assert!(!after.contains_key("c6"));
    let new: Vec<_> = after.keys().filter(|id| !before.contains_key(*id)).collect();
    assert_eq!(new.len(), 1, "{new:?}");
    for id in ["c1", "c7"] {
        assert_eq!(differ(&before[id], &after[id]), Vec::<String>::new(), "{id}");
    }
}

/// A row of a hot cue past H (rekordbox's I is `Kind` 10) stays when the track's other cues are
/// edited; hot cues up to P can be given, and none past it.
#[test]
fn hot_cues_past_h_are_kept_and_go_up_to_p() {
    let f = writable();
    f.exec(&format!(
        "INSERT INTO djmdCue (ID, ContentID, ContentUUID, InMsec, OutMsec, Kind) VALUES ('i', '{TRACK}', 'uuid-{TRACK}', 7000, -1, 10)"
    ));
    let lib = Lib::new(&f, false);
    let before = lib.cue_rows(TRACK)["i"].clone();
    let mut cues = lib.entries(84935471);
    assert!(cues.iter().any(|c| c.hot_cue == Some(8)));
    cues.push(hot(15, 8000));
    write(&lib.target, |e| e.set_cues(TRACK, &cues)).unwrap();
    assert_eq!(lib.cue_rows(TRACK)["i"], before);
    assert_eq!(lib.query::<i64>("SELECT Kind FROM djmdCue WHERE InMsec = 8000"), 17);
    cues.push(hot(16, 9000));
    let e = write(&lib.target, |e| e.set_cues(TRACK, &cues)).unwrap_err();
    assert!(e.to_string().contains("past P"), "{e}");
}

/// A list made from rows in rekordbox's full shape is byte for byte what rekordbox 7.2.14 writes:
/// the order and spelling of every key a row can fill (seen on 62 real lists), an empty name and
/// NULLs left out, times in its list format.
#[test]
fn a_list_is_what_rekordbox_writes() {
    let f = writable();
    f.exec(
        "INSERT INTO djmdCue (ID, ContentID, ContentUUID, InMsec, InFrame, InMpegFrame, InMpegAbs, OutMsec, OutFrame,
             OutMpegFrame, OutMpegAbs, Kind, Color, ColorTableIndex, ActiveLoop, Comment, BeatLoopSize, CueMicrosec,
             UUID, created_at, updated_at) VALUES
             ('101', '4', 'uuid-4', 48, 7, 0, 0, -1, 0, 0, 0, 1, 255, 5, 0, '', 0, 0, 'cue-101',
                 '2026-01-02 03:04:05.678 +00:00', '2026-01-02 03:04:05.679 +00:00'),
             ('102', '4', 'uuid-4', 8048, 1207, 0, 0, 16048, 2407, 0, 0, 2, 255, 30, 0, 'build', 262145, 0, 'cue-102',
                 '2026-01-02 03:04:05.679 +00:00', '2026-01-02 03:04:05.679 +00:00'),
             ('103', '4', 'uuid-4', 100, 15, 0, 0, -1, 0, 0, 0, 0, -1, NULL, NULL, NULL, NULL, NULL, 'cue-103',
                 '2026-02-03 01:02:03.004 +00:00', '2026-02-03 01:02:03.004 +00:00');",
    );
    let lib = Lib::new(&f, false);
    let cues = lib.entries(4);
    write(&lib.target, |e| e.set_cues("4", &cues)).unwrap();
    let common = r#""ContentID":"4","ContentUUID":"uuid-4""#;
    let want = format!(
        r#"[{{"ID":"101",{common},"InMsec":48,"InFrame":7,"InMpegFrame":0,"InMpegAbs":0,"OutMsec":-1,"OutFrame":0,"OutMpegFrame":0,"OutMpegAbs":0,"Kind":1,"Color":255,"ColorTableIndex":5,"ActiveLoop":0,"BeatLoopSize":0,"CueMicrosec":0,"UUID":"cue-101","created_at":"2026-01-02T03:04:05.678+00:00","updated_at":"2026-01-02T03:04:05.679+00:00"}},{{"ID":"102",{common},"InMsec":8048,"InFrame":1207,"InMpegFrame":0,"InMpegAbs":0,"OutMsec":16048,"OutFrame":2407,"OutMpegFrame":0,"OutMpegAbs":0,"Kind":2,"Color":255,"ColorTableIndex":30,"ActiveLoop":0,"Comment":"build","BeatLoopSize":262145,"CueMicrosec":0,"UUID":"cue-102","created_at":"2026-01-02T03:04:05.679+00:00","updated_at":"2026-01-02T03:04:05.679+00:00"}},{{"ID":"103",{common},"InMsec":100,"InFrame":15,"InMpegFrame":0,"InMpegAbs":0,"OutMsec":-1,"OutFrame":0,"OutMpegFrame":0,"OutMpegAbs":0,"Kind":0,"Color":-1,"UUID":"cue-103","created_at":"2026-02-03T01:02:03.004+00:00","updated_at":"2026-02-03T01:02:03.004+00:00"}}]"#
    );
    assert_eq!(lib.query::<String>("SELECT Cues FROM contentCue WHERE ContentID = '4'"), want);
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

/// A drawn run of writes, each a whole cue list for a drawn track, on a collection encrypted or
/// not, some row perhaps stamped past the count: each reads back through the collection's reader,
/// and the counter only ever rises, past every stamp in every table.
#[hegel::test(test_cases = 15)]
fn prop_writes_read_back_and_the_counter_only_rises(tc: TestCase) {
    let f = writable();
    if tc.draw(gs::booleans()) {
        let past = UPDATE_COUNT + tc.draw(gs::integers::<i64>().min_value(1).max_value(100));
        f.exec(&format!("UPDATE djmdPlaylist SET rb_local_usn = {past} WHERE ID = '11'"));
    }
    let lib = Lib::new(&f, tc.draw(gs::booleans()));
    let mut count = UPDATE_COUNT;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let track = tc.draw(gs::sampled_from(vec!["84935471", "205356547", "4"]));
        let n = tc.draw(gs::integers::<usize>().max_value(5));
        let mut cues = Vec::new();
        let mut letters: Vec<u8> = (0..=LAST).collect();
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
            let active_loop = end.is_some() && tc.draw(gs::booleans());
            cues.push(CueEntry { hot_cue, start_ms: start, end_ms: end, comment, color, active_loop });
        }
        let written = write(&lib.target, |e| e.set_cues(track, &cues)).unwrap();
        assert_eq!(written.count.0, count);
        assert!(written.count.1 > count);
        count = written.count.1;
        let (_dir, conn) = lib.sql();
        assert_eq!(Stamp::highest(&conn).unwrap(), count);

        let mut want: Vec<_> = cues
            .iter()
            .map(|c| (c.hot_cue, c.start_ms, c.end_ms, c.comment.clone(), c.color.and_then(hot_cue_palette)))
            .collect();
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

/// The last hot cue a drawn write gives: P.
const LAST: u8 = 15;

fn wal(l: &Lib, end: &str, bytes: &[u8]) {
    std::fs::write(format!("{}{end}", l.target.master_db.display()), bytes).unwrap();
}

/// Each refusal leaves master.db as it was, byte for byte, and nothing behind; the ones that come
/// before the backup don't even make its folder.
#[test]
fn refusals_leave_master_db_untouched() {
    fn rekordbox() -> Option<Running> {
        Some(Running { rekordbox: true, agent: true })
    }
    fn agent() -> Option<Running> {
        Some(Running { rekordbox: false, agent: true })
    }
    fn unknown() -> Option<Running> {
        None
    }
    type Setup = fn(&mut Lib);
    type Expected = fn(&Refusal) -> bool;
    let odd = |r: &Refusal| matches!(r, Refusal::Odd(_));
    let cases: &[(&str, Setup, Expected, bool)] = &[
        ("rekordbox open", |l| l.target.running = rekordbox, |r| *r == Refusal::RekordboxRunning, false),
        ("agent running", |l| l.target.running = agent, |r| *r == Refusal::AgentRunning, false),
        ("programs can't be listed", |l| l.target.running = unknown, odd, false),
        ("a WAL with changes", |l| wal(l, "-wal", b"x"), |r| matches!(r, Refusal::Unsaved(_)), false),
        ("shared memory in use", |l| wal(l, "-shm", b"x"), |r| matches!(r, Refusal::Unsaved(_)), false),
        ("a journal", |l| wal(l, "-journal", b"x"), |r| matches!(r, Refusal::Unsaved(_)), false),
        ("no version", |l| l.target.version = || None, odd, false),
        ("a version not checked", |l| l.target.version = || Some("7.3.0".into()), odd, false),
        ("no agent log", |l| l.target.agent_log = None, |r| matches!(r, Refusal::CloudSync(_)), true),
        ("a log saying sync", |l| log(l, "  rekordboxAgent/3\nI/sync mode: standalone => cloud\n"), sync, true),
        ("a log saying nothing", |l| log(l, "  rekordboxAgent/3\nI/started\n"), sync, true),
        ("a last run saying nothing", |l| log(l, &format!("{STANDALONE}  rekordboxAgent/3\n")), sync, true),
        ("no run's start", |l| log(l, "I/sync mode: null => standalone\n"), sync, true),
        ("an old log", |l| aged(l, 11 * 60), sync, true),
    ];
    fn sync(r: &Refusal) -> bool {
        matches!(r, Refusal::CloudSync(_))
    }
    fn log(l: &mut Lib, text: &str) {
        std::fs::write(l.target.agent_log.as_ref().unwrap(), text).unwrap();
    }
    fn aged(l: &mut Lib, secs: u64) {
        let log = File::options().write(true).open(l.target.agent_log.as_ref().unwrap()).unwrap();
        log.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs)).unwrap();
    }
    for (what, setup, expected, backed_up) in cases {
        let mut lib = Lib::new(&writable(), false);
        setup(&mut lib);
        let before = lib.bytes();
        let Err(e) = write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])) else { panic!("{what} was written") };
        let Error::Refused(why) = &e else { panic!("{what}: {e}") };
        assert!(expected(why), "{what}: {e}");
        assert_eq!(lib.bytes(), before, "{what}");
        let left: Vec<String> = lib.leftovers(false).into_iter().filter(|p| !p.contains("master.db-")).collect();
        assert_eq!(left, Vec::<String>::new(), "{what}");
        assert_eq!(lib.target.backups.exists(), *backed_up, "{what}");
        assert!(!e.to_string().is_empty());
    }
    // A log a few minutes older than master.db still speaks for it.
    let mut lib = Lib::new(&writable(), false);
    aged(&mut lib, 9 * 60);
    write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap();
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

/// What in the database says sync is (or was) on refuses the write; the storage service rekordbox
/// names even without sync doesn't.
#[test]
fn cloud_library_sync_in_the_database_refuses() {
    for (what, sql) in [
        ("usn", "UPDATE djmdContent SET usn = 5 WHERE ID = '4'"),
        ("synced", "UPDATE djmdPlaylist SET rb_local_synced = 1 WHERE ID = '10'"),
        ("cloud agent", "INSERT INTO cloudAgentRegistry (ID, int_1) VALUES ('x', 1)"),
        ("last count", "INSERT INTO agentRegistry (registry_id, int_1) VALUES ('lastUpdateCount', 7)"),
        ("last synced", "INSERT INTO agentRegistry (registry_id, str_1) VALUES ('lastSyncTime', '2026-11-01')"),
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
    let f = writable();
    f.exec(&format!(
        "INSERT INTO agentRegistry (registry_id) VALUES ('lastSyncTime');
         CREATE TABLE djmdCloudProperty (ID VARCHAR(255) PRIMARY KEY, Reserved1 TEXT, Reserved2 TEXT, {});
         INSERT INTO djmdCloudProperty (ID, Reserved1, Reserved2) VALUES ('1', 'SyncService', 'Dropbox');",
        crate::test_util::STATS
    ));
    write(&Lib::new(&f, false).target, |e| e.set_cues(TRACK, &[])).unwrap();
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

/// rekordbox starting while the edit is made stops the swap: nothing changes, nothing's left.
#[test]
fn a_failed_swap_leaves_nothing_behind() {
    fn starts_later() -> Option<Running> {
        Some(Running { rekordbox: CALLS.fetch_add(1, Ordering::SeqCst) > 0, agent: false })
    }
    let mut lib = Lib::new(&writable(), false);
    lib.target.running = starts_later;
    let before = lib.bytes();
    let e = write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::RekordboxRunning)), "{e}");
    assert_eq!(lib.bytes(), before);
    assert_eq!(lib.leftovers(false), Vec::<String>::new());
}

/// rekordbox starting as the new file is swapped in: the edit is written, the backup kept, and
/// the write says rekordbox may have the old collection open.
#[test]
fn rekordbox_starting_during_the_swap_is_told() {
    fn starts_during() -> Option<Running> {
        Some(Running { rekordbox: CALLS.fetch_add(1, Ordering::SeqCst) > 1, agent: false })
    }
    let mut lib = Lib::new(&writable(), false);
    lib.target.running = starts_during;
    let e = write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap_err();
    let Error::OpenedDuringSwap(backup) = &e else { panic!("{e}") };
    assert!(e.to_string().contains(&backup.display().to_string()), "{e}");
    assert!(backup.is_file());
    assert_eq!(lib.cues(84935471).len(), 1);
}

/// A rename that fails keeps the backup and the new file, and says where both are; the next write
/// removes the new file left and goes through.
#[test]
fn a_rename_that_fails_keeps_both_files() {
    let lib = Lib::new(&writable(), false);
    let before = lib.bytes();
    SWAP_FAILS.set(true);
    let e = write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap_err();
    SWAP_FAILS.set(false);
    let Error::SwapFailed { master_db, new, backup, .. } = &e else { panic!("{e}") };
    assert_eq!((master_db, new), (&lib.target.master_db, &beside(&lib.target.master_db, "locallibrary-new")));
    assert!(new.is_file() && backup.is_file(), "{e}");
    assert!(e.to_string().contains(&new.display().to_string()), "{e}");
    assert_eq!(lib.bytes(), before);
    write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap();
    assert!(!new.exists());
    assert_eq!(lib.cues(84935471).len(), 1);
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

/// A WAL rekordbox starts while the edit is made stops the swap: its changes would be lost.
#[test]
fn a_wal_that_appears_meanwhile_stops_the_swap() {
    let lib = Lib::new(&writable(), false);
    let before = lib.bytes();
    let e = write(&lib.target, |e| {
        wal(&lib, "-wal", b"x");
        e.set_cues(TRACK, &[hot(0, 1)])
    })
    .unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::Unsaved(_))), "{e}");
    assert_eq!(lib.bytes(), before);
    let wal = format!("{}-wal", lib.target.master_db.display());
    assert_eq!(lib.leftovers(false), [wal]);
}

/// An edit that fails changes nothing, and so does one given cues rekordbox can't hold.
#[test]
fn a_failed_edit_changes_nothing() {
    let lib = Lib::new(&writable(), false);
    let before = lib.bytes();
    let bad: &[&[CueEntry]] = &[
        &[hot(16, 1)],
        &[hot(0, 1), hot(0, 2)],
        &[CueEntry { end_ms: Some(5), ..hot(0, 10) }],
        &[CueEntry { color: Some(63), ..hot(0, 10) }],
        &[CueEntry { start_ms: 10, color: Some(3), ..Default::default() }],
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

/// Lists that aren't the ones their rows make (another name, a cue added by a script) are made
/// to; with `create_missing`, tracks without a list get one. A list marked deleted isn't one.
#[test]
fn repair_makes_the_lists_agree() {
    let f = writable();
    // A list from before a script added c3 (and a count that agrees with it), and one for a
    // track with no cues left.
    let stale = r#"[{"ID":"c1","InMsec":1000,"OutMsec":-1,"Kind":1},{"ID":"c2","InMsec":500,"OutMsec":-1,"Kind":0}]"#;
    f.exec(&format!(
        "INSERT INTO contentCue (ID, ContentID, Cues, rb_cue_count) VALUES
             ('uuid-84935471', '84935471', '{stale}', 2), ('uuid-4', '4', '{stale}', 2);
         INSERT INTO contentCue (ID, ContentID, Cues, rb_cue_count, rb_local_deleted) VALUES ('gone', '205356547', '[]', 0, 1);
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

    // A name that differs is a list that disagrees, though every cue is where the rows say.
    let f = writable();
    f.exec(&format!(
        "INSERT INTO contentCue (ID, ContentID, Cues, rb_cue_count) VALUES ('uuid-{TRACK}', '{TRACK}', '[]', 3)"
    ));
    let lib2 = Lib::new(&f, false);
    write(&lib2.target, |e| e.repair_cue_lists(false)).unwrap();
    let list: String = lib2.query(&format!("SELECT Cues FROM contentCue WHERE ContentID = '{TRACK}'"));
    write(&lib2.target, |e| {
        e.conn()
            .execute(
                &format!("UPDATE contentCue SET Cues = replace(Cues, 'Drop', 'Kick') WHERE ContentID = '{TRACK}'"),
                [],
            )
            .unwrap();
        assert_eq!(e.cue_list_mismatches(false)?, [TRACK]);
        e.repair_cue_lists(false)
    })
    .unwrap();
    assert_eq!(lib2.query::<String>(&format!("SELECT Cues FROM contentCue WHERE ContentID = '{TRACK}'")), list);

    // The deleted list isn't brought back: cues aren't written over it.
    let e = write(&lib.target, |e| e.repair_cue_lists(true)).unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::Odd(_))) && e.to_string().contains("marked deleted"), "{e}");
}

/// Only a track's `BPM` (an integer) and `KeyID` (a key there is) are set through the write path,
/// stamped, and put back the same way, whatever they held.
#[test]
fn track_columns_are_set_stamped_and_put_back() {
    let f = writable();
    f.exec("INSERT INTO djmdKey (ID, ScaleName) VALUES ('2', 'Bm')");
    let lib = Lib::new(&f, false);
    let set = [("BPM", Value::Integer(12400)), ("KeyID", Value::Text("2".into()))];
    let first = write(&lib.target, |e| e.update_track(TRACK, &set)).unwrap();
    assert_eq!(first.value.values, [("BPM".to_string(), 12280.into()), ("KeyID".to_string(), "1".into())]);
    let row = format!("SELECT BPM || ' ' || KeyID || ' ' || rb_local_usn FROM djmdContent WHERE ID = '{TRACK}'");
    assert_eq!(lib.query::<String>(&row), format!("12400 2 {}", UPDATE_COUNT + 1));
    write(&lib.target, |e| e.restore_track(&first.value)).unwrap();
    assert_eq!(lib.query::<String>(&row), format!("12280 1 {}", UPDATE_COUNT + 2));
    // A key with no row (rekordbox leaves some) is put back as it was.
    let gone = TrackBefore { track_id: TRACK.into(), values: vec![("KeyID".into(), "0".into())] };
    write(&lib.target, |e| e.restore_track(&gone)).unwrap();
    for kept in ["ID", "uuid", "USN", "Created_At", "updated_at", "rb_local_usn", "rb_local_deleted", "Title", "Nope"] {
        let e = write(&lib.target, |e| e.update_track(TRACK, &[(kept, Value::Null)])).unwrap_err();
        assert!(matches!(e, Error::Invalid(_)), "{kept}: {e}");
        let back = TrackBefore { track_id: TRACK.into(), values: vec![(kept.into(), serde_json::Value::Null)] };
        let e = write(&lib.target, |e| e.restore_track(&back)).unwrap_err();
        assert!(matches!(e, Error::Invalid(_)), "{kept} put back: {e}");
    }
    let bad: [&[(&str, Value)]; 5] = [
        &[("BPM", Value::Text("fast".into()))],
        &[("BPM", Value::Real(128.5))],
        &[("BPM", Value::Integer(-5))],
        &[("KeyID", Value::Text("999999".into()))],
        &[("bpm", Value::Integer(1)), ("BPM", Value::Integer(2))],
    ];
    for values in bad {
        let e = write(&lib.target, |e| e.update_track(TRACK, values)).unwrap_err();
        assert!(matches!(e, Error::Invalid(_)), "{values:?}: {e}");
    }
}

/// A collection without a localUpdateCount isn't one this writes, and nothing's left of the try.
#[test]
fn no_update_count_is_refused() {
    let f = writable();
    f.exec("DELETE FROM agentRegistry WHERE registry_id = 'localUpdateCount'");
    let lib = Lib::new(&f, false);
    let before = lib.bytes();
    let e = write(&lib.target, |e| e.set_cues(TRACK, &[])).unwrap_err();
    assert!(matches!(e, Error::Refused(Refusal::Odd(_))), "{e}");
    assert_eq!(lib.bytes(), before);
    assert_eq!(lib.leftovers(false), Vec::<String>::new());
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
fn rekordbox_and_its_agent_are_told_apart_by_name() {
    use super::super::files::{is_agent, is_rekordbox};
    use std::ffi::OsStr;
    for (name, agent, rekordbox) in [
        ("rekordboxAgent", true, false),
        ("rekordboxAgent.exe", true, false),
        ("rekordboxAgent Helper (Renderer)", true, false),
        ("rekordbox", false, true),
        ("rekordbox.exe", false, true),
        ("rekordbox Helper", false, true),
        ("Rekordbox 8", false, true),
        ("Finder", false, false),
    ] {
        assert_eq!((is_agent(OsStr::new(name)), is_rekordbox(OsStr::new(name))), (agent, rekordbox), "{name}");
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

/// A hot cue bank list entry left pointing at a cue an edit took away stops the write; one that
/// pointed nowhere already doesn't.
#[test]
fn hot_cue_bank_lists_keep_their_cues() {
    let f = writable();
    f.exec(&format!(
        "CREATE TABLE djmdSongHotCueBanklist (ID VARCHAR(255) PRIMARY KEY, HotCueBanklistID VARCHAR(255),
             ContentID VARCHAR(255), CueID VARCHAR(255), {});
         INSERT INTO djmdSongHotCueBanklist (ID, ContentID, CueID) VALUES ('b1', '{TRACK}', 'c1'), ('b2', '4', 'gone');",
        crate::test_util::STATS
    ));
    let lib = Lib::new(&f, false);
    let mut cues = lib.entries(84935471);
    let moved = cues.iter_mut().find(|c| c.hot_cue == Some(0)).unwrap();
    moved.start_ms += 10;
    write(&lib.target, |e| e.set_cues(TRACK, &cues)).unwrap();
    let before = lib.bytes();
    let e = write(&lib.target, |e| e.set_cues(TRACK, &[])).unwrap_err();
    assert!(e.to_string().contains("djmdSongHotCueBanklist"), "{e}");
    assert_eq!(lib.bytes(), before);
}

/// This computer's collection is the one the environment names, with what finds rekordbox
/// running (which lists this process, so it can tell) and its version.
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
    assert!((target.running)().is_some());
    let _ = (target.version)();
}

/// Without a folder of its own for working copies, a write uses the system's temporary folder.
#[test]
fn working_copies_go_in_the_systems_temporary_folder_by_default() {
    let mut lib = Lib::new(&writable(), false);
    lib.target.work = None;
    write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap();
    assert_eq!(lib.cues(84935471).len(), 1);
}

/// Only refusals that pass once rekordbox has quit, or another write is done, are worth trying
/// again.
#[test]
fn only_a_running_rekordbox_a_change_or_another_write_waits() {
    for waits in [Refusal::RekordboxRunning, Refusal::AgentRunning, Refusal::Changed, Refusal::Busy] {
        assert!(waits.waits(), "{waits}");
    }
    let path = PathBuf::from("master.db-wal");
    for not in [Refusal::Unsaved(path), Refusal::CloudSync("x".into()), Refusal::Odd("x".into())] {
        assert!(!not.waits(), "{not}");
    }
}

/// A collection with what rekordbox never makes, a `master.db` that isn't a file, a track without
/// a UUID or with two cue lists, tables laid out as no version checked lays them out: none is
/// written, none leaves anything, and none of it is worth waiting out.
#[test]
fn collections_a_write_cant_vouch_for_are_refused() {
    let odd = |e: &Error| matches!(e, Error::Refused(r @ Refusal::Odd(_)) if !r.waits());
    let cases: &[(&str, &str)] = &[
        ("a view", "CREATE VIEW tracks AS SELECT ID FROM djmdContent"),
        ("a trigger", "CREATE TRIGGER t AFTER UPDATE ON djmdContent BEGIN SELECT 1; END"),
        (
            "two lists",
            "INSERT INTO contentCue (ID, ContentID, Cues, rb_cue_count) VALUES ('a', '84935471', '[]', 0), ('b', '84935471', '[]', 0)",
        ),
        ("a column more", "ALTER TABLE djmdCue ADD COLUMN Lyrics TEXT"),
        ("no cue lists (rekordbox 6)", "DROP TABLE contentCue"),
        ("no CueUpdated", "ALTER TABLE djmdContent DROP COLUMN CueUpdated"),
    ];
    for (what, sql) in cases {
        let f = writable();
        f.exec(sql);
        let lib = Lib::new(&f, false);
        let before = lib.bytes();
        let e = write(&lib.target, |e| e.set_cues(TRACK, &[])).unwrap_err();
        assert!(odd(&e), "{what}: {e}");
        assert_eq!(lib.bytes(), before, "{what}");
        assert_eq!(lib.leftovers(false), Vec::<String>::new(), "{what}");
    }

    let mut lib = Lib::new(&writable(), false);
    lib.target.master_db = lib.dir.path().join("work");
    let e = write(&lib.target, |_| Ok(())).unwrap_err();
    assert!(odd(&e), "a folder: {e}");

    let f = writable();
    f.exec(&format!("UPDATE djmdContent SET UUID = NULL WHERE ID = '{TRACK}'"));
    let lib = Lib::new(&f, false);
    let before = lib.bytes();
    let e = write(&lib.target, |e| e.set_cues(TRACK, &[])).unwrap_err();
    assert!(matches!(e, Error::Invalid(_)), "no UUID: {e}");
    assert_eq!(lib.bytes(), before);
    assert_eq!(lib.leftovers(false), Vec::<String>::new());
}

/// What an edit does through `conn` is checked too: a stamp past the count, a cue list that no
/// longer follows its rows, a cue pointing at another track, a trigger made: none is written.
#[test]
fn what_an_edit_does_by_hand_is_checked() {
    let cases: &[(&str, &str, &str)] = &[
        ("a stamp past the count", "UPDATE djmdContent SET rb_local_usn = 999999 WHERE ID = '4'", "localUpdateCount"),
        ("a list changed", "UPDATE contentCue SET Cues = '[]'", "disagreeing"),
        ("a cue astray", "UPDATE djmdCue SET ContentUUID = 'uuid-4'", "another track's UUID"),
        ("a trigger", "CREATE TRIGGER t AFTER UPDATE ON djmdContent BEGIN SELECT 1; END", "trigger"),
    ];
    for (what, sql, says) in cases {
        let lib = Lib::new(&writable(), false);
        let before = lib.bytes();
        let e = write(&lib.target, |e| {
            e.set_cues(TRACK, &[hot(0, 1)])?;
            e.conn().execute_batch(sql).map_err(|e| Error::Invalid(e.to_string()))
        })
        .unwrap_err();
        assert!(matches!(e, Error::Refused(Refusal::Odd(_))) && e.to_string().contains(says), "{what}: {e}");
        assert_eq!(lib.bytes(), before, "{what}");
        assert_eq!(lib.leftovers(false), Vec::<String>::new(), "{what}");
    }
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

/// A `master.db` with another name (a hard link) isn't written: the rename would leave that name
/// on the old collection.
#[cfg(unix)]
#[test]
fn a_hard_linked_collection_is_refused() {
    let lib = Lib::new(&writable(), false);
    std::fs::hard_link(&lib.target.master_db, lib.dir.path().join("other.db")).unwrap();
    let e = write(&lib.target, |_| Ok(())).unwrap_err();
    assert!(e.to_string().contains("hard links"), "{e}");
}

/// The permissions `master.db` has are the new file's.
#[cfg(unix)]
#[test]
fn the_new_file_keeps_master_dbs_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let lib = Lib::new(&writable(), false);
    std::fs::set_permissions(&lib.target.master_db, std::fs::Permissions::from_mode(0o640)).unwrap();
    write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap();
    assert_eq!(std::fs::metadata(&lib.target.master_db).unwrap().permissions().mode() & 0o777, 0o640);
}

/// A backup is the file as it was, under a name nothing has yet, or there's none; what's there
/// already is left alone.
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
    // A partial copy that's there already isn't this one's to remove.
    let to = lib.dir.path().join("next.db");
    std::fs::write(beside(&to, "partial"), b"someone else's").unwrap();
    assert!(matches!(backup_as(master, &to, &before), Err(Error::Io { .. })));
    assert_eq!(std::fs::read(beside(&to, "partial")).unwrap(), b"someone else's");
}

/// Another write of the collection holding its lock refuses this one, which is worth trying
/// again; what a write that didn't finish left (its new file, a partial backup) goes with the next.
#[test]
fn writes_take_turns_and_clear_what_an_unfinished_one_left() {
    let lib = Lib::new(&writable(), false);
    let lock = File::create(beside(&lib.target.master_db, "locallibrary-lock")).unwrap();
    lock.try_lock().unwrap();
    let before = lib.bytes();
    let e = write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap_err();
    assert!(matches!(&e, Error::Refused(r) if *r == Refusal::Busy && r.waits()), "{e}");
    assert_eq!(lib.bytes(), before);
    drop(lock);

    let new = beside(&lib.target.master_db, "locallibrary-new");
    std::fs::write(&new, b"a killed write's").unwrap();
    std::fs::create_dir(&lib.target.backups).unwrap();
    let partial = lib.target.backups.join("master.2026-11-26T072406.375Z.db.partial");
    std::fs::write(&partial, b"half").unwrap();
    let other = lib.target.backups.join("notes.txt");
    std::fs::write(&other, b"mine").unwrap();
    write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap();
    assert!(!new.exists() && !partial.exists() && other.exists());
}

/// Only filesystems that replace a file in one rename take a write; a system rekordbox doesn't
/// run on says nothing, and the folder tests use is fine.
#[test]
fn only_filesystems_that_swap_in_one_step_are_written() {
    for fs in ["apfs", "hfs", "NTFS", "ReFS"] {
        assert!(swaps_in_one_step(Some(fs)), "{fs}");
    }
    for fs in ["msdos", "exfat", "exFAT", "FAT32", "smbfs", "nfs", "webdav", ""] {
        assert!(!swaps_in_one_step(Some(fs)), "{fs}");
    }
    assert!(swaps_in_one_step(None));
    let dir = tempfile::tempdir().unwrap();
    let x = dir.path().join("x");
    std::fs::write(&x, b"").unwrap();
    replaced_in_one_step(&x, filesystem(&x).unwrap()).unwrap();
    let e = replaced_in_one_step(&x, Some("exfat".into())).unwrap_err();
    assert!(matches!(&e, Error::Refused(Refusal::Odd(_))) && e.to_string().contains("exfat drive"), "{e}");
}

/// A version is one of those checked by its first three numbers.
#[test]
fn versions_checked_go_by_their_first_three_numbers() {
    for (version, is) in [
        (Some("7.2.14"), true),
        (Some("7.2.14.0323"), true),
        (Some(" 7.2.18 "), true),
        (Some("7.2.1"), false),
        (Some("7.2.15"), false),
        (Some("6.8.5"), false),
        (None, false),
    ] {
        assert_eq!(measured(version), is, "{version:?}");
    }
}

/// [`writable`] with an index SQLite's quick check passes and its integrity check doesn't: it says
/// it indexes one column and holds another's values.
fn misindexed() -> Fixture {
    let f = writable();
    f.exec(
        "CREATE INDEX by_title ON djmdContent (Title);
         PRAGMA writable_schema = ON;
         UPDATE sqlite_master SET sql = 'CREATE INDEX by_title ON djmdContent (FolderPath)' WHERE name = 'by_title';
         PRAGMA writable_schema = OFF;",
    );
    f
}

/// A collection SQLite finds damaged isn't written, and no backup of it is kept.
#[test]
fn a_damaged_collection_isnt_written() {
    let lib = Lib::new(&misindexed(), false);
    let before = lib.bytes();
    let e = write(&lib.target, |e| e.set_cues(TRACK, &[hot(0, 1)])).unwrap_err();
    assert!(e.to_string().contains("damaged"), "{e}");
    assert_eq!(lib.bytes(), before);
    assert_eq!(lib.leftovers(false), Vec::<String>::new());
    assert_eq!(std::fs::read_dir(&lib.target.backups).unwrap().count(), 0);
}

/// The file written is read back whole, with the count the edit ended at.
#[test]
fn the_file_written_is_read_back_whole_with_its_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.db");
    std::fs::write(&path, writable().plain()).unwrap();
    read_back(&path, DEFAULT_KEY, dir.path(), UPDATE_COUNT, &path, false).unwrap();
    let e = read_back(&path, DEFAULT_KEY, dir.path(), UPDATE_COUNT + 1, &path, false).unwrap_err();
    assert!(e.to_string().contains("localUpdateCount"), "{e}");
    let damaged = dir.path().join("misindexed.db");
    std::fs::write(&damaged, misindexed().plain()).unwrap();
    let e = read_back(&damaged, DEFAULT_KEY, dir.path(), UPDATE_COUNT, &damaged, false).unwrap_err();
    assert!(e.to_string().contains("damaged"), "{e}");
}

/// A new file that doesn't read back (cut short), or reads back as something other than the
/// edited copy, isn't swapped in, and isn't left.
#[test]
fn a_new_file_that_doesnt_read_back_isnt_swapped_in() {
    fn cut_short(path: &Path) {
        let file = File::options().write(true).open(path).unwrap();
        file.set_len(file.metadata().unwrap().len() / 2).unwrap();
    }
    fn the_old_one(path: &Path) {
        std::fs::copy(path.with_file_name("master.db"), path).unwrap();
    }
    for (hook, says) in [(cut_short as fn(&Path), ""), (the_old_one, "doesn't hold what the edited copy does")] {
        let lib = Lib::new(&writable(), false);
        let before = lib.bytes();
        BEFORE_READ_BACK.set(Some(hook));
        // A change that takes no stamp, so the old file has the count the new one should.
        let e = write(&lib.target, |e| {
            e.conn()
                .execute("UPDATE djmdContent SET Title = 'x' WHERE ID = '4'", [])
                .map_err(|e| Error::Invalid(e.to_string()))
        })
        .unwrap_err();
        BEFORE_READ_BACK.set(None);
        assert!(e.to_string().contains(says), "{e}");
        assert_eq!(lib.bytes(), before);
        assert_eq!(lib.leftovers(false), Vec::<String>::new());
    }
}

/// A plain (decrypted) collection's working copy has a WAL; what an edit changes by hand, with
/// no stamp, is in the file written all the same.
#[test]
fn a_plain_collections_changes_are_all_written() {
    let lib = Lib::new(&writable(), true);
    write(&lib.target, |e| {
        e.conn()
            .execute("UPDATE djmdContent SET Title = 'renamed' WHERE ID = '4'", [])
            .map_err(|e| Error::Invalid(e.to_string()))
    })
    .unwrap();
    assert_eq!(lib.query::<String>("SELECT Title FROM djmdContent WHERE ID = '4'"), "renamed");
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

/// The guard panics for a path outside the temporary folder, under rekordbox's, or reached
/// through a link to one.
#[test]
#[should_panic(expected = "a test tried to write")]
fn the_test_guard_stops_writes_outside_the_temporary_folder() {
    let home = directories::BaseDirs::new().unwrap().home_dir().to_path_buf();
    not_rekordboxs_own(&home.join("Library/Pioneer/rekordbox/master.db"));
}

#[cfg(unix)]
#[test]
#[should_panic(expected = "a test tried to write")]
fn the_test_guard_follows_links() {
    let dir = tempfile::tempdir().unwrap();
    let home = directories::BaseDirs::new().unwrap().home_dir().to_path_buf();
    std::os::unix::fs::symlink(home, dir.path().join("away")).unwrap();
    not_rekordboxs_own(&dir.path().join("away/master.db"));
}

/// An edit that changes nothing writes nothing: no swap, and no backup kept.
#[test]
fn an_edit_that_changes_nothing_writes_nothing() {
    let lib = Lib::new(&writable(), false);
    let before = lib.bytes();
    let written = write(&lib.target, |e| e.cue_list_mismatches(false)).unwrap();
    assert_eq!((written.backup, written.count), (None, (UPDATE_COUNT, UPDATE_COUNT)));
    assert_eq!(lib.bytes(), before);
    assert_eq!(lib.leftovers(false), Vec::<String>::new());
    assert_eq!(std::fs::read_dir(&lib.target.backups).unwrap().count(), 0);
}

/// A track's rows and list read without a write are what an edit sees of them.
#[test]
fn cues_read_without_a_write_are_what_an_edit_sees() {
    let lib = Lib::new(&kept(), false);
    let read = cue_rows_at(&lib.target.master_db, DEFAULT_KEY, &[TRACK, "4"]).unwrap();
    assert_eq!(read.iter().map(|b| (b.track_id.as_str(), b.rows.len())).collect::<Vec<_>>(), [(TRACK, 6), ("4", 0)]);
    assert!(read[0].content_cue.is_some() && read[1].content_cue.is_none());
    let seen = write(&lib.target, |e| e.cues_before(TRACK)).unwrap();
    assert_eq!(seen.value, read[0]);
}

/// A repair of the tracks named makes each one's list from its rows, and is undone by putting
/// the lists back: the text and count they had, or no list where there was none.
#[test]
fn a_repair_of_tracks_named_is_undone_by_putting_their_lists_back() {
    let lib = Lib::new(&kept(), false);
    let list = format!("SELECT Cues || ' ' || rb_cue_count FROM contentCue WHERE ContentID = '{TRACK}'");
    let stale = lib.query::<String>(&list);
    let repaired = write(&lib.target, |e| {
        let made = e.cue_list_of(TRACK)?;
        let before = e.repair_cue_lists_of(&[TRACK, "4"])?;
        assert_eq!(e.cue_list_of(TRACK)?, made);
        Ok((made, before))
    })
    .unwrap();
    let (made, before) = repaired.value;
    assert_eq!(lib.query::<String>(&list), format!("{made} 6"));
    assert_eq!(lib.query::<i64>("SELECT count(*) FROM contentCue WHERE ContentID = '4'"), 1);
    let undone =
        write(&lib.target, |e| before.iter().map(|b| e.restore_cue_list(b)).collect::<Result<Vec<_>>>()).unwrap();
    assert_eq!(lib.query::<String>(&list), stale);
    assert_eq!(lib.query::<i64>("SELECT count(*) FROM contentCue WHERE ContentID = '4'"), 0);
    assert!(undone.count.1 > repaired.count.1);
    // A list gone since is made again as it was.
    write(&lib.target, |e| {
        e.conn().execute(&format!("DELETE FROM contentCue WHERE ContentID = '{TRACK}'"), []).unwrap();
        e.restore_cue_list(&undone.value[0])
    })
    .unwrap();
    assert_eq!(lib.query::<String>(&list), format!("{made} 6"));
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
    f.exec(&format!(
        "INSERT INTO contentCue (ID, ContentID, Cues, rb_cue_count) VALUES ('uuid-{TRACK}', '{TRACK}', '[]', 9)"
    ));
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

    // Opened again, so SQLite reads the schema the fixture rewrote.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("misindexed.db");
    std::fs::write(&path, misindexed().plain()).unwrap();
    let e = check(&scratch_db(&path), count, &before, &none, &none).unwrap_err();
    assert!(e.contains("damaged"), "{e}");
}

/// A loop made active and a hot cue given another colour are the same rows, with only that
/// column (and the time) changed.
#[test]
fn a_loop_made_active_and_a_colour_changed_stay_in_their_rows() {
    let lib = Lib::new(&kept(), false);
    let before = lib.cue_rows(TRACK);
    let mut cues = lib.entries(84935471);
    cues.iter_mut().find(|c| c.hot_cue == Some(1)).unwrap().active_loop = true;
    cues.iter_mut().find(|c| c.hot_cue == Some(0)).unwrap().color = Some(5);
    write(&lib.target, |e| e.set_cues(TRACK, &cues)).unwrap();
    let after = lib.cue_rows(TRACK);
    let get = |id: &str, name: &str| after[id].iter().find(|(n, _)| n == name).unwrap().1.clone();
    assert_eq!(differ(&before["c3"], &after["c3"]), ["ActiveLoop", "updated_at"]);
    assert_eq!(differ(&before["c1"], &after["c1"]), ["ColorTableIndex", "updated_at"]);
    assert_eq!((get("c3", "ActiveLoop"), get("c1", "ColorTableIndex")), (1.into(), 5.into()));
}
