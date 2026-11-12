//! manifest.schema.json beside the crate: the JSON Schema of `locallibrary-plugin.toml`, made from
//! [`Manifest`] as TOML is read (a field with a default may be left out).

use std::path::{Path, PathBuf};

use loclib_plugin::Manifest;
use schemars::generate::SchemaSettings;
use serde_json::Value;

const HEAD: &str = "id = \"demo\"\nname = \"Demo\"\nversion = \"0.1.0\"\nprotocol = \"^0.2\"\n";

const ECHO: &str = include_str!("../examples/echo/locallibrary-plugin.toml");

fn path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("manifest.schema.json")
}

/// manifest.schema.json is what `Manifest` makes. When the manifest changes, regenerate it: run
/// this test with `LOCALLIBRARY_BLESS=1`.
#[test]
fn the_manifest_schema_beside_the_crate_is_current() {
    let mut schema =
        SchemaSettings::draft2020_12().for_deserialize().into_generator().into_root_schema_for::<Manifest>();
    schema.insert("$id".into(), "manifest.schema.json".into());
    schema.insert("title".into(), "locallibrary-plugin.toml".into());
    schema.insert(
        "description".into(),
        "A locallibrary plugin's manifest as its author writes it: who the plugin is, what it may call back \
         for and what it adds. Keys it doesn't list are allowed, so an older locallibrary reads a newer \
         plugin's manifest. `Manifest::validate` in loclib-plugin checks the rest: names, versions, schemas \
         that compile and what needs a capability."
            .into(),
    );
    let mut made = serde_json::to_string_pretty(&schema).expect("serialisable");
    made.push('\n');
    let kept = std::fs::read_to_string(path()).unwrap_or_default();
    if made != kept && std::env::var_os("LOCALLIBRARY_BLESS").is_some_and(|v| v == "1") {
        std::fs::write(path(), made).unwrap();
        return;
    }
    assert!(
        made == kept,
        "crates/loclib-plugin/manifest.schema.json is out of date: the manifest changed. Regenerate it with \
         `LOCALLIBRARY_BLESS=1 cargo test -p loclib-plugin --test schema`"
    );
}

/// The schema takes manifests as their authors write them: the example, a newer plugin's with keys
/// this locallibrary doesn't know (which `Manifest::parse` leaves out) and `net:` capabilities. It
/// refuses a capability that isn't one, and a manifest without its id.
#[test]
fn the_schema_takes_manifests_as_written() {
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(path()).unwrap()).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let problems = |toml: &str| -> Vec<String> {
        let json: Value = toml::from_str(toml).unwrap();
        validator.iter_errors(&json).map(|e| e.to_string()).collect()
    };
    assert_eq!(problems(ECHO), Vec::<String>::new());
    let newer = format!(
        "{HEAD}colour = \"red\"\n[[contributes.commands]]\nname = \"c\"\ntitle = \"C\"\nmode = \"headless\"\n\
         icon = \"star\"\n[[contributes.widgets]]\nname = \"w\"\n"
    );
    Manifest::parse(&newer).unwrap().validate(false).unwrap();
    assert_eq!(problems(&newer), Vec::<String>::new());
    let net = format!("{HEAD}capabilities = [\"net:example.com\", \"net:*.archive.org\"]\n");
    assert_eq!(problems(&net), Vec::<String>::new());
    assert!(!problems(&format!("{HEAD}capabilities = [\"library:write\"]\n")).is_empty());
    assert!(!problems(&HEAD.replace("id = \"demo\"\n", "")).is_empty());
}
