//! What the modules share: the drives they run against, a store and a folder of drives of a test's
//! own, and running `locallibrary` in them.

use std::io::{BufRead, BufReader, Lines, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::OnceLock;

use locallibrary::device::dunce_canonicalize;
use rekordbox_pdb::{Database, PdbEditor};
use serde_json::{Value, json};

use crate::files::Template;
pub use crate::files::copy_dir;

pub const PDB: &str = "PIONEER/rekordbox/export.pdb";

/// The library's test fixtures: real rekordbox exports (see the README there).
pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/locallibrary/tests/fixtures")
}

pub fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// The line of `out` holding `needle`.
pub fn line<'a>(out: &'a str, needle: &str) -> &'a str {
    out.lines().find(|l| l.contains(needle)).unwrap_or_else(|| panic!("no {needle:?} in:\n{out}"))
}

/// rekordcrate's demo export (Loopmasters' two demo tracks, with their analysis files and
/// rekordbox's default My Tags) as the drive `name` in `dir`, with placeholder audio.
pub fn demo_drive(dir: &Path, name: &str) -> PathBuf {
    let root = dir.join(name);
    copy_dir(&fixtures().join("rekordcrate/demo_tracks"), &root);
    for t in &Database::from_file(root.join(PDB)).unwrap().tracks {
        let p = root.join(t.file_path().trim_start_matches('/'));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, format!("audio {}", t.title()).repeat(100)).unwrap();
    }
    root
}

/// [`demo_drive`] as a DJ brings it back: a playlist "Set" holds both tracks, and a player
/// recorded both in "HISTORY 001".
pub fn dj_drive(dir: &Path, name: &str) -> PathBuf {
    let root = demo_drive(dir, name);
    let mut ed = PdbEditor::from_file(root.join(PDB)).unwrap();
    let set = ed.create_playlist("Set", 0, false).unwrap();
    for id in [1, 2] {
        ed.add_to_playlist(set, id, None).unwrap();
    }
    ed.save(root.join(PDB)).unwrap();
    play(&root, "HISTORY 001", &[1, 2]);
    root
}

/// A drive with nothing on it, as `name` in `dir`.
pub fn blank_drive(dir: &Path, name: &str) -> PathBuf {
    let root = dir.join(name);
    std::fs::create_dir(&root).unwrap();
    root
}

/// rekordbox-pdb's twelve-track export.pdb: named tracks with keys and tempos, and a playlist
/// "aab" of them all.
pub fn club_export() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/rekordbox-pdb/tests/data/bigger-export.pdb")
}

/// [`club_export`] (and no audio) as the drive `name` in `dir`.
pub fn club_drive(dir: &Path, name: &str) -> PathBuf {
    let root = dir.join(name);
    std::fs::create_dir_all(root.join("PIONEER/rekordbox")).unwrap();
    std::fs::copy(club_export(), root.join(PDB)).unwrap();
    root
}

/// A player recorded a history list of `tracks` (ids) on the drive at `root`.
pub fn play(root: &Path, name: &str, tracks: &[u32]) {
    let mut ed = PdbEditor::from_file(root.join(PDB)).unwrap();
    let id = ed.create_history_playlist(name).unwrap();
    for t in tracks {
        ed.add_to_history(id, *t).unwrap();
    }
    ed.save(root.join(PDB)).unwrap();
}

/// What a run printed, and how it exited.
pub struct Run {
    pub out: String,
    pub err: String,
    pub code: i32,
}

/// A store, and a folder of drives that is the only place drives are looked for, in a temporary
/// folder of the test's own.
pub struct Env {
    _dir: tempfile::TempDir,
    /// The folder the commands look for drives in.
    pub volumes: PathBuf,
    pub store: PathBuf,
    ascii: bool,
    /// The commands get a home folder of their own ([`Env::home`]).
    own_home: bool,
}

impl Env {
    pub fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        // As the commands print it (on macOS, /var is /private/var).
        let root = dunce_canonicalize(dir.path());
        let volumes = root.join("volumes");
        std::fs::create_dir(&volumes).unwrap();
        Env { _dir: dir, volumes, store: root.join("store"), ascii: false, own_home: false }
    }

    /// Drives (A, [`dj_drive`] adopted and pulled into the library A; Club, [`club_drive`], not
    /// adopted; Blank, [`blank_drive`]) and a store, made once and written out for each test case
    /// of a property test; and a home folder of the commands' own with a cover image in it
    /// (`cover.png`), so nothing of the person running the tests (rekordbox's settings and
    /// collection) is read.
    pub fn from_template() -> Env {
        static TEMPLATE: OnceLock<Template> = OnceLock::new();
        let template = TEMPLATE.get_or_init(|| {
            let env = Env::new();
            env.pulled("A");
            env.club("Club");
            env.blank("Blank");
            Template::of(env.volumes.parent().unwrap())
        });
        let env = Env { own_home: true, ..Env::new() };
        template.write(env.volumes.parent().unwrap());
        std::fs::create_dir(env.outside("home")).unwrap();
        image::RgbImage::new(8, 8).save(env.home("cover.png")).unwrap();
        env
    }

    /// [`Env::new`], with every run printing ASCII stand-ins for the symbols (`--ascii`).
    pub fn ascii() -> Env {
        Env { ascii: true, ..Env::new() }
    }

    /// A path outside the drives, for what the commands read and write on "this computer".
    pub fn outside(&self, name: &str) -> PathBuf {
        self.volumes.parent().unwrap().join(name)
    }

    /// A path in the commands' home folder.
    pub fn home(&self, name: &str) -> PathBuf {
        self.outside("home").join(name)
    }

    /// [`demo_drive`], mounted as `name`.
    pub fn demo(&self, name: &str) -> PathBuf {
        demo_drive(&self.volumes, name)
    }

    /// [`dj_drive`], mounted as `name`.
    pub fn drive(&self, name: &str) -> PathBuf {
        dj_drive(&self.volumes, name)
    }

    /// [`club_drive`], mounted as `name`.
    pub fn club(&self, name: &str) -> PathBuf {
        club_drive(&self.volumes, name)
    }

    /// [`blank_drive`], mounted as `name`.
    pub fn blank(&self, name: &str) -> PathBuf {
        blank_drive(&self.volumes, name)
    }

    /// [`Env::drive`], adopted as `name` and pulled into the store as the library `name`.
    pub fn pulled(&self, name: &str) -> PathBuf {
        let root = self.drive(name);
        self.ok(&["--yes", "adopt", path(&root), "--name", name]);
        self.ok(&["pull", name]);
        root
    }

    pub fn command(&self) -> Command {
        let mut c = crate::locallibrary(&self.store, &[&self.volumes]);
        if self.ascii {
            c.arg("--ascii");
        }
        if self.own_home {
            c.env("HOME", self.outside("home"))
                .env("LOCALLIBRARY_MASTERDB", self.home("master.db"))
                .current_dir(self.outside(""));
        }
        c
    }

    pub fn run(&self, args: &[&str]) -> Run {
        let out = self.command().args(args).output().unwrap();
        Run {
            out: String::from_utf8(out.stdout).unwrap(),
            err: String::from_utf8(out.stderr).unwrap(),
            code: out.status.code().unwrap(),
        }
    }

    /// stdout of a run that exits with `code`.
    pub fn exits(&self, code: i32, args: &[&str]) -> String {
        let r = self.run(args);
        assert_eq!(r.code, code, "{args:?}\n{}\n{}", r.out, r.err);
        r.out
    }

    /// stderr of a run that fails with `code`.
    pub fn fails(&self, code: i32, args: &[&str]) -> String {
        let r = self.run(args);
        assert_eq!(r.code, code, "{args:?}\n{}\n{}", r.out, r.err);
        r.err
    }

    /// stdout of a run that succeeds.
    pub fn ok(&self, args: &[&str]) -> String {
        self.exits(0, args)
    }

    /// `--json` output of a run that succeeds.
    pub fn json(&self, args: &[&str]) -> Value {
        let out = self.ok(&[&["--json"], args].concat());
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("{args:?}: {e}\n{out}"))
    }

    /// The bytes of every file under `dir`, by path, but the cache of file hashes a drive keeps
    /// (`.locallibrary/index.json`), which any read may bring up to date (a hash taken within two
    /// seconds of its file's last change, as a template's are, is taken again).
    pub fn files_in(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for e in entries.flatten() {
                if e.file_type().unwrap().is_dir() {
                    walk(&e.path(), out);
                } else if e.path().ends_with(".locallibrary/index.json") {
                    continue;
                } else if let Ok(bytes) = std::fs::read(e.path()) {
                    out.push((e.path(), bytes));
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, &mut out);
        out.sort();
        out
    }

    /// The bytes of every file on the drives and in the store, by path.
    pub fn files(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Env::files_in(&self.volumes);
        out.extend(Env::files_in(&self.store));
        out
    }
}

/// The RPC sidecar (`command` with `rpc`), asked one thing at a time.
pub struct Rpc {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Lines<BufReader<ChildStdout>>,
}

impl Rpc {
    pub fn start(mut command: Command) -> Rpc {
        let mut child = command.arg("rpc").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let stdin = child.stdin.take();
        let lines = BufReader::new(child.stdout.take().unwrap()).lines();
        Rpc { child, stdin, lines }
    }

    /// The response to request `id` (events and other responses before it are skipped). With
    /// `params` null the request has none, as a client may send it.
    pub fn call(&mut self, id: i64, method: &str, params: Value) -> Value {
        let mut request = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if !params.is_null() {
            request["params"] = params;
        }
        writeln!(self.stdin.as_mut().unwrap(), "{request}").unwrap();
        loop {
            let v: Value = serde_json::from_str(&self.lines.next().expect("a response").unwrap()).unwrap();
            if v["id"] == id {
                return v;
            }
        }
    }
}

/// Ends the sidecar as a client does, by closing its input: it exits cleanly (a killed one
/// would leave what it ran out of a coverage report).
impl Drop for Rpc {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

/// A terminal of its own for a command (a pseudo-terminal): the end the test reads and writes, and
/// the end the command is given.
#[cfg(unix)]
pub fn terminal() -> (std::fs::File, std::os::fd::OwnedFd) {
    use std::os::fd::FromRawFd;
    use std::ptr::null_mut;
    let (mut ours, mut theirs) = (0, 0);
    // SAFETY: openpty fills in two new descriptors, owned from here on.
    assert_eq!(unsafe { libc::openpty(&mut ours, &mut theirs, null_mut(), null_mut(), null_mut()) }, 0);
    unsafe { (std::fs::File::from_raw_fd(ours), std::os::fd::OwnedFd::from_raw_fd(theirs)) }
}

/// Sets `path`'s permission bits.
#[cfg(unix)]
pub fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Whether permissions stop this process. They don't stop the superuser (a test run as root, in
/// a container say), so a test that needs a file it can't read or a folder it can't write to
/// checks this first and has nothing to test without it.
#[cfg(unix)]
pub fn permissions_enforced() -> bool {
    let dir = tempfile::tempdir().unwrap();
    set_mode(dir.path(), 0o555);
    let enforced = std::fs::write(dir.path().join("probe"), "").is_err();
    set_mode(dir.path(), 0o755);
    enforced
}

/// Signed in to an Atmosphere account (as a sign-in with `--session-file` leaves it, never in the
/// keychain), at a pretend data server.
#[cfg(feature = "account")]
pub mod account {
    use std::path::Path;

    use locallibrary::atmosphere::store::{ACCOUNT_FILE, AccountFile};
    use serde_json::{Value, json};

    pub use crate::session::DID;
    use crate::session::{session_json, sign_in_as};

    /// Permission to publish locallibrary's records.
    pub const PUBLISHING: [&str; 4] = [
        "atproto",
        "repo:computer.localnetwork.locallibrary.settings",
        "repo:computer.localnetwork.dj.profile",
        "repo:computer.localnetwork.dj.set",
    ];

    /// A P-256 key made for these tests: it signs nothing but proofs sent to the pretend server.
    pub fn test_key() -> Value {
        json!({
            "kty": "EC",
            "crv": "P-256",
            "x": "laxgzfuTILMca4EtW61jYuY5U6rrU0SriQbuzl5PbVk",
            "y": "_cGaHm7PKttheS1e6rIuy21TxDGGGqkgc5L02W1VewM",
            "d": "vSy5sRAucJv2w6BzKciV3Y57kx-EgAFnM2QTVlvdyrY",
        })
    }

    /// Sign `DID` (@dj.example.com) in at `pds` with [`test_key`], allowing `scopes`, with the
    /// store at `store`, in place of whoever was (signing in over an account kept in the keychain
    /// would remove its keychain entry). `info` replaces what the account's details say (when to
    /// sign in again, the client, where the session is kept).
    pub fn sign_in(store: &Path, pds: &str, scopes: &[&str], info: Value) {
        let _ = std::fs::remove_file(store.join(ACCOUNT_FILE));
        let mut session =
            session_json(DID, "s1", pds, &scopes.join(" "), Some("refresh-1"), "2099-01-01T00:00:00.000Z");
        session["dpop_key"] = test_key();
        sign_in_as(store, session);
        let mut file = AccountFile::load(store).unwrap();
        let account = file.account.as_mut().unwrap();
        account.requested_scopes = scopes.iter().map(|s| s.to_string()).collect();
        let mut details = serde_json::to_value(&account.info).unwrap();
        let said = json!({
            "handle": "dj.example.com",
            "scopes": scopes,
            "signed_in_at": "2026-10-05T00:00:00Z",
            "sign_in_again_by": "2099-01-01T00:00:00Z",
            "access_expires_at": null,
        });
        for v in [said, info] {
            details.as_object_mut().unwrap().extend(v.as_object().unwrap().clone());
        }
        account.info = serde_json::from_value(details).unwrap();
        file.save(store).unwrap();
    }
}
