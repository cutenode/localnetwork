use std::path::Path;

fn pretty<T: serde::Serialize + ?Sized>(value: &T) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("serialisable");
    text.push('\n');
    text
}

/// equipment.json (and, with the `schemars` feature, equipment.schema.json) is the table as serde
/// writes it. When the table changes, regenerate them: run this test with `LOCALLIBRARY_BLESS=1`.
#[test]
fn the_json_beside_the_crate_is_current() {
    let files = [
        ("equipment.json", pretty(loclib_equipment::MODELS)),
        #[cfg(feature = "schemars")]
        ("equipment.schema.json", pretty(&schemars::schema_for!([loclib_equipment::Model]))),
    ];
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bless = std::env::var_os("LOCALLIBRARY_BLESS").is_some_and(|v| v == "1");
    let mut stale = Vec::new();
    for (name, made) in files {
        let kept = std::fs::read_to_string(root.join(name)).unwrap_or_default();
        if made == kept {
            continue;
        }
        if bless {
            std::fs::write(root.join(name), made).unwrap();
        } else {
            stale.push(name);
        }
    }
    assert!(
        stale.is_empty(),
        "crates/loclib-equipment/{stale:?} is out of date: the equipment table changed. Regenerate it with \
         `LOCALLIBRARY_BLESS=1 cargo test -p loclib-equipment --all-features --test json`"
    );
}
