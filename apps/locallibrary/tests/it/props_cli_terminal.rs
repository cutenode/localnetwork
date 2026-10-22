//! Property tests of what the command line does in a shell and a terminal: Tab offers what the
//! listings list (playlists, My Tags, history lists, sets, sessions, libraries, models, steps),
//! the completion scripts and man pages are what `--json` says they are, a long listing paged is
//! the listing printed, and a question asked in a terminal is answered by what's typed.

use std::collections::BTreeSet;
use std::process::Command;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::Value;

use crate::support::{Env, path, play};

/// What Tab offers after `words`: each candidate with its description.
fn offered(fx: &Env, words: &[&str]) -> Vec<(String, String)> {
    let out = fx.command().env("COMPLETE", "fish").arg("--").arg("locallibrary").args(words).arg("").output().unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            let (v, h) = l.split_once('\t').unwrap_or((l, ""));
            (v.to_string(), h.to_string())
        })
        .collect()
}

/// The values offered, flags left out.
fn values(c: &[(String, String)]) -> BTreeSet<String> {
    c.iter().map(|(v, _)| v.clone()).filter(|v| !v.starts_with('-')).collect()
}

/// Playlists made, My Tags put on tracks, sessions recorded and sets declared, drawn: what Tab
/// offers for each kind of argument is what the listings list, with the counts they give.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn tab_offers_what_the_listings_list(tc: TestCase) {
    let fx = Env::from_template();
    let names = ["Friday", "Ünïcödé", "Warm-ups", "Sets/Closing"];
    for name in tc.draw(gs::subsequences(names.to_vec())) {
        let track = tc.draw(gs::sampled_from(&["1", "2"][..]));
        fx.ok(&["--yes", "playlist", "new", "A", name, "--track", track]);
    }
    for tag in tc.draw(gs::subsequences(vec!["Genre/Deep", "Situation/Warm-up"])) {
        fx.ok(&["--yes", "tags", "add", "A", tag, "--track", "1"]);
    }
    let club = fx.volumes.join("Club");
    for i in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        let tracks: Vec<u32> = tc.draw(gs::subsequences((1..=12).collect::<Vec<u32>>()));
        play(&club, &format!("HISTORY {:03}", i + 1), &tracks);
    }
    if tc.draw(gs::booleans()) {
        fx.ok(&["--yes", "sets", "new", "--title", "Friday", "--date", "2026-10-03"]);
    }
    // A library named unlike any drive.
    fx.ok(&["pull", "Club", "--library", "Clubland", "--new-library"]);

    // Naming the drive narrows playlists, tags and lists to its own.
    let drive = tc.draw(gs::sampled_from(&["A", "Club"][..]));
    let tab = offered(&fx, &["playlist", "add", drive]);
    let lists = fx.json(&["playlist", "ls", drive]);
    let lists = lists.as_array().unwrap();
    let by_id = |id: &Value| lists.iter().find(|p| p["id"] == *id);
    for p in lists {
        let mut parts = vec![p["name"].as_str().unwrap().to_string()];
        let mut up = p["parent_id"].clone();
        while let Some(parent) = by_id(&up) {
            parts.insert(0, parent["name"].as_str().unwrap().to_string());
            up = parent["parent_id"].clone();
        }
        let help = match p["is_folder"] == true {
            true => "folder".to_string(),
            false => format!("{} tracks", p["track_ids"].as_array().unwrap().len()),
        };
        assert!(tab.contains(&(parts.join("/"), help.clone())), "{} ({help}): {tab:?}", parts.join("/"));
    }
    if drive == "A" {
        let tab = offered(&fx, &["tags", "add", "A"]);
        let tags = fx.json(&["tags", "list", "A"]);
        for c in tags["categories"].as_array().unwrap() {
            assert!(tab.contains(&(c["name"].as_str().unwrap().into(), "category".into())), "{c}: {tab:?}");
            for t in c["tags"].as_array().unwrap() {
                let help = format!("{} tracks", t["tracks"].as_array().unwrap().len());
                assert!(tab.contains(&(t["path"].as_str().unwrap().into(), help)), "{t}: {tab:?}");
            }
        }
    }
    // Naming none offers every mounted library's.
    let everywhere = values(&offered(&fx, &["tracks", "--playlist"]));
    let each: BTreeSet<String> =
        ["A", "Club"].iter().flat_map(|d| values(&offered(&fx, &["playlist", "add", d]))).collect();
    assert_eq!(everywhere, each);
    let tab = offered(&fx, &["plays", "export", drive]);
    let history = fx.json(&["plays", "lists", drive]);
    let listed: BTreeSet<String> =
        history["drive"]["lists"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap().into()).collect();
    assert_eq!(values(&tab), listed, "{tab:?}");

    let tab = offered(&fx, &["sets", "edit"]);
    let sets: BTreeSet<String> =
        fx.json(&["sets"]).as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap().into()).collect();
    assert_eq!(values(&tab), sets, "{tab:?}");
    let tab = offered(&fx, &["sets", "new", "--from"]);
    let sessions = fx.json(&["plays", "lists", "--all"]);
    for s in sessions.as_array().into_iter().flatten().flat_map(|d| d["sessions"].as_array().into_iter().flatten()) {
        assert!(values(&tab).contains(&s["id"].as_str().unwrap()[..8]), "{s}: {tab:?}");
    }
    let tab = offered(&fx, &["pull", "--library"]);
    let libraries: BTreeSet<String> = fx
        .json(&["library", "ls"])
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|l| l["name"].as_str().map(String::from))
        .collect();
    assert_eq!(values(&tab), libraries, "{tab:?}");
    let tab = offered(&fx, &["players", "set"]);
    let models: BTreeSet<String> = fx
        .json(&["players", "models"])
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap().into())
        .collect();
    assert_eq!(values(&tab), models, "{tab:?}");
    let tab = offered(&fx, &["refresh", "--skip"]);
    assert!(!tab.is_empty() && !values(&tab).contains("check") && !values(&tab).contains("eject"), "{tab:?}");

    // A drive the store knows that isn't plugged in is offered as such.
    let gone = tc.draw(gs::booleans());
    if gone {
        std::fs::rename(fx.volumes.join("A"), fx.outside("A-unplugged")).unwrap();
    }
    let tab = offered(&fx, &["sync"]);
    let a = tab.iter().find(|(v, _)| v == "A").unwrap_or_else(|| panic!("{tab:?}"));
    assert_eq!(a.1 == "not mounted" || a.1 == "library", gone, "{tab:?}");
    assert!(tab.contains(&("Clubland".into(), "library".into())), "{tab:?}");
}

/// The man pages: the top-level page printed is the one `--json` holds, and written to a folder,
/// there's a page for every command, the top-level one among them; a folder they can't be written
/// to is an error that names it.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(nested = false, json = true, file_in_the_way = true)]
#[hegel::explicit_test_case(nested = true, json = false, file_in_the_way = false)]
fn man_pages_are_written_or_printed(tc: TestCase) {
    let fx = Env::from_template();
    let printed = fx.ok(&["man"]);
    let v = fx.json(&["man"]);
    assert_eq!(v["page"].as_str(), Some(printed.as_str()));
    assert!(printed.starts_with(".ie"), "{}", &printed[..80.min(printed.len())]);
    let nested = tc.draw(gs::booleans());
    let dir = fx.home(if nested { "share/man/man1" } else { "man" });
    let json = tc.draw(gs::booleans());
    let r = fx.run(&[if json { "--json" } else { "--quiet" }, "man", "-o", path(&dir)]);
    assert_eq!(r.code, 0, "{}", r.err);
    let pages: Vec<String> =
        std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into()).collect();
    assert!(pages.contains(&"locallibrary.1".to_string()) && pages.len() > 40, "{pages:?}");
    assert!(!pages.iter().any(|p| p.contains("-help")), "no page for `help`: {pages:?}");
    match json {
        true => assert_eq!(serde_json::from_str::<Value>(&r.out).unwrap()["report"]["written"], path(&dir)),
        false => assert!(r.out.contains(&format!("wrote the man pages to {}", dir.display())), "{}", r.out),
    }

    // A folder that can't be made (a file is where it would go), or a page that can't be written
    // (a folder is where it would go), is an error naming the folder, whatever else was written.
    let file_in_the_way = tc.draw(gs::booleans());
    let blocked = match file_in_the_way {
        true => {
            std::fs::write(fx.home("in the way"), "a file").unwrap();
            fx.home("in the way/man")
        }
        false => {
            std::fs::create_dir_all(fx.home("taken/locallibrary.1")).unwrap();
            fx.home("taken")
        }
    };
    let r = fx.run(&[if json { "--json" } else { "--quiet" }, "man", "-o", path(&blocked)]);
    assert_eq!(r.code, 1, "{}\n{}", r.out, r.err);
    let said = match json {
        true => serde_json::from_str::<Value>(&r.out).unwrap()["error"]["message"].as_str().unwrap().to_string(),
        false => r.err,
    };
    let what = if file_in_the_way { "making" } else { "writing to" };
    assert!(said.contains(&format!("{what} {}", blocked.display())), "{said}");
}

/// For any shell, the script printed is the one `--json` holds; the dynamic one has the shell run
/// this binary on Tab, by its full path unless it was found on the PATH.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn completion_scripts_run_the_binary_found(tc: TestCase) {
    let fx = Env::from_template();
    let shell = tc.draw(gs::sampled_from(&["bash", "zsh", "fish", "elvish", "powershell"][..]));
    let fixed = tc.draw(gs::booleans());
    let mut args = vec!["completions", shell];
    if fixed {
        args.push("--static");
    }
    let script = fx.ok(&args);
    let v = fx.json(&args);
    assert_eq!(
        (v["script"].as_str(), v["shell"].as_str(), v["static"].as_bool()),
        (Some(script.as_str()), Some(shell), Some(fixed))
    );
    // Looked up when the test runs, as in completion_candidates.
    let exe = assert_cmd::cargo::cargo_bin("locallibrary");
    if fixed {
        assert!(!script.contains("COMPLETE"), "a static script doesn't ask the binary");
        return;
    }
    // The script quotes the path for its shell, which escapes a Windows path's backslashes.
    #[cfg(unix)]
    assert!(script.contains(exe.to_str().unwrap()), "{script}");
    let mut by_name = Command::new("locallibrary");
    by_name
        .env("PATH", exe.parent().unwrap())
        .env("LOCALLIBRARY_STORE", &fx.store)
        .env("LOCALLIBRARY_VOLUMES", &fx.volumes)
        .args(&args);
    let found = String::from_utf8(by_name.output().unwrap().stdout).unwrap();
    assert!(found.contains("locallibrary") && !found.contains(exe.to_str().unwrap()), "{found}");
}

/// `s` without the terminal's colour codes, and with its line ends as a file has them.
#[cfg(unix)]
fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out.replace("\r\n", "\n")
}

/// Listings that page, run with their stdout a terminal and any pager setting: through a pager
/// (`$PAGER`, run with `LESS=FRX` unless `LESS` is set) the pager is given the listing as it's
/// printed piped; with the pager off (`--no-pager`, `PAGER=cat` or empty) the terminal shows it.
#[cfg(unix)]
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn a_paged_listing_is_the_listing(tc: TestCase) {
    use std::io::Read;
    use std::process::Stdio;

    use crate::support::terminal;
    let fx = Env::from_template();
    let listing = tc.draw(gs::sampled_from(
        &[
            &["players", "models"][..],
            &["tracks", "Club"],
            &["playlist", "ls", "A"],
            &["sets"],
            &["plays", "Club"],
            &["tags", "A"],
        ][..],
    ));
    let piped = fx.ok(listing);
    let (less, paged) = (fx.home("less"), fx.home("paged"));
    let to_file = format!("printf %s \"$LESS\" > '{}'; cat > '{}'", less.display(), paged.display());
    let how = tc.draw(gs::integers::<u8>().max_value(3));
    let user_less = tc.draw(gs::booleans());
    let (mut screen_end, stdout) = terminal();
    let mut c = fx.command();
    c.env_remove("LESS");
    if user_less {
        c.env("LESS", "S");
    }
    match how {
        0 => c.env("PAGER", &to_file),
        1 => c.env("PAGER", &to_file).arg("--no-pager"),
        2 => c.env("PAGER", "cat"),
        _ => c.env("PAGER", " "),
    };
    let mut child = c.args(listing).stdout(Stdio::from(stdout)).stderr(Stdio::null()).spawn().unwrap();
    drop(c);
    let mut screen = Vec::new();
    let _ = screen_end.read_to_end(&mut screen);
    assert!(child.wait().unwrap().success());
    let screen = plain(&String::from_utf8_lossy(&screen));
    if how == 0 {
        assert_eq!(plain(&std::fs::read_to_string(&paged).unwrap()), piped, "{listing:?}");
        assert_eq!(std::fs::read_to_string(&less).unwrap(), if user_less { "S" } else { "FRX" });
        assert!(!screen.contains(piped.lines().last().unwrap_or("")) || piped.is_empty(), "{screen}");
    } else {
        assert!(!paged.exists());
        assert_eq!(screen, piped, "{listing:?}");
    }
}

/// The top-level help in a terminal of any width lists the commands it lists piped, word for
/// word, each description wrapped to the terminal (at most 100 columns), but never narrower than
/// 30 columns.
#[cfg(unix)]
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(columns = 40u16)]
#[hegel::explicit_test_case(columns = 160u16)]
fn help_fits_the_terminal_it_is_shown_in(tc: TestCase) {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::process::Stdio;

    use crate::support::terminal;
    let fx = Env::from_template();
    let columns = tc.draw(gs::integers::<u16>().min_value(20).max_value(200));
    let (mut screen_end, stdout) = terminal();
    let size = libc::winsize { ws_row: 40, ws_col: columns, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: TIOCSWINSZ only reads the size given, for a terminal this test opened.
    assert_eq!(unsafe { libc::ioctl(stdout.as_raw_fd(), libc::TIOCSWINSZ, &size) }, 0);
    let mut c = fx.command();
    let mut child = c.arg("--help").stdout(Stdio::from(stdout)).stderr(Stdio::null()).spawn().unwrap();
    drop(c);
    let mut screen = Vec::new();
    let _ = screen_end.read_to_end(&mut screen);
    assert!(child.wait().unwrap().success());
    // The command list: from the usage line to the options.
    let commands = |help: &str| -> Vec<String> {
        help.lines()
            .skip_while(|l| !l.starts_with("Usage:"))
            .skip(1)
            .take_while(|l| *l != "Options:")
            .map(String::from)
            .collect()
    };
    let (shown, piped) = (commands(&plain(&String::from_utf8_lossy(&screen))), commands(&fx.ok(&["--help"])));
    let words =
        |lines: &[String]| lines.iter().flat_map(|l| l.split_whitespace().map(String::from)).collect::<Vec<_>>();
    assert_eq!(words(&shown), words(&piped), "{shown:#?}");
    // Where the descriptions start: a wrapped one's next line is indented that far.
    let indent = piped.iter().filter(|l| l.starts_with("   ")).map(|l| l.len() - l.trim_start().len()).min().unwrap();
    let fits = usize::from(columns).min(100).max(indent + 30);
    for line in &shown {
        assert!(line.chars().count() <= fits, "{line:?} is wider than {fits} ({columns} columns)");
    }
}

/// A change asked about in a terminal: typing yes makes it, anything else declines it (exit
/// status 3) and leaves the drive as it was.
#[cfg(unix)]
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_question_is_answered_by_what_is_typed(tc: TestCase) {
    use std::io::Write;
    use std::process::Stdio;

    use crate::support::terminal;
    let fx = Env::from_template();
    let answer = tc.draw(gs::sampled_from(&["y", "yes", "Y", "n", "", "nope"][..]));
    let (mut keyboard, stdin) = terminal();
    let child = fx
        .command()
        .args(["playlist", "new", "A", "Friday"])
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(keyboard, "{answer}").unwrap();
    let out = child.wait_with_output().unwrap();
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(stderr.contains("Make these changes? [y/N]"), "{stderr}");
    let yes = ["y", "yes", "Y"].contains(&answer);
    assert_eq!(out.status.code(), Some(if yes { 0 } else { 3 }), "{answer:?}\n{stdout}\n{stderr}");
    let names: BTreeSet<String> = fx
        .json(&["playlist", "ls", "A"])
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().into())
        .collect();
    assert_eq!(names.contains("Friday"), yes, "{names:?}");
}

/// `set` was the night-before job before it was a spelling of `sets`: a command line that reads
/// as the job (`set` and a drive, or `set`/`sets` and one of the job's flags) is told where the
/// job went, and any other mistake with `sets` gets clap's own message; both are usage errors.
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn the_night_before_job_is_said_to_have_moved(tc: TestCase) {
    let fx = Env::from_template();
    let a = path(&fx.volumes.join("A")).to_string();
    let first = tc.draw(gs::sampled_from(&["set", "sets"][..]));
    let flags = ["--no-eject", "--players=CDJ-3000", "--audio", "--library", "-l", "--verbose", "-v", "--dry-run"];
    let (rest, job): (Vec<String>, bool) = match tc.draw(gs::integers::<u8>().max_value(3)) {
        // A drive: the job, after `set` only (`sets` and a drive is more likely a set's name).
        0 => {
            let drive = tc.draw(gs::sampled_from(&["A", "Club", a.as_str()][..])).to_string();
            (vec![drive], first == "set")
        }
        1 => (vec![tc.draw(gs::sampled_from(&flags[..])).to_string()], true),
        2 => (vec![tc.draw(gs::sampled_from(&["--nonsense", "Nope"][..])).to_string()], false),
        // After a subcommand of `sets`, a flag is that subcommand's business.
        _ => (vec!["show".into(), "--no-eject".into()], false),
    };
    let json = tc.draw(gs::booleans());
    let mut args: Vec<&str> = if json { vec!["--json", first] } else { vec![first] };
    args.extend(rest.iter().map(String::as_str));
    let r = fx.run(&args);
    assert_eq!(r.code, 2, "{args:?}\n{}\n{}", r.out, r.err);
    let said = match json {
        true => serde_json::from_str::<Value>(&r.out).unwrap()["error"]["message"].as_str().unwrap().to_string(),
        false => r.err.clone(),
    };
    assert_eq!(said.contains("is `locallibrary prep` now"), job, "{args:?}: {said}");
}

/// `--color always` colours what's printed even into a pipe, `never` never does, and `auto` only
/// for a terminal (so not here); the words and columns are the same either way, with ASCII
/// symbols or not, and some listing is coloured.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn colour_is_what_color_says(tc: TestCase) {
    let fx = Env::from_template();
    let listings: [&[&str]; 14] = [
        &["profile", "sections"],
        &["profile", "Club"],
        &["profile", "prefs"],
        &["playlist", "ls", "A"],
        &["tags", "A"],
        &["tracks", "Club"],
        &["track", "matches", "Club", "1"],
        &["cues", "A", "--track", "1"],
        &["sets"],
        &["plays", "A"],
        &["plays", "stats", "Club"],
        &["players", "models"],
        &["undo", "--list"],
        &["account"],
    ];
    // `profile` says when it was built, to the minute, so runs either side of a minute's turn
    // differ there: that's left out.
    #[cfg(unix)]
    let unstamped =
        |s: &str| s.lines().map(|l| l.split_once(" built ").map_or(l, |(head, _)| head)).collect::<Vec<_>>().join("\n");
    let mut coloured_any = false;
    for listing in tc.draw(gs::permutations(&listings[..])) {
        let ascii: &[&str] = if tc.draw(gs::booleans()) { &["--ascii"] } else { &[] };
        #[cfg(unix)]
        let piped = unstamped(&fx.ok(&[ascii, listing].concat()));
        for when in tc.draw(gs::permutations(vec!["always", "never", "auto"])) {
            let coloured = fx.ok(&[ascii, &["--color", when], listing].concat());
            // Only `always` colours (a listing with nothing styled has no colour to show).
            assert!(when == "always" || !coloured.contains("\u{1b}["), "{listing:?} {when}: {coloured:?}");
            coloured_any |= coloured.contains("\u{1b}[");
            #[cfg(unix)]
            assert_eq!(unstamped(&plain(&coloured)), piped, "{listing:?} {when}");
        }
    }
    assert!(coloured_any);
}

/// The locale says whether the terminal takes UTF-8: the first of `LC_ALL`, `LC_CTYPE` and
/// `LANG` that's set decides, and one that doesn't name UTF-8 gets ASCII stand-ins for the
/// symbols (as `--ascii` does); with none set, UTF-8 is taken.
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn the_locale_decides_the_symbols(tc: TestCase) {
    let fx = Env::from_template();
    let values = ["en_GB.UTF-8", "de_DE.utf8", "C", "en_US.ISO-8859-1", ""];
    let mut c = fx.command();
    let mut first: Option<&str> = None;
    for var in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Some(v) = tc.draw(gs::optional(gs::sampled_from(&values[..]))) {
            c.env(var, v);
            if first.is_none() && !v.is_empty() {
                first = Some(v);
            }
        }
    }
    let out = c.args(["profile", "sections"]).output().unwrap();
    let out = String::from_utf8(out.stdout).unwrap();
    let utf8 = first.is_none_or(|v| v.to_lowercase().contains("utf-8") || v.to_lowercase().contains("utf8"));
    assert_eq!(out.contains('●'), utf8, "{first:?}: {out}");
    assert_eq!(out.contains("  * summary"), !utf8, "{first:?}: {out}");
    assert_eq!(out, fx.ok(&[if utf8 { "--quiet" } else { "--ascii" }, "profile", "sections"]), "{first:?}");
}
