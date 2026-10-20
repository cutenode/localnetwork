//! Field roles: the analyzers' fields with each role, in the order the plugins are, and an
//! analyzer's key and BPM written to the tracks as an edit plan.

use locallibrary::api::DriveRef;
use locallibrary::edit::{Edit, TrackFields};
use locallibrary::fields::Role;
use locallibrary::fields::roles::RoleField;
use locallibrary::harmony::Camelot;
use locallibrary::paths;
use locallibrary::plugin::ApplyFields;
use locallibrary::query::TrackSelector;

use super::*;

/// The echo manifest (as plugin `id`) with an analyzer, `heard`, answering what the test writes
/// in each audio file: a key and a BPM, and how sure it is of each.
fn heard(id: &str) -> String {
    ECHO.replace("id = \"echo\"", &format!("id = \"{id}\"")).replace(
        "fields = { bytes = \"integer\" }\n",
        "fields = { bytes = \"integer\" }\n\n[[contributes.analyzers]]\nname = \"heard\"\ntitle = \"Heard\"\n\
         version = \"1\"\nfields = { tone = \"string\", tempo = \"number\", sure = \"number\", steady = \"number\" }\n\
         roles = { key = \"tone\", bpm = \"tempo\", key_confidence = \"sure\", bpm_confidence = \"steady\" }\n",
    )
}

#[test]
fn roles_are_offered_in_the_order_the_plugins_are() {
    let f =
        Fixture::new(|dir| vec![spec(dir, &heard("echo"), &[]), spec(dir, &heard("second"), &[])], Options::default());
    let offered = f.plugins.roles();
    let names = |role| offered[&role].iter().map(RoleField::name).collect::<Vec<_>>();
    assert_eq!(
        offered.keys().copied().collect::<Vec<_>>(),
        [Role::Key, Role::Bpm, Role::KeyConfidence, Role::BpmConfidence]
    );
    assert_eq!(names(Role::Key), ["echo.heard", "second.heard"]);
    assert_eq!(
        (offered[&Role::Bpm][0].field.as_str(), offered[&Role::Bpm][0].analyzer.version.as_str()),
        ("tempo", "1")
    );
    assert!(Fixture::echo().plugins.roles().is_empty(), "a plugin without roles offers none");
}

impl Fixture {
    /// The `heard` analyzer's answers for each track: what's written in its audio file.
    fn hear(&self, answers: &[Value]) -> Vec<locallibrary::Track> {
        let tracks = common::library(&self.drive).tracks;
        for (t, answer) in tracks.iter().zip(answers) {
            std::fs::write(paths::to_fs(&self.drive, &paths::key(&t.file_path)), answer.to_string()).unwrap();
        }
        tracks
    }

    fn apply_fields(&self, fields: &[Role], overwrite: bool, min_confidence: Option<f64>) -> Result<Planned<EditPlan>> {
        let req = ApplyFields {
            plugin: "echo".into(),
            analyzer: "heard".into(),
            drive: "USB".into(),
            fields: fields.to_vec(),
            tracks: None,
            overwrite,
            min_confidence,
        };
        self.plugins.plan_apply_fields(&req, &self.store, &Ctx::silent())
    }
}

use locallibrary::edit::EditPlan;
use locallibrary::op::Planned;

/// What an analyzer heard is written to the tracks as an edit plan, journaled: a key as rekordbox
/// names it, a BPM to hundredths, only where a track has none unless overwriting, never one it has
/// in another notation, and none the analyzer is less sure of than asked.
#[test]
fn an_analyzers_key_and_bpm_are_written_as_an_edit() {
    let f = Fixture::new(|dir| vec![spec(dir, &heard("echo"), &[])], Options::default());
    let before = common::library(&f.drive).tracks;
    let key = |t: &locallibrary::Track| Camelot::parse(&t.key).unwrap();
    let answers = [
        json!({ "tone": key(&before[0]).parallel().code(), "tempo": before[0].bpm * 2.0 + 0.004, "sure": 0.9, "steady": 0.3 }),
        json!({ "tone": key(&before[1]).code(), "tempo": before[1].bpm, "sure": 0.9, "steady": 0.9 }),
    ];
    let tracks = f.hear(&answers);
    // Only the first is analysed yet.
    f.plugins.analyze("echo", "heard", "USB", Some(&[tracks[0].id]), &f.store, &Ctx::silent()).unwrap();
    let kept = f.apply_fields(&[Role::Key, Role::Bpm], false, None).unwrap();
    assert!(kept.noop, "both tracks have a key and a BPM: {:?}", kept.summary);
    assert_eq!(kept.warnings, ["1 track(s) aren't analysed by Heard yet"]);
    f.plugins.analyze("echo", "heard", "USB", None, &f.store, &Ctx::silent()).unwrap();

    let unsure = f.apply_fields(&[Role::Key, Role::Bpm], true, Some(0.5)).unwrap();
    assert_eq!(unsure.summary[0], "write key and bpm from Heard to 1 track(s)");
    assert_eq!(unsure.warnings, ["1 value(s) left out: Heard is less sure of them than 0.5"]);
    let planned = f.apply_fields(&[Role::Key, Role::Bpm], true, None).unwrap();
    assert_eq!(&*planned.operation, "plugins.apply_fields");
    planned.apply(None, &Ctx::silent()).unwrap();
    let after = common::library(&f.drive).tracks;
    assert_eq!((after[0].key.clone(), after[0].bpm), (key(&before[0]).parallel().note(), before[0].bpm * 2.0));
    assert_eq!((&after[1].key, after[1].bpm), (&before[1].key, before[1].bpm), "as it was");
    assert_eq!(api::edits_list(&DriveRef { drive: common::path(&f.drive) }).unwrap().len(), 1, "journaled");

    // A track without a key gets one, keeping its BPM.
    let clear = TrackFields { key: Some(String::new()), ..TrackFields::default() };
    common::apply(
        &f.drive,
        &[Edit::EditTracks { tracks: TrackSelector { ids: vec![after[1].id], ..Default::default() }, fields: clear }],
    );
    let filled = f.apply_fields(&[Role::Key, Role::Bpm], false, None).unwrap();
    assert_eq!(filled.summary[0], "write key and bpm from Heard to 1 track(s)");
    filled.apply(None, &Ctx::silent()).unwrap();
    assert_eq!(common::library(&f.drive).tracks[1].key, key(&before[1]).note());
}

#[test]
fn only_a_tracks_key_and_bpm_the_analyzer_has_are_written() {
    let f = Fixture::new(|dir| vec![spec(dir, &heard("echo"), &[])], Options::default());
    let e = |fields: &[Role]| f.apply_fields(fields, false, None).unwrap_err().to_string();
    assert_eq!(e(&[]), "say what to write: key, bpm");
    assert_eq!(e(&[Role::Loudness]), "a track has no loudness to write: key or bpm");
    let req = ApplyFields {
        plugin: "echo".into(),
        analyzer: "size".into(),
        drive: "USB".into(),
        fields: vec![Role::Key],
        tracks: Some(vec![9999]),
        overwrite: false,
        min_confidence: None,
    };
    let size = f.plugins.plan_apply_fields(&req, &f.store, &Ctx::silent()).unwrap_err();
    assert_eq!(size.to_string(), "File size works out no key");
    let req = ApplyFields { analyzer: "heard".into(), ..req };
    assert_eq!(f.plugins.plan_apply_fields(&req, &f.store, &Ctx::silent()).unwrap_err().code(), "not_found");
    let req = ApplyFields { analyzer: "nope".into(), ..req };
    assert_eq!(f.plugins.plan_apply_fields(&req, &f.store, &Ctx::silent()).unwrap_err().code(), "not_found");
}
