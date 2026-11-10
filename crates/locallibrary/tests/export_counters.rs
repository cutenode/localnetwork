//! Exact counts of what writing a library to a blank drive asks of its filesystem: files and
//! folders made, renames, flushes, writes and their size. A drive's time goes on these, not on
//! anything a benchmark on a CI runner can time, and unlike a time they come out the same on any
//! machine, so CI holds them to the budgets in `benchmarks/budgets/export.json` (see
//! `benchmarks/README.md`). With `LOCALLIBRARY_BUDGETS_OUT` set, this runs the write under `strace`
//! and writes the counts there as `export.json`, for `tools/budgets/cli.ts` to check.

use std::fs;
use std::path::Path;
use std::process::Command;

use indexmap::IndexMap;
use locallibrary::device::Device;
use locallibrary::pack::{self, PackOptions};
use locallibrary::progress::Ctx;

#[allow(unused_imports, reason = "this test uses little of what's there for the others")]
mod common;

const SOURCE: &str = "EXPORT_COUNTERS_SOURCE";
const TARGET: &str = "EXPORT_COUNTERS_TARGET";

/// The write that's traced, run as a test of its own so that the trace holds nothing else.
#[test]
#[ignore = "run by a_write_costs_what_its_budgets_say"]
fn the_traced_write() {
    write_library(Path::new(&std::env::var(SOURCE).unwrap()), Path::new(&std::env::var(TARGET).unwrap()));
}

fn write_library(source: &Path, target: &Path) {
    let source = Device::open_export(source).unwrap();
    let mut target = Device::open(target).unwrap();
    let ctx = Ctx::silent();
    let library = source.library().unwrap();
    let playlists = library.playlists.iter().map(|p| library.playlist_path(p.id)).collect();
    let plan = pack::plan(&source, &target, &PackOptions { playlists, ..Default::default() }, &ctx).unwrap();
    pack::apply(&plan, &source, &mut target, None, &ctx).unwrap();
}

/// What's counted works everywhere, `strace` or not.
#[test]
fn the_write_that_is_counted_writes_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let source = common::bigger_drive(dir.path(), "source");
    let target = dir.path().join("target");
    fs::create_dir(&target).unwrap();
    write_library(&source, &target);
    assert!(!common::library(&target).tracks.is_empty());
}

/// One traced call, as `strace -y` wrote it, split into its name, its arguments and what it
/// returned: `write(5</drive/Contents/a.mp3>, "..."..., 4096) = 4096`. strace pads a short call
/// to line up what it returned: `fsync(4</drive>)        = 0`.
fn call(line: &str) -> Option<(&str, &str, i64)> {
    let (name, rest) = line.split_once('(')?;
    let (args, returned) = rest.rsplit_once(") ")?;
    let returned = returned.trim_start().strip_prefix("= ")?;
    let returned = returned.split_whitespace().next()?.split('<').next()?.parse().ok()?;
    Some((name, args, returned))
}

/// What the calls in `trace` did below `target`.
fn counted(trace: &str, target: &str, counts: &mut IndexMap<&'static str, u64>) {
    for (name, args, returned) in trace.lines().filter_map(call) {
        if returned < 0 || !args.contains(target) {
            continue;
        }
        let mut add = |metric, by| *counts.entry(metric).or_default() += by;
        match name {
            "openat" if args.contains("O_CREAT") => add("files_made", 1),
            "mkdir" | "mkdirat" => add("folders_made", 1),
            "rename" | "renameat" | "renameat2" => add("renames", 1),
            "fsync" | "fdatasync" => add("flushes", 1),
            "utimensat" | "futimens" => add("times_set", 1),
            "write" | "pwrite64" => {
                add("writes", 1);
                add("bytes_written", returned as u64);
                let largest = counts.entry("largest_write").or_default();
                *largest = (*largest).max(returned as u64);
            }
            _ => {}
        }
    }
}

#[test]
fn a_write_costs_what_its_budgets_say() {
    let Ok(out) = std::env::var("LOCALLIBRARY_BUDGETS_OUT") else { return };
    let dir = tempfile::tempdir().unwrap();
    let source = common::bigger_drive(dir.path(), "source");
    // Another volume, as a drive is: a copy on the source's own may go another way (a clone).
    let drive = tempfile::tempdir_in("/dev/shm").expect("the budgets job runs on Linux");
    let target = drive.path().join("target");
    fs::create_dir(&target).unwrap();
    let trace = dir.path().join("trace");
    let traced = Command::new("strace")
        .args(["-ff", "-y", "-qq", "-e"])
        .arg("trace=openat,mkdir,mkdirat,rename,renameat,renameat2,fsync,fdatasync,utimensat,write,pwrite64")
        .arg("-o")
        .arg(&trace)
        .arg(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "the_traced_write"])
        .env(SOURCE, &source)
        .env(TARGET, &target)
        .output()
        .expect("the budgets job installs strace");
    let said = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    assert!(traced.status.success(), "{}{}", said(&traced.stdout), said(&traced.stderr));

    let mut counts = IndexMap::new();
    for file in fs::read_dir(dir.path()).unwrap().map(Result::unwrap) {
        if file.file_name().to_string_lossy().starts_with("trace.") {
            let text = String::from_utf8_lossy(&fs::read(file.path()).unwrap()).into_owned();
            counted(&text, &target.to_string_lossy(), &mut counts);
        }
    }
    assert!(counts["files_made"] > 0, "nothing traced below {}", target.display());
    fs::create_dir_all(&out).unwrap();
    fs::write(Path::new(&out).join("export.json"), serde_json::to_vec_pretty(&counts).unwrap()).unwrap();
}

#[test]
fn traced_calls_are_counted_by_what_they_touched() {
    let trace = r#"openat(AT_FDCWD</x>, "/drive/Contents/a.mp3", O_RDWR|O_CREAT|O_EXCL|O_CLOEXEC, 0666) = 5</drive/Contents/a.mp3>
openat(AT_FDCWD</x>, "/drive/Contents/b.mp3", O_RDONLY|O_CLOEXEC) = 6</drive/Contents/b.mp3>
openat(AT_FDCWD</x>, "/elsewhere/a.mp3", O_RDWR|O_CREAT, 0666) = 7</elsewhere/a.mp3>
mkdir("/drive/Contents", 0777) = 0
mkdir("/drive/Contents", 0777) = -1 EEXIST (File exists)
renameat2(AT_FDCWD</x>, "/drive/.t", AT_FDCWD</x>, "/drive/a", 0) = 0
write(5</drive/Contents/a.mp3>, "ab"..., 1048576) = 1048576
write(5</drive/Contents/a.mp3>, "ab"..., 12) = 12
write(7</elsewhere/a.mp3>, "ab"..., 99) = 99
fsync(5</drive/Contents/a.mp3>) = 0
fsync(8</drive>)                        = 0
utimensat(AT_FDCWD</x>, "/drive/Contents/a.mp3", [UTIME_OMIT, {tv_sec=1}], 0) = 0
"#;
    let mut counts = IndexMap::new();
    counted(trace, "/drive", &mut counts);
    let want = [
        ("files_made", 1),
        ("folders_made", 1),
        ("renames", 1),
        ("writes", 2),
        ("bytes_written", 1_048_588),
        ("largest_write", 1_048_576),
        ("flushes", 2),
        ("times_set", 1),
    ];
    assert_eq!(counts, want.into_iter().collect::<IndexMap<_, _>>());
}
