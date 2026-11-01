//! Stand-ins for the operating system's disk tools (diskutil, lsblk, PowerShell...), for tests
//! that run the code which calls them. While a [`FakeTools`] lives, `PATH` names only its folder,
//! so no real tool can run, whatever the code under test asks for: a tool the test didn't set up
//! is missing, and a set-up tool answers only the command lines it was told about.
//!
//! `PATH` belongs to the whole process: nextest runs each test in a process of its own, and the
//! tests using this run their tools on the test's own thread.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use rustc_hash::FxHashMap;
use serde_json::json;

use crate::format::Cmd;

/// What a fake tool does when called with the arguments of a [`Rule`].
enum Reply {
    /// Print this and succeed.
    Out(String),
    /// Print this to standard error and fail.
    Fail(String),
    /// Print what came in on standard input (plutil turning a plist into JSON).
    Echo,
    /// Succeed silently, and from then on answer as told for the next stage.
    Advance,
}

struct Rule {
    stage: usize,
    args: Vec<String>,
    reply: Reply,
}

pub(crate) struct FakeTools {
    dir: tempfile::TempDir,
    path: Option<OsString>,
    rules: FxHashMap<String, Vec<Rule>>,
    stage: usize,
}

/// `s` as one word for `sh`, taken literally (in a `case` pattern too).
fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

impl FakeTools {
    pub fn new() -> FakeTools {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("bin")).unwrap();
        std::fs::write(dir.path().join("stage"), "0").unwrap();
        std::fs::write(dir.path().join("calls"), "").unwrap();
        let path = std::env::var_os("PATH");
        // SAFETY: nextest gives each test a process of its own, and no other thread of it reads
        // the environment while the test runs.
        unsafe { std::env::set_var("PATH", dir.path().join("bin")) };
        FakeTools { dir, path, rules: FxHashMap::default(), stage: 0 }
    }

    /// A scratch folder that goes when the tools do.
    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    /// A drive folder here named `name`, holding the demo rekordbox export.
    pub fn demo_drive(&self, name: &str) -> std::path::PathBuf {
        let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rekordcrate/demo_tracks");
        let root = self.dir().join(name);
        for e in walkdir::WalkDir::new(&demo).into_iter().map(Result::unwrap) {
            let to = root.join(e.path().strip_prefix(&demo).unwrap());
            if e.file_type().is_dir() {
                std::fs::create_dir_all(&to).unwrap();
            } else {
                std::fs::copy(e.path(), &to).unwrap();
            }
        }
        root
    }

    /// `program` runs `body` as a shell script, for a tool that does more than answer (writes the
    /// file it's told to, say). `$d` is the tools' folder.
    pub fn script(&self, program: &str, body: &str) {
        let d = quoted(&self.dir.path().to_string_lossy());
        let file = self.dir.path().join("bin").join(program);
        std::fs::write(&file, format!("#!/bin/sh\nd={d}\n{body}")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// `program args` prints `stdout` and succeeds.
    pub fn answer(&mut self, program: &str, args: &[&str], stdout: &str) {
        self.rule(program, args, Reply::Out(stdout.into()));
    }

    /// `program args` prints `stderr` to standard error and fails.
    pub fn refuse(&mut self, program: &str, args: &[&str], stderr: &str) {
        self.rule(program, args, Reply::Fail(stderr.into()));
    }

    /// `program args` prints its standard input, at every stage.
    pub fn echo(&mut self, program: &str, args: &[&str]) {
        self.rule(program, args, Reply::Echo);
    }

    /// `program args` succeeds, and the tools then answer as told after this call: what a disk
    /// looks like after it was erased, say.
    pub fn then_after(&mut self, program: &str, args: &[&str]) {
        self.rule(program, args, Reply::Advance);
        self.stage += 1;
    }

    /// Tell the platform's tools that the one partition of a USB disk, with lsblk's file system
    /// `fs` (`vfat`, `exfat`), is mounted at `root`, on an MBR partition table or (`mbr` false) a
    /// GUID one: diskutil and ioreg on macOS, lsblk elsewhere (its device files in this folder).
    /// The disk is a 64 GB "Ultra Fit" with serial number 4C530001, the volume's UUID 1234-ABCD.
    /// Returns the partition's identifier and file system name as the tools give them.
    pub fn volume_at(&mut self, root: &Path, fs: &str, mbr: bool) -> (String, String) {
        let root = root.to_string_lossy();
        if cfg!(target_os = "macos") {
            let content = if mbr { "FDisk_partition_scheme" } else { "GUID_partition_scheme" };
            let name = if fs == "exfat" { "ExFAT" } else { "MS-DOS FAT32" };
            let disk = json!({ "DeviceIdentifier": "disk4", "ParentWholeDisk": "disk4", "Size": 64_000_000_000u64,
                    "Content": content, "RemovableMediaOrExternalDevice": true, "MediaName": "Ultra Fit" });
            let part = json!({ "DeviceIdentifier": "disk4s1", "ParentWholeDisk": "disk4", "MountPoint": root,
                    "FilesystemType": fs.replace("vfat", "msdos"), "FilesystemName": name });
            let listing = json!({ "AllDisksAndPartitions": [{ "DeviceIdentifier": "disk4", "Partitions": [
                    { "DeviceIdentifier": "disk4s1", "Size": 63_000_000_000u64, "MountPoint": root,
                      "VolumeUUID": "1234-ABCD" }] }] });
            let usb = r#"+-o Ultra Fit@01100000  <class IOUSBHostDevice>
  | {
  |   "USB Serial Number" = "4C530001"
  | }
  +-o Media  <class IOMedia>
    | {
    |   "BSD Name" = "disk4"
    | }
"#;
            self.answer("ioreg", &["-r", "-c", "IOUSBHostDevice", "-l", "-w0"], usb);
            self.echo("plutil", &["-convert", "json", "-o", "-", "--", "-"]);
            for (args, out) in [(["info", "-plist", &*root], &part), (["info", "-plist", "disk4"], &disk)] {
                self.answer("diskutil", &args, &out.to_string());
            }
            self.answer("diskutil", &["list", "-plist", "disk4"], &listing.to_string());
            self.answer("diskutil", &["info", "-plist", "disk4s1"], &part.to_string());
            return ("disk4s1".into(), name.into());
        }
        let (disk, part) = (self.dir().join("sdb"), self.dir().join("sdb1"));
        for f in [&disk, &part] {
            std::fs::write(f, "").unwrap();
        }
        let listing = json!({ "blockdevices": [{ "name": "sdb", "path": disk, "type": "disk", "tran": "usb",
                "pttype": if mbr { "dos" } else { "gpt" }, "model": "Ultra Fit", "serial": "4C530001",
                "size": 64_000_000_000u64, "children": [{ "name": "sdb1", "path": part, "type": "part", "fstype": fs,
                "mountpoint": root, "uuid": "1234-ABCD" }] }] });
        let columns = "NAME,PATH,TYPE,SIZE,RM,HOTPLUG,RO,TRAN,VENDOR,MODEL,SERIAL,FSTYPE,LABEL,UUID,MOUNTPOINT,PTTYPE,LOG-SEC,FSVER";
        self.answer("lsblk", &["--json", "--bytes", "--output", columns], &listing.to_string());
        (part.display().to_string(), fs.into())
    }

    /// Every call made so far, in order, as the command lines that made them. Input is read a
    /// line at a time, so its last line always ends in a newline.
    pub fn calls(&self) -> Vec<Cmd> {
        let log = std::fs::read_to_string(self.dir.path().join("calls")).unwrap();
        log.split_terminator('\x1d')
            .map(|record| {
                let (words, input) = record.split_once('\x1e').unwrap();
                let mut words = words.split_terminator('\x1f').map(String::from);
                let program = words.next().unwrap();
                Cmd { program, args: words.collect(), input: Some(input.to_string()).filter(|i| !i.is_empty()) }
            })
            .collect()
    }

    fn rule(&mut self, program: &str, args: &[&str], reply: Reply) {
        let rule = Rule { stage: self.stage, args: args.iter().map(|a| a.to_string()).collect(), reply };
        self.rules.entry(program.to_string()).or_default().push(rule);
        self.write_script(program);
    }

    /// The tool: it logs the call (words split by US, input after RS, the record ended by GS),
    /// then answers by the stage it's in and its arguments as `$*` joins them. Only shell
    /// builtins run, so a call costs one process.
    fn write_script(&self, program: &str) {
        let d = quoted(&self.dir.path().to_string_lossy());
        let mut s = format!(
            "#!/bin/sh\nd={d}\ninput=\n\
             while IFS= read -r line || [ -n \"$line\" ]; do input=\"$input$line\n\"; done\n\
             {{ printf '%s\\037' {} \"$@\"; printf '\\036%s\\035' \"$input\"; }} >>\"$d/calls\"\n\
             read -r stage <\"$d/stage\"\ncase \"$stage $*\" in\n",
            quoted(program)
        );
        for r in &self.rules[program] {
            let action = match &r.reply {
                Reply::Out(text) => format!("printf '%s' {}; exit 0", quoted(text)),
                Reply::Fail(text) => format!("printf '%s' {} >&2; exit 1", quoted(text)),
                Reply::Echo => "printf '%s' \"$input\"; exit 0".into(),
                Reply::Advance => format!("echo {} >\"$d/stage\"; exit 0", r.stage + 1),
            };
            let args = quoted(&format!(" {}", r.args.join(" ")));
            match r.reply {
                Reply::Echo => writeln!(s, "*{args}) {action} ;;"),
                _ => writeln!(s, "{}{args}) {action} ;;", r.stage),
            }
            .unwrap();
        }
        writeln!(s, "esac\necho \"fake {program}: nothing to say to $*\" >&2\nexit 1").unwrap();
        let file = self.dir.path().join("bin").join(program);
        std::fs::write(&file, s).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

impl Drop for FakeTools {
    fn drop(&mut self) {
        // SAFETY: as in `new`.
        unsafe {
            match &self.path {
                Some(p) => std::env::set_var("PATH", p),
                None => std::env::remove_var("PATH"),
            }
        }
    }
}
