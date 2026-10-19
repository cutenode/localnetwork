use hegel::TestCase;
use hegel::generators as gs;
use loclib_plugin::{ProgressEvent, StepStarted};
use serde_json::{Value, json};

use super::*;
use crate::progress::{CancelToken, Phase, Progress, Scope};

/// A line a plugin might write to its stderr, and what of it is printable: drawn as text, with
/// control sequences (colours, cursor moves), strings (window titles, hyperlinks, ended by BEL or
/// ESC \, or never), two-character escapes and other control characters between, none of which
/// is printable. Tabs are.
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
                printable.push_str(&text);
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
            _ => line.push(tc.draw(gs::sampled_from(vec!['\r', '\u{0}', '\u{7}', '\u{8}', '\u{7f}', '\u{9b}']))),
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
