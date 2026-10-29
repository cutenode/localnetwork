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

/// `secs` of test signal `seed` at `rate` and `bits` bits, after `delay_ms` of silence, as a WAV.
pub fn recording(seed: u32, secs: f64, rate: u32, bits: u16, delay_ms: usize) -> Vec<u8> {
    use loclib_mixid::test_util::{resample, synth, wav_of};
    let mut pcm = vec![0.0f32; rate as usize * delay_ms / 1000];
    pcm.extend(resample(&synth(secs, seed), loclib_mixid::fingerprint::RATE, rate));
    let scale = 12_000.0 * f32::from(1u16 << (bits - 16));
    wav_of(rate, bits, pcm.iter().map(|s| (s * scale) as i32))
}

/// loclib-plugin's echo plugin, which the plugin host's test plugin is, with its export format
/// renamed: `m3u` is one of locallibrary's.
#[cfg(debug_assertions)]
pub fn echo_plugin() -> String {
    const ECHO: &str = include_str!("../../../../crates/loclib-plugin/examples/echo/locallibrary-plugin.toml");
    ECHO.replace("format = \"m3u\"", "format = \"echo-m3u\"")
}

/// A JPEG, of `side` pixels square.
pub fn jpeg(side: u32) -> Vec<u8> {
    let image = image::RgbImage::from_fn(side, side, |x, y| image::Rgb([x as u8, y as u8, 128]));
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

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
        std::fs::write(&p, format!("audio {}", t.title()).repeat(100)).unwrap();
        // From long before any pull, as a drive's audio is: a snapshot taken within FAT's 2 s of
        // a file's mtime can't tell damage from a rewrite since.
        filetime::set_file_mtime(&p, filetime::FileTime::from_unix_time(1_790_000_000, 0)).unwrap();
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
    /// The plugins the commands have besides locallibrary's (see [`Env::plugins`]).
    plugins: Vec<PathBuf>,
    /// Where the commands find Local Network's directory and website ([`Env::with_directory`]).
    directory: Option<String>,
}

impl Env {
    pub fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        // As the commands print it (on macOS, /var is /private/var).
        let root = dunce_canonicalize(dir.path());
        let volumes = root.join("volumes");
        std::fs::create_dir(&volumes).unwrap();
        Env {
            _dir: dir,
            volumes,
            store: root.join("store"),
            ascii: false,
            own_home: false,
            plugins: Vec::new(),
            directory: None,
        }
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

    /// The commands have these plugins too: manifests the plugin host's test plugin serves
    /// (`LOCALLIBRARY_TEST_PLUGINS`), written beside the drives. Only in builds with debug
    /// assertions, which serve it.
    #[cfg(debug_assertions)]
    pub fn plugins(&mut self, manifests: &[&str]) {
        self.plugins = manifests
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let file = self.outside(&format!("plugin-{i}.toml"));
                std::fs::write(&file, m).unwrap();
                file
            })
            .collect();
    }

    /// The commands find Local Network's directory and website at `url` (a pretend data server,
    /// which stands in for both), not nowhere.
    pub fn with_directory(mut self, url: &str) -> Env {
        self.directory = Some(url.to_string());
        self
    }

    pub fn command(&self) -> Command {
        let mut c = crate::locallibrary(&self.store, &[&self.volumes]);
        if let Some(url) = &self.directory {
            c.env("LOCALLIBRARY_HAPPYVIEW_URL", url).env("LOCALLIBRARY_WEBSITE_URL", url);
        }
        if !self.plugins.is_empty() {
            c.env("LOCALLIBRARY_TEST_PLUGINS", std::env::join_paths(&self.plugins).unwrap());
        }
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

    /// `args` run with a terminal for stdin, on which `typed` was typed ahead (stdout and stderr
    /// piped, so it isn't a terminal for output).
    #[cfg(unix)]
    pub fn typed_ahead(&self, args: &[&str], typed: &str) -> Run {
        let (mut keyboard, stdin) = terminal();
        keyboard.write_all(typed.as_bytes()).unwrap();
        let out = self.command().args(args).stdin(Stdio::from(stdin)).output().unwrap();
        Run {
            out: String::from_utf8(out.stdout).unwrap(),
            err: String::from_utf8(out.stderr).unwrap(),
            code: out.status.code().unwrap(),
        }
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
    /// The `event` notifications [`Rpc::call`] read past, for [`Rpc::event`].
    events: Vec<Value>,
}

impl Rpc {
    pub fn start(mut command: Command) -> Rpc {
        let mut child = command.arg("rpc").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let stdin = child.stdin.take();
        let lines = BufReader::new(child.stdout.take().unwrap()).lines();
        Rpc { child, stdin, lines, events: Vec::new() }
    }

    /// For signalling it, which only Unix tests do.
    #[cfg(all(unix, debug_assertions))]
    pub fn pid(&self) -> u32 {
        self.child.id()
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
            if v["method"] == "event" {
                self.events.push(v["params"].clone());
            }
        }
    }

    /// The params of the first `event` notification `wanted`, read past already or still to
    /// come (the others are dropped).
    pub fn event(&mut self, wanted: impl Fn(&Value) -> bool) -> Value {
        if let Some(i) = self.events.iter().position(&wanted) {
            return self.events.remove(i);
        }
        loop {
            let v: Value = serde_json::from_str(&self.lines.next().expect("an event").unwrap()).unwrap();
            if v["method"] == "event" && wanted(&v["params"]) {
                return v["params"].clone();
            }
        }
    }
}

/// Waits up to `grace` for `child` to exit on its own, then kills it. Killing one that is already
/// exiting can cut off the coverage profile it writes as it goes.
pub fn end(child: &mut Child, grace: std::time::Duration) {
    let started = std::time::Instant::now();
    while matches!(child.try_wait(), Ok(None)) && started.elapsed() < grace {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// A child process ended (and waited for) when it's dropped, so a test that fails or is stopped
/// part way leaves nothing of it running.
// Only props_cli_asked, which is Unix-only, uses it so far.
#[cfg_attr(not(unix), allow(dead_code))]
pub struct Running(pub Child);

impl Drop for Running {
    fn drop(&mut self) {
        end(&mut self.0, std::time::Duration::ZERO);
    }
}

impl std::ops::Deref for Running {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}

impl std::ops::DerefMut for Running {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
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

/// `command` run with its stdout a terminal of its own (of `size`, if given): how it exited, and
/// what the terminal was shown.
#[cfg(unix)]
pub fn shown_in_a_terminal(command: &mut Command, size: Option<libc::winsize>) -> (std::process::ExitStatus, String) {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    // Written after everything the command wrote, once it has exited.
    const END: &[u8] = b"<the command has exited>";
    let (mut screen, stdout) = terminal();
    if let Some(size) = size {
        // SAFETY: TIOCSWINSZ only reads the size given, for a terminal this opened.
        assert_eq!(unsafe { libc::ioctl(stdout.as_raw_fd(), libc::TIOCSWINSZ, &size) }, 0);
    }
    // Reading to the end of the terminal would lose output: on macOS, the last close of a terminal
    // drops what hasn't been read yet, and the command's exit can come before its last write is
    // read. So the terminal stays open here, and reading stops at a mark written after the command
    // (and its pager) have exited.
    let mut ours = std::fs::File::from(stdout.try_clone().unwrap());
    let mut child = command.stdout(Stdio::from(stdout)).spawn().unwrap();
    let exited = std::thread::spawn(move || {
        let status = child.wait().unwrap();
        ours.write_all(END).unwrap();
        status
    });
    let (mut shown, mut buf) = (Vec::new(), [0; 4096]);
    while !shown.ends_with(END) {
        let n = screen.read(&mut buf).unwrap();
        shown.extend_from_slice(&buf[..n]);
    }
    shown.truncate(shown.len() - END.len());
    (exited.join().unwrap(), String::from_utf8_lossy(&shown).into_owned())
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

    /// What a sign-in that allows publishing asks for: locallibrary's permission set, and to upload
    /// audio files.
    pub const PUBLISHING: [&str; 3] =
        ["atproto", "include:computer.localnetwork.locallibraryPublishing", "blob:audio/*"];

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

    /// Sign `DID` (@dj.example.com) in at `pds` with [`test_key`], asking for `scopes` and granted
    /// them as a server grants them ([`crate::mock_pds::granted`]), with the store at `store`, in
    /// place of whoever was (signing in over an account kept in the keychain would remove its
    /// keychain entry). `info` replaces what the account's details say (when to
    /// sign in again, the client, where the session is kept).
    pub fn sign_in(store: &Path, pds: &str, scopes: &[&str], info: Value) {
        let _ = std::fs::remove_file(store.join(ACCOUNT_FILE));
        let granted = crate::mock_pds::granted(scopes);
        let mut session =
            session_json(DID, "s1", pds, &granted.join(" "), Some("refresh-1"), "2099-01-01T00:00:00.000Z");
        session["dpop_key"] = test_key();
        sign_in_as(store, session);
        let mut file = AccountFile::load(store).unwrap();
        let account = file.account.as_mut().unwrap();
        account.requested_scopes = scopes.iter().map(|s| s.to_string()).collect();
        let mut details = serde_json::to_value(&account.info).unwrap();
        // The granted scopes stay as locallibrary saved them from the session: as jacquard writes them.
        let said = json!({
            "handle": "dj.example.com",
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
