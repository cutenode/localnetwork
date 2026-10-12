//! Properties of [`Software`], as settings name it.

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use loclib_interop::Software;
use serde::Deserialize;
use serde::de::IntoDeserializer;
use serde::de::value::{Error, StrDeserializer};

/// A setting names the software in lowercase, `traktor`, `serato` or `engine`, and nothing else
/// reads as one; each reads as a different program, shown by its own name.
#[hegel::test(test_cases = 200)]
#[hegel::explicit_test_case(s = "traktor".to_string())]
#[hegel::explicit_test_case(s = "serato".to_string())]
#[hegel::explicit_test_case(s = "engine".to_string())]
fn software_reads_only_from_its_names(tc: TestCase) {
    let s = tc.draw(hegel::one_of!(
        gs::sampled_from(vec!["traktor", "serato", "engine", "Traktor", "ENGINE", "engine dj", "rekordbox", ""])
            .map(str::to_string),
        gs::text().max_size(10),
    ));
    let read = Software::deserialize::<StrDeserializer<'_, Error>>(s.as_str().into_deserializer());
    let want = match s.as_str() {
        "traktor" => Some((Software::Traktor, "Traktor")),
        "serato" => Some((Software::Serato, "Serato")),
        "engine" => Some((Software::Engine, "Engine DJ")),
        _ => None,
    };
    assert_eq!(read.ok().map(|w| (w, w.name())), want, "{s:?}");
}
