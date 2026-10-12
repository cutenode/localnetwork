use super::*;
use hegel::Generator;
use hegel::generators as gs;

fn key(s: &str) -> Camelot {
    Camelot::parse(s).unwrap()
}

#[test]
fn reads_giantsteps_annotations() {
    assert_eq!(parse_key("C minor").unwrap(), key("5A"));
    assert_eq!(parse_key("Eb major\n").unwrap(), key("5B"));
    assert!(matches!(parse_key("unknown"), Err(Error::Annotation { line: 1, .. })));
    assert_eq!(parse_tempo("126").unwrap(), Some(126.0));
    assert_eq!(parse_tempo("127.0\n").unwrap(), Some(127.0));
    assert_eq!(parse_tempo("0.0").unwrap(), None);
    assert!(parse_tempo("fast").is_err());
    assert!(parse_tempo("-5").is_err());
    assert!(parse_tempo("inf").is_err());
}

#[test]
fn reads_the_mtg_key_table() {
    let text = "ID\tMANUAL KEY\tC\n5061\tD# minor\t2\n10981\t-\t0\n\n7\tF minor / G# major\t1\n8\tG minor/major\t1\n";
    let rows = parse_mtg_keys(text).unwrap();
    assert_eq!(rows[0], MtgKey { id: "5061".into(), keys: vec![key("D#m")], confidence: 2 });
    assert_eq!(rows[1].keys, []);
    assert_eq!(rows[2].keys, [key("Fm"), key("G#")]);
    assert_eq!(rows[3].keys, [key("Gm"), key("G")]);
    let err = |text| match parse_mtg_keys(text) {
        Err(Error::Annotation { line, message }) => (line, message),
        other => panic!("{other:?}"),
    };
    assert_eq!(err("ID\tMANUAL KEY\tC\n1\tC minor\n").0, 2);
    assert_eq!(err("1\tC minor\thigh\n").1, "the confidence isn't 0, 1 or 2");
    assert_eq!(err("1\tH minor\t2\n").1, "unreadable key");
}

#[test]
fn keys_score_as_mirex_does() {
    let am = key("Am");
    let of = |e: &str| KeyOutcome::of(am, Camelot::parse(e));
    assert_eq!(of("Am"), KeyOutcome::Correct);
    assert_eq!(of("Em"), KeyOutcome::Fifth);
    assert_eq!(of("Dm"), KeyOutcome::Fifth);
    assert_eq!(of("C"), KeyOutcome::Relative);
    assert_eq!(of("A"), KeyOutcome::Parallel);
    assert_eq!(of("E"), KeyOutcome::Other);
    assert_eq!(of("Bbm"), KeyOutcome::Other);
    assert_eq!(of(""), KeyOutcome::Missing);
    let credits: Vec<f64> = KeyOutcome::ALL.iter().map(|o| o.credit()).collect();
    assert_eq!(credits, [1.0, 0.5, 0.3, 0.2, 0.0, 0.0]);
}

fn any_key(tc: &hegel::TestCase) -> Camelot {
    let (number, major) = tc.draw(gs::tuples!(gs::integers::<u8>().min_value(1).max_value(12), gs::booleans()));
    Camelot { number, major }
}

/// A weighted key score is between 0 and 1, 1 only for the right key, and the same whichever
/// of the two keys is the reference.
#[hegel::test(test_cases = 300)]
fn key_scores_are_bounded_and_symmetric(tc: hegel::TestCase) {
    let (a, b) = (any_key(&tc), any_key(&tc));
    let credit = KeyOutcome::of(a, Some(b)).credit();
    assert!((0.0..=1.0).contains(&credit));
    assert_eq!(credit == 1.0, a == b);
    assert_eq!(KeyOutcome::of(b, Some(a)), KeyOutcome::of(a, Some(b)));
}

#[test]
fn tempos_score_with_both_accuracies() {
    let of = |e| TempoOutcome::of(120.0, Some(e));
    assert_eq!(of(124.7), TempoOutcome::Correct);
    assert_eq!(of(125.0), TempoOutcome::Other);
    assert_eq!(of(240.0), TempoOutcome::Double);
    assert_eq!(of(61.0), TempoOutcome::Half);
    assert_eq!(of(360.0), TempoOutcome::Triple);
    assert_eq!(of(40.0), TempoOutcome::Third);
    assert_eq!(of(180.0), TempoOutcome::Other);
    assert_eq!(TempoOutcome::of(120.0, None), TempoOutcome::Missing);
    assert_eq!(TempoOutcome::of(120.0, Some(0.0)), TempoOutcome::Missing);
}

/// Accuracy2 is never below Accuracy1, both are percentages, and every track is counted once.
#[hegel::test(test_cases = 200)]
fn tempo_reports_add_up(tc: hegel::TestCase) {
    let pairs = tc.draw(gs::vecs(gs::tuples!(
        gs::floats::<f64>().min_value(60.0).max_value(200.0),
        gs::optional(gs::floats::<f64>().min_value(20.0).max_value(600.0))
    )));
    let rows = pairs.iter().enumerate().map(|(i, &(reference, estimate))| Row {
        analyzer: "a".into(),
        track: i.to_string(),
        reference,
        estimate,
    });
    let reports = score::<TempoOutcome>(rows);
    let Some(report) = reports.get("a") else { return assert!(pairs.is_empty()) };
    assert_eq!(report.counts.values().sum::<usize>(), pairs.len());
    let accuracy1 = report.percent(TempoOutcome::Correct);
    assert!((0.0..=100.0).contains(&accuracy1));
    assert!(report.accuracy2() >= accuracy1 && report.accuracy2() <= 100.0);
    assert!((report.score * 100.0 - accuracy1).abs() < 1e-9);
    assert_eq!(report.misses.len(), pairs.len() - report.count(TempoOutcome::Correct));
}

#[test]
fn an_empty_report_scores_zero() {
    let report = Report::<KeyOutcome>::new();
    assert_eq!((report.score, report.percent(KeyOutcome::Correct)), (0.0, 0.0));
    assert_eq!(Report::<TempoOutcome>::new().accuracy2(), 0.0);
}

/// Tonics as the GiantSteps annotations spell them.
const TONICS: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// `key` as a GiantSteps annotation writes it: `C# minor`.
fn spelled(key: Camelot) -> String {
    format!("{} {}", TONICS[key.pitch() as usize], if key.major { "major" } else { "minor" })
}

/// Any key written as GiantSteps writes it reads back as that key, with any blanks around
/// it; any tempo written out reads back, 0 as none, and a negative or unreadable one is an
/// error on line 1.
#[hegel::test(test_cases = 300)]
fn annotations_read_back_what_was_written(tc: hegel::TestCase) {
    let key = any_key(&tc);
    let pad = |tc: &hegel::TestCase| tc.draw(gs::sampled_from(vec!["", " ", "\n", "\t \n"]));
    assert_eq!(parse_key(&format!("{}{}{}", pad(&tc), spelled(key), pad(&tc))).unwrap(), key);
    let bpm = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(1e6));
    let read = parse_tempo(&format!("{}{bpm}{}", pad(&tc), pad(&tc))).unwrap();
    assert_eq!(read, (bpm > 0.0).then_some(bpm));
    let bad = tc.draw(gs::one_of(vec![
        gs::floats::<f64>().max_value(-1e-9).map(|v| v.to_string()).boxed(),
        gs::sampled_from(vec!["NaN".to_string(), "inf".into(), "fast".into(), String::new()]).boxed(),
    ]));
    assert!(matches!(parse_tempo(&bad), Err(Error::Annotation { line: 1, .. })), "{bad:?}");
}

/// The MTG table reads back the rows it was written from, whatever blank lines and header
/// it has; a row without its three fields is an error on its own line.
#[hegel::test(test_cases = 200)]
fn the_mtg_table_reads_back_its_rows(tc: hegel::TestCase) {
    let n = tc.draw(gs::integers::<usize>().max_value(8));
    let rows: Vec<MtgKey> = (0..n)
        .map(|i| MtgKey {
            id: format!("{}", 1000 + i),
            keys: (0..tc.draw(gs::integers::<usize>().max_value(2))).map(|_| any_key(&tc)).collect(),
            confidence: tc.draw(gs::integers::<u8>().max_value(2)),
        })
        .collect();
    let mut lines = Vec::new();
    if tc.draw(gs::booleans()) {
        lines.push("ID\tMANUAL KEY\tC".to_string());
    }
    for r in &rows {
        if tc.draw(gs::booleans()) {
            lines.push(String::new());
        }
        let keys = match &r.keys[..] {
            [] => "-".to_string(),
            keys => keys.iter().map(|k| spelled(*k)).collect::<Vec<_>>().join(" / "),
        };
        lines.push(format!("{}\t{keys}\t{}", r.id, r.confidence));
    }
    let text = lines.join("\n");
    assert_eq!(parse_mtg_keys(&text).unwrap(), rows);
    let broken = tc.draw(gs::integers::<usize>().max_value(lines.len()));
    lines.insert(broken, "1\tC minor".into());
    assert!(matches!(parse_mtg_keys(&lines.join("\n")), Err(Error::Annotation { line, .. }) if line == broken + 1));
}

/// Key reports count every track once under its analyzer, in the order analyzers first
/// answer; the score is the mean credit, and the misses are the tracks not fully right.
#[hegel::test(test_cases = 200)]
fn key_reports_add_up(tc: hegel::TestCase) {
    let n = tc.draw(gs::integers::<usize>().max_value(30));
    let rows: Vec<Row<Camelot>> = (0..n)
        .map(|i| Row {
            analyzer: tc.draw(gs::sampled_from(vec!["a", "b", "c"])).to_string(),
            track: i.to_string(),
            reference: any_key(&tc),
            estimate: tc.draw(gs::booleans()).then(|| any_key(&tc)),
        })
        .collect();
    let reports = score::<KeyOutcome>(rows.clone());
    let mut order: Vec<&str> = Vec::new();
    for r in &rows {
        if !order.contains(&r.analyzer.as_str()) {
            order.push(&r.analyzer);
        }
    }
    assert_eq!(reports.keys().map(String::as_str).collect::<Vec<_>>(), order);
    for (analyzer, report) in &reports {
        let mine: Vec<&Row<Camelot>> = rows.iter().filter(|r| &r.analyzer == analyzer).collect();
        assert_eq!((report.tracks, report.counts.values().sum::<usize>()), (mine.len(), mine.len()));
        let credit: f64 = mine.iter().map(|r| KeyOutcome::of(r.reference, r.estimate).credit()).sum();
        assert!((report.score - credit / mine.len() as f64).abs() < 1e-12);
        let missed: Vec<&str> = mine
            .iter()
            .filter(|r| KeyOutcome::of(r.reference, r.estimate) != KeyOutcome::Correct)
            .map(|r| r.track.as_str())
            .collect();
        assert_eq!(report.misses.iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), missed);
        let percents: f64 = KeyOutcome::ALL.iter().map(|o| report.percent(*o)).sum();
        assert!((percents - 100.0).abs() < 1e-9);
    }
}

/// A row of the MTG table with a field that can't be read is an error on its own line, saying
/// which: a confidence that isn't a number, a key that isn't one, or no key. A second key
/// written as only its mode (`G minor/major`) is on the first's tonic. A GiantSteps key that
/// isn't one is an error on line 1. No rows make no report, and a report of no tracks
/// scores 0 for every outcome.
#[hegel::test(test_cases = 200)]
fn unreadable_annotations_say_where_and_what(tc: hegel::TestCase) {
    let key = any_key(&tc);
    let tonic = TONICS[key.pitch() as usize];
    let blanks = tc.draw(gs::integers::<usize>().max_value(3));
    let row = tc.draw(gs::sampled_from(vec!["both modes", "a word", "no key", "not a key"]));
    let other = if key.major { "minor" } else { "major" };
    let line = match row {
        "both modes" => format!("1\t{}/{other}\t2", spelled(key)),
        "a word" => format!("1\t{}\thigh", spelled(key)),
        "no key" => "1\t\t2".to_string(),
        _ => format!("1\tH{} {other}\t2", &tonic[1..]),
    };
    let text = format!("ID\tMANUAL KEY\tC\n{}{line}\n", "\n".repeat(blanks));
    match (row, parse_mtg_keys(&text)) {
        ("both modes", Ok(rows)) => {
            let both = Camelot::parse(&format!("{tonic} {other}")).unwrap();
            assert_eq!(rows, [MtgKey { id: "1".into(), keys: vec![key, both], confidence: 2 }]);
        }
        ("a word", Err(Error::Annotation { line, message })) => {
            assert_eq!((line, message.as_str()), (blanks + 2, "the confidence isn't 0, 1 or 2"));
        }
        ("no key" | "not a key", Err(Error::Annotation { line, message })) => {
            assert_eq!((line, message.as_str()), (blanks + 2, "unreadable key"), "{row}");
        }
        (row, other) => panic!("{row}: {other:?}"),
    }
    let garbage = tc.draw(gs::sampled_from(vec!["", "H minor", "minor", "C sharp"]));
    assert!(matches!(parse_key(garbage), Err(Error::Annotation { line: 1, message }) if message.contains(garbage)));
    assert!(score::<KeyOutcome>(Vec::new()).is_empty());
    let outcome = tc.draw(gs::sampled_from(KeyOutcome::ALL.to_vec()).print_as_debug());
    assert_eq!(Report::<KeyOutcome>::new().percent(outcome), 0.0);
    let tempo = tc.draw(gs::sampled_from(TempoOutcome::ALL.to_vec()).print_as_debug());
    assert_eq!(Report::<TempoOutcome>::new().percent(tempo), 0.0);
}
