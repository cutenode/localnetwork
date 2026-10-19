//! Property tests of the questions commands ask in a terminal: `setup` answered at random (a
//! drive taken or left out, the library's name, which drive rekordbox exports to, the others'
//! roles, each change said yes or no to, answers it won't take) sets up what was answered and
//! nothing else, and `pull` into a library the store hasn't got starts it only when told to.

#![cfg(unix)]

use std::io::{Read, Write};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
use serde_json::Value;

use crate::props_cli_human::{FLAGS, State, build};
use crate::support::{Env, Run, Running, path, terminal};

/// `args` run in `fx` with a terminal to answer on: each question (the last on stderr, starting
/// `? `, once nothing more comes) is answered with what `answer` makes of it. The run, once it
/// ends (a question asked over and over ends it after a minute).
fn conversation(fx: &Env, args: &[&str], mut answer: impl FnMut(&str) -> String) -> Run {
    let (mut keyboard, stdin) = terminal();
    let mut command = fx.command();
    command.args(args).stdin(Stdio::from(stdin)).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = Running(command.spawn().unwrap());
    let err = Arc::new(Mutex::new(Vec::new()));
    let mut stderr = child.stderr.take().unwrap();
    let reading = {
        let err = err.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                err.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        })
    };
    let mut stdout = child.stdout.take().unwrap();
    let printing = std::thread::spawn(move || {
        let mut out = String::new();
        stdout.read_to_string(&mut out).unwrap();
        out
    });
    // How much of stderr there was when the last question was answered.
    let (started, mut answered, mut seen, mut quiet_since) = (Instant::now(), 0, 0, Instant::now());
    while child.try_wait().unwrap().is_none() && started.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(20));
        let text = String::from_utf8_lossy(&err.lock().unwrap()).to_string();
        if text.len() != seen {
            (seen, quiet_since) = (text.len(), Instant::now());
            continue;
        }
        // The answers aren't echoed to stderr: the questions follow one another on a line, each
        // `? ` at the start of the line or after the one before.
        let line = text.rsplit('\n').next().unwrap_or("");
        let start = (line.char_indices().rev().map(|(i, _)| i))
            .find(|&i| line[i..].starts_with("? ") && (i == 0 || line[..i].ends_with(' ')));
        let last = start.map_or("", |i| &line[i..]);
        let waiting = !last.is_empty() && last.ends_with(' ') && quiet_since.elapsed() > Duration::from_millis(150);
        if waiting && text.len() > answered {
            answered = text.len();
            writeln!(keyboard, "{}", answer(last)).unwrap();
        }
    }
    let _ = child.kill();
    let status = child.wait().unwrap();
    reading.join().unwrap();
    let err = String::from_utf8(err.lock().unwrap().clone()).unwrap();
    Run { out: printing.join().unwrap(), err, code: status.code().unwrap_or(-1) }
}

/// What a question offers when it's answered with nothing (`[Y/n]`, `[backup]`).
fn default_of(question: &str) -> Option<&str> {
    let start = question.rfind('[')?;
    Some(&question[start + 1..question[start..].find(']')? + start])
}

/// The library names `setup` may be told.
const NAMES: &[&str] = &["", "Home", "My library", "Club"];

/// `setup` in a terminal, answered at random, does what was answered: the library it was told to
/// start (or join) holds the drive said to be the one rekordbox exports to, as its primary, and
/// each other drive taken has the role it was given; the players and contact details go where
/// they were said yes to. Said no to, nothing of that is made.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(flags = vec![false; FLAGS], answers = vec![0u8; 40])]
#[hegel::explicit_test_case(flags = vec![true; FLAGS], answers = vec![1u8; 40])]
#[hegel::explicit_test_case(flags = vec![false; FLAGS], answers = vec![1u8; 40])]
#[hegel::explicit_test_case(flags = vec![false; FLAGS], answers = vec![2u8; 40])]
#[hegel::explicit_test_case(flags = vec![false; FLAGS], answers = vec![3u8; 40])]
// Club retired, taken and the contact details written: Club is kept as it is.
#[hegel::explicit_test_case(
    flags = (0..FLAGS).map(|i| [0, 9].contains(&i)).collect::<Vec<_>>(),
    answers = (0..40u8).map(|i| u8::from(i == 8)).collect::<Vec<_>>()
)]
// Club and its twin in Club2, both named Club, each taken and given a role: they're one drive,
// with the role given last.
#[hegel::explicit_test_case(
    flags = (0..FLAGS).map(|i| [11, 24].contains(&i)).collect::<Vec<_>>(),
    answers = [vec![0u8, 0, 1, 0, 0, 0, 0, 0, 0, 2], vec![0; 30]].concat()
)]
// Club and its twin: only the twin, in Club2, is taken, and only it gets the contact details.
#[hegel::explicit_test_case(
    flags = (0..FLAGS).map(|i| [8, 11, 24].contains(&i)).collect::<Vec<_>>(),
    answers = vec![
        0u8, 0, 127, 59, 0, 109, 0, 0, 209, 0, 0, 0, 0, 0, 15, 0, 0, 0, 0, 143, 116, 0, 0, 0, 192, 111, 0, 3, 103, 77,
        175, 201, 169, 128, 173, 232, 222, 154, 0, 0,
    ]
)]
fn setup_does_what_it_was_told(tc: TestCase) {
    let flags = tc.draw(gs::vecs(gs::booleans()).min_size(FLAGS).max_size(FLAGS));
    let answers = tc.draw(gs::vecs(gs::integers::<u8>()).min_size(40).max_size(40));
    let fx = build(State::from_flags(&flags));
    // Installed already, so setup never offers to install it (which would start it).
    let agent = match cfg!(target_os = "macos") {
        true => fx.home("Library/LaunchAgents/local.locallibrary.watch.plist"),
        false => fx.home(".config/systemd/user/locallibrary-watch.service"),
    };
    crate::props_cli_human::write(&agent, "");
    // Now and then a computer with only A plugged in, or with no store yet.
    match answers[0] % 3 {
        1 => std::fs::rename(fx.volumes.join("Club"), fx.home("club-drive")).unwrap(),
        2 => std::fs::remove_dir_all(&fx.store).unwrap(),
        _ => {}
    }
    let mut next = answers.into_iter().skip(1);
    let mut pick = move |n: usize| usize::from(next.next().unwrap_or(0)) % n;
    // The model: drives said to be copies, in the order asked, and the place each was asked in;
    // the library; the primary's place among them; the roles; whether the library was started.
    let (mut taken, mut library, mut primary) = (Vec::<String>::new(), None::<String>, 0usize);
    let (mut asked, mut taken_at) = (0usize, Vec::<usize>::new());
    // Each "Is ... a copy" question, and the drive name it's about.
    let mut questions = Vec::<(String, String)>::new();
    // A drive that shares its name with another is asked about with its folder, which is left
    // out of the name here.
    let in_volumes = format!(" ({}", fx.volumes.display());
    let (mut roles, mut saved, mut players, mut contact) = (Vec::<(String, String)>::new(), None, None, None);
    let mut contact_given = None::<String>;
    let mut models_given = None::<String>;
    let mut refused = false;
    let run = conversation(&fx, &["setup", "--no-settings"], |q| {
        let q = q.trim_start_matches("? ").trim_end();
        let yes = |n: usize| if n == 0 { "y" } else { "n" };
        let a: String = if let Some(rest) = q.strip_prefix("Is ") {
            let drive = rest.split(" a copy of your library?").next().unwrap();
            let drive = drive.split(&in_volumes).next().unwrap().to_string();
            questions.push((drive.clone(), q.to_string()));
            let a = ["y", "n", ""][pick(3)];
            let took = a == "y" || (a.is_empty() && default_of(q) == Some("Y/n"));
            if took {
                taken.push(drive);
                taken_at.push(asked);
            }
            asked += 1;
            a.into()
        } else if q.starts_with("Name for your library") {
            let a = NAMES[pick(NAMES.len())];
            library = Some(if a.is_empty() { default_of(q).unwrap().to_string() } else { a.to_string() });
            a.into()
        } else if q.starts_with("Which one does rekordbox export to") {
            // Now and then (once) an answer it won't take, asked again.
            match pick(4) {
                0 if !refused => {
                    refused = true;
                    "9".into()
                }
                n => {
                    primary = n % taken.len().max(1);
                    (primary + 1).to_string()
                }
            }
        } else if let Some(rest) = q.strip_prefix("Role for ") {
            let drive = rest.split(" (backup").next().unwrap();
            let drive = drive.split(&in_volumes).next().unwrap().to_string();
            let a = ["", "backup", "set", "archive", "bogus"][pick(5)];
            if a != "bogus" {
                let role = if a.is_empty() { default_of(q).unwrap().to_string() } else { a.to_string() };
                roles.push((drive, role));
            }
            a.into()
        } else if q.starts_with("Start your library?") || q.starts_with("Save them in it?") {
            let a = yes(pick(2));
            saved = Some(a == "y");
            a.into()
        } else if q.starts_with("Contact details") {
            let a = ["", "dj@example.com"][pick(2)];
            contact_given = Some(a.to_string()).filter(|a| !a.is_empty());
            a.into()
        } else if q.starts_with("Write them?") {
            let a = yes(pick(2));
            contact = Some(a == "y");
            a.into()
        } else if q.starts_with("Which player models") {
            let a = ["", "CDJ-3000", "NOPE-1"][pick(3)];
            models_given = Some(a.to_string()).filter(|a| a == "CDJ-3000");
            a.into()
        } else if q.starts_with("Remember them?") {
            let a = yes(pick(2));
            players = Some(a == "y");
            a.into()
        } else if q.starts_with("Device Name for") {
            ["", "DJ DECK"][pick(2)].into()
        } else if q.starts_with("Save drives into the store") {
            "n".into()
        } else {
            // The rest (marking the drives, a primary of another library, the playlist with the
            // contact details, Device Names), yes or no.
            yes(pick(2)).into()
        };
        a
    });
    let said = format!("{flags:?}\n{}\n{}", run.out, run.err);
    assert!(!run.err.contains("panicked at"), "{said}");
    if run.code != 0 {
        // It stopped: told no where it needs a yes, or with nothing to set up.
        assert!(run.err.contains("error:"), "{said}");
        return;
    }
    let (Some(library), Some(true)) = (library, saved) else { return };
    let libraries = fx.json(&["library", "ls"]);
    let names: Vec<&str> = libraries.as_array().unwrap().iter().filter_map(|l| l["name"].as_str()).collect();
    assert!(names.contains(&library.as_str()), "no library {library:?}: {names:?}\n{said}");
    let drives = fx.json(&["drive", "list", "--retired"]);
    let record = |name: &str| drives.as_array().unwrap().iter().find(|d| d["name"] == name);
    let role_of = |name: &str| record(name).map(|d| d["role"].clone());
    if let Some(p) = taken.get(primary).filter(|_| roles.iter().all(|(d, _)| d != &taken[primary])) {
        assert_eq!(role_of(p), Some(Value::from("primary")), "{p}\n{said}");
    }
    for (i, (d, role)) in roles.iter().enumerate() {
        // Twins (Club and its copy in Club2) are one drive, with one record: the role given last
        // is the one it has.
        if roles[i + 1..].iter().any(|(later, _)| later == d) {
            continue;
        }
        assert_eq!(role_of(d), Some(Value::from(role.as_str())), "{d}\n{said}");
    }
    if let (Some(m), Some(true)) = (&models_given, players) {
        assert_eq!(fx.json(&["players"])["default"], serde_json::json!([m]), "{said}");
    }
    // Archive and retired drives are kept as they are; the others get the contact details. Two
    // drives can share a name (Club and its twin, in Club2), so each answer is about the folder
    // listed in its place: setup lists the drives mounted, then asks about each in that order.
    let listed: Vec<String> = run
        .out
        .lines()
        .skip_while(|l| !l.ends_with("rekordbox drive(s) mounted"))
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .filter_map(|l| l.split("  ").filter(|p| !p.is_empty()).nth(1))
        .filter_map(|root| Some(std::path::Path::new(root).file_name()?.to_string_lossy().into_owned()))
        .collect();
    // Where two drives share a name, each question says which folder it's about.
    for (at, (name, q)) in questions.iter().enumerate() {
        if questions.iter().filter(|(n, _)| n == name).count() > 1 {
            let folder = fx.volumes.join(&listed[at]);
            assert!(q.contains(&format!("({})", folder.display())), "{q}\n{said}");
        }
    }
    if let (Some(c), Some(yes)) = (&contact_given, contact) {
        for (d, at) in taken.iter().zip(&taken_at) {
            let kept = record(d).is_some_and(|r| r["role"] == "archive" || !r["log"]["retired"].is_null());
            let folder = fx.volumes.join(&listed[*at]);
            let found = std::fs::read_to_string(folder.join("IF FOUND.txt")).unwrap_or_default();
            assert_eq!(found.contains(c.as_str()), yes && !kept, "{d} in {}\n{said}", folder.display());
        }
    }
}

/// `pull` into a library the store hasn't got, in a terminal: asked, it starts the library only
/// when told yes (and says no otherwise), and the store's libraries are what it said.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(yes = true, name = "Fresh".to_string())]
#[hegel::explicit_test_case(yes = false, name = "A".to_string())]
fn pull_starts_a_library_only_when_told(tc: TestCase) {
    let yes = tc.draw(gs::booleans());
    let name = tc.draw(gs::sampled_from(vec!["Fresh", "A", "Ünïcödé Club"]).map(str::to_string));
    let fx = Env::from_template();
    let before = fx.json(&["library", "ls"]);
    let known = before.as_array().unwrap().iter().any(|l| l["name"] == name.as_str());
    let mut asked = None;
    let run = conversation(&fx, &["pull", path(&fx.volumes.join("Club")), "--library", &name], |q| {
        asked = Some(q.to_string());
        if yes { "y" } else { "n" }.into()
    });
    let said = format!("{name} {yes}\n{}\n{}", run.out, run.err);
    // A library the store has is pulled into without asking.
    assert_eq!(asked.is_some(), !known, "{said}");
    if let Some(q) = &asked {
        assert!(q.contains(&format!("There's no library named {name:?} (the store has \"A\")")), "{said}");
    }
    let started = fx.json(&["library", "ls"]).as_array().unwrap().iter().any(|l| l["name"] == name.as_str());
    assert_eq!(started, known || yes, "{said}");
    if !known {
        assert_eq!(run.code == 0, yes, "{said}");
    }
}

/// A command that runs until Ctrl-C: once its stdout holds `ready` (when it's waiting), `meanwhile`
/// runs (given a way to wait for the stdout to hold something more), then Ctrl-C is pressed
/// `presses` times. Its run.
fn until_ctrl_c(
    mut command: std::process::Command,
    ready: &str,
    meanwhile: impl FnOnce(&dyn Fn(&str)),
    presses: usize,
) -> Run {
    let mut child =
        Running(command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap());
    let out = Arc::new(Mutex::new(String::new()));
    let mut stdout = child.stdout.take().unwrap();
    let reading = {
        let out = out.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stdout.read(&mut buf) {
                if n == 0 {
                    break;
                }
                out.lock().unwrap().push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        })
    };
    let wait_for = |text: &str| {
        let started = Instant::now();
        while !out.lock().unwrap().contains(text) && started.elapsed() < Duration::from_secs(60) {
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    wait_for(ready);
    meanwhile(&wait_for);
    let pid = i32::try_from(child.id()).unwrap();
    for _ in 0..presses {
        // SAFETY: a signal to the child this test started, which hasn't been waited for yet.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGINT) }, 0);
        // Long enough for the first to be handled, too short for the run to stop.
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut err = String::new();
    child.stderr.take().unwrap().read_to_string(&mut err).unwrap();
    let status = child.wait().unwrap();
    reading.join().unwrap();
    let out = out.lock().unwrap().clone();
    Run { out, err, code: status.code().unwrap_or(-1) }
}

/// `watch` runs until Ctrl-C: pressed once it stops at the next safe point and exits cleanly
/// (said in a line, `--json` or not); pressed twice it stops at once (exit status 130). Before
/// that it said, as text or as one JSON event a line, what it found of each mounted drive, and
/// when a drive was plugged in and out.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(json = false, presses = 1usize)]
#[hegel::explicit_test_case(json = true, presses = 2usize)]
fn watch_stops_at_ctrl_c(tc: TestCase) {
    let json = tc.draw(gs::booleans());
    let presses = tc.draw(gs::integers::<usize>().min_value(1).max_value(2));
    let fx = Env::from_template();
    let mut command = fx.command();
    if json {
        command.arg("--json");
    }
    command.args(["watch", "--settle", "0"]);
    // A drive plugged in once it has looked at those there (made beside the volumes, then moved
    // in at once), and pulled out again.
    let (outside, inside) = (fx.home("Late"), fx.volumes.join("Late"));
    crate::support::demo_drive(&fx.home(""), "Late");
    let (plugged, unplugged) =
        if json { ("drive.attached", "drive.detached") } else { ("Late plugged in", "Late unplugged") };
    let run = until_ctrl_c(
        command,
        "Club",
        |wait_for| {
            std::fs::rename(&outside, &inside).unwrap();
            wait_for(plugged);
            std::fs::rename(&inside, &outside).unwrap();
            wait_for(unplugged);
        },
        presses,
    );
    assert!(run.out.contains(plugged) && run.out.contains(unplugged), "{}", run.out);
    let said = format!("json {json}, {presses} press(es)\n{}\n{}", run.out, run.err);
    assert!(!run.err.contains("panicked at"), "{said}");
    // Pressed again before it stopped, it stops at once; it may have stopped in between.
    let codes: &[i32] = if presses == 1 { &[0] } else { &[0, 130] };
    assert!(codes.contains(&run.code), "{said}");
    assert!(run.err.contains("stopping safely"), "{said}");
    match json {
        true => {
            for line in run.out.lines() {
                let v: Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}\n{said}"));
                assert_eq!(v["schema"], 1, "{said}");
            }
        }
        false => {
            assert!(run.out.contains("Watching for drives") && run.out.contains("A: the store holds it (A)"), "{said}")
        }
    }
}

/// A store served on this computer's own network (until Ctrl-C) and fetched into another: the
/// fetch says the same with and without `--json`, a dry run copies nothing, and the other store
/// then has the libraries the first has (up to date when fetched again). The server says who
/// connected and what they took.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(json = false, only = false)]
#[hegel::explicit_test_case(json = true, only = true)]
fn a_served_store_is_fetched_whole(tc: TestCase) {
    let json = tc.draw(gs::booleans());
    let only = tc.draw(gs::booleans());
    let laptop = build(State::from_flags(&[false, true]));
    // A store of its own (the template's is the laptop's, by id too).
    let studio = Env::from_template();
    std::fs::remove_dir_all(&studio.store).unwrap();
    let mut serving = laptop.command();
    if json {
        serving.arg("--json");
    }
    serving.args(["store", "serve", "--local"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = Running(serving.spawn().unwrap());
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    // The code: the ready event's, or on the line telling the other computer what to run.
    let code = loop {
        let mut line = String::new();
        assert_ne!(std::io::BufRead::read_line(&mut stdout, &mut line).unwrap(), 0, "serve ended: {first}");
        first.push_str(&line);
        if json {
            let v: Value = serde_json::from_str(&line).unwrap();
            break v["code"].as_str().unwrap().to_string();
        }
        if let Some(code) = line.trim().strip_prefix("locallibrary store fetch ") {
            break code.to_string();
        }
    };
    let universe = crate::props_cli_human::universe(&laptop);
    let mut fetch = vec!["store", "fetch", &code, "--local"];
    if only {
        fetch.extend(["--library", "clubland"]);
    }
    let spec = ["{snapshots} snapshot(s)", "{files} file(s)", "libraries.[].name"];
    let dry: Vec<&str> = [&fetch[..], &["--dry-run"]].concat();
    crate::props_cli_human::agree(&studio, &dry, &spec, &universe);
    let names = |fx: &Env| -> Vec<Value> {
        let mut v: Vec<Value> =
            fx.json(&["library", "ls"]).as_array().unwrap().iter().map(|l| l["name"].clone()).collect();
        v.sort_by_key(|n| n.to_string());
        v
    };
    assert_eq!(names(&studio), Vec::<Value>::new(), "a dry run copies nothing");
    // Fetched for real into two copies of the studio's store, one as a person runs it and the
    // other with `--json`.
    let other = crate::props_cli_human::twin(&studio);
    let by_person = studio.run(&fetch);
    let by_program = other.run(&[&["--json"], &fetch[..]].concat());
    let root = |fx: &Env| path(fx.volumes.parent().unwrap()).to_string();
    let by_program = Run { out: by_program.out.replace(&root(&other), &root(&studio)), ..by_program };
    let fetched = crate::props_cli_human::agree_runs(&fetch, &by_person, &by_program, &spec, &universe)
        .unwrap_or_else(|| panic!("not fetched: {}", by_person.err));
    assert_eq!(names(&studio), names(&other));
    let again = crate::props_cli_human::agree(&studio, &fetch, &spec, &universe).expect("fetched again");
    assert!(again["libraries"].as_array().unwrap().iter().all(|l| l["outcome"] == "up_to_date"), "{again}");
    let mut want: Vec<Value> = names(&laptop);
    if only {
        want.retain(|n| n == "Clubland");
    }
    assert_eq!(names(&studio), want, "{fetched}");
    // SAFETY: a signal to the child this test started, which hasn't been waited for yet.
    assert_eq!(unsafe { libc::kill(i32::try_from(child.id()).unwrap(), libc::SIGINT) }, 0);
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).unwrap();
    let status = child.wait().unwrap();
    assert!(status.success(), "{first}{rest}");
    if !json {
        assert!(rest.contains("connected") && rest.contains("fetched"), "{first}{rest}");
    }
}
