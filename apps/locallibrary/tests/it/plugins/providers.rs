//! `art fetch`, `track fetch` and `plugin providers` on the command line, with the test plugin's
//! providers answering as its `answer` setting says (see the plugin host's test plugin).

use std::path::{Path, PathBuf};

use hegel::TestCase;
use hegel::generators as gs;
use serde_json::{Value, json};

use crate::support::{Env, echo_plugin, jpeg};

/// The echo manifest with a metadata provider `found` (in place of its own) and an artwork
/// provider `covers`, each answering as its `answer` setting says.
fn providing() -> String {
    let more = "[[contributes.providers]]\nname = \"covers\"\ntitle = \"Covers\"\nkind = \"artwork\"\n\n\
                [[contributes.settings]]\nkey = \"answer\"\ntitle = \"Answer\"\ntype = \"string\"\n\n\
                [[contributes.settings]]";
    echo_plugin()
        .replace("name = \"tags\"\ntitle = \"The tags as they are\"", "name = \"found\"\ntitle = \"Found\"")
        .replacen("[[contributes.settings]]", more, 1)
}

/// An [`Env`] with [`providing`]'s plugin, and the demo drive USB (two tracks), where it is. Each
/// fetch asks echo alone (`--from echo`): the coverart plugin's providers would go online.
fn env() -> (Env, PathBuf) {
    let mut env = Env::new();
    env.plugins(&[&providing()]);
    let usb = env.demo("USB");
    (env, usb)
}

/// The echo plugin's providers answer `answer`.
fn answer(env: &Env, answer: Value) {
    env.ok(&["--yes", "plugin", "settings", "echo", "answer", &answer.to_string()]);
}

/// A candidate of tags, scoring `score`, of `year`.
fn year(score: f64, year: u16) -> Value {
    json!({ "score": score, "source": "Tags", "label": format!("from {year}"), "metadata": { "year": year } })
}

/// The drive's tracks, by id, with their years.
fn years(usb: &Path) -> Vec<(u32, u16)> {
    let lib = locallibrary::Device::open(usb).unwrap().library().unwrap();
    let mut years: Vec<(u32, u16)> = lib.tracks.iter().map(|t| (t.id, t.year)).collect();
    years.sort();
    years
}

/// A candidate alone scoring the auto-pick or more is chosen for you, the others asked about:
/// without a terminal, refused saying how to answer each with `--choose TRACK=N`, which picks the
/// candidate numbered N, or none with `null`.
#[test]
fn track_fetch_picks_the_sure_one_and_asks_about_the_rest() {
    let (env, usb) = env();
    let ids: Vec<u32> = years(&usb).iter().map(|(id, _)| *id).collect();
    answer(&env, json!({ "each": [year(0.95, 1999), year(0.2, 1950)], "warnings": ["one not found"] }));
    let r = env.run(&["--yes", "track", "fetch", "USB", "--from", "echo"]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    for said in ["track details found for 2 of 2 track(s)", "fill in the details of 2 track(s) from Tags"] {
        assert!(r.out.contains(said), "{said}: {}", r.out);
    }
    assert!(format!("{}{}", r.out, r.err).contains("Found: one not found"), "{}\n{}", r.out, r.err);
    assert_eq!(years(&usb), ids.iter().map(|id| (*id, 1999)).collect::<Vec<_>>());

    answer(&env, json!({ "each": [year(0.5, 2001), year(0.4, 2002)] }));
    let fetch = ["--yes", "track", "fetch", "USB", "--from", "echo", "--replace"];
    let asked = env.fails(2, &fetch);
    for id in &ids {
        assert!(asked.contains(&format!(": answer with --choose {id}=VALUE, VALUE one of:")), "{asked}");
    }
    for item in ["  1     from 2001 (Tags, 50%: 2001)", "  2     from 2002 (Tags, 40%: 2002)", "  null  none of these"]
    {
        assert!(asked.contains(item), "{item}: {asked}");
    }
    assert_eq!(years(&usb), ids.iter().map(|id| (*id, 1999)).collect::<Vec<_>>(), "nothing changed");
    let (one, two) = (format!("{}=2", ids[0]), format!("{}=null", ids[1]));
    let out = env.ok(&[&fetch[..], &["--choose", &one, "--choose", &two]].concat());
    assert!(out.contains("fill in the details of 1 track(s) from Tags"), "{out}");
    assert_eq!(years(&usb), [(ids[0], 2002), (ids[1], 1999)]);

    let err = env.fails(2, &[&fetch[..], &["--choose", "99=1"]].concat());
    assert!(err.contains("--choose 99=...: nothing was found for track #99 to choose from"), "{err}");
    // An answer that isn't one of the question's is refused, naming those that are.
    for wrong in ["3", "0", "\"2\"", "true"] {
        let err = env.fails(2, &[&fetch[..], &["--choose", &format!("{}={wrong}", ids[0])]].concat());
        assert!(err.contains(&format!("{wrong} isn't an answer: ")) && err.contains("  null  none of these"), "{err}");
    }
    let err = env.fails(2, &[&fetch[..], &["--choose", "nothing"]].concat());
    assert!(err.contains("--choose takes PARAM=VALUE, not \"nothing\""), "{err}");
}

/// `--json` gives the plan and what applying it did; `--dry-run` changes nothing; `--from` asks
/// only that plugin's providers, or that one, and names the others when it's none of them.
#[test]
fn track_fetch_in_json_from_one_provider() {
    let (env, usb) = env();
    let ids: Vec<u32> = years(&usb).iter().map(|(id, _)| *id).collect();
    answer(&env, json!({ "each": [year(1.0, 1984)] }));
    let dry = env.json(&["track", "fetch", "USB", "--from", "echo.found", "--dry-run"]);
    assert_eq!((&dry["applied"], &dry["outcome"]), (&json!(false), &json!("dry_run")), "{dry}");
    assert_eq!(dry["plan"]["summary"][0], "fill in the details of 2 track(s) from Tags");
    assert!(years(&usb).iter().all(|(_, y)| *y != 1984));
    let one = ids[0].to_string();
    let v = env.json(&["--yes", "track", "fetch", "USB", "--from", "echo", "--track", &one]);
    assert_eq!(
        (&v["applied"], &v["plan"]["summary"][0]),
        (&json!(true), &json!("fill in the details of 1 track(s) from Tags"))
    );
    assert_eq!(years(&usb)[0], (ids[0], 1984));
    let err = env.fails(1, &["track", "fetch", "USB", "--from", "nope"]);
    assert!(err.contains("nope provides no metadata: coverart.tags, echo.found do"), "{err}");
    let err = env.fails(1, &["art", "fetch", "USB", "--from", "echo.found"]);
    assert!(err.contains("echo.found provides no artwork: coverart.covers, echo.covers do"), "{err}");
}

/// The cover an artwork provider finds is set on the tracks it's for; once they have one, they're
/// left alone without `--replace`.
#[test]
fn art_fetch_sets_the_cover_found() {
    let (env, _usb) = env();
    let cover = json!({ "score": 1, "source": "Covers", "label": "front", "artwork": { "blob": "a.jpg" } });
    answer(&env, json!({ "blobs": { "a.jpg": jpeg(64) }, "each": [cover] }));
    let out = env.ok(&["--yes", "art", "fetch", "USB", "--from", "echo", "--replace"]);
    assert!(out.contains("set cover art on 2 track(s) from Covers"), "{out}");
    assert!(env.ok(&["art", "missing", "USB"]).contains("0 track(s) without artwork"));
    let out = env.ok(&["--yes", "art", "fetch", "USB", "--from", "echo"]);
    assert!(out.contains("cover art found for 0 of 0 track(s)"), "{out}");
    assert!(out.contains("nothing to change"), "{out}");
}

/// The drive's tracks, by id.
fn tracks(usb: &Path) -> Vec<locallibrary::Track> {
    let mut tracks = locallibrary::Device::open(usb).unwrap().library().unwrap().tracks;
    tracks.sort_by_key(|t| t.id);
    tracks
}

/// Without `--replace`, a run sets cover art only on the tracks without any: one with some keeps
/// it while another gets the cover found, and only that one is asked about.
#[test]
fn art_fetch_leaves_a_track_with_cover_art_alone() {
    let (env, usb) = env();
    let art = || tracks(&usb).into_iter().map(|t| t.artwork_path).collect::<Vec<_>>();
    assert_eq!(art(), [None, None]);
    let first = tracks(&usb)[0].id.to_string();
    let cover = |blob: &str| json!({ "score": 1, "source": "Covers", "label": blob, "artwork": { "blob": blob } });
    answer(&env, json!({ "blobs": { "a.jpg": jpeg(64) }, "each": [cover("a.jpg")] }));
    env.ok(&["--yes", "art", "fetch", "USB", "--from", "echo", "--track", &first]);
    let had = art();
    assert!(had[0].is_some() && had[1].is_none(), "{had:?}");
    answer(&env, json!({ "blobs": { "b.jpg": jpeg(48) }, "each": [cover("b.jpg")] }));
    let out = env.ok(&["--yes", "art", "fetch", "USB", "--from", "echo"]);
    for said in ["cover art found for 1 of 1 track(s)", "set cover art on 1 track(s) from Covers"] {
        assert!(out.contains(said), "{said}: {out}");
    }
    let now = art();
    assert_eq!(now[0], had[0], "kept");
    assert!(now[1].is_some() && now[1] != had[0], "{now:?}");
}

/// Without `--replace`, a run fills in tags only on the tracks missing one, and only those they're
/// missing: one with every tag is left alone (and not asked about) while another is filled in.
#[test]
fn track_fetch_leaves_a_track_with_every_tag_alone() {
    let (env, usb) = env();
    let tags = |title: &str, year: u16| {
        let metadata = json!({
            "title": title, "artist": "Artist", "album": "Album", "year": year, "genre": "Genre", "label": "Label",
            "isrc": "GBDUW0000059"
        });
        json!({ "score": 1, "source": "Tags", "label": title, "metadata": metadata })
    };
    let before = tracks(&usb);
    assert!(before.iter().all(|t| t.year == 0 && !t.title.is_empty()), "{before:?}");
    answer(&env, json!({ "each": [tags("First", 1999)] }));
    env.ok(&["--yes", "track", "fetch", "USB", "--from", "echo", "--track", &before[0].id.to_string()]);
    let had = tracks(&usb);
    let full = &had[0];
    assert_eq!((full.title.as_str(), full.year, full.genre.as_str()), (before[0].title.as_str(), 1999, "Genre"));
    assert_eq!(had[1].year, 0);
    answer(&env, json!({ "each": [tags("Second", 2005)] }));
    let out = env.ok(&["--yes", "track", "fetch", "USB", "--from", "echo"]);
    for said in ["track details found for 1 of 1 track(s)", "fill in the details of 1 track(s) from Tags"] {
        assert!(out.contains(said), "{said}: {out}");
    }
    let now = tracks(&usb);
    assert_eq!(now[0], had[0], "kept");
    let filled = &now[1];
    assert_eq!(
        (filled.title.as_str(), filled.year, filled.isrc.as_str()),
        (before[1].title.as_str(), 2005, "GBDUW0000059")
    );
}

/// `plugin providers` lists every provider and the auto-pick, which it sets (asked about first)
/// and puts back; a score it can't be is refused.
#[test]
fn plugin_providers_shows_and_sets_the_auto_pick() {
    let (env, usb) = env();
    let out = env.ok(&["plugin", "providers"]);
    for line in [
        "coverart.covers  artwork   Cover Art Archive",
        "coverart.tags    metadata  MusicBrainz",
        "echo.found       metadata  Found",
        "echo.covers      artwork   Covers",
    ] {
        assert!(out.contains(line), "{line}: {out}");
    }
    assert!(out.contains("a candidate alone scoring 0.9 or more is chosen for you"), "{out}");
    let err = env.fails(3, &["plugin", "providers", "--auto-pick", "0.5"]);
    assert!(err.contains("not confirmed"), "{err}");
    let out = env.ok(&["--yes", "plugin", "providers", "--auto-pick", "0.5"]);
    assert!(out.contains("a candidate alone scoring 0.5 or more is chosen for you"), "{out}");
    assert_eq!(env.json(&["plugin", "providers"])["auto_pick"], json!(0.5));
    // Now one alone at 0.5 is chosen for you.
    answer(&env, json!({ "each": [year(0.5, 2001), year(0.4, 2002)] }));
    env.ok(&["--yes", "track", "fetch", "USB", "--from", "echo", "--replace"]);
    assert!(years(&usb).iter().all(|(_, y)| *y == 2001));
    env.ok(&["--yes", "plugin", "providers", "--reset"]);
    assert_eq!(env.json(&["plugin", "providers"])["auto_pick"], json!(0.9));
    let err = env.fails(1, &["--yes", "plugin", "providers", "--auto-pick=-1"]);
    assert!(err.contains("a score is 0 to 1 (more than 1: never), not -1"), "{err}");
    env.fails(2, &["plugin", "providers", "--auto-pick", "0.5", "--reset"]);
    // Listed with the plugin, and what each finds.
    let listed = env.ok(&["plugin", "list"]);
    assert!(listed.contains("providers found, covers"), "{listed}");
    let info = env.ok(&["plugin", "info", "echo"]);
    for line in [
        "  Providers:",
        "found   Found: track details, for locallibrary track fetch",
        "covers  Covers: cover art, for locallibrary art fetch",
    ] {
        assert!(info.contains(line), "{line}: {info}");
    }
}

/// Whatever candidates the provider finds for each track (none to three, of any score below the
/// auto-pick), answered any way with `--choose`: each track gets the year of the candidate
/// numbered as answered, best first, or keeps its own with `null`.
#[hegel::test(test_cases = 4)]
fn any_answers_fill_in_the_candidates_numbered(tc: TestCase) {
    let (env, usb) = env();
    let before = years(&usb);
    let mut candidates = Vec::new();
    let mut args = vec![
        "--yes".to_string(),
        "track".into(),
        "fetch".into(),
        "USB".into(),
        "--from".into(),
        "echo".into(),
        "--replace".into(),
    ];
    let mut want = Vec::new();
    for (k, (id, had)) in before.iter().enumerate() {
        let n = tc.draw(gs::integers::<usize>().max_value(3));
        // Distinct scores, best first, and distinct years: none alike, none picked.
        let found: Vec<(f64, u16)> = (0..n).map(|i| (0.8 - 0.2 * i as f64, 2000 + (k * 10 + i) as u16)).collect();
        for (score, y) in &found {
            let mut c = year(*score, *y);
            c["track"] = json!(id);
            candidates.push(c);
        }
        let chosen = match n {
            0 => None,
            _ => Some(tc.draw(gs::integers::<usize>().max_value(n))),
        };
        if let Some(i) = chosen {
            let value = if i == 0 { "null".to_string() } else { i.to_string() };
            args.extend(["--choose".into(), format!("{id}={value}")]);
        }
        want.push((*id, chosen.filter(|i| *i > 0).map_or(*had, |i| found[i - 1].1)));
    }
    answer(&env, json!({ "candidates": candidates }));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    env.ok(&args);
    assert_eq!(years(&usb), want, "{args:?}");
}

/// Any steps of `plugin providers` (an auto-pick from 0 to 1, or one it can't be, or back to its
/// default; confirmed or not; shown in text or `--json`): the auto-pick reads back as last set,
/// 0.9 until then and once put back, and a score it can't be is refused. The text lists every
/// provider as the JSON does, with the auto-pick.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_auto_pick_reads_back_as_last_set(tc: TestCase) {
    let (env, _usb) = env();
    let mut kept = 0.9;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let score = tc.draw(gs::sampled_from(vec![Some(0.0), Some(0.5), Some(0.75), Some(1.0), Some(-1.0), None]));
        let yes = tc.draw(gs::weighted_booleans(0.8));
        let set = score.map(|s| format!("--auto-pick={s}"));
        let mut args = vec!["plugin", "providers", set.as_deref().unwrap_or("--reset")];
        if yes {
            args.insert(0, "--yes");
        }
        let r = env.run(&args);
        let said = format!("{args:?}: exit {}\n{}{}", r.code, r.out, r.err);
        match (score, yes) {
            (Some(s), true) if s < 0.0 => {
                assert_eq!(r.code, 1, "{said}");
                assert!(r.err.contains("a score is 0 to 1 (more than 1: never), not -1"), "{said}");
            }
            (_, false) => assert_eq!(r.code, 3, "{said}"),
            (score, true) => {
                assert_eq!(r.code, 0, "{said}");
                kept = score.unwrap_or(0.9);
            }
        }
        let shown = env.json(&["plugin", "providers"]);
        assert_eq!(shown["auto_pick"].as_f64(), Some(kept), "{said}");
        let text = env.ok(&["plugin", "providers"]);
        for p in shown["providers"].as_array().unwrap() {
            let at = text.find(&format!("{}.{} ", p["plugin"].as_str().unwrap(), p["name"].as_str().unwrap()));
            let line = text[at.unwrap_or_else(|| panic!("{p} isn't in {text}"))..].lines().next().unwrap();
            assert!(line.ends_with(p["title"].as_str().unwrap()), "{line}");
        }
        assert!(text.ends_with(&format!("a candidate alone scoring {kept} or more is chosen for you\n")), "{text}");
    }
}
