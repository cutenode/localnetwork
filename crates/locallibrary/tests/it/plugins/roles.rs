//! Field roles: the analyzers' fields with each role, in the order the plugins are, and an
//! analyzer's key and BPM written to the tracks as an edit plan.

use locallibrary::api::DriveRef;
use locallibrary::edit::{Edit, TrackFields};
use locallibrary::fields::roles::RoleField;
use locallibrary::fields::{FieldValue, Role};
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

/// Analysing again works a track out over what was kept for it, as when the library said
/// something else of it before (its audio file, which values are kept by, the same); analysing
/// leaves what was kept.
#[test]
fn analysing_again_works_out_over_what_was_kept() {
    let f = Fixture::new(|dir| vec![spec(dir, &heard("echo"), &[])], Options::default());
    let tracks = f.hear(&[json!({ "tone": "8A" })]);
    let ids = [tracks[0].id];
    f.plugins.analyze("echo", "heard", "USB", Some(&ids), &f.store, &Ctx::silent()).unwrap();
    let audio = paths::to_fs(&f.drive, &paths::key(&tracks[0].file_path));
    let heard = locallibrary::fields::Analyzer { plugin: "echo".into(), analyzer: "heard".into(), version: "1".into() };
    let fields = locallibrary::fields::FieldStore::in_store(&f.store);
    fields.put(&audio, &heard, &[("tone".into(), FieldValue::String("1A".into()))].into_iter().collect()).unwrap();
    let tone = || fields.get(&audio, std::slice::from_ref(&heard))["echo"]["heard"]["tone"].clone();
    let kept = f.plugins.analyze("echo", "heard", "USB", Some(&ids), &f.store, &Ctx::silent()).unwrap();
    assert_eq!((kept.analyzed, kept.kept, tone()), (0, 1, FieldValue::String("1A".into())));
    let again = f.plugins.analyze_again("echo", "heard", "USB", &ids, &f.store, &Ctx::silent()).unwrap();
    assert_eq!((again.analyzed, again.kept, tone()), (1, 0, FieldValue::String("8A".into())));
}

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

/// What `heard` kept for one track, as a test draws it: a value for each of its fields, or none.
#[derive(Debug, Clone)]
struct Kept {
    tone: Option<FieldValue>,
    tempo: Option<FieldValue>,
    sure: Option<f64>,
    steady: Option<f64>,
}

/// What `heard` might have kept for `t`: its key as the track has it, the parallel one, any other
/// Camelot code, Open Key, or not a key; a BPM as the track has it, twice it, of any size, a whole
/// number, null; how sure it is of each, or not saying; or nothing at all (not analysed).
#[hegel::test_helper]
fn kept(tc: &hegel::TestCase, t: &locallibrary::Track) -> Option<Kept> {
    use hegel::generators as gs;
    if tc.draw(gs::integers::<u8>().max_value(5)) == 0 {
        return None;
    }
    let own = Camelot::parse(&t.key);
    let number = |n: u8| (n % 12) + 1;
    let tone = match tc.draw(gs::integers::<u8>().max_value(6)) {
        0 => own.map(|k| FieldValue::String(k.code())),
        1 => own.map(|k| FieldValue::String(k.parallel().code())),
        2 => {
            let n = number(tc.draw(gs::integers::<u8>()));
            Some(FieldValue::String(format!("{n}{}", if tc.draw(gs::booleans()) { "A" } else { "B" })))
        }
        3 => Some(FieldValue::String(format!("{}m", number(tc.draw(gs::integers::<u8>()))))),
        4 => Some(FieldValue::String(tc.draw(gs::sampled_from(vec!["", "not a key", "13A"])).into())),
        5 => Some(FieldValue::Null),
        _ => None,
    };
    let tempo = match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => Some(FieldValue::Number(t.bpm)),
        1 => Some(FieldValue::Number(t.bpm * 2.0 + tc.draw(gs::sampled_from(vec![0.0, 0.004, 0.006])))),
        // To thousandths: rounded to hundredths, it's another number than to tenths or wholes.
        2 => Some(FieldValue::Number(tc.draw(gs::integers::<u32>().max_value(1_200_000)) as f64 / 1000.0)),
        3 => Some(FieldValue::Integer(tc.draw(gs::integers::<i64>().min_value(0).max_value(1100)))),
        4 => Some(FieldValue::Null),
        _ => None,
    };
    let sure = || tc.draw(gs::optional(gs::floats::<f64>().min_value(0.0).max_value(1.0)));
    Some(Kept { tone, tempo, sure: sure(), steady: sure() })
}

/// For any values `heard` kept for the drive's tracks and any request (which roles, which
/// tracks, over what they have or not, how sure it must be), the plan edits exactly the tracks
/// that would change: a key in Camelot code becomes rekordbox's name for it, unless the track has
/// it already (in any notation) or has one and isn't to be overwritten; a BPM is kept to
/// hundredths, between 1 and 999.99, likewise; a value the analyzer is less sure of than asked
/// (or doesn't say) is left out and counted; a track not analysed is counted.
#[hegel::test(test_cases = 40, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_analyzers_values_are_planned_onto_the_tracks_as_the_rules_say(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::fields::{Analyzer, FieldStore};
    let f = Fixture::new(|dir| vec![spec(dir, &heard("echo"), &[])], Options::default());
    let library = common::library(&f.drive);
    let analyzer = Analyzer { plugin: "echo".into(), analyzer: "heard".into(), version: "1".into() };
    let store = FieldStore::in_store(&f.store);
    let mut drawn = Vec::new();
    for t in &library.tracks {
        let kept = kept(&tc, t);
        if let Some(k) = &kept {
            let values = [("tone", k.tone.clone()), ("tempo", k.tempo.clone())]
                .into_iter()
                .chain([("sure", k.sure), ("steady", k.steady)].map(|(n, v)| (n, v.map(FieldValue::Number))))
                .filter_map(|(name, v)| Some((name.to_string(), v?)))
                .collect();
            store.put(&paths::to_fs(&f.drive, &paths::key(&t.file_path)), &analyzer, &values).unwrap();
        }
        drawn.push((t, kept));
    }
    let fields: Vec<Role> = tc.draw(gs::subsequences(vec![Role::Key, Role::Bpm]).min_size(1));
    let ids: Vec<u32> = library.tracks.iter().map(|t| t.id).collect();
    let tracks = tc.draw(gs::optional(gs::subsequences(ids)));
    let overwrite = tc.draw(gs::booleans());
    let min_confidence = tc.draw(gs::optional(gs::floats::<f64>().min_value(0.0).max_value(1.0)));

    // The model.
    let (mut edits, mut unanalysed, mut unsure) = (Vec::new(), 0, 0);
    for (t, kept) in drawn.iter().filter(|(t, _)| tracks.as_ref().is_none_or(|ids| ids.contains(&t.id))) {
        let Some(k) = kept else {
            unanalysed += 1;
            continue;
        };
        let sure = |confidence: Option<f64>| min_confidence.is_none_or(|min| confidence.is_some_and(|c| c >= min));
        let wanted = |role, has: bool, same: bool| fields.contains(&role) && !same && (overwrite || !has);
        let (mut key, mut bpm) = (None, None);
        if let Some(FieldValue::String(code)) = &k.tone
            && let Some(c) = Camelot::parse(code)
            && wanted(Role::Key, !t.key.trim().is_empty(), Camelot::parse(&t.key) == Some(c))
        {
            match sure(k.sure) {
                true => key = Some(c.note()),
                false => unsure += 1,
            }
        }
        let tempo = match k.tempo {
            Some(FieldValue::Number(n)) => Some(n),
            Some(FieldValue::Integer(n)) => Some(n as f64),
            _ => None,
        };
        if let Some(b) = tempo.map(|b| (b * 100.0).round() / 100.0)
            && (1.0..=999.99).contains(&b)
            && wanted(Role::Bpm, t.bpm > 0.0, b == t.bpm)
        {
            match sure(k.steady) {
                true => bpm = Some(b),
                false => unsure += 1,
            }
        }
        if key.is_some() || bpm.is_some() {
            edits.push((vec![t.file_path.clone()], key, bpm));
        }
    }

    let req = ApplyFields {
        plugin: "echo".into(),
        analyzer: "heard".into(),
        drive: "USB".into(),
        fields: fields.clone(),
        tracks,
        overwrite,
        min_confidence,
    };
    let planned = f.plugins.plan_apply_fields(&req, &f.store, &Ctx::silent()).unwrap();
    let got: Vec<(Vec<String>, Option<String>, Option<f64>)> = planned
        .recorded
        .iter()
        .map(|e| match e {
            Edit::EditTracks { tracks, fields } => (tracks.paths.clone(), fields.key.clone(), fields.bpm),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(got, edits);
    let what: Vec<&str> = fields.iter().map(|r| r.name()).collect();
    assert_eq!(planned.summary[0], format!("write {} from Heard to {} track(s)", what.join(" and "), edits.len()));
    let mut warnings = Vec::new();
    if unanalysed > 0 {
        warnings.push(format!("{unanalysed} track(s) aren't analysed by Heard yet"));
    }
    if let Some(min) = min_confidence.filter(|_| unsure > 0) {
        warnings.push(format!("{unsure} value(s) left out: Heard is less sure of them than {min}"));
    }
    assert_eq!(planned.warnings, warnings);
}
