//! src/tokens.rs is packages/tokens/colors.json as Rust. The crate is published on its own, so it
//! can't read the JSON as it builds; this test keeps the two in step. When the JSON changes,
//! regenerate the Rust: run this test with `LOCALLIBRARY_BLESS=1`.

use std::fmt::Write;
use std::path::Path;

use serde_json::Value;

const RERUN: &str = "LOCALLIBRARY_BLESS=1 cargo test -p loclib-model --test tokens";

/// `#rrggbb` as a Rust `[u8; 3]`.
fn rgb(v: &Value, at: &str) -> String {
    let hex = v.as_str().and_then(|s| s.strip_prefix('#')).filter(|h| h.len() == 6);
    let hex = hex.unwrap_or_else(|| panic!("{at} isn't #rrggbb: {v}"));
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or_else(|_| panic!("{at}: {v}"));
    format!("[{}, {}, {}]", byte(0), byte(2), byte(4))
}

/// A module of `Rgb` constants, one per key of `group` in `keys`' order.
fn module(out: &mut String, doc: &str, name: &str, group: &Value, keys: &[&str], indent: &str) {
    for line in doc.lines() {
        writeln!(out, "{indent}/// {line}").unwrap();
    }
    writeln!(out, "{indent}pub mod {name} {{").unwrap();
    writeln!(out, "{indent}    use super::Rgb;").unwrap();
    for key in keys {
        let value = rgb(&group[key], &format!("{name}.{key}"));
        writeln!(out, "{indent}    pub const {}: Rgb = {value};", key.to_ascii_uppercase()).unwrap();
    }
}

fn generate(json: &Value) -> String {
    let mut out = String::from(
        "//! Colours that mean the same wherever DJ data is drawn: the player, locallibrary's HTML report\n\
         //! and the website's mix report. Each keeps its own chrome and accent.\n\
         //!\n\
         //! Generated from packages/tokens/colors.json in the repository: don't edit it by hand. After\n",
    );
    writeln!(out, "//! changing the JSON, regenerate it with `{RERUN}`.").unwrap();
    out.push_str("\n/// An 8-bit sRGB colour.\npub type Rgb = [u8; 3];\n\n");

    let bands = &json["bands"];
    module(&mut out, "The 3-band waveform.", "bands", bands, &["low", "mid", "high"], "");
    let scale = |k: &str| bands["scales"][k].as_f64().unwrap_or_else(|| panic!("bands.scales.{k} isn't a number"));
    out.push_str("    /// Each band's height against the lows' (low, mid, high): lows outermost, the highs a bright\n");
    out.push_str("    /// core.\n");
    writeln!(out, "    pub const SCALES: [f32; 3] = [{:?}, {:?}, {:?}];", scale("low"), scale("mid"), scale("high"))
        .unwrap();
    out.push_str("}\n\n");

    let phrases = ["intro", "up", "verse", "chorus", "down", "bridge", "outro", "unknown"];
    module(&mut out, "rekordbox's phrase kinds by role.", "phrases", &json["phrases"], &phrases, "");
    out.push_str("}\n\n");

    let cue_doc = "Cues without a colour of their own.";
    module(&mut out, cue_doc, "cues", &json["cues"], &["hot", "memory", "loop"], "");
    out.push_str("}\n\n");

    let colors = ["pink", "red", "orange", "yellow", "green", "aqua", "blue", "purple"];
    let track_doc = "rekordbox's track colours (and memory cue colour table), roughly as its players show them.";
    module(&mut out, track_doc, "track_colors", &json["trackColors"], &colors, "");
    out.push_str("}\n\n");

    out.push_str("/// Good, warning and critical.\npub mod status {\n    use super::Rgb;\n\n");
    let levels = ["good", "warning", "critical"];
    module(&mut out, "For a dark background.", "dark", &json["status"]["dark"], &levels, "    ");
    out.push_str("    }\n\n");
    let light_doc = "For a light background: 4.5:1 on a near-white page, so they work as text.";
    module(&mut out, light_doc, "light", &json["status"]["light"], &levels, "    ");
    out.push_str("    }\n}\n\n");

    writeln!(out, "/// The playhead.\npub const PLAYHEAD: Rgb = {};", rgb(&json["playhead"], "playhead")).unwrap();
    out
}

#[test]
fn tokens_match_the_shared_json() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = root.join("../../packages/tokens/colors.json");
    // A packaged crate has no repository around it: nothing to compare with.
    let Ok(text) = std::fs::read_to_string(&source) else {
        eprintln!("skipped: {} isn't here", source.display());
        return;
    };
    let json: Value = serde_json::from_str(&text).expect("colors.json is JSON");
    let made = generate(&json);
    let target = root.join("src/tokens.rs");
    let kept = std::fs::read_to_string(&target).unwrap_or_default();
    if made == kept {
        return;
    }
    if std::env::var_os("LOCALLIBRARY_BLESS").is_some_and(|v| v == "1") {
        std::fs::write(&target, made).unwrap();
        return;
    }
    panic!(
        "crates/loclib-model/src/tokens.rs is out of date with packages/tokens/colors.json. Regenerate it with `{RERUN}`"
    );
}

/// The colours a theme sets are colors.schema.json's colours, but a light background's status
/// colours (hosts work those out from the dark ones), in its order, then the player's own.
#[test]
fn theme_tokens_are_the_schemas_colours() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let Ok(text) = std::fs::read_to_string(root.join("../../packages/tokens/colors.schema.json")) else {
        eprintln!("skipped: colors.schema.json isn't here");
        return;
    };
    let schema: Value = serde_json::from_str(&text).expect("the schema is JSON");
    // Each property whose value is a colour (`#/$defs/hex`), as a dotted name.
    fn colours(schema: &Value, defs: &Value, at: &str, out: &mut Vec<String>) {
        let schema = match schema["$ref"].as_str().and_then(|r| r.strip_prefix("#/$defs/")) {
            Some("hex") => return out.push(at.to_string()),
            Some(def) => &defs[def],
            None => schema,
        };
        for (key, property) in schema["properties"].as_object().into_iter().flatten() {
            let name = if at.is_empty() { key.clone() } else { format!("{at}.{key}") };
            colours(property, defs, &name, out);
        }
    }
    let mut named = Vec::new();
    colours(&schema, &schema["$defs"], "", &mut named);
    let named: Vec<&str> = named.iter().map(String::as_str).filter(|n| !n.starts_with("status.light.")).collect();
    let tokens = loclib_model::theme::TOKENS;
    let ours = tokens.iter().position(|t| t.starts_with("ui.")).expect("the player's own");
    assert_eq!(&tokens[..ours], &named[..]);
    // Not echoed on failure: CodeQL takes what's named `tokens` for something secret.
    assert!(tokens[ours..].iter().all(|t| t.starts_with("ui.") || t.starts_with("waveform.")));
}
