use super::*;

#[test]
fn rejects_garbage_without_panicking() {
    let p = Path::new("x.pdb");
    assert!(parse_database(vec![], p).is_err());
    assert!(parse_database(vec![0u8; 100], p).is_err());
    // Valid-looking header, garbage pages.
    let mut buf = vec![0xffu8; 4096 * 4];
    buf[0..4].copy_from_slice(&0u32.to_le_bytes());
    buf[4..8].copy_from_slice(&4096u32.to_le_bytes());
    buf[8..12].copy_from_slice(&3u32.to_le_bytes());
    assert!(parse_database(buf, p).is_err());
}

/// Whatever the database editor does, a caller gets its result: what it returns, or an error
/// saying the database is malformed (and was left unchanged) when it panics.
#[hegel::test(test_cases = 50)]
fn prop_an_editor_that_panics_gives_an_error(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let value = tc.draw(gs::integers::<u32>());
    let got = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => guard_editor(|| Ok(value)).map_err(|e| e.to_string()),
        1 => guard_editor(|| -> Result<u32> { Err(Error::Pdb(format!("refused {value}"))) }).map_err(|e| e.to_string()),
        _ => {
            let err = guard_editor(|| -> Result<u32> { panic!("row {value} past the page") }).unwrap_err();
            assert_eq!(
                err.to_string(),
                Error::Pdb("the database is malformed; it was left unchanged".into()).to_string()
            );
            return;
        }
    };
    let want = [Ok(value), Err(Error::Pdb(format!("refused {value}")).to_string())];
    assert!(want.contains(&got), "{got:?}");
}
