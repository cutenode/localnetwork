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

/// Tags that would change nothing on a track (what it has, or a tag it has without `replace`)
/// aren't offered, so they're never picked; a track found nothing else isn't asked about.
#[test]
fn tags_changing_nothing_are_not_offered() {
    let f = Fixture::providers(&["echo"]);
    let tracks = common::library(&f.drive).tracks;
    assert!(tracks.iter().all(|t| !t.title.is_empty() && t.year == 0), "{tracks:?}");
    let year = json!({ "score": 0.95, "source": "S", "label": "2001", "metadata": { "year": 2001 } });
    f.answer("echo", json!({ "each": [tags(0.95, "Another"), year] }));
    let p = f.provide(ProviderKind::Metadata, Some("echo"), false).unwrap();
    let ids: Vec<u32> = tracks.iter().map(|t| t.id).collect();
    let offered: Vec<(u32, Vec<String>, Option<usize>)> =
        ids.iter().map(|id| (*id, vec!["2001".to_string()], Some(0))).collect();
    assert_eq!(titles(&p), offered, "only the year, picked");
    f.answer("echo", json!({ "each": [tags(0.95, "Another")] }));
    let p = f.provide(ProviderKind::Metadata, Some("echo"), false).unwrap();
    assert_eq!((p.asked, p.offers.len()), (tracks.len(), 0));
    // With `replace`, a title the track has already changes nothing either.
    let theirs: Vec<Value> = tracks.iter().map(|t| for_track(t.id, tags(0.95, &t.title))).collect();
    f.answer("echo", json!({ "candidates": theirs }));
    assert!(f.provide(ProviderKind::Metadata, Some("echo"), true).unwrap().offers.is_empty());
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
/// theirs that would change it, best first, each once (a track with none offered nothing), and
/// picked exactly when one alone scores the setting or more;
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
    // Each track asked about is offered the tags found that would change it, the same once; one
    // with none isn't offered anything.
    let changes = |t: &locallibrary::Track, c: &Value| match c["metadata"]["title"].as_str() {
        Some(title) => title != t.title && (replace || t.title.trim().is_empty()),
        None => {
            let year = c["metadata"]["year"].as_u64().unwrap() as u16;
            year != t.year && (replace || t.year == 0)
        }
    };
    let asked: Vec<&locallibrary::Track> = before.iter().filter(|t| replace || lacks_a_tag(t)).collect();
    assert_eq!(p.asked, asked.len());
    let mut offers = p.offers.iter();
    for t in asked {
        let mut distinct: Vec<&Value> = Vec::new();
        for c in all.iter().filter(|c| changes(t, c)) {
            if !distinct.contains(&&c["metadata"]) {
                distinct.push(&c["metadata"]);
            }
        }
        if distinct.is_empty() {
            continue;
        }
        let o = offers.next().unwrap_or_else(|| panic!("#{} is offered nothing", t.id));
        assert_eq!((o.track, o.candidates.len()), (t.id, distinct.len()));
        let scores: Vec<f64> = o.candidates.iter().map(|c| c.score).collect();
        assert!(scores.windows(2).all(|w| w[0] >= w[1]), "{scores:?}");
        let sure = scores.iter().filter(|s| **s >= threshold).count();
        assert_eq!(o.picked, (sure == 1).then_some(0));
    }
    assert!(offers.next().is_none(), "offered more: {:?}", titles(&p));
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

/// For any way a provider misbehaves (a candidate for a track it wasn't asked about, scoring
/// past 0 to 1, of the other kind, or an image that isn't in its scratch folder), what it found is
/// a warning saying so, nothing of it offered; one that fails is a warning too, one cancelled
/// cancels, and tags that are all blank, or that the track has, are no candidate. Its parts of a
/// view, sent as any request's may be, change nothing.
#[hegel::test(test_cases = 1)]
fn prop_a_provider_misbehaving_is_a_warning(tc: TestCase) {
    use ProviderKind::{Artwork, Metadata};
    let either = [0, 1, 2, 3, 5].into_iter().flat_map(|case| [(case, Metadata), (case, Artwork)]);
    let one = [(4, Artwork), (6, Metadata), (7, Metadata)];
    for (case, kind) in either.chain(one) {
        prop_a_provider_misbehaving_is_a_warning_case(&tc, case, kind);
    }
}

/// [`prop_a_provider_misbehaving_is_a_warning`] for one `case`.
#[hegel::test_helper]
fn prop_a_provider_misbehaving_is_a_warning_case(tc: &TestCase, case: u8, kind: ProviderKind) {
    let f = Fixture::new(|dir| vec![spec(dir, &providing("echo"), &["--partials"])], Options::default());
    let t = common::library(&f.drive).tracks[0].clone();
    let id = t.id;
    let tag: String = tc.draw(gs::from_regex("[a-z]{1,8}").fullmatch(true));
    let image = json!({ "score": 1, "source": "S", "label": "L", "artwork": { "blob": "a.jpg" } });
    let (title, own, other) = match kind {
        ProviderKind::Metadata => ("Found", tags(1.0, &tag), image),
        ProviderKind::Artwork => ("Covers", image, tags(1.0, &tag)),
    };
    let scoring = |mut c: Value, score: f64| {
        c["score"] = score.into();
        c
    };
    let failed = |why: &str| Some(format!("{title}: the Echo plugin failed: {why}"));
    let (answer, warned) = match case {
        0 => (
            json!({ "candidates": [for_track(9999, own)] }),
            failed("its candidate 0 is for track #9999, not asked about"),
        ),
        1 => (
            json!({ "candidates": [for_track(id, scoring(own, 2.5))] }),
            failed("its candidate 0 scores 2.5, not 0 to 1"),
        ),
        2 => {
            let why = format!("its candidate 0 isn't {}'s: an image for artwork, tags for metadata", kind.name());
            (json!({ "candidates": [for_track(id, other)] }), failed(&why))
        }
        3 => (json!({ "fail": "not_found" }), Some(format!("{title}: not found: it failed (the Echo plugin)"))),
        4 => (json!({ "candidates": [for_track(id, own)] }), failed("its candidate 0: its file \"a.jpg\" isn't there")),
        7 => (json!({ "candidates": [for_track(id, tags(1.0, "  "))] }), None),
        // Tags the track has: nothing to offer.
        6 => (json!({ "candidates": [for_track(id, tags(1.0, &t.title))] }), None),
        _ => (json!({ "fail": "cancelled" }), None),
    };
    let cancelled = answer["fail"] == "cancelled";
    f.answer("echo", answer);
    let provided = f.provide(kind, Some("echo"), true);
    if cancelled {
        assert!(matches!(provided, Err(Error::Cancelled)), "{provided:?}");
        return;
    }
    let p = provided.unwrap();
    assert!(p.offers.is_empty(), "{:?}", titles(&p));
    match warned {
        Some(warned) => assert!(p.warnings.len() == 1 && p.warnings[0].starts_with(&warned), "{:?}", p.warnings),
        None => assert_eq!(p.warnings, Vec::<String>::new()),
    }
}

/// For any drive that isn't known here or whose library can't be read, nothing is asked of the
/// providers; and for any drive no edit is planned for, a pick isn't planned: each fails, saying
/// why.
#[hegel::test(test_cases = 1)]
fn prop_providing_on_a_drive_it_cant_read_or_plan_for_fails(tc: TestCase) {
    use super::analysis::Unplannable;
    for how in Unplannable::ALL {
        prop_providing_on_a_drive_it_cant_read_or_plan_for_fails_case(&tc, how);
    }
}

/// [`prop_providing_on_a_drive_it_cant_read_or_plan_for_fails`] for one `how`.
#[hegel::test_helper]
fn prop_providing_on_a_drive_it_cant_read_or_plan_for_fails_case(tc: &TestCase, how: super::analysis::Unplannable) {
    use super::analysis::Unplannable;
    let f = Fixture::providers(&["echo"]);
    let t = common::library(&f.drive).tracks[0].clone();
    let (drive, code) = how.make(&f);
    let req = Provide {
        kind: ProviderKind::Metadata,
        drive: drive.into(),
        tracks: TrackSelector::default(),
        from: None,
        replace: true,
    };
    let provided = f.plugins.provide(&req, Some(&f.store), &Ctx::silent());
    match how {
        Unplannable::Unknown | Unplannable::Library => assert_eq!(provided.unwrap_err().code(), code),
        Unplannable::Journal | Unplannable::Archive => assert!(provided.is_ok(), "{provided:?}"),
    }
    let metadata = TrackMetadata {
        title: Some(format!("{} {}", t.title, tc.draw(gs::from_regex("[a-z]{1,6}").fullmatch(true)))),
        ..TrackMetadata::default()
    };
    let found = Found {
        provider: "echo.found".into(),
        source: "Tags".into(),
        label: "L".into(),
        score: 1.0,
        artwork: None,
        metadata: Some(metadata),
    };
    let picks = vec![locallibrary::plugin::Pick { track: t.id, candidate: found }];
    let plan = PlanProvided {
        kind: ProviderKind::Metadata,
        drive: drive.into(),
        picks,
        images: Default::default(),
        replace: true,
    };
    let e = plan_provided(&plan, &Ctx::silent()).unwrap_err();
    assert_eq!(e.code(), code, "{how:?}: {e}");
}

/// For any pick of a track the drive doesn't have, or of the other kind, nothing is planned,
/// saying why; a pick changing nothing (cover art on a track that has some, or tags it has,
/// without replacing) is left out.
#[hegel::test(test_cases = 1)]
fn prop_a_pick_that_isnt_one_is_refused_or_left_out(tc: TestCase) {
    for case in 0..=4 {
        prop_a_pick_that_isnt_one_is_refused_or_left_out_case(&tc, case);
    }
}

/// [`prop_a_pick_that_isnt_one_is_refused_or_left_out`] for one `case`.
#[hegel::test_helper]
fn prop_a_pick_that_isnt_one_is_refused_or_left_out_case(tc: &TestCase, case: u8) {
    let f = Fixture::providers(&["echo"]);
    let t = common::library(&f.drive).tracks[0].clone();
    let source: String = tc.draw(gs::from_regex("[A-Z]{1,8}").fullmatch(true));
    let images: indexmap::IndexMap<String, String> =
        [("cover".to_string(), BASE64_STANDARD.encode(jpeg(16)))].into_iter().collect();
    let found = |artwork: Option<&str>, metadata: Option<TrackMetadata>| Found {
        provider: "echo.found".into(),
        source: source.clone(),
        label: "L".into(),
        score: 1.0,
        artwork: artwork.map(String::from),
        metadata,
    };
    let same = TrackMetadata { title: Some(t.title.clone()), ..TrackMetadata::default() };
    let plan = |kind: ProviderKind, track: u32, candidate: Found, replace: bool| {
        let picks = vec![locallibrary::plugin::Pick { track, candidate }];
        let req = PlanProvided { kind, drive: "USB".into(), picks, images: images.clone(), replace };
        plan_provided(&req, &Ctx::silent())
    };
    match case {
        0 => {
            let e = plan(ProviderKind::Metadata, 9999, found(None, Some(same)), true).unwrap_err();
            assert_eq!(e.code(), "not_found", "{e}");
        }
        1 => {
            let e = plan(ProviderKind::Metadata, t.id, found(Some("cover"), None), true).unwrap_err();
            assert_eq!(e.to_string(), format!("the candidate for #{} isn't metadata's", t.id));
        }
        2 => {
            let e = plan(ProviderKind::Artwork, t.id, found(None, Some(same)), true).unwrap_err();
            assert_eq!(e.to_string(), format!("the candidate for #{} isn't artwork's", t.id));
        }
        3 => {
            for replace in [false, true] {
                assert!(plan(ProviderKind::Metadata, t.id, found(None, Some(same.clone())), replace).unwrap().noop);
            }
        }
        _ => {
            let set = plan(ProviderKind::Artwork, t.id, found(Some("cover"), None), true).unwrap();
            set.apply(None, &Ctx::silent()).unwrap();
            assert!(plan(ProviderKind::Artwork, t.id, found(Some("cover"), None), false).unwrap().noop, "it has some");
        }
    }
}

/// For any year a track has with all its text tags, nothing is left to find; with those tags and
/// no year, it's asked about without replacing.
#[hegel::test(test_cases = 1)]
fn prop_a_track_with_every_text_tag_lacks_only_a_year(tc: TestCase) {
    use locallibrary::api::{EditRequest, edit_plan};
    use locallibrary::edit::{Edit, EditOptions, TrackFields};
    let f = Fixture::providers(&["echo"]);
    let year = tc.draw(gs::integers::<u16>().min_value(1950).max_value(2025));
    let tagged = |track: u32, year: u16| Edit::EditTracks {
        tracks: TrackSelector { ids: vec![track], ..TrackSelector::default() },
        fields: TrackFields {
            title: Some("Title".into()),
            artist: Some("Artist".into()),
            album: Some("Album".into()),
            genre: Some("Genre".into()),
            label: Some("Label".into()),
            isrc: Some("GBDUW0000059".into()),
            year: Some(year),
            ..TrackFields::default()
        },
    };
    let ids: Vec<u32> = common::library(&f.drive).tracks.iter().map(|t| t.id).collect();
    let edits = vec![tagged(ids[0], 0), tagged(ids[1], year)];
    let req = EditRequest { drive: "USB".into(), edits, options: EditOptions::default() };
    edit_plan(&req, &Ctx::silent()).unwrap().apply(Some(&f.store), &Ctx::silent()).unwrap();
    let found = json!({ "score": 1, "source": "S", "label": "L", "metadata": { "year": year } });
    f.answer("echo", json!({ "each": [found] }));
    let p = f.provide(ProviderKind::Metadata, None, false).unwrap();
    assert_eq!((p.asked, p.offers.iter().map(|o| o.track).collect::<Vec<_>>()), (1, vec![ids[0]]));
}

/// For any providers asked (both kinds, two plugins, without replacing: only tracks lacking what
/// the kind finds), what they find is offered, each ISRC as one is written (capitals, without
/// hyphens) and an offer picks one only when one alone scores the setting; asking for a kind no
/// plugin provides, with tracks a selector can't select, or once cancelled, fails.
#[hegel::test(test_cases = 1)]
fn prop_providers_are_asked_about_what_tracks_lack(tc: TestCase) {
    for kind in [ProviderKind::Metadata, ProviderKind::Artwork] {
        for score in [0.2, 0.95] {
            prop_providers_are_asked_about_what_tracks_lack_case(&tc, kind, score);
        }
    }
}

/// [`prop_providers_are_asked_about_what_tracks_lack`] for one `kind`, its candidates scoring
/// `score`.
#[hegel::test_helper]
fn prop_providers_are_asked_about_what_tracks_lack_case(tc: &TestCase, kind: ProviderKind, score: f64) {
    let f = Fixture::providers(&["echo", "second"]);
    let tracks = common::library(&f.drive).tracks;
    let year: u16 = tc.draw(gs::integers::<u16>().min_value(1950).max_value(2025));
    for (plugin, side) in [("echo", 16), ("second", 24)] {
        let answer = match kind {
            ProviderKind::Metadata => {
                let tags = json!({ "isrc": "gb-duw-00-00059", "title": plugin, "year": year });
                json!({ "each": [{ "score": score, "source": plugin, "label": plugin, "metadata": tags }] })
            }
            ProviderKind::Artwork => {
                let c = json!({ "score": score, "source": plugin, "label": plugin, "artwork": { "blob": "a.jpg" } });
                json!({ "blobs": { "a.jpg": jpeg(side) }, "each": [c] })
            }
        };
        f.answer(plugin, answer);
    }
    let lacking = tracks
        .iter()
        .filter(|t| match kind {
            ProviderKind::Artwork => t.artwork_path.is_none(),
            ProviderKind::Metadata => lacks_a_tag(t),
        })
        .count();
    let p = f.provide(kind, None, false).unwrap();
    assert_eq!(p.asked, lacking);
    let first = TrackSelector { ids: vec![tracks[0].id], ..TrackSelector::default() };
    let req = Provide { kind, drive: "USB".into(), tracks: first, from: None, replace: true };
    assert_eq!(f.plugins.provide(&req, Some(&f.store), &Ctx::silent()).unwrap().asked, 1);
    assert!(lacking > 0);
    assert_eq!(p.offers.len(), lacking);
    for o in &p.offers {
        assert_eq!(o.candidates.len(), 2);
        assert_eq!(o.picked, None, "two score the same");
        assert!(o.pick().is_none() && o.chosen(&json!(3)).is_none());
        let said: Vec<String> = o.choice().items.iter().filter_map(|i| i.detail.clone()).collect();
        assert_eq!(said.len(), 2);
        if kind == ProviderKind::Metadata {
            assert!(said.iter().all(|d| d.ends_with(&format!(", {year}, GBDUW0000059"))), "{said:?}");
        }
        if kind == ProviderKind::Metadata {
            assert_eq!(o.candidates[0].metadata.as_ref().unwrap().isrc.as_deref(), Some("GBDUW0000059"));
        }
    }
    let one = f.provide(kind, Some("echo"), true).unwrap();
    for o in &one.offers {
        assert_eq!(o.picked, (score >= AUTO_PICK).then_some(0));
        assert_eq!(o.pick().map(|p| p.candidate.label), o.picked.map(|_| "echo".to_string()));
    }
    let unselectable = TrackSelector { filter: Some("((".into()), ..TrackSelector::default() };
    let req = Provide { kind, drive: "USB".into(), tracks: unselectable, from: None, replace: true };
    assert!(f.plugins.provide(&req, Some(&f.store), &Ctx::silent()).is_err());
    let cancel = locallibrary::CancelToken::new();
    cancel.cancel();
    let req = Provide { tracks: TrackSelector::default(), ..req };
    let cancelled = f.plugins.provide(&req, Some(&f.store), &Ctx::new(&locallibrary::progress::NoProgress, cancel));
    assert!(matches!(cancelled, Err(Error::Cancelled)), "{cancelled:?}");
    // Last: another fixture's drives are the thread's from then on.
    let e = Fixture::echo().provide(ProviderKind::Artwork, None, false).unwrap_err();
    assert_eq!(e.to_string(), "no plugin provides artwork");
}

/// The score picked for the user, and a plugin's settings, can't be kept where the store can't
/// be written (on Unix): the score stays as it was.
#[cfg(unix)]
#[hegel::test(test_cases = 5)]
fn prop_the_score_picked_cant_be_set_where_it_cant_be_kept(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::providers(&["echo"]);
    let before = tc.draw(gs::sampled_from(vec![0.3, 0.6]));
    set_auto_pick(&f.store, Some(before)).unwrap();
    let wrong = tc.draw(gs::sampled_from(vec![-0.1, f64::NAN, f64::NEG_INFINITY]));
    assert!(set_auto_pick(&f.store, Some(wrong)).unwrap_err().to_string().starts_with("a score is 0 to 1"));
    f.answer("echo", json!({}));
    let (plugins, own) = (f.store.root().join("plugins"), f.store.root().join("plugins/echo"));
    for dir in [&plugins, &own] {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    }
    let set = set_auto_pick(&f.store, Some(0.9));
    let answered = f.plugins.set_setting("echo", &f.store, "answer", Some(json!("{}")));
    for dir in [&plugins, &own] {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    assert_eq!((set.unwrap_err().code(), answered.unwrap_err().code()), ("io", "io"));
    assert_eq!(auto_pick(Some(&f.store)), before);
}
