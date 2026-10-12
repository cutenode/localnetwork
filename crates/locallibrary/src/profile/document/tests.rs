use super::*;

#[test]
fn counts_round_down_to_round_numbers() {
    let r = Round(Precision::Rounded);
    assert_eq!(r.count(7), Count::Exact { count: 7 });
    assert_eq!(r.count(47), Count::AtLeast { at_least: 45 });
    assert_eq!(r.count(3886), Count::AtLeast { at_least: 3500 });
    assert_eq!(Round(Precision::Exact).count(47), Count::Exact { count: 47 });
    assert_eq!(serde_json::to_value(r.count(47)).unwrap(), serde_json::json!({ "atLeast": 45 }));
    assert_eq!(r.share(0.68), 0.7);
    assert_eq!(r.share(0.66), 0.65);
}

#[test]
fn shares_of_a_whole_still_add_up_rounded() {
    let r = Round(Precision::Rounded);
    assert_eq!(r.shares(&[0.84, 0.08, 0.08]), [0.85, 0.1, 0.05]);
    assert_eq!(r.shares(&[0.86, 0.07, 0.07]), [0.85, 0.1, 0.05]);
    // Part of a whole (the rest is 35%): together they're still 65%.
    assert_eq!(r.shares(&[0.31, 0.22, 0.12]), [0.3, 0.25, 0.1]);
    assert_eq!(Round(Precision::Exact).shares(&[1.0 / 3.0; 3]), [0.334, 0.333, 0.333]);
}

#[test]
fn sections_parse_by_name_and_alias() {
    assert_eq!(Section::parse("signature-tracks"), Some(Section::Signature));
    assert_eq!(Section::parse("Set-Shape"), Some(Section::Arc));
    assert_eq!(Section::parse("nope"), None);
    for s in Section::ALL {
        assert_eq!(Section::parse(s.name()), Some(s));
    }
}

#[test]
fn prefs_updates_validate_and_normalise() {
    let mut p = ProfilePrefs::default();
    assert!(p.published().contains(&Section::Tempo));
    assert!(!p.published().contains(&Section::Signature));
    p.apply(&PrefsUpdate { include: vec![Section::Signature], exclude: vec![Section::Style], ..Default::default() })
        .unwrap();
    assert!(p.published().contains(&Section::Signature));
    assert!(!p.published().contains(&Section::Style));

    p.apply(&PrefsUpdate { pin: vec!["Bicep - Glue".into(), "bicep -  glue".into()], ..Default::default() }).unwrap();
    assert_eq!(p.pinned, ["Bicep - Glue"]);
    // Hiding a pinned track unpins it.
    p.apply(&PrefsUpdate { hide: vec!["BICEP - GLUE".into()], ..Default::default() }).unwrap();
    assert!(p.pinned.is_empty());
    assert!(!p.may_name("Bicep - Glue", "Bicep"));
    assert!(p.apply(&PrefsUpdate { unpin: vec!["Bicep - Glue".into()], ..Default::default() }).is_err());

    p.apply(&PrefsUpdate { hide_artist: vec!["Clara La San".into()], ..Default::default() }).unwrap();
    assert!(!p.may_name("Bicep - Saku", "Bicep feat. Clara La San"));
    assert!(p.may_name("Bicep - Apricots", "Bicep"));

    p.apply(&PrefsUpdate { headline: Some("Warm-up specialist".into()), ..Default::default() }).unwrap();
    assert_eq!(p.headline.as_deref(), Some("Warm-up specialist"));
    p.apply(&PrefsUpdate { headline: Some(" ".into()), ..Default::default() }).unwrap();
    assert_eq!(p.headline, None);

    p.apply(&PrefsUpdate { reset: true, ..Default::default() }).unwrap();
    assert_eq!(p, ProfilePrefs::default());
}

#[test]
fn every_trait_is_backed_by_a_section() {
    // Each `tag("id", …)` the summary can add: a trait without a section is never shown.
    let src = include_str!("../mod.rs");
    let ids: Vec<&str> = src
        .match_indices("tag(")
        .filter_map(|(i, _)| src[i + 4..].trim_start().strip_prefix('"')?.split('"').next())
        .collect();
    assert!(ids.len() >= 12, "{ids:?}");
    for id in ids {
        assert!(tag_section(id).is_some(), "{id}");
    }
}

#[test]
fn setting_keys_become_camel_case() {
    assert_eq!(camel("vinyl_speed_adjust"), "vinylSpeedAdjust");
    assert_eq!(camel("quantize"), "quantize");
}

use hegel::generators as gs;
use hegel::{Generator, TestCase};

/// Rounded, a count is a round number it's at least: itself below 10, then a multiple of 5,
/// 50, 500 or 5000 as it grows, less than one step below it. Exact, it's itself.
#[hegel::test(test_cases = 300)]
fn counts_round_down_to_round_numbers_they_reach(tc: TestCase) {
    let n = tc.draw(hegel::one_of!(gs::integers::<usize>().max_value(200), gs::integers::<usize>().max_value(100_000)));
    assert_eq!(Round(Precision::Exact).count(n), Count::Exact { count: n as u64 });
    let steps = [(10, 1), (100, 5), (1000, 50), (10_000, 500)];
    let step = steps.iter().find(|(below, _)| n < *below).map_or(5000, |s| s.1);
    match Round(Precision::Rounded).count(n) {
        Count::Exact { count } => assert!(n < 10 && count == n as u64, "{n}"),
        Count::AtLeast { at_least } => {
            let a = at_least as usize;
            assert!(n >= 10 && a.is_multiple_of(step) && a <= n && n - a < step, "{n}: {a}");
        }
    }
}

/// Shares of one whole, rounded, are each on the grid (5%, or 0.1% exactly), within a step
/// of what they round, and together within a step of what they add up to: exactly the whole
/// when nothing is left out.
#[hegel::test(test_cases = 300)]
fn rounded_shares_still_add_up(tc: TestCase) {
    let r = Round(if tc.draw(gs::booleans()) { Precision::Exact } else { Precision::Rounded });
    let step = 1.0 / r.units();
    let weights: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().max_value(100)).min_size(1).max_size(6));
    let rest = tc.draw(gs::integers::<u32>().max_value(100));
    let whole = weights.iter().sum::<u32>() + rest;
    tc.assume(whole > 0);
    let parts: Vec<f64> = weights.iter().map(|&w| f64::from(w) / f64::from(whole)).collect();
    let out = r.shares(&parts);
    assert_eq!(out.len(), parts.len());
    for (x, p) in out.iter().zip(&parts) {
        assert!(((x * r.units()).round() - x * r.units()).abs() < 1e-6, "{x} isn't on the grid");
        assert!((0.0..=1.0).contains(x) && (x - p).abs() <= step + 1e-9, "{p} rounded to {x}");
    }
    let (want, got) = (parts.iter().sum::<f64>(), out.iter().sum::<f64>());
    assert!((got - want).abs() < step + 1e-9, "{parts:?} as {out:?}");
    if rest == 0 {
        assert!((got - 1.0).abs() < 1e-9, "{parts:?} as {out:?}");
    }
}

/// Every style setting's key reads as serde's `camelCase` would name it, like the rest of the
/// document. Any key at all loses its underscores, and otherwise only the case of its letters
/// can change; a key read once reads the same again.
#[hegel::test(test_cases = 200)]
fn setting_keys_read_in_camel_case(tc: TestCase) {
    #[derive(Serialize, Default)]
    #[serde(rename_all = "camelCase")]
    struct Named {
        quantize: (),
        sync: (),
        master_tempo: (),
        jog_mode: (),
        vinyl_speed_adjust: (),
        tempo_range: (),
        auto_cue: (),
    }
    let serde = serde_json::to_value(Named::default()).unwrap();
    let serde: Vec<&String> = serde.as_object().unwrap().keys().collect();
    let ours: Vec<String> = crate::profile::STYLE_SETTINGS.iter().map(|k| camel(k)).collect();
    assert_eq!(serde.len(), ours.len(), "Named lists every style setting");
    assert!(ours.iter().all(|k| serde.contains(&k)), "{ours:?} against {serde:?}");

    let key = tc.draw(hegel::one_of!(
        gs::sampled_from(crate::profile::STYLE_SETTINGS.to_vec()).map(String::from),
        gs::text().alphabet("ab_Z9").max_size(10)
    ));
    let read = camel(&key);
    assert!(!read.contains('_'), "{key:?} read as {read:?}");
    assert_eq!(read.to_lowercase(), key.replace('_', "").to_lowercase(), "{key:?} read as {read:?}");
    assert_eq!(camel(&read), read, "{key:?}");
}
