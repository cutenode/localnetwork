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
    assert!(p.published().contains(&Section::Genres));
    assert!(!p.published().contains(&Section::Artists));
    p.apply(&PrefsUpdate { include: vec![Section::Artists], exclude: vec![Section::Era], ..Default::default() })
        .unwrap();
    assert!(p.published().contains(&Section::Artists));
    assert!(!p.published().contains(&Section::Era));

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

/// Only Energy, Genres, Artists, Era, Sets and Collection can be published; of those, Artists and
/// Collection are opt-in.
#[test]
fn only_what_describes_the_collection_and_the_sets_can_be_published() {
    let publishable: Vec<Section> = Section::ALL.into_iter().filter(|s| s.publishable()).collect();
    assert_eq!(
        publishable,
        [Section::Sets, Section::Energy, Section::Genres, Section::Artists, Section::Era, Section::Collection]
    );
    let defaults: Vec<Section> = ProfilePrefs::default().published().into_iter().collect();
    assert_eq!(defaults, [Section::Sets, Section::Energy, Section::Genres, Section::Era]);
    for s in Section::ALL {
        assert!(!s.published_by_default() || s.publishable(), "{s}");
    }
}

/// Preferences saved before a section stopped being publishable still name it: it's ignored,
/// and the next change drops it. Asking for one now is refused, and changes nothing.
#[test]
fn a_section_that_cant_be_published_is_ignored_when_saved_and_refused_when_asked_for() {
    let saved: ProfilePrefs =
        serde_json::from_value(serde_json::json!({ "sections": ["summary", "tempo", "genres", "signature"] })).unwrap();
    assert_eq!(saved.published().into_iter().collect::<Vec<_>>(), [Section::Genres]);

    let mut p = saved.clone();
    p.apply(&PrefsUpdate { include: vec![Section::Era], ..Default::default() }).unwrap();
    assert_eq!(p.sections, Some([Section::Genres, Section::Era].into()));

    for u in [
        PrefsUpdate { include: vec![Section::Tempo], ..Default::default() },
        PrefsUpdate { sections: Some(vec![Section::Genres, Section::Signature]), ..Default::default() },
    ] {
        let mut p = saved.clone();
        let e = p.apply(&u).unwrap_err().to_string();
        assert!(e.contains("stays on this computer"), "{e}");
        assert_eq!(p, saved);
    }
    // Leaving one out is harmless.
    let mut p = saved.clone();
    p.apply(&PrefsUpdate { exclude: vec![Section::Tempo], ..Default::default() }).unwrap();
    assert_eq!(p.published().into_iter().collect::<Vec<_>>(), [Section::Genres]);
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

/// A section parses from its name or any of its other names, whatever their case, with `-` for
/// `_` and space around them; nothing else names one.
#[hegel::test(test_cases = 200)]
fn prop_a_section_parses_from_any_of_its_names(tc: TestCase) {
    let mut names: Vec<(String, Section)> = Section::ALL.iter().map(|s| (s.name().to_string(), *s)).collect();
    for (alias, s) in [
        ("shape", Section::Arc),
        ("set_shape", Section::Arc),
        ("signature_tracks", Section::Signature),
        ("tracks", Section::Signature),
        ("player", Section::Style),
        ("player_style", Section::Style),
        ("settings", Section::Style),
        ("headline", Section::Summary),
        ("tags", Section::Summary),
        ("harmony", Section::Mixing),
        ("bpm", Section::Tempo),
        ("mood", Section::Energy),
    ] {
        names.push((alias.to_string(), s));
    }
    let (name, want) = tc.draw(gs::sampled_from(names));
    let spelled: String = name
        .chars()
        .map(|c| match c {
            '_' if tc.draw(gs::booleans()) => '-',
            c if tc.draw(gs::booleans()) => c.to_ascii_uppercase(),
            c => c,
        })
        .collect();
    let padded = format!(
        "{}{spelled}{}",
        " ".repeat(tc.draw(gs::integers::<usize>().max_value(2))),
        "\t".repeat(tc.draw(gs::integers::<usize>().max_value(1)))
    );
    assert_eq!(Section::parse(&padded), Some(want), "{padded:?}");
    assert_eq!(Section::parse(&format!("{spelled}x")), None);
}

/// A genre alias needs the tagged text and the genre; one that has both replaces the alias of
/// that text however it's spelled and keeps the others, and removing an alias there isn't is
/// refused.
#[hegel::test(test_cases = 100)]
fn prop_genre_aliases_need_both_sides(tc: TestCase) {
    let text = || tc.draw(gs::sampled_from(vec!["", " ", "Deep House", "deep house ", "Techno"]));
    let mut p = ProfilePrefs::default();
    let earlier = tc.draw(gs::booleans());
    if earlier {
        p.genre_aliases.insert("Deep House".into(), "House".into());
    }
    let (from, to) = (text(), text());
    let added = p.apply(&PrefsUpdate { alias: [(from.to_string(), to.to_string())].into(), ..Default::default() });
    let blank = from.trim().is_empty() || to.trim().is_empty();
    assert_eq!(added.is_err(), blank, "{from:?} -> {to:?}: {added:?}");
    let replaced = !blank && normalize(from) == normalize("Deep House");
    if !blank {
        assert_eq!(p.genre_aliases.get(from.trim()).map(String::as_str), Some(to.trim()));
    }
    assert_eq!(p.genre_aliases.len(), usize::from(!blank) + usize::from(earlier && !replaced), "{:?}", p.genre_aliases);
    let gone = text();
    let had = p.genre_aliases.keys().any(|k| normalize(k) == normalize(gone));
    let removed = p.apply(&PrefsUpdate { unalias: vec![gone.to_string()], ..Default::default() });
    assert_eq!(removed.is_ok(), had, "{gone:?} from {:?}: {removed:?}", p.genre_aliases);
    assert!(!p.genre_aliases.keys().any(|k| normalize(k) == normalize(gone)));
    if !had {
        assert!(matches!(removed, Err(Error::NotFound(_))), "{removed:?}");
    }
}

/// Every trait the summary can show is backed by a section, and nothing else is.
#[hegel::test(test_cases = 100)]
fn prop_only_the_summarys_traits_have_a_section(tc: TestCase) {
    let src = include_str!("../mod.rs");
    let mut ids: Vec<String> = src
        .match_indices("tag(")
        .filter_map(|(i, _)| src[i + 4..].trim_start().strip_prefix('"')?.split('"').next())
        .map(str::to_string)
        .collect();
    let known = ids.len();
    ids.extend(["", "harmonic_", "Focused", "nope"].map(String::from));
    let i = tc.draw(gs::integers::<usize>().max_value(ids.len() - 1));
    assert_eq!(tag_section(&ids[i]).is_some(), i < known, "{:?}", ids[i]);
}
