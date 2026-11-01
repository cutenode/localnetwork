//! Property tests of `locallibrary mix` on synthetic audio: drive A's two tracks made of drawn
//! sounds, and a recording that plays some of them (and sounds the drive doesn't have) in any
//! order. The tracklist printed is the one its `--json` reports, it names no track that wasn't
//! played, and the report and data files are written where they're asked for, never over a file
//! that's there unless `--replace` says to.

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use loclib_mixid::test_util::{synth, wav_of};
use serde_json::Value;

use crate::support::{Env, path};

/// The rate fingerprints are made at: the audio needs nothing finer.
const RATE: u32 = 11_025;

/// A 16-bit WAV of `samples`.
fn wav(samples: &[f32]) -> Vec<u8> {
    wav_of(RATE, 16, samples.iter().map(|s| (s * 12_000.0) as i32).collect::<Vec<_>>().into_iter())
}

/// Sounds: each seed is a different "track".
const SEEDS: [u32; 3] = [7919, 104_729, 1_299_709];

/// What the drive doesn't have.
const UNKNOWN: u32 = 31_337;

/// Whether `line` is a play's row of a printed tracklist (`  1      0:00  Artist - Title ...`).
fn is_play(line: &str) -> bool {
    line.get(..3).is_some_and(|n| n.trim().parse::<usize>().is_ok()) && line.get(3..5) == Some("  ")
}

/// Drive A's tracks, each a drawn sound (two may be the same) or no audio, fingerprinted ahead of
/// time, then found in a recording of some of them and sounds the drive doesn't have (each
/// printed as a gap in the tracklist when it's long enough to be one).
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_recorded_mix_prints_the_tracklist_its_json_has(tc: TestCase) {
    let fx = Env::from_template();
    let a = fx.volumes.join("A");
    // Drive A's tracks: a sound each (maybe the same one), no audio at all, or a file that isn't
    // audio (the drive's placeholder).
    let mut sounds = Vec::new();
    let mut unreadable = 0;
    for n in [1, 2] {
        let file = a.join(format!("Contents/Loopmasters/UnknownAlbum/Demo Track {n}.mp3"));
        match tc.draw(gs::integers::<u8>().max_value(6)) {
            0 => {
                std::fs::remove_file(&file).unwrap();
                sounds.push(None);
                continue;
            }
            1 => {
                unreadable += 1;
                sounds.push(None);
                continue;
            }
            _ => {}
        }
        let seed = tc.draw(gs::sampled_from(&SEEDS[..]));
        let secs = f64::from(tc.draw(gs::integers::<u32>().min_value(25).max_value(40)));
        std::fs::write(&file, wav(&synth(secs, seed))).unwrap();
        sounds.push(Some((seed, secs)));
    }

    // Fingerprinting ahead of time: as many tracks as have audio, all of them new.
    let json = tc.draw(gs::booleans());
    let r = fx.run(&[if json { "--json" } else { "--quiet" }, "mix", "--drive", "A"]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    let with_audio = sounds.iter().flatten().count();
    match json {
        true => {
            let v: Value = serde_json::from_str(&r.out).unwrap();
            assert_eq!((v["tracks"].as_u64(), v["indexed"].as_u64()), (Some(2), Some(with_audio as u64)), "{v}");
            assert_eq!(v["missing"].as_array().unwrap().len(), 2 - with_audio - unreadable, "{v}");
            assert_eq!(v["failed"].as_array().unwrap().len(), unreadable, "{v}");
        }
        false => {
            let line = format!("A: {with_audio} of 2 tracks fingerprinted ({with_audio} new)");
            assert!(r.out.contains(&line), "{line:?}: {}", r.out);
            let missing = 2 - with_audio - unreadable;
            assert_eq!(r.out.contains("have no audio file on the drive"), missing > 0, "{}", r.out);
            assert_eq!(r.out.matches("couldn't read Loopmasters - Demo Track").count(), unreadable, "{}", r.out);
        }
    }

    // The recording: drive tracks and sounds it doesn't have, one after another.
    let parts: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(2)).min_size(1).max_size(3));
    let mut recording = Vec::new();
    let mut played = Vec::new();
    for p in &parts {
        match p {
            0 | 1 => {
                let Some((seed, secs)) = sounds[*p as usize] else { continue };
                recording.extend(synth(secs, seed));
                // Either track with the same sound is a fair answer.
                played.extend(
                    sounds
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| s.is_some_and(|s| s.0 == seed))
                        .map(|(i, _)| i as u64 + 1),
                );
            }
            // Long enough or not to be told as a stretch the library doesn't have (30 s).
            _ => {
                let unknown_secs = tc.draw(gs::integers::<u32>().min_value(20).max_value(40));
                recording.extend(synth(f64::from(unknown_secs), UNKNOWN));
            }
        }
    }
    if recording.is_empty() {
        recording.extend(synth(25.0, UNKNOWN));
    }
    let rec = fx.home("friday.wav");
    std::fs::write(&rec, wav(&recording)).unwrap();

    // The same recording twice over, now and then: each identified, written into the folders.
    let twice = tc.draw(gs::weighted_booleans(0.3));
    let again_rec = fx.home("saturday.wav");
    if twice {
        std::fs::copy(&rec, &again_rec).unwrap();
    }
    let html = fx.home("pages/");
    let data = if twice { fx.home("data/") } else { fx.home("friday.json") };
    let mut args = vec!["mix", path(&rec), "--drive", "A"];
    if twice {
        args.insert(2, path(&again_rec));
    }
    let want_html = tc.draw(gs::booleans());
    let want_data = tc.draw(gs::booleans());
    if want_html {
        args.extend(["--html", html.to_str().unwrap()]);
    }
    if want_data {
        args.extend(["--export", data.to_str().unwrap()]);
    }
    let v = fx.json(&args);
    let reports = v.as_array().unwrap();
    assert_eq!(reports.len(), 1 + usize::from(twice), "{v}");
    let report = &reports[0];
    let plays = report["plays"].as_array().unwrap();
    for r in reports {
        assert_eq!(r["plays"].as_array().unwrap().len(), plays.len(), "the same recording: {v}");
    }
    for p in plays {
        let id = p["track"]["id"].as_u64().unwrap();
        assert!(played.contains(&id), "track {id} wasn't played ({parts:?}, {sounds:?}): {v}");
    }
    // Written again with --replace, the tracklist printed is the JSON's.
    let mut again = args.clone();
    if want_html || want_data {
        // Files that are there are kept unless --replace is given.
        let r = fx.run(&args);
        assert_eq!(r.code, 1, "{}", r.err);
        assert!(r.err.contains("--replace"), "{}", r.err);
        again.push("--replace");
    }
    let out = fx.ok(&again);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "friday", "{out}");
    assert_eq!(out.contains("\n\nsaturday\n"), twice, "{out}");
    assert!(lines[1].contains(&format!(" · {} tracks from A · ", plays.len())), "{out}");
    let coverage = format!("{:.0}% identified", report["coverage"].as_f64().unwrap() * 100.0);
    assert!(lines[1].contains(&coverage), "{coverage:?}: {out}");
    let rows: Vec<&&str> = lines.iter().filter(|l| is_play(l)).collect();
    assert_eq!(rows.len(), plays.len() * reports.len(), "{out}");
    for (row, p) in rows.iter().zip(plays.iter().cycle()) {
        let name = format!("{} - {}", p["track"]["artist"].as_str().unwrap(), p["track"]["title"].as_str().unwrap());
        assert!(row.contains(&name), "{row:?}: {p}");
        let speed = format!("{:+.1}%", (p["tempo"].as_f64().unwrap() - 1.0) * 100.0);
        assert!(row.contains(&speed), "{row:?}: {p}");
    }
    let gaps = lines.iter().filter(|l| l.starts_with("  ?  ")).count();
    assert_eq!(gaps, report["unidentified"].as_array().unwrap().len() * reports.len(), "{out}");
    for (wanted, what) in [(want_html, "report: "), (want_data, "data: ")] {
        assert_eq!(out.matches(&format!("✓ {what}")).count(), if wanted { reports.len() } else { 0 }, "{out}");
    }
    if want_data {
        let files: Vec<std::path::PathBuf> = match twice {
            true => std::fs::read_dir(&data).unwrap().map(|e| e.unwrap().path()).collect(),
            false => vec![data.clone()],
        };
        assert_eq!(files.len(), reports.len(), "{files:?}");
        for f in files {
            let doc: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
            assert_eq!(doc["plays"].as_array().unwrap().len(), plays.len(), "{doc}");
        }
    }
    if want_html {
        let pages: Vec<std::path::PathBuf> = std::fs::read_dir(&html).unwrap().map(|e| e.unwrap().path()).collect();
        assert_eq!(pages.len(), reports.len(), "{pages:?}");
        for page in pages {
            assert!(
                page.extension().is_some_and(|e| e == "html") && out.contains(&format!("report: {}", page.display())),
                "{out}"
            );
        }
    }
}

/// `--open` opens each report page written (in the store's reports folder unless `--html` says
/// where) with the system's opener; when the opener fails, the run still succeeds and says which
/// page it couldn't open.
#[cfg(unix)]
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(twice = false, html = false, opener_works = false)]
fn reports_are_opened_where_they_are_written(tc: TestCase) {
    let fx = Env::from_template();
    let sound = wav(&synth(30.0, SEEDS[0]));
    std::fs::write(fx.volumes.join("A/Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3"), &sound).unwrap();
    let twice = tc.draw(gs::booleans());
    let recordings: Vec<std::path::PathBuf> =
        (0..1 + usize::from(twice)).map(|i| fx.home(&format!("set {i}.wav"))).collect();
    for r in &recordings {
        std::fs::write(r, &sound).unwrap();
    }
    // The system's opener (`open` on macOS, `xdg-open` elsewhere): one that notes what it's
    // given, and fails or not.
    let opener_works = tc.draw(gs::booleans());
    let (bin, opened) = (fx.home("bin"), fx.home("opened"));
    std::fs::create_dir_all(&bin).unwrap();
    let script =
        format!("#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{}'\nexit {}\n", opened.display(), u8::from(!opener_works));
    for name in ["open", "xdg-open"] {
        std::fs::write(bin.join(name), &script).unwrap();
        crate::support::set_mode(&bin.join(name), 0o755);
    }
    let mut args: Vec<&str> = recordings.iter().map(|r| path(r)).collect();
    args.splice(0..0, ["mix"]);
    args.extend(["--drive", "A", "--open"]);
    let html = tc.draw(gs::booleans());
    let pages = fx.home("pages/");
    if html {
        args.extend(["--html", path(&pages)]);
    }
    let out = fx.command().env("PATH", &bin).args(&args).output().unwrap();
    let (stdout, stderr) = (String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap());
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{stderr}");
    let opened: Vec<String> = std::fs::read_to_string(&opened).unwrap().lines().map(String::from).collect();
    assert_eq!(opened.len(), recordings.len(), "{opened:?}");
    let folder = if html { pages.clone() } else { fx.store.join("reports") };
    for page in &opened {
        let page = std::path::Path::new(page);
        assert!(page.extension().is_some_and(|e| e == "html") && page.is_file(), "{page:?}");
        assert!(
            page.starts_with(&folder) || page.starts_with(folder.canonicalize().unwrap()),
            "{page:?} not in {folder:?}"
        );
        assert_eq!(stdout.contains(&format!("couldn't open {}", page.display())), !opener_works, "{stdout}");
    }
}

/// Only files `mix` can read are taken, before anything is fingerprinted: a recording that isn't
/// there, one that isn't audio, and `--audio` for several recordings are refused, saying why.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn only_recordings_mix_can_read_are_taken(tc: TestCase) {
    let fx = Env::from_template();
    let notes = fx.home("notes.txt");
    std::fs::write(&notes, "not audio").unwrap();
    let set = fx.home("set.wav");
    std::fs::write(&set, wav(&synth(1.0, 1))).unwrap();
    let gone = fx.home("gone.wav");
    let json = tc.draw(gs::booleans());
    let (args, why): (Vec<&str>, String) = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => (vec!["mix", path(&gone)], format!("{} doesn't exist", gone.display())),
        1 => (vec!["mix", path(&notes)], format!("{} isn't a format mix reads", notes.display())),
        _ => (vec!["mix", path(&set), path(&set), "--audio", path(&set)], "--audio goes with a single mix".into()),
    };
    let mut all = if json { vec!["--json"] } else { vec![] };
    all.extend(args);
    let r = fx.run(&all);
    assert_eq!(r.code, 1, "{}\n{}", r.out, r.err);
    let said = if json {
        serde_json::from_str::<Value>(&r.out).unwrap()["error"]["message"].as_str().unwrap().to_string()
    } else {
        r.err
    };
    assert!(said.contains(&why), "{why:?}: {said}");
}
