use hegel::TestCase;
use hegel::generators as gs;
use loclib_plugin::{ProgressEvent, StepStarted};
use serde_json::{Value, json};

use super::*;
use crate::progress::{CancelToken, Phase, Progress, Scope};

/// A line a plugin might write to its stderr, and what of it is printable: drawn as text, with
/// control sequences (colours, cursor moves), strings (window titles, hyperlinks, ended by BEL or
/// ESC \, or never), two-character escapes and other control characters between, none of which
/// is printable. Tabs and carriage returns print as a space.
#[hegel::test_helper]
fn stderr_line(tc: &TestCase) -> (String, String) {
    let plain = |tc: &TestCase| -> String {
        // Backslashes and tabs often: a string can end with ESC and one, and tabs print.
        let text = tc.draw(gs::sampled_from(vec!["", "\\", "\t"])).to_string() + &tc.draw(gs::text().max_size(6));
        text.chars().filter(|c| !c.is_control() || *c == '\t').collect()
    };
    let (mut line, mut printable) = (String::new(), String::new());
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
        match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 => {
                let text = plain(tc);
                line.push_str(&text);
                printable.push_str(&text.replace('\t', " "));
            }
            1 => {
                let params = tc.draw(gs::text().alphabet("0123456789;?").max_size(5));
                let last = tc.draw(gs::sampled_from(vec!['m', 'H', 'J', 'K', '@', '~', 'h']));
                line.push_str(&format!("\u{1b}[{params}{last}"));
            }
            2 => {
                let kind = tc.draw(gs::sampled_from(vec![']', 'P', 'X', '^', '_']));
                let end = tc.draw(gs::sampled_from(vec!["\u{7}", "\u{1b}\\"]));
                line.push_str(&format!("\u{1b}{kind}{}{end}", plain(tc)));
            }
            3 => {
                let second = tc.draw(gs::sampled_from(vec!['7', '8', 'c', '=', '(', 'M']));
                line.push_str(&format!("\u{1b}{second}"));
            }
            _ => {
                let c = tc.draw(gs::sampled_from(vec!['\r', '\u{0}', '\u{7}', '\u{8}', '\u{7f}', '\u{9b}']));
                line.push(c);
                if c == '\r' {
                    printable.push(' ');
                }
            }
        }
    }
    // A sequence the line ends inside of takes the rest.
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => line.push_str("\u{1b}[12;3"),
        1 => line.push_str(&format!("\u{1b}]0;{}", plain(tc))),
        2 => line.push('\u{1b}'),
        _ => {}
    }
    (line, printable)
}

/// Whatever a plugin writes to its log, what reaches the host's is its printable text, in order:
/// nothing that would draw on a terminal UI's screen.
#[hegel::test(test_cases = 300)]
fn a_plugins_log_keeps_only_what_prints(tc: TestCase) {
    let (line, expected) = stderr_line(&tc);
    assert_eq!(printable(&line), expected, "{line:?}");
}

/// A JSON value a plugin might answer, its strings and keys drawn as [`stderr_line`]s, nested
/// `depth` deep at most, and that value with only what of each prints.
#[hegel::test_helper]
fn answer(tc: &TestCase, depth: u8) -> (Value, Value) {
    let kinds = if depth == 0 { 2 } else { 4 };
    match tc.draw(gs::integers::<u8>().max_value(kinds)) {
        0 => {
            let n = tc.draw(gs::integers::<i64>());
            (json!(n), json!(n))
        }
        1 | 2 => {
            let (said, printed) = stderr_line(tc);
            (json!(said), json!(printed))
        }
        3 => {
            let each: Vec<(Value, Value)> =
                (0..tc.draw(gs::integers::<usize>().max_value(3))).map(|_| answer(tc, depth - 1)).collect();
            let (said, printed) = each.into_iter().unzip::<_, _, Vec<_>, Vec<_>>();
            (Value::Array(said), Value::Array(printed))
        }
        _ => {
            let (mut said, mut printed) = (serde_json::Map::new(), serde_json::Map::new());
            for _ in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
                let ((key, printed_key), (value, printed_value)) = (stderr_line(tc), answer(tc, depth - 1));
                // Two keys that print the same are one: the last's value.
                if said.contains_key(&key) || printed.contains_key(&printed_key) {
                    continue;
                }
                said.insert(key, value);
                printed.insert(printed_key, printed_value);
            }
            (Value::Object(said), Value::Object(printed))
        }
    }
}

/// For any answer a plugin sends, its strings and keys with control characters among them, what
/// the host takes in is only what of each prints, in its place. Regression: only check findings,
/// notices and the plugin's log were made printable, and the rest reached the terminal as sent.
#[hegel::test(test_cases = 300)]
fn what_a_plugin_answers_is_taken_in_printable(tc: TestCase) {
    // In a list: an answer that's only text keeps its lines (see the property below).
    let (said, printed) = answer(&tc, 3);
    let (mut said, printed) = (json!([said]), json!([printed]));
    let sent = said.clone();
    printable_value(&mut said);
    assert_eq!(said, printed, "{sent}");
    let from_file: Value = printable_json(sent.to_string().as_bytes()).unwrap();
    assert_eq!(from_file, printed, "a blob's answer is taken in the same way");
}

/// For any text with newlines, carriage returns, tabs and escape sequences among its letters, a
/// text view's text and an answer that's only text (a headless command's) keep its lines, without
/// the rest, as they're drawn as those lines; anywhere else in an answer it's one line, a newline
/// a space. Regression: a text view's lines were made one.
#[hegel::test(test_cases = 200)]
fn text_drawn_as_lines_keeps_them(tc: TestCase) {
    let pieces =
        vec![("\n", "\n", " "), ("\r", "", " "), ("\t", " ", " "), ("\u{1b}[31m", "", ""), ("\u{1b}]0;t\u{7}", "", "")];
    let (mut said, mut lines, mut line) = (String::new(), String::new(), String::new());
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
        let letters: String = tc.draw(gs::from_regex("[a-z]{0,3}").fullmatch(true));
        let (piece, as_lines, as_line) = tc.draw(gs::sampled_from(pieces.clone()));
        said.push_str(&format!("{letters}{piece}"));
        lines.push_str(&format!("{letters}{as_lines}"));
        line.push_str(&format!("{letters}{as_line}"));
    }
    let mut answer = json!(said);
    printable_value(&mut answer);
    assert_eq!(answer, json!(lines), "an answer that's only text");
    let mut view = json!({ "kind": "sections", "items": [{ "title": said, "view": { "kind": "text", "title": said, "text": said } }] });
    printable_value(&mut view);
    let item = json!({ "title": line, "view": { "kind": "text", "title": line, "text": lines } });
    assert_eq!(view, json!({ "kind": "sections", "items": [item] }), "a text view's text, in a section");
    let mut list = json!({ "kind": "list", "text": said, "items": [{ "label": said }] });
    printable_value(&mut list);
    assert_eq!(list, json!({ "kind": "list", "text": line, "items": [{ "label": line }] }));
}

/// Progress events, each with the drive it's about.
#[derive(Default)]
struct Recorded(Mutex<Vec<(Option<String>, Event)>>);

impl Progress for Recorded {
    fn on_event(&self, event: &Event) {
        self.0.lock().unwrap().push((None, event.clone()));
    }

    fn on_scoped_event(&self, scope: &Scope, event: &Event) {
        self.0.lock().unwrap().push((scope.target.clone(), event.clone()));
    }

    fn on_step(&self, scope: &Scope) {
        panic!("a plugin's steps aren't passed on: {scope:?}");
    }
}

/// For any progress a plugin reports, the caller gets each event the host knows, about the drive
/// the plugin says, an item standing for several as that many (up to 10,000) and every other
/// event once; steps (the plugin's own names) and events from a newer plugin are left out.
#[hegel::test(test_cases = 100)]
fn a_plugins_progress_reaches_the_caller_as_it_says(tc: TestCase) {
    let (recorded, mut expected) = (Recorded::default(), Vec::new());
    let ctx = Ctx::new(&recorded, CancelToken::new());
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(5)) {
        let target = tc.draw(gs::optional(gs::sampled_from(vec!["USB", "SD"]))).map(String::from);
        let items = tc.draw(gs::optional(gs::sampled_from(vec![0, 1, 3, 9_999, 10_000, 10_001, u64::MAX])));
        let event = match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 => Progressed::Event(Event::Item { phase: Phase::Scan, path: "a.mp3".into(), bytes: 0 }),
            1 => Progressed::Event(Event::Warning { message: "odd".into() }),
            2 => Progressed::Event(Event::Bytes { phase: Phase::Scan, bytes: 7 }),
            3 => Progressed::Step(StepStarted::default()),
            _ => {
                let newer: Value = json!({ "event": "teleport", "where": "moon" });
                pass_on(ProgressReport::Unknown(newer), &ctx);
                continue;
            }
        };
        if let Progressed::Event(e) = &event {
            let times = match e {
                Event::Item { .. } => items.unwrap_or(1).min(10_000),
                _ => 1,
            };
            expected.extend((0..times).map(|_| (target.clone(), e.clone())));
        }
        pass_on(ProgressReport::Known(ProgressEvent { event, target, step: None, items }), &ctx);
    }
    let got = recorded.0.into_inner().unwrap();
    assert_eq!(got.len(), expected.len());
    // Events have no equality of their own: compared as they print.
    assert!(
        got.iter().zip(&expected).all(|(g, e)| format!("{g:?}") == format!("{e:?}")),
        "{:?}",
        &got[..got.len().min(5)]
    );
}

/// A string (a window title) ended by BEL is left out, and what follows it prints, a backslash
/// too: only ESC ends a string with one after it.
#[test]
fn a_backslash_after_a_string_ended_by_bel_prints() {
    assert_eq!(printable("\u{1b}]0;title\u{7}\\Music"), "\\Music");
}
