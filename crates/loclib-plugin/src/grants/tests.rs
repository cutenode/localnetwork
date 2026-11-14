//! The README's table of reads is made from [`READS`]. When the table changes, update it with
//! `LOCALLIBRARY_BLESS=1 cargo nextest run -p loclib-plugin -E 'test(/grants::tests::/)'`.

use std::path::Path;

use super::*;

const RERUN: &str = "LOCALLIBRARY_BLESS=1 cargo nextest run -p loclib-plugin -E 'test(/grants::tests::/)'";
/// Comments Markdown drops, in the protocol's list.
const START: &str = "   [//]: # (The table of reads is made from READS: see src/grants/tests.rs.)";
const END: &str = "   [//]: # (End of the table of reads.)";

#[test]
fn each_read_is_listed_once() {
    for (at, read) in READS.iter().enumerate() {
        assert!(READS[..at].iter().all(|r| r.method != read.method), "{} is listed twice", read.method);
    }
}

#[test]
fn a_read_takes_each_param_once_a_library_read_drive_first() {
    for read in READS {
        let names: Vec<&str> = read.params().map(|(name, _)| name).collect();
        for (at, name) in names.iter().enumerate() {
            assert!(!names[..at].contains(name), "{} takes `{name}` twice", read.method);
        }
        if read.library {
            assert_eq!(names[..2], ["drive", "combined"], "{}", read.method);
        }
    }
    let tree = READS.iter().find(|r| r.method == "playlists.tree").unwrap();
    assert_eq!(tree.params().collect::<Vec<_>>(), [("drive", Param::Drive), ("combined", Param::Plain)]);
    let tags = READS.iter().find(|r| r.method == "tags.list").unwrap();
    assert_eq!(tags.params().collect::<Vec<_>>(), [("drive", Param::Drive)], "not a library's read");
}

/// A param as the README says it.
fn said((name, param): (&str, Param)) -> String {
    match param {
        Param::Plain => format!("`{name}`"),
        Param::Drive => format!("`{name}` (a drive's id or name)"),
        Param::Drives => format!("`{name}` (drives' ids or names)"),
        Param::AtMost(most) => format!("`{name}` (at most {most})"),
        Param::Limit(most) => format!("`{name}` (at most {most}, {most} unless given)"),
        Param::Or(default) => format!("`{name}` (`{default}` unless given)"),
        Param::Scratch => format!("`{name}` (a file in the scratch folder)"),
    }
}

/// [`READS`] as the README has it: a row each, its capability where that changes.
fn table() -> String {
    let rows: String = READS
        .iter()
        .enumerate()
        .map(|(at, read)| {
            let first = at == 0 || READS[at - 1].capability != read.capability;
            let capability = if first { format!("`{}`", read.capability) } else { String::new() };
            let params: Vec<String> = read.params().map(said).collect();
            let params = if params.is_empty() { "none".to_string() } else { params.join(", ") };
            format!("   | {capability} | `{}` | {params} |\n", read.method)
        })
        .collect();
    format!("{START}\n\n   | Capability | Read | Params |\n   | --- | --- | --- |\n{rows}\n{END}")
}

#[test]
fn the_readme_lists_the_reads() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md");
    let readme = std::fs::read_to_string(&path).unwrap();
    let start = readme.find(START).expect("the start of the table of reads");
    let end = readme.find(END).expect("the end of the table of reads") + END.len();
    let made = format!("{}{}{}", &readme[..start], table(), &readme[end..]);
    if std::env::var_os("LOCALLIBRARY_BLESS").is_some_and(|v| v == "1") {
        return std::fs::write(&path, made).unwrap();
    }
    assert!(readme == made, "README.md's table of reads isn't READS: update it with\n  {RERUN}");
}
