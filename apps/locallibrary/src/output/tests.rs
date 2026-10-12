use super::*;

#[test]
fn erasing_takes_the_name_not_just_yes() {
    let out = Output { json: true, quiet: true, yes: true, pager: false };
    assert_eq!(out.confirm_typed("", "KEYCHAIN", None), Some(Outcome::NotConfirmed), "--yes alone isn't enough");
    assert_eq!(out.confirm_typed("", "KEYCHAIN", Some("keychain")), None);
    assert_eq!(out.confirm_typed("", "KEYCHAIN", Some("BACKUP")), Some(Outcome::Declined));
    // Other questions take --yes.
    assert_eq!(out.ask("Go ahead?"), Answer::Yes);
    assert_eq!(out.decide(true, "Go ahead?"), Some(Outcome::DryRun));
}

#[test]
fn columns_count_what_the_terminal_shows() {
    // Wide characters take two columns, and a cut is made between characters.
    assert_eq!(fit_with("日本語のタイトル", 9, "…"), "日本語の…");
    assert_eq!(fit_with("Stavöstranos", 14, "…"), "Stavöstranos  ");
    assert_eq!(fit_with("abcdef", 4, "…"), "abc…");
    assert_eq!(fit_with("abcdefgh", 6, "..."), "abc...");
    // An emoji and its presentation selector are two columns together, one apart.
    assert_eq!(fit_with("a☺\u{fe0f}bc", 4, "…"), "a☺\u{fe0f}…");
    // Too narrow for the ellipsis: blank.
    assert_eq!(fit_with("abcdef", 2, "..."), "  ");
}

/// A name typed wrong declines the erase, whether or not the output is JSON.
#[test]
fn a_wrong_name_declines_an_erase() {
    let out = Output { json: false, quiet: false, yes: false, pager: false };
    assert_eq!(out.confirm_typed("", "KEYCHAIN", Some(" keychain ")), None);
    assert_eq!(out.confirm_typed("", "KEYCHAIN", Some("BACKUP")), Some(Outcome::Declined));
}

/// An item bar counts files and shows the latest one's name, a byte bar counts bytes; a
/// new phase replaces the bar and its end clears it.
#[test]
fn progress_bars_count_what_their_phase_counts() {
    let p = CliProgress { bar: Mutex::new(None), enabled: true };
    let bar = || p.bar.lock().unwrap().as_ref().map(|(b, _)| (b.prefix(), b.position(), b.message()));
    let item = |path: &str| Event::Item { phase: Phase::Scan, path: path.into(), bytes: 10 };
    p.on_event(&Event::PhaseStart { phase: Phase::Scan, items: 3, bytes: 0 });
    p.on_event(&item("Contents/a.mp3"));
    p.on_event(&item("Contents/b.mp3"));
    p.on_event(&Event::Bytes { phase: Phase::Scan, bytes: 100 });
    assert_eq!(bar(), Some(("Scanning".into(), 2, "b.mp3".into())));

    p.on_event(&Event::PhaseStart { phase: Phase::Copy, items: 3, bytes: 1000 });
    p.on_event(&item("c.mp3"));
    p.on_event(&Event::Bytes { phase: Phase::Copy, bytes: 250 });
    assert_eq!(bar(), Some(("Copying".into(), 250, "c.mp3".into())));
    p.on_event(&Event::Warning { message: "skipped one".into() });
    p.on_event(&Event::PhaseEnd { phase: Phase::Copy });
    assert_eq!(bar(), None);

    // Nothing to count: a spinner.
    p.on_event(&Event::PhaseStart { phase: Phase::Verify, items: 0, bytes: 0 });
    assert_eq!(bar().map(|(prefix, position, _)| (prefix, position)), Some(("Verifying".into(), 0)));
    p.on_event(&Event::PhaseEnd { phase: Phase::Verify });

    // `--json`, `--quiet` or no terminal: no bars.
    let off = CliProgress { bar: Mutex::new(None), enabled: false };
    off.on_event(&Event::PhaseStart { phase: Phase::Scan, items: 3, bytes: 0 });
    off.on_event(&Event::Warning { message: "said anyway".into() });
    assert!(off.bar.lock().unwrap().is_none());
}

#[test]
fn ascii_terminals_get_stand_ins_for_symbols_not_names() {
    assert_eq!(ascii("✓ copied · Café → Bar ★★☆"), "v copied - Café -> Bar **.");
    assert!(matches!(ascii("plain"), Cow::Borrowed(_)));
}

/// Counts read back as the number they print: digits in groups of three from the right,
/// and a plural only for other than one.
#[hegel::test(test_cases = 300)]
fn counts_read_back(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<usize>());
    let s = thousands(n);
    assert_eq!(s.replace(',', ""), n.to_string());
    let groups: Vec<&str> = s.split(',').collect();
    assert!((1..=3).contains(&groups[0].len()), "{s}");
    assert!(groups[1..].iter().all(|g| g.len() == 3), "{s}");
    let one = tc.draw(gs::sampled_from(&["track", "drive", "set"][..]));
    let expected = if n == 1 { format!("1 {one}") } else { format!("{s} {one}s") };
    assert_eq!(plural(n, one), expected);
}

/// A clock reads back as the seconds rounded to the nearest one, with minutes and seconds
/// below 60 and hours only past an hour; a duration as the hours and minutes it covers.
#[hegel::test(test_cases = 300)]
fn clocks_and_durations_read_back(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let secs = tc.draw(gs::floats::<f64>().min_value(-100.0).max_value(400_000.0));
    let rounded = secs.max(0.0).round() as u64;
    let parts: Vec<u64> = clock(secs).split(':').map(|p| p.parse().unwrap()).collect();
    let back = match parts[..] {
        [m, s] => {
            assert!(rounded < 3600 && s < 60, "{secs}: {}", clock(secs));
            m * 60 + s
        }
        [h, m, s] => {
            assert!(h >= 1 && m < 60 && s < 60, "{secs}: {}", clock(secs));
            h * 3600 + m * 60 + s
        }
        _ => panic!("{}", clock(secs)),
    };
    assert_eq!(back, rounded, "{secs}: {}", clock(secs));

    let whole = tc.draw(gs::integers::<u64>().max_value(1 << 40));
    let d = duration(whole);
    let num = |p: &str, unit: char| p.strip_suffix(unit).unwrap().parse::<u64>().unwrap();
    let (a, b) = d.split_once(' ').unwrap();
    if a.ends_with('h') {
        let covered = num(a, 'h') * 3600 + num(b, 'm') * 60;
        assert!(covered <= whole && whole - covered < 60 && whole >= 3600, "{whole}: {d}");
    } else {
        assert_eq!(num(a, 'm') * 60 + num(b, 's'), whole, "{d}");
    }
}

/// Fitted text takes exactly the columns asked for; clipped, it's as much of the start of
/// the text as fits beside the ellipsis.
#[hegel::test(test_cases = 300)]
fn fitted_text_takes_its_columns(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use unicode_width::UnicodeWidthStr;
    let s: String = tc.draw(gs::text().alphabet("ab é日本😹\u{301}\u{fe0f}").max_size(20));
    let width = tc.draw(gs::integers::<usize>().max_value(25));
    let ellipsis = tc.draw(gs::sampled_from(&["…", "...", ""][..]));
    assert_eq!(fit_with(&s, width, ellipsis).width(), width, "{s:?} in {width}");
    let clipped = clip(&s, width, ellipsis);
    assert!(clipped.width() <= width, "{clipped:?}");
    if s.width() <= width {
        assert_eq!(clipped, s);
        return;
    }
    if width < ellipsis.width() {
        assert_eq!(clipped, "");
        return;
    }
    let kept = clipped.strip_suffix(ellipsis).unwrap_or_else(|| panic!("{clipped:?} has no {ellipsis:?}"));
    assert!(s.starts_with(kept), "{kept:?} isn't the start of {s:?}");
    if let Some(c) = s[kept.len()..].chars().next() {
        let longer = &s[..kept.len() + c.len_utf8()];
        assert!(longer.width() + ellipsis.width() > width, "{longer:?} would have fit");
    }
}

/// ASCII output has a stand-in for every symbol and leaves every other character, and an
/// ASCII line, alone.
#[hegel::test(test_cases = 300)]
fn ascii_replaces_only_symbols(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let symbols: String = SYMBOLS.iter().map(|(c, _)| *c).collect();
    let s: String = tc.draw(gs::text().alphabet(&format!("{symbols}aé日 ")).max_size(30));
    let out = ascii(&s);
    assert!(!out.chars().any(|c| symbols.contains(c)), "{out:?}");
    let mapped: String = s
        .chars()
        .map(|c| SYMBOLS.iter().find(|(u, _)| *u == c).map_or(c.to_string(), |(_, a)| a.to_string()))
        .collect();
    assert_eq!(out, mapped);
    assert_eq!(ascii(&out), out, "a second pass changes nothing");
    if s.is_ascii() {
        assert!(matches!(out, Cow::Borrowed(_)));
    }
}

/// Without a terminal to ask on, a change goes ahead only with `--yes`, a dry run never; an
/// erase only with its name typed, `--yes` or not. Each outcome's status is the one its
/// name says.
#[hegel::test(test_cases = 200)]
fn changes_go_ahead_only_when_confirmed(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let yes = tc.draw(gs::booleans());
    // `--json` never prompts, so nothing here waits on stdin.
    let out = Output { json: true, quiet: tc.draw(gs::booleans()), yes, pager: tc.draw(gs::booleans()) };
    let dry_run = tc.draw(gs::booleans());
    let decided = out.decide(dry_run, "Go ahead?");
    let expected = match (dry_run, yes) {
        (true, _) => Some(Outcome::DryRun),
        (false, true) => None,
        (false, false) => Some(Outcome::NotConfirmed),
    };
    assert_eq!(decided, expected);
    assert_eq!(out.confirm("Go ahead?"), yes);
    let name = tc.draw(gs::sampled_from(&["KEYCHAIN", "Rekordbox", "sd card"][..]));
    let given: Option<String> = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => None,
        1 => Some(tc.draw(gs::text().alphabet("abcdeKEYCHAINrekordbx ").max_size(12))),
        _ => Some(tc.draw(gs::sampled_from(&[" keychain ", "REKORDBOX", "SD CARD", "sd card"][..])).to_string()),
    };
    let typed = out.confirm_typed("", name, given.as_deref());
    let expected = match &given {
        Some(g) if g.trim().eq_ignore_ascii_case(name.trim()) => None,
        Some(_) => Some(Outcome::Declined),
        None => Some(Outcome::NotConfirmed),
    };
    assert_eq!(typed, expected, "{given:?} for {name}");
    for o in [decided, typed].into_iter().flatten() {
        let ok = matches!(o.name(), "applied" | "nothing_to_do" | "dry_run");
        assert_eq!(o.code() == 0, ok, "{}", o.name());
        let v = envelope(&serde_json::json!({}), &serde_json::Value::Null, false, o);
        assert_eq!(with_schema(v)["outcome"], o.name());
    }
}

/// Any sequence of progress events keeps the bar a model says: each phase's bar from its start
/// to its end (or the next phase's start), named for the phase, counting the items it was told
/// it has (or the bytes, when it has a size; a spinner when it has neither) and showing the
/// latest item's file name; warnings change nothing.
#[hegel::test(test_cases = 200)]
fn progress_bars_follow_their_events(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let phases = [
        Phase::Scan,
        Phase::Hash,
        Phase::Copy,
        Phase::Write,
        Phase::Delete,
        Phase::Verify,
        Phase::Format,
        Phase::Fingerprint,
        Phase::Index,
        Phase::Listen,
        Phase::Match,
        Phase::Analyze,
    ];
    let p = CliProgress { bar: Mutex::new(None), enabled: true };
    // (label, counts items, position, message), as the bar should have them.
    let mut model: Option<(&str, bool, u64, String)> = None;
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(20)) {
        let phase = phases[tc.draw(gs::integers::<usize>().max_value(phases.len() - 1))];
        let event = match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 => {
                let items = tc.draw(gs::integers::<u64>().max_value(4));
                let bytes = tc.draw(gs::sampled_from(&[0u64, 0, 1000][..]));
                model = Some((phase_label(phase), bytes == 0 && items > 0, 0, String::new()));
                Event::PhaseStart { phase, items, bytes }
            }
            1 => {
                let name = format!("{}.mp3", tc.draw(gs::sampled_from(&["a", "Café", "日本"][..])));
                let path = match tc.draw(gs::booleans()) {
                    true => format!("Contents/Artist/{name}"),
                    false => name.clone(),
                };
                if let Some((_, counts, position, message)) = &mut model {
                    *message = name;
                    if *counts {
                        *position += 1;
                    }
                }
                Event::Item { phase, path, bytes: 10 }
            }
            2 => {
                let bytes = tc.draw(gs::integers::<u64>().max_value(300));
                if let Some((_, false, position, _)) = &mut model {
                    *position += bytes;
                }
                Event::Bytes { phase, bytes }
            }
            3 => {
                model = None;
                Event::PhaseEnd { phase }
            }
            _ => Event::Warning { message: "skipped one".into() },
        };
        p.on_event(&event);
        let bar = p.bar.lock().unwrap().as_ref().map(|(b, counts)| (b.prefix(), *counts, b.position(), b.message()));
        let want = model.as_ref().map(|(l, c, n, m)| (l.to_string(), *c, *n, m.clone()));
        assert_eq!(bar, want, "after {event:?}");
    }
}

/// Every outcome exits with the status `locallibrary --help` gives it, and `--json` reports it by
/// its name.
#[hegel::test(test_cases = 50)]
fn every_outcome_exits_as_the_help_says(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let all = [
        (Outcome::Applied, "applied", 0),
        (Outcome::NothingToDo, "nothing_to_do", 0),
        (Outcome::DryRun, "dry_run", 0),
        (Outcome::Declined, "declined", 3),
        (Outcome::NotConfirmed, "not_confirmed", 3),
        (Outcome::Problems, "problems", 4),
        (Outcome::Partial, "partial", 5),
    ];
    let (o, name, status) = all[tc.draw(gs::integers::<usize>().max_value(all.len() - 1))];
    assert_eq!((o.name(), o.code()), (name, status));
    assert_eq!(o.exit(), ExitCode::from(status));
    let applied = tc.draw(gs::booleans());
    let v = with_schema(envelope(&serde_json::json!({ "x": 1 }), &serde_json::Value::Null, applied, o));
    assert_eq!((&v["outcome"], &v["applied"]), (&name.into(), &applied.into()));
}
