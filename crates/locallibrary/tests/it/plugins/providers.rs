//! Providers: what plugins find for a drive's tracks, cover art or tags, asked of every provider
//! of a kind, merged best first for each track, one alone above the setting picked for the user,
//! and made edits of by the host itself; each image held once, and what a call holds bounded.

use base64::prelude::*;
use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::plugin::{
    AUTO_PICK, Found, PlanProvided, Provide, Provided, auto_pick, plan_provided, set_auto_pick,
};
use locallibrary::query::TrackSelector;
use loclib_plugin::{ProviderKind, TrackMetadata};
use rustc_hash::FxHashSet;

use super::*;

/// The echo manifest (as plugin `id`) with a metadata provider `found` (in place of its own) and
/// an artwork provider `covers`, each answering as its `answer` setting says (see the test
/// plugin's `provide`).
fn providing(id: &str) -> String {
    let more = "[[contributes.providers]]\nname = \"covers\"\ntitle = \"Covers\"\nkind = \"artwork\"\n\n\
                [[contributes.settings]]\nkey = \"answer\"\ntitle = \"Answer\"\ntype = \"string\"\n\n\
                [[contributes.settings]]";
    ECHO.replace("id = \"echo\"", &format!("id = \"{id}\""))
        .replace("name = \"tags\"\ntitle = \"The tags as they are\"", "name = \"found\"\ntitle = \"Found\"")
        .replacen("[[contributes.settings]]", more, 1)
}

impl Fixture {
    /// The plugins `ids`, each with [`providing`]'s providers.
    fn providers(ids: &[&str]) -> Fixture {
        Fixture::bounded(ids, Options::default())
    }

    /// [`Fixture::providers`], the host run with `options`.
    fn bounded(ids: &[&str], options: Options) -> Fixture {
        let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        Fixture::new(|dir| ids.iter().map(|id| spec(dir, &providing(id), &[])).collect(), options)
    }

    /// `plugin`'s providers answer `answer`.
    fn answer(&self, plugin: &str, answer: Value) {
        self.plugins.set_setting(plugin, &self.store, "answer", Some(answer.to_string().into())).unwrap();
    }

    fn provide(&self, kind: ProviderKind, from: Option<&str>, replace: bool) -> Result<Provided> {
        let req = Provide {
            kind,
            drive: "USB".into(),
            tracks: TrackSelector::default(),
            from: from.map(String::from),
            replace,
        };
        self.plugins.provide(&req, Some(&self.store), &Ctx::silent())
    }
}

fn tags(score: f64, title: &str) -> Value {
    json!({ "score": score, "source": "Tags", "label": title, "metadata": { "title": title } })
}

/// `candidate`, for track `id`.
fn for_track(id: u32, mut candidate: Value) -> Value {
    candidate["track"] = id.into();
    candidate
}

/// Whether `t` lacks a tag a metadata provider finds.
fn lacks_a_tag(t: &locallibrary::Track) -> bool {
    [&t.title, &t.artist, &t.album, &t.genre, &t.label, &t.isrc].iter().any(|s| s.trim().is_empty()) || t.year == 0
}

fn titles(p: &Provided) -> Vec<(u32, Vec<String>, Option<usize>)> {
    let found = |o: &locallibrary::plugin::Offer| o.candidates.iter().map(|c| c.label.clone()).collect();
    p.offers.iter().map(|o| (o.track, found(o), o.picked)).collect()
}

/// Every provider of a kind is asked (in the order the plugins are), what they find merged for
/// each track, best first, the same twice once, and the one alone scoring the setting or more
/// picked.
#[test]
fn every_provider_is_asked_and_the_best_found_first() {
    let f = Fixture::providers(&["echo", "second"]);
    let ids: Vec<u32> = common::library(&f.drive).tracks.iter().map(|t| t.id).collect();
    f.answer("echo", json!({ "each": [tags(0.5, "maybe")], "candidates": [for_track(ids[0], tags(0.95, "sure"))] }));
    f.answer("second", json!({ "each": [tags(0.6, "likely"), tags(0.5, "maybe")], "warnings": ["one missing"] }));
    let p = f.provide(ProviderKind::Metadata, None, true).unwrap();
    assert_eq!(p.asked, 2);
    let first = vec!["sure".to_string(), "likely".into(), "maybe".into()];
    let second = vec!["likely".to_string(), "maybe".into()];
    assert_eq!(titles(&p), [(ids[0], first, Some(0)), (ids[1], second, None)]);
    assert_eq!(p.warnings, ["Found: one missing"]);
    let c = &p.offers[0].candidates[0];
    assert_eq!((c.provider.as_str(), c.source.as_str(), c.score), ("echo.found", "Tags", 0.95));
    // Two sure ones: the user picks.
    f.answer("second", json!({ "each": [tags(0.9, "likely")] }));
    let p = f.provide(ProviderKind::Metadata, None, true).unwrap();
    assert_eq!(p.offers[0].picked, None);
    // Only one plugin's, or one provider.
    let p = f.provide(ProviderKind::Metadata, Some("second.found"), true).unwrap();
    assert!(p.offers.iter().all(|o| o.candidates.iter().all(|c| c.provider == "second.found")));
    let e = f.provide(ProviderKind::Metadata, Some("third"), true).unwrap_err();
    assert_eq!(e.to_string(), "third provides no metadata: echo.found, second.found do");
    let e = Fixture::echo().provide(ProviderKind::Artwork, None, false).unwrap_err();
    assert_eq!(e.to_string(), "no plugin provides artwork");
}

/// Without `replace`, only tracks without cover art, or without a tag, are asked about; tags are
/// tidied, and a candidate left with none is no candidate.
#[test]
fn tracks_lacking_what_a_kind_finds_are_asked_about() {
    let f = Fixture::providers(&["echo"]);
    let tracks = common::library(&f.drive).tracks;
    f.answer(
        "echo",
        json!({ "each": [{ "score": 1, "source": "S", "label": "L", "metadata": {
        "title": "  Spaced  ", "album": "", "isrc": "gb-duw-00-00059", "year": 0 } }, tags(0.2, " ")] }),
    );
    let p = f.provide(ProviderKind::Metadata, Some("echo.found"), false).unwrap();
    let lacking = tracks.iter().filter(|t| lacks_a_tag(t)).count();
    assert_eq!(p.asked, lacking);
    let tidy =
        TrackMetadata { title: Some("Spaced".into()), isrc: Some("GBDUW0000059".into()), ..TrackMetadata::default() };
    assert!(p.offers.iter().all(|o| o.candidates.len() == 1 && o.candidates[0].metadata.as_ref() == Some(&tidy)));
    let with_art = tracks.iter().filter(|t| t.artwork_path.is_some()).count();
    let p = f.provide(ProviderKind::Artwork, None, false).unwrap();
    assert_eq!(p.asked, tracks.len() - with_art);
}

/// A provider that fails is a warning, the others' offered; cancelled, it cancels. One that answers
/// what it wasn't asked fails.
#[test]
fn a_provider_failing_is_a_warning_and_cancelling_cancels() {
    let f = Fixture::providers(&["echo", "second"]);
    let id = common::library(&f.drive).tracks[0].id;
    f.answer("second", json!({ "each": [tags(0.5, "kept")] }));
    let warned = |answer: Value| {
        f.answer("echo", answer);
        let p = f.provide(ProviderKind::Metadata, Some("echo"), true).unwrap();
        assert!(p.offers.is_empty(), "nothing of a provider that failed is offered");
        p.warnings.into_iter().find(|w| w.starts_with("Found: ")).unwrap()
    };
    assert_eq!(warned(json!({ "fail": "not_found" })), "Found: not found: it failed (the Echo plugin)");
    assert_eq!(
        warned(json!({ "candidates": [for_track(0, tags(1.0, "x"))] })),
        "Found: the Echo plugin failed: its candidate 0 is for track #0, not asked about"
    );
    assert_eq!(
        warned(json!({ "candidates": [for_track(id, tags(1.5, "x"))] })),
        "Found: the Echo plugin failed: its candidate 0 scores 1.5, not 0 to 1"
    );
    let image = json!({ "score": 1, "source": "S", "label": "L", "artwork": { "blob": "a.jpg" } });
    assert_eq!(
        warned(json!({ "each": [image] })),
        "Found: the Echo plugin failed: its candidate 0 isn't metadata's: an image for artwork, tags for metadata"
    );
    f.answer("echo", json!({ "fail": "not_found" }));
    let p = f.provide(ProviderKind::Metadata, None, true).unwrap();
    assert!(p.offers.iter().all(|o| o.candidates.iter().any(|c| c.label == "kept")), "the others' are offered");
    f.answer("echo", json!({ "fail": "cancelled" }));
    assert!(matches!(f.provide(ProviderKind::Metadata, None, true), Err(Error::Cancelled)));
}

/// An artwork provider's images are files in its scratch folder, each read once however many
/// tracks it's for, and only once it's an image; held once (by id) for all of them, and picked,
/// the host sets them.
#[test]
fn found_cover_art_is_set_by_the_host() {
    let f = Fixture::providers(&["echo"]);
    let cover = jpeg(64);
    let image = json!({ "score": 1, "source": "Covers", "label": "front", "artwork": { "blob": "a.jpg" } });
    f.answer("echo", json!({ "blobs": { "a.jpg": cover }, "each": [image] }));
    let p = f.provide(ProviderKind::Artwork, None, true).unwrap();
    assert!(p.offers.iter().all(|o| o.picked == Some(0)));
    let images: Vec<(&String, &String)> = p.images.iter().collect();
    let [(id, held)] = images[..] else { panic!("{:?}", p.images.keys()) };
    assert_eq!(*held, BASE64_STANDARD.encode(&cover));
    assert_eq!(p.offers.len(), 2);
    assert!(p.offers.iter().all(|o| o.candidates[0].artwork.as_ref() == Some(id)), "each track's is that one");
    let picks: Vec<_> = p.offers.iter().map(|o| o.chosen(&json!(1)).unwrap()).collect();
    let none = PlanProvided {
        kind: ProviderKind::Artwork,
        drive: "USB".into(),
        picks: picks.clone(),
        images: Default::default(),
        replace: true,
    };
    let e = plan_provided(&none, &Ctx::silent()).unwrap_err();
    assert_eq!(e.to_string(), format!("the image {id} picked for #{} isn't given", picks[0].track));
    let req = PlanProvided { images: p.images.clone(), ..none };
    let plan = plan_provided(&req, &Ctx::silent()).unwrap();
    assert_eq!(plan.summary[0], "set cover art on 2 track(s) from Covers");
    plan.apply(None, &Ctx::silent()).unwrap();
    assert!(common::library(&f.drive).tracks.iter().all(|t| t.artwork_path.is_some()));
    // Kept without `replace`, now that they have some.
    let plan = plan_provided(&PlanProvided { replace: false, ..req }, &Ctx::silent()).unwrap();
    assert!(plan.noop);
    let not = json!({ "score": 1, "source": "Covers", "label": "front", "artwork": { "blob": "not.jpg" } });
    f.answer("echo", json!({ "blobs": { "not.jpg": [1, 2, 3] }, "each": [not] }));
    let p = f.provide(ProviderKind::Artwork, None, true).unwrap();
    assert!(p.warnings[0].starts_with("Covers: the Echo plugin failed: its candidate 0's image: "), "{:?}", p.warnings);
    // The same image in two files is held once, and offered once.
    let named = |blob: &str| json!({ "score": 1, "source": "Covers", "label": blob, "artwork": { "blob": blob } });
    f.answer("echo", json!({ "blobs": { "a.jpg": cover, "b.jpg": cover }, "each": [named("a.jpg"), named("b.jpg")] }));
    let p = f.provide(ProviderKind::Artwork, None, true).unwrap();
    assert_eq!(p.images.len(), 1);
    assert!(p.offers.iter().all(|o| o.candidates.len() == 1), "{:?}", titles(&p));
}

/// Picked tags fill in what tracks lack, or with `replace` go over what they have; a pick
/// changing nothing is left out, and one of the wrong kind refused.
#[test]
fn found_tags_are_written_by_the_host() {
    let f = Fixture::providers(&["echo"]);
    let t = common::library(&f.drive).tracks[0].clone();
    // By id: an edit can change the order the library lists tracks in.
    let now = || common::library(&f.drive).tracks.into_iter().find(|n| n.id == t.id).unwrap();
    let found = |m: Value| Found {
        provider: "echo.found".into(),
        source: "Tags".into(),
        label: "L".into(),
        score: 1.0,
        artwork: None,
        metadata: Some(serde_json::from_value(m).unwrap()),
    };
    let plan = |m: Value, replace: bool| {
        let picks = vec![locallibrary::plugin::Pick { track: t.id, candidate: found(m) }];
        plan_provided(
            &PlanProvided {
                kind: ProviderKind::Metadata,
                drive: "USB".into(),
                picks,
                images: Default::default(),
                replace,
            },
            &Ctx::silent(),
        )
    };
    assert!(!t.title.is_empty() && !t.label.is_empty() && t.year == 0, "{t:?}");
    let kept = plan(json!({ "title": "Other", "label": "Label", "year": 1999 }), false).unwrap();
    assert_eq!(kept.summary[0], "fill in the details of 1 track(s) from Tags");
    kept.apply(None, &Ctx::silent()).unwrap();
    let kept = now();
    assert_eq!((kept.title.as_str(), kept.label.as_str(), kept.year), (t.title.as_str(), t.label.as_str(), 1999));
    let over = plan(json!({ "title": "Other", "year": 2001 }), true).unwrap();
    over.apply(None, &Ctx::silent()).unwrap();
    let over = now();
    assert_eq!((over.title.as_str(), over.year), ("Other", 2001));
    assert!(plan(json!({ "title": "Other" }), true).unwrap().noop, "nothing changes");
    let wrong = PlanProvided {
        kind: ProviderKind::Artwork,
        drive: "USB".into(),
        picks: vec![locallibrary::plugin::Pick { track: t.id, candidate: found(json!({})) }],
        images: Default::default(),
        replace: true,
    };
    let e = plan_provided(&wrong, &Ctx::silent()).unwrap_err();
    assert_eq!(e.to_string(), format!("the candidate for #{} isn't artwork's", t.id));
}

/// A track's question names it, its candidates by place from 1, and none; answered, the candidate
/// chosen.
#[test]
fn a_tracks_question_is_its_candidates_and_none() {
    let f = Fixture::providers(&["echo"]);
    f.answer("echo", json!({ "each": [tags(0.4, "b"), tags(0.5, "a")] }));
    let p = f.provide(ProviderKind::Metadata, Some("echo.found"), true).unwrap();
    let o = &p.offers[0];
    let c = o.choice();
    assert_eq!((c.title.as_str(), c.param.clone()), (o.name.as_str(), o.track.to_string()));
    let items: Vec<(&str, Option<&str>, Value)> =
        c.items.iter().map(|i| (i.label.as_str(), i.detail.as_deref(), i.value.clone())).collect();
    assert_eq!(
        items,
        [
            ("a", Some("Tags, 50%: a"), json!(1)),
            ("b", Some("Tags, 40%: b"), json!(2)),
            ("none of these", None, Value::Null)
        ]
    );
    assert_eq!(o.chosen(&json!(2)).map(|p| p.candidate.label), Some("b".into()));
    for none in [Value::Null, json!(0), json!(3), json!("1")] {
        assert!(o.chosen(&none).is_none(), "{none}");
    }
}

#[test]
fn the_score_picked_for_the_user_is_a_setting() {
    let f = Fixture::providers(&["echo"]);
    assert_eq!(auto_pick(Some(&f.store)), AUTO_PICK);
    assert_eq!(set_auto_pick(&f.store, Some(0.4)).unwrap(), 0.4);
    f.answer("echo", json!({ "each": [tags(0.45, "a"), tags(0.2, "b")] }));
    let p = f.provide(ProviderKind::Metadata, Some("echo.found"), true).unwrap();
    assert!(p.offers.iter().all(|o| o.picked == Some(0)));
    assert_eq!(set_auto_pick(&f.store, None).unwrap(), AUTO_PICK);
    for wrong in [-0.1, f64::NAN] {
        assert!(set_auto_pick(&f.store, Some(wrong)).unwrap_err().to_string().starts_with("a score is 0 to 1"));
    }
    assert_eq!(auto_pick(None), AUTO_PICK);
}

/// A candidate as the test plugin's script has it: a score, and a title or a year.
#[hegel::test_helper]
fn drawn_candidate(tc: &TestCase) -> Value {
    let score = tc.draw(gs::integers::<u8>().max_value(100)) as f64 / 100.0;
    let metadata = match tc.draw(gs::booleans()) {
        true => json!({ "title": tc.draw(gs::sampled_from(vec!["One", "Two", "Three"])) }),
        false => json!({ "year": tc.draw(gs::integers::<u16>().min_value(1950).max_value(2030)) }),
    };
    json!({ "score": score, "source": "S", "label": "L", "metadata": metadata })
}

/// Whatever two providers find for the tracks, with any setting: each track's candidates are
/// theirs, best first, each once, and picked exactly when one alone scores the setting or more;
/// planned, the picks write each tag a track lacks (or, with `replace`, any it doesn't have).
#[hegel::test(test_cases = 20)]
fn any_candidates_are_offered_best_first_and_planned_as_picked(tc: TestCase) {
    let f = Fixture::providers(&["echo", "second"]);
    let threshold = tc.draw(gs::integers::<u8>().max_value(100)) as f64 / 100.0;
    set_auto_pick(&f.store, Some(threshold)).unwrap();
    let mut all = Vec::new();
    for plugin in ["echo", "second"] {
        let n = tc.draw(gs::integers::<usize>().max_value(3));
        let each: Vec<Value> = (0..n).map(|_| drawn_candidate(&tc)).collect();
        f.answer(plugin, json!({ "each": each }));
        all.extend(each);
    }
    let replace = tc.draw(gs::booleans());
    let before = common::library(&f.drive).tracks;
    let p = f.provide(ProviderKind::Metadata, None, replace).unwrap();
    // Each found for every track asked about, the same tags once.
    let mut distinct: Vec<&Value> = Vec::new();
    for c in &all {
        if !distinct.contains(&&c["metadata"]) {
            distinct.push(&c["metadata"]);
        }
    }
    let asked = before.iter().filter(|t| replace || lacks_a_tag(t)).count();
    assert_eq!((p.asked, p.offers.len()), (asked, if distinct.is_empty() { 0 } else { asked }));
    for o in &p.offers {
        assert_eq!(o.candidates.len(), distinct.len());
        let scores: Vec<f64> = o.candidates.iter().map(|c| c.score).collect();
        assert!(scores.windows(2).all(|w| w[0] >= w[1]), "{scores:?}");
        let sure = scores.iter().filter(|s| **s >= threshold).count();
        assert_eq!(o.picked, (sure == 1).then_some(0));
    }
    let picks: Vec<_> = (p.offers.iter())
        .filter_map(|o| o.chosen(&json!(tc.draw(gs::integers::<usize>().max_value(o.candidates.len())))))
        .collect();
    let req = PlanProvided {
        kind: ProviderKind::Metadata,
        drive: "USB".into(),
        picks: picks.clone(),
        images: Default::default(),
        replace,
    };
    let plan = plan_provided(&req, &Ctx::silent()).unwrap();
    if !plan.noop {
        plan.apply(None, &Ctx::silent()).unwrap();
    }
    let after = common::library(&f.drive).tracks;
    for t in &before {
        let now = after.iter().find(|a| a.id == t.id).unwrap();
        let picked = picks.iter().find(|p| p.track == t.id).and_then(|p| p.candidate.metadata.clone());
        let m = picked.unwrap_or_default();
        let title = m.title.filter(|_| replace || t.title.is_empty()).unwrap_or_else(|| t.title.clone());
        let year = m.year.filter(|_| replace || t.year == 0).unwrap_or(t.year);
        assert_eq!((now.title.as_str(), now.year), (title.as_str(), year), "#{}", t.id);
    }
}

/// Whatever covers a provider finds, in files of its scratch folder some tracks share and some
/// that aren't images, asked of any provider by any name: a name that's none of the providers' is
/// refused naming them; otherwise each track is offered each image once, each held once for all
/// the tracks, a provider whose file isn't an image a warning, and the covers picked (by answering
/// each track's question) are set on exactly those tracks, given their images.
#[hegel::test(test_cases = 15)]
fn any_covers_found_are_offered_once_and_set_as_picked(tc: TestCase) {
    let f = Fixture::providers(&["echo"]);
    let files = [jpeg(32), jpeg(48), vec![0u8; 4]];
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let named: Vec<usize> = (0..n).map(|_| tc.draw(gs::integers::<usize>().max_value(2))).collect();
    let each: Vec<Value> = named
        .iter()
        .map(|&k| json!({ "score": 0.5, "source": "Covers", "label": format!("cover {k}"), "artwork": { "blob": format!("{k}.jpg") } }))
        .collect();
    let blobs: serde_json::Map<String, Value> = (0..3).map(|k| (format!("{k}.jpg"), json!(files[k]))).collect();
    f.answer("echo", json!({ "blobs": blobs, "each": each }));
    let from = tc.draw(gs::sampled_from(vec!["echo", "echo.covers", "echo.found", "other"]));
    let provided = f.provide(ProviderKind::Artwork, Some(from), true);
    if from == "echo.found" || from == "other" {
        assert_eq!(provided.unwrap_err().to_string(), format!("{from} provides no artwork: echo.covers do"));
        return;
    }
    let p = provided.unwrap();
    if named.contains(&2) {
        assert!(p.offers.is_empty() && p.warnings.len() == 1, "{:?}", p.warnings);
        return;
    }
    let distinct: Vec<usize> =
        named.iter().enumerate().filter(|(i, k)| !named[..*i].contains(k)).map(|(_, k)| *k).collect();
    let held: Vec<String> = distinct.iter().map(|&k| BASE64_STANDARD.encode(&files[k])).collect();
    assert_eq!(p.images.values().cloned().collect::<Vec<_>>(), held, "each once");
    let mut picks = Vec::new();
    for o in &p.offers {
        let labels: Vec<String> = o.candidates.iter().map(|c| c.label.clone()).collect();
        assert_eq!(labels, distinct.iter().map(|k| format!("cover {k}")).collect::<Vec<_>>());
        let ids: Vec<&String> = o.candidates.iter().filter_map(|c| c.artwork.as_ref()).collect();
        assert_eq!(ids, p.images.keys().collect::<Vec<_>>());
        let c = o.choice();
        let item = tc.draw(gs::integers::<usize>().max_value(c.items.len() - 1));
        picks.extend(o.chosen(&c.items[item].value));
    }
    let before = common::library(&f.drive).tracks;
    let images = match tc.draw(gs::booleans()) {
        true => p.images.clone(),
        false => Default::default(),
    };
    let given = !images.is_empty();
    let req =
        PlanProvided { kind: ProviderKind::Artwork, drive: "USB".into(), picks: picks.clone(), images, replace: true };
    let plan = plan_provided(&req, &Ctx::silent());
    if !given && !picks.is_empty() {
        assert!(plan.unwrap_err().to_string().contains("isn't given"));
        return;
    }
    let plan = plan.unwrap();
    assert_eq!(plan.noop, picks.is_empty());
    if !plan.noop {
        plan.apply(None, &Ctx::silent()).unwrap();
    }
    for t in common::library(&f.drive).tracks {
        let was = before.iter().find(|b| b.id == t.id).unwrap();
        let picked = picks.iter().any(|p| p.track == t.id);
        assert_eq!(t.artwork_path.is_some(), picked || was.artwork_path.is_some(), "#{}", t.id);
    }
}

/// The candidates a provider answers for a track past the most a call keeps for one, past the
/// most it keeps in all, and those of an image past the most their images may come to in base64,
/// are left out, saying so once; what's held is within each.
#[test]
fn a_provider_past_what_a_call_holds_is_cut_off() {
    let five = (0..5).map(|i| tags(0.5, &format!("t{i}"))).collect::<Vec<_>>();
    let f = Fixture::bounded(&["echo"], Options { track_candidates: 2, ..Options::default() });
    f.answer("echo", json!({ "each": five }));
    let p = f.provide(ProviderKind::Metadata, Some("echo.found"), true).unwrap();
    assert!(p.offers.iter().all(|o| o.candidates.len() == 2), "{:?}", titles(&p));
    assert_eq!(p.warnings, ["Found: more than 2 candidates for a track, the rest left out"]);

    let f = Fixture::bounded(&["echo"], Options { candidates: 3, ..Options::default() });
    f.answer("echo", json!({ "each": five }));
    let p = f.provide(ProviderKind::Metadata, Some("echo.found"), true).unwrap();
    assert_eq!(p.offers.iter().map(|o| o.candidates.len()).sum::<usize>(), 3, "{:?}", titles(&p));
    assert_eq!(p.warnings, ["Found: more than 3 candidates, the rest left out"]);

    // The scratch folder holds both files, but not both images in base64: the first, shared by
    // the tracks, is held once.
    let (a, b) = (jpeg(64), jpeg(48));
    let most = (a.len() + b.len()) as u64;
    let f = Fixture::bounded(&["echo"], Options { max_blobs: most, ..Options::default() });
    let cover = |k: &str| json!({ "score": 0.5, "source": "Covers", "label": k, "artwork": { "blob": k } });
    f.answer("echo", json!({ "blobs": { "a.jpg": a, "b.jpg": b }, "each": [cover("a.jpg"), cover("b.jpg")] }));
    let p = f.provide(ProviderKind::Artwork, None, true).unwrap();
    assert_eq!(p.images.values().cloned().collect::<Vec<_>>(), [BASE64_STANDARD.encode(&a)]);
    assert!(p.images.values().map(|i| i.len() as u64).sum::<u64>() <= most);
    assert_eq!(titles(&p).iter().map(|(_, found, _)| found.clone()).collect::<Vec<_>>(), [["a.jpg"], ["a.jpg"]]);
    let [w] = &p.warnings[..] else { panic!("{:?}", p.warnings) };
    assert!(w.starts_with("Covers: images coming to more than ") && w.ends_with(", the rest left out"), "{w}");
}

/// Whatever covers a provider answers and however little a call may hold: what's kept is what the
/// provider answered, in turn, until a track has as many candidates as a call keeps for one, or all
/// of them as many as it keeps, or an image would take their images in base64 past what they may
/// come to; each image held once, only the candidates', and each limit passed said once.
#[hegel::test(test_cases = 15)]
fn any_answer_is_held_within_what_a_call_holds(tc: TestCase) {
    let files = [jpeg(32), jpeg(48)];
    let raw = (files[0].len() + files[1].len()) as u64;
    let options = Options {
        track_candidates: tc.draw(gs::integers::<usize>().min_value(1).max_value(4)),
        candidates: tc.draw(gs::integers::<usize>().min_value(1).max_value(8)),
        // The scratch folder holds both files.
        max_blobs: tc.draw(gs::integers::<u64>().min_value(raw).max_value(2 * raw)),
        ..Options::default()
    };
    let f = Fixture::bounded(&["echo"], options.clone());
    let n = tc.draw(gs::integers::<usize>().max_value(5));
    let named: Vec<usize> = (0..n).map(|_| tc.draw(gs::integers::<usize>().max_value(1))).collect();
    let each: Vec<Value> = (named.iter())
        .map(|&k| json!({ "score": 0.5, "source": "Covers", "label": format!("cover {k}"), "artwork": { "blob": format!("{k}.jpg") } }))
        .collect();
    let blobs: serde_json::Map<String, Value> = (0..2).map(|k| (format!("{k}.jpg"), json!(files[k]))).collect();
    f.answer("echo", json!({ "blobs": blobs, "each": each }));
    let p = f.provide(ProviderKind::Artwork, None, true).unwrap();
    // The test plugin answers each cover for each track in turn.
    let tracks = common::library(&f.drive).tracks;
    let (mut kept, mut held, mut past) = (vec![Vec::new(); tracks.len()], Vec::new(), Vec::new());
    let (mut all, mut size) = (0, 0);
    for &k in &named {
        for t in &mut kept {
            let more = BASE64_STANDARD.encode(&files[k]).len();
            let why = match () {
                _ if all >= options.candidates => format!("more than {} candidates", options.candidates),
                _ if t.len() >= options.track_candidates => {
                    format!("more than {} candidates for a track", options.track_candidates)
                }
                _ if !held.contains(&k) && (size + more) as u64 > options.max_blobs => {
                    format!("images coming to more than {}", locallibrary::op::bytes(options.max_blobs))
                }
                _ => {
                    if !held.contains(&k) {
                        size += more;
                        held.push(k);
                    }
                    t.push(k);
                    all += 1;
                    continue;
                }
            };
            let why = format!("Covers: {why}, the rest left out");
            if !past.contains(&why) {
                past.push(why);
            }
        }
    }
    let images: Vec<String> = held.iter().map(|&k| BASE64_STANDARD.encode(&files[k])).collect();
    assert_eq!(p.images.values().cloned().collect::<Vec<_>>(), images);
    let offered: Vec<(u32, Vec<String>)> = (tracks.iter().zip(&kept))
        .filter(|(_, k)| !k.is_empty())
        .map(|(t, k)| {
            let distinct = k.iter().enumerate().filter(|(i, c)| !k[..*i].contains(c));
            (t.id, distinct.map(|(_, c)| format!("cover {c}")).collect())
        })
        .collect();
    assert_eq!(titles(&p).into_iter().map(|(t, found, _)| (t, found)).collect::<Vec<_>>(), offered);
    let ids: FxHashSet<&String> =
        p.offers.iter().flat_map(|o| o.candidates.iter().filter_map(|c| c.artwork.as_ref())).collect();
    assert_eq!(ids, p.images.keys().collect(), "only the candidates'");
    assert_eq!(p.warnings, past);
}
