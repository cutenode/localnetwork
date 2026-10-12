//! Rule-based playlists against a model: rules set and removed by path in any order (paths that
//! differ in spacing are one rule, in case two), invalid ones refused, and a drive's copies
//! holding exactly the tracks each rule's filter, sort and limit select.

use crate::drawn;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::edit::{self, EditOptions};
use locallibrary::progress::Ctx;
use locallibrary::query::{self, Filter, QueryContext};
use locallibrary::smart::{SmartPlaylist, SmartPlaylists};
use uuid::Uuid;

const PATHS: &[&str] = &["Smart/Peak", "Smart / Peak ", "smart/peak", "Warm", "Smart//Warm", ""];
const FILTERS: &[(&str, bool)] = &[
    ("bpm:>=120", true),
    ("rating:>=1", true),
    ("artist:bicep", true),
    ("bpm:100-130 -artist:bicep", true),
    ("key:H", false),
    ("  ", false),
];
const SORTS: &[Option<&str>] = &[None, Some("-bpm"), Some("title"), Some("nonsense:")];

/// The path a rule is told apart by: names trimmed, empty ones left out.
fn key(path: &str) -> String {
    path.split('/').map(str::trim).filter(|n| !n.is_empty()).collect::<Vec<_>>().join("/")
}

#[hegel::test(test_cases = 200)]
fn rules_match_the_model(tc: TestCase) {
    let mut rules = SmartPlaylists::default();
    let mut model: Vec<(String, String)> = Vec::new();
    let drive = Uuid::from_u128(7);
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(8)) {
        let path = tc.draw(gs::sampled_from(PATHS));
        if tc.draw(gs::weighted_booleans(0.7)) {
            let (filter, filter_ok) = tc.draw(gs::sampled_from(FILTERS));
            let sort = tc.draw(gs::sampled_from(SORTS));
            let rule = SmartPlaylist {
                path: path.into(),
                filter: filter.into(),
                sort: sort.map(Into::into),
                limit: tc.draw(gs::optional(gs::integers::<usize>().max_value(5))),
                drives: if tc.draw(gs::booleans()) { vec![drive] } else { vec![] },
            };
            let valid = !key(path).is_empty() && filter_ok && sort != Some("nonsense:");
            assert_eq!(rule.validate().is_ok(), valid, "{rule:?}");
            assert_eq!(rules.set(rule).is_ok(), valid);
            if valid {
                match model.iter_mut().find(|(k, _)| *k == key(path)) {
                    Some(slot) => slot.1 = filter.into(),
                    None => model.push((key(path), filter.into())),
                }
            }
        } else {
            let exact = model.iter().position(|(k, _)| *k == key(path));
            let loose: Vec<usize> = (0..model.len()).filter(|i| model[*i].0.eq_ignore_ascii_case(&key(path))).collect();
            let r = rules.remove(path);
            match (exact, loose.as_slice()) {
                (Some(i), _) | (None, &[i]) => {
                    assert_eq!(key(&r.unwrap().path), model[i].0);
                    model.remove(i);
                }
                _ => assert!(r.is_err()),
            }
        }
        let held: Vec<(String, String)> = rules.playlists.iter().map(|p| (key(&p.path), p.filter.clone())).collect();
        assert_eq!(held, model);
        for (k, f) in &model {
            assert_eq!(rules.get(k).map(|r| r.filter.as_str()), Some(f.as_str()));
        }
        let everywhere = rules.playlists.iter().filter(|p| p.drives.is_empty()).count();
        assert_eq!(rules.edits_for(&[]).len(), everywhere);
        assert_eq!(rules.edits_for(&[drive]).len(), rules.playlists.len());
    }
}

/// Written to a drive, each rule's playlist holds the tracks its filter selects, in its order and
/// no more than its limit; written again, nothing changes.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn a_rules_playlist_holds_what_it_selects(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (root, lib) = drawn::drive(&tc, dir.path(), "A");
    let mut rules = SmartPlaylists::default();
    for path in ["Smart/One", "Smart/Two"] {
        let (filter, ok) = tc.draw(gs::sampled_from(FILTERS));
        if !ok {
            continue;
        }
        let sort = tc.draw(gs::sampled_from(&SORTS[..3]));
        let limit = tc.draw(gs::optional(gs::integers::<usize>().min_value(1).max_value(5)));
        rules
            .set(SmartPlaylist {
                path: path.into(),
                filter: filter.into(),
                sort: sort.map(Into::into),
                limit,
                drives: vec![],
            })
            .unwrap();
    }
    let edits = rules.edits_for(&[]);
    let mut d = Device::open_export(&root).unwrap();
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let after = d.library().unwrap();
    for r in &rules.playlists {
        let mut want: Vec<&locallibrary::Track> = Filter::parse(&r.filter).unwrap().select(&QueryContext::new(&lib));
        if let Some(s) = &r.sort {
            query::sort_tracks(&mut want, &query::parse_sort(s).unwrap());
        }
        want.truncate(r.limit.unwrap_or(usize::MAX));
        let p = after.find_playlist(&r.path).unwrap();
        let got: Vec<&str> = p.track_ids.iter().map(|id| after.track(*id).unwrap().file_path.as_str()).collect();
        assert_eq!(got, want.iter().map(|t| t.file_path.as_str()).collect::<Vec<_>>(), "{r:?}");
    }
    let again = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    assert!(again.is_noop(), "written again, nothing changes");
}
