use super::*;
use crate::Error;

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
