use super::*;

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn plist_runs_watch_and_escapes_paths() {
    let p = plist(
        Path::new("/Users/r&b/.cargo/bin/locallibrary"),
        &args(&["--store", "/Volumes/<NAS>", "watch", "--pull"]),
        Path::new("/Users/r&b/Library/Logs/locallibrary-watch.log"),
    );
    assert!(p.contains("<string>local.locallibrary.watch</string>"));
    assert!(p.contains(
        "    <string>/Users/r&amp;b/.cargo/bin/locallibrary</string>\n    <string>--store</string>\n    \
             <string>/Volumes/&lt;NAS&gt;</string>\n    <string>watch</string>\n    <string>--pull</string>\n  </array>"
    ));
    assert!(p.contains("<key>StandardErrorPath</key>\n  <string>/Users/r&amp;b/Library/Logs/locallibrary-watch.log"));
}

#[test]
fn unit_quotes_each_argument() {
    let u = unit(
        Path::new("/home/dj/.cargo/bin/locallibrary"),
        &args(&["--store", "/mnt/100% \"mine\" $HOME", "watch", "--pull"]),
    );
    assert!(u.contains(
        r#"ExecStart="/home/dj/.cargo/bin/locallibrary" "--store" "/mnt/100%% \"mine\" $$HOME" "watch" "--pull""#
    ));
    assert!(u.contains("WantedBy=default.target"));
}

use hegel::TestCase;
use hegel::generators as gs;

/// The text in `s` as XML has it.
fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// Any executable and arguments (markup, quotes, `%`, `$`, spaces and all) come back out of the
/// launchd agent's `ProgramArguments` as they went in, one `<string>` each, and nothing of them
/// is left unescaped to be read as markup.
#[hegel::test(test_cases = 200)]
fn plist_arguments_read_back(tc: TestCase) {
    let exe = tc.draw(gs::text().min_size(1).max_size(20));
    let args = tc.draw(gs::vecs(gs::text().max_size(20)).max_size(6));
    let log = tc.draw(gs::text().min_size(1).max_size(20));
    let want: Vec<String> = std::iter::once(Path::new(&exe).display().to_string()).chain(args.clone()).collect();
    // A line break would read as two lines: not something a path or flag holds.
    tc.assume(want.iter().chain([&log]).all(|a| !a.contains('\n') && !a.contains('\r')));
    let p = plist(Path::new(&exe), &args, Path::new(&log));
    let array = &p[p.find("<array>\n").unwrap() + 8..p.find("  </array>").unwrap()];
    let read: Vec<String> = array
        .lines()
        .map(|l| {
            let inner = l.strip_prefix("    <string>").and_then(|l| l.strip_suffix("</string>"));
            let inner = inner.unwrap_or_else(|| panic!("not one argument: {l:?}\n{p}"));
            assert!(!inner.contains('<') && !inner.contains('>'), "markup in an argument: {l:?}");
            unescape_xml(inner)
        })
        .collect();
    assert_eq!(read, want, "{p}");
    for key in ["StandardOutPath", "StandardErrorPath"] {
        let start = format!("<key>{key}</key>\n  <string>");
        let at = p.find(&start).unwrap_or_else(|| panic!("no {key}: {p}"));
        let value = &p[at + start.len()..];
        let value = &value[..value.find("</string>").unwrap()];
        assert!(!value.contains('<'), "markup in the log's path: {p}");
        assert_eq!(unescape_xml(value), Path::new(&log).display().to_string(), "{p}");
    }
}

/// systemd's `ExecStart` words, read back: double-quoted, a backslash escaping the next
/// character, `%%` and `$$` for a literal `%` and `$`.
fn exec_words(line: &str) -> Vec<String> {
    let (mut words, mut chars) = (Vec::new(), line.chars().peekable());
    while let Some(c) = chars.next() {
        match c {
            ' ' => {}
            '"' => {
                let mut w = String::new();
                loop {
                    match chars.next().expect("a closing quote") {
                        '"' => break,
                        '\\' => w.push(chars.next().expect("an escaped character")),
                        c @ ('%' | '$') => {
                            assert_eq!(chars.next(), Some(c), "a lone {c} in {line:?}");
                            w.push(c);
                        }
                        c => w.push(c),
                    }
                }
                words.push(w);
            }
            c => panic!("{c:?} outside quotes in {line:?}"),
        }
    }
    words
}

/// Any executable and arguments come back out of the systemd unit's `ExecStart` as they went in:
/// each quoted, with no `%` specifier or `$` variable left for systemd to expand.
#[hegel::test(test_cases = 200)]
fn unit_arguments_read_back(tc: TestCase) {
    let exe = tc.draw(gs::text().min_size(1).max_size(20));
    let args = tc.draw(gs::vecs(gs::text().max_size(20)).max_size(6));
    let want: Vec<String> = std::iter::once(Path::new(&exe).display().to_string()).chain(args.clone()).collect();
    // A line break would end the setting: not something a path or flag holds.
    tc.assume(want.iter().all(|a| !a.contains('\n') && !a.contains('\r')));
    let u = unit(Path::new(&exe), &args);
    let line = u.lines().find_map(|l| l.strip_prefix("ExecStart=")).unwrap_or_else(|| panic!("{u}"));
    assert_eq!(exec_words(line), want, "{u}");
}
