//! What the modules share: the drives they run against, a store and a folder of drives of a test's
//! own, and running `locallibrary` in them.

use std::io::{BufRead, BufReader, Lines, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use locallibrary::device::dunce_canonicalize;
use rekordbox_pdb::{Database, PdbEditor};
use serde_json::{Value, json};

pub const PDB: &str = "PIONEER/rekordbox/export.pdb";

/// The library's test fixtures: real rekordbox exports (see the README there).
pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../locallibrary/tests/fixtures")
}

pub fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
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
}

impl Env {
    pub fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        // As the commands print it (on macOS, /var is /private/var).
        let root = dunce_canonicalize(dir.path());
        let volumes = root.join("volumes");
        std::fs::create_dir(&volumes).unwrap();
        Env { _dir: dir, volumes, store: root.join("store"), ascii: false }
    }

    /// [`Env::new`], with every run printing ASCII stand-ins for the symbols (`--ascii`).
    pub fn ascii() -> Env {
        Env { ascii: true, ..Env::new() }
    }

    /// A path outside the drives, for what the commands read and write on "this computer".
    pub fn outside(&self, name: &str) -> PathBuf {
        self.volumes.parent().unwrap().join(name)
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

    use serde_json::{Value, json};

    pub const DID: &str = "did:plc:testdj";

    /// Permission to publish locallibrary's records.
    pub const PUBLISHING: [&str; 4] = [
        "atproto",
        "repo:computer.localnetwork.locallibrary.settings",
        "repo:computer.localnetwork.locallibrary.profile",
        "repo:computer.localnetwork.locallibrary.set",
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

    /// Sign `DID` (@dj.example.com) in at `pds`, allowing `scopes`, with the store at `store`.
    /// `info` replaces what the account's details say (when to sign in again, the client, where
    /// the session is kept).
    pub fn sign_in(store: &Path, pds: &str, scopes: &[&str], info: Value) {
        let scope = scopes.join(" ");
        let session = json!({
            "account_did": DID,
            "session_id": "s1",
            "host_url": pds,
            "authserver_url": pds,
            "authserver_token_endpoint": format!("{pds}/oauth/token"),
            "authserver_revocation_endpoint": format!("{pds}/oauth/revoke"),
            "scopes": scope,
            "dpop_authserver_nonce": "",
            "dpop_host_nonce": "",
            "iss": pds,
            "sub": DID,
            "aud": pds,
            "scope": scope,
            "access_token": "access-token",
            "token_type": "DPoP",
            "expires_at": "2099-01-01T00:00:00.000Z",
        });
        let mut details = json!({
            "did": DID,
            "handle": "dj.example.com",
            "pds": pds,
            "auth_server": pds,
            "scopes": scopes,
            "signed_in_at": "2026-10-05T00:00:00Z",
            "sign_in_again_by": "2099-01-01T00:00:00Z",
            "access_expires_at": null,
            "client": { "kind": "loopback" },
            "storage": "file",
        });
        details.as_object_mut().unwrap().extend(info.as_object().unwrap().clone());
        let account = json!({
            "account": {
                "info": details,
                "session_id": "s1",
                "redirect_uri": "http://127.0.0.1:1/callback",
                "requested_scopes": scopes,
                "session": session,
            }
        });
        locallibrary::Store::open_or_init(store).unwrap();
        std::fs::write(store.join("atmosphere.json"), account.to_string()).unwrap();
        let secrets = json!({ "did": DID, "dpop_key": test_key(), "refresh_token": "refresh-1" });
        std::fs::write(store.join("atmosphere-session.json"), secrets.to_string()).unwrap();
    }
}
