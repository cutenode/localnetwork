use super::*;
use crate::Error;
use crate::format::players::{EXFAT, NO_EXFAT};

/// The table agrees with the exFAT lists `disks` and `format` show.
#[test]
fn agrees_with_the_exfat_lists() {
    let parse = |entry: &str| -> Vec<(String, Option<String>)> {
        let (names, note) = match entry.split_once(" (") {
            Some((n, rest)) => {
                (n, rest.trim_end_matches(')').strip_prefix("firmware ").map(|v| v.trim_end_matches('+')))
            }
            None => (entry, None),
        };
        names.split('/').map(|n| (n.to_string(), note.map(str::to_string))).collect()
    };
    for (name, firmware) in EXFAT.iter().flat_map(|e| parse(e)) {
        let m = find(&name).unwrap_or_else(|| panic!("{name} isn't in the table"));
        match (firmware, m.exfat.value) {
            (Some(v), Exfat::Firmware(f)) => assert_eq!(f, v, "{name}"),
            (None, Exfat::Yes) => {}
            (listed, table) => panic!("{name}: the list says {listed:?}, the table {table:?}"),
        }
    }
    for (name, _) in NO_EXFAT.iter().flat_map(|e| parse(e)) {
        let m = find(&name).unwrap_or_else(|| panic!("{name} isn't in the table"));
        assert_eq!(m.exfat.value, Exfat::No, "{name}");
    }
    let listed = EXFAT.len() + NO_EXFAT.iter().map(|e| parse(e).len()).sum::<usize>();
    assert_eq!(listed, MODELS.len(), "every model is on one of the lists");
}

#[test]
fn remembered_models() {
    let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let e = Error::from(canonical(&names(&["CDJ-3000", "XYZ-9"])).unwrap_err());
    assert_eq!(e.code(), "invalid");
    let e = e.to_string();
    assert!(
        e.starts_with("not a model locallibrary knows: XYZ-9 (it knows CDJ-3000X, ") && e.contains("OPUS-QUAD"),
        "{e}"
    );
    let lib = Uuid::new_v4();
    let r = Remembered { default: names(&["CDJ-3000"]), libraries: [(lib, names(&["OPUS-QUAD"]))].into() };
    assert_eq!(r.for_library(Some(lib)), (&names(&["OPUS-QUAD"])[..], true));
    assert_eq!(r.for_library(Some(Uuid::new_v4())), (&names(&["CDJ-3000"])[..], false));
    assert_eq!(r.for_library(None), (&names(&["CDJ-3000"])[..], false));
}
