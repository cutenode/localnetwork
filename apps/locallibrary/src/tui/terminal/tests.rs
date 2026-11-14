use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitStatus};

use hegel::TestCase;
use hegel::generators as gs;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::KeyEventKind;
use ratatui::layout::Rect;
use ratatui::{TerminalOptions, Viewport};

use super::super::app::View;
use super::super::app::testing::blank;
use super::super::theme::{self, Tier};
use super::super::{draw_synchronized, icons};
use super::*;

const TC: &[u8] = b"\x1bP1+r5463=\x1b\\";
const RGB: &[u8] = b"\x1bP1+r524742=382F382F38\x1b\\";
const NOT_FOUND: &[u8] = b"\x1bP0+r\x1b\\";
const READ_BACK: &[u8] = b"\x1bP1$r0;48;2;1;2;3m\x1b\\";
const DA1: &[u8] = b"\x1b[?62;22c";

#[test]
fn a_terminal_that_says_truecolor() {
    for reply in [
        TC,
        RGB,
        READ_BACK,
        b"\x1bP1+r524742\x1b\\",
        b"\x1bP1+r524742=FF\x1b\\",
        b"\x1bP1$r48:2::1:2:3m\x1b\\",
        b"\x1bP1$r48:2:1:2:3m\x1b\\",
    ] {
        assert!(says_truecolor(reply), "{:?}", String::from_utf8_lossy(reply));
    }
}

#[test]
fn a_terminal_that_says_nothing_of_it() {
    for reply in [
        &b""[..],
        NOT_FOUND,
        DA1,
        b"\x1bP0$r\x1b\\",
        // A palette colour read back: 16 million colours aren't what kept this one.
        b"\x1bP1$r0;48;5;16m\x1b\\",
        b"\x1bP1$r0m\x1b\\",
        // Asked for something else.
        b"\x1bP1+r4d73\x1b\\",
    ] {
        assert!(!says_truecolor(reply), "{:?}", String::from_utf8_lossy(reply));
    }
}

#[test]
fn da1_ends_the_answers() {
    assert!(finished(DA1));
    assert!(finished(b"\x1b[?1;2c"));
    assert!(finished(&[TC, DA1].concat()));
    assert!(!finished(b""));
    assert!(!finished(b"\x1b[?62;2"), "still coming");
    assert!(!finished(TC));
    assert!(!finished(b"\x1b[?1049h"), "something else");
}

/// Attribute 4 of DA1's answer is sixels, among the other answers or alone, and only there.
#[test]
fn a_terminal_that_lists_sixels() {
    assert!(says_sixel(b"\x1b[?62;4;22c"));
    assert!(says_sixel(&[TC, b"\x1b[?1;2;4c"].concat()));
    assert!(!says_sixel(DA1));
    assert!(!says_sixel(b"\x1b[?4;1c"), "the first is the terminal's level");
    assert!(!says_sixel(b"\x1b[?62;14c"));
    assert!(!says_sixel(b"\x1b[?62;4"), "cut off");
    assert!(!says_sixel(b""));
}

/// The two ends of a pty: the terminal's, and the one a program runs on.
fn pty() -> (File, File) {
    let (mut ours, mut theirs) = (0, 0);
    // SAFETY: both pointers are live, and the names, attributes and size may be null.
    let opened = unsafe {
        libc::openpty(&mut ours, &mut theirs, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
    };
    assert_eq!(opened, 0);
    // Kept from whatever's spawned: a player holding its own terminal's far end open would never
    // see it close.
    for fd in [ours, theirs] {
        // SAFETY: a descriptor just opened.
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) }, 0);
    }
    // SAFETY: both descriptors were just opened and nothing else owns them.
    let (ours, theirs) = unsafe { (File::from_raw_fd(ours), File::from_raw_fd(theirs)) };
    (ours, theirs)
}

/// A pty that doesn't echo or turn newlines round, as the player's end is while it asks.
fn raw_pty() -> (File, File) {
    let (far, near) = pty();
    // SAFETY: a live descriptor and termios.
    unsafe {
        let mut raw = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(near.as_raw_fd(), &mut raw), 0);
        libc::cfmakeraw(&mut raw);
        assert_eq!(libc::tcsetattr(near.as_raw_fd(), libc::TCSANOW, &raw), 0);
    }
    (far, near)
}

/// A terminal on the far end of a pty: reads what's asked, then sends `reply`. Stops when it's
/// read all of what's asked, so nothing's left waiting on it.
fn answering(mut far: File, reply: Vec<u8>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut asked = vec![0; ASK.len()];
        far.read_exact(&mut asked).unwrap();
        far.write_all(&reply).unwrap();
        far.flush().unwrap();
        // Held open until the player has read it all.
        std::thread::sleep(Duration::from_millis(20));
        asked
    })
}

/// A terminal asked over a pty: the query goes out whole, the answer comes back however it's cut,
/// and the wait ends at DA1, not at the deadline.
#[hegel::test(test_cases = 10)]
fn prop_asking_ends_at_the_sentinel(tc: TestCase) {
    let says_it = tc.draw(gs::booleans());
    let reply = [if says_it { RGB } else { NOT_FOUND }, NOT_FOUND, DA1].concat();
    let (far, mut near) = raw_pty();
    let terminal = answering(far, reply);
    let started = Instant::now();
    let (got, done) = ask(&mut near, started + Duration::from_secs(20)).unwrap();
    assert!(started.elapsed() < Duration::from_secs(10), "not at the deadline");
    assert_eq!(terminal.join().unwrap(), ASK);
    assert!(done);
    assert_eq!(says_truecolor(&got), says_it);
}

/// A terminal that answers nothing costs the deadline, and says so.
#[test]
fn a_terminal_that_answers_nothing_takes_the_timeout() {
    let (far, mut near) = raw_pty();
    let started = Instant::now();
    let (got, done) = ask(&mut near, started + Duration::from_millis(30)).unwrap();
    assert_eq!((got, done), (Vec::new(), false));
    assert!(started.elapsed() >= Duration::from_millis(30));
    assert!(started.elapsed() < Duration::from_secs(2));
    drop(far);
}

/// Off a terminal nothing's asked, though there's one to ask: nothing's written to it.
#[hegel::test(test_cases = 2)]
fn prop_nothing_is_asked_off_a_terminal(_tc: TestCase) {
    let answering = Terminal { capability: true, read_back: true, colours: true, dark: true, device: true };
    let (out, status) =
        on_a_pty(probe_alone_command(&[("LOCALLIBRARY_TEST_PROBE", "off")]), answering, b"\xff", b"unused");
    let out = String::from_utf8_lossy(&out);
    assert!(status.success() && out.contains("said {false}"), "{out}");
    assert!(!out.contains('\x1b'), "{out:?}");
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent { code, modifiers, kind: KeyEventKind::Press, state: ratatui::crossterm::event::KeyEventState::NONE }
}

/// `bytes` as what's reading keys sees them: ESC and a character as Alt and the character.
fn keys(bytes: &[u8]) -> Vec<KeyEvent> {
    let mut out = Vec::new();
    let mut it = bytes.iter().copied().peekable();
    while let Some(b) = it.next() {
        out.push(match b {
            0x1b => key(KeyCode::Char(it.next().unwrap() as char), KeyModifiers::ALT),
            0x07 => key(KeyCode::Char('g'), KeyModifiers::CONTROL),
            b => key(KeyCode::Char(b as char), KeyModifiers::NONE),
        });
    }
    out
}

/// An answer that comes after the wait is dropped, all of it, however it ends, and what's typed
/// around it isn't. Once the time's up, or when nothing was late, keys are keys.
#[hegel::test(test_cases = 200)]
fn prop_late_answers_never_reach_the_keys(tc: TestCase) {
    let now = Instant::now();
    let answer = tc.draw(gs::sampled_from(
        &[
            &b"\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\"[..],
            b"\x1b]10;rgb:cdcd/d6d6/f4f4\x07",
            b"\x1bP1$r0;48;2;1;2;3m\x1b\\",
            b"\x1bP1+r524742=\x1b\\",
        ][..],
    ));
    let typed = tc.draw(gs::text().alphabet("abq1").max_size(3));
    let mut late = LateAnswers::after(&Answers { late: true, ..Default::default() }, now);
    let soon = now + Duration::from_millis(5);
    let mut through = |bytes: &[u8]| keys(bytes).iter().filter(|k| !late.swallows(k, soon)).count();
    assert_eq!(through(typed.as_bytes()), typed.len(), "typed before");
    assert_eq!(through(answer), 0, "{answer:?}");
    assert_eq!(through(typed.as_bytes()), typed.len(), "typed after");

    assert!(!late.swallows(&keys(answer)[0], now + LATE), "too late to be an answer");
    assert!(!late.swallows(&keys(answer)[0], soon), "and that's the end of it");
    let mut none = LateAnswers::after(&Answers::default(), now);
    assert!(!none.swallows(&keys(answer)[0], soon));
}

const LIGHT_PAGE: &[u8] = b"\x1b]11;rgb:efef/f1f1/f5f5\x1b\\";
const DARK_PAGE: &[u8] = b"\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\";

/// What a terminal answers.
#[derive(Debug, Clone, Copy)]
struct Terminal {
    capability: bool,
    read_back: bool,
    colours: bool,
    /// Its background is the dark one, not the light.
    dark: bool,
    /// Answers DA1 at all: one that doesn't is out of the spec, or a very slow line.
    device: bool,
}

/// A terminal that answers nothing.
const SILENT: Terminal = Terminal { capability: false, read_back: false, colours: false, dark: false, device: false };

impl Terminal {
    /// Each query and what it gets.
    fn answers(&self) -> [(&'static [u8], Vec<u8>); 6] {
        let found = |yes: bool, reply: &[u8]| if yes { reply.to_vec() } else { Vec::new() };
        [
            (b"\x1bP+q5463\x1b\\", found(self.capability, TC)),
            (b"\x1bP+q524742\x1b\\", found(false, RGB)),
            (b"\x1bP$qm\x1b\\", found(self.read_back, READ_BACK)),
            (b"\x1b]10;?\x07", found(self.colours, b"\x1b]10;rgb:8080/8080/8080\x1b\\")),
            (b"\x1b]11;?\x07", found(self.colours, if self.dark { DARK_PAGE } else { LIGHT_PAGE })),
            (b"\x1b[c", found(self.device, DA1)),
        ]
    }
}

/// The colour set and read back means 24-bit colour however the terminal writes it, and among
/// whatever else is set; a colour that came back changed doesn't.
#[hegel::test(test_cases = 100)]
fn prop_the_colour_read_back_says_truecolor_however_it_is_written(tc: TestCase) {
    let colour = tc.draw(gs::sampled_from(&[[1, 2, 3], [0, 0, 0], [1, 2, 4], [3, 2, 1]][..]));
    let sep = tc.draw(gs::sampled_from(&[";", ":"][..]));
    let space = tc.draw(gs::sampled_from(&["", ":"][..]));
    let before = tc.draw(gs::sampled_from(&["", "0;", "0;1;"][..]));
    let after = tc.draw(gs::sampled_from(&["", ";1", ";38;5;7"][..]));
    let [r, g, b] = colour;
    let reply = format!("\x1bP1$r{before}48{sep}2{sep}{space}{r}{sep}{g}{sep}{b}{after}m\x1b\\\x1b[?62c");
    assert_eq!(says_truecolor(reply.as_bytes()), colour == [1, 2, 3], "{reply:?}");
}

/// A program a test started, stopped if the test fails first.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Runs `command` on a pty as its controlling terminal, playing `terminal` at the far end and
/// typing `keys` once `ready` shows in its output, or hanging up if `keys` is empty: all it wrote,
/// and how it ended.
fn on_a_pty(mut command: Command, terminal: Terminal, ready: &[u8], keys: &[u8]) -> (Vec<u8>, ExitStatus) {
    let (mut far, near) = pty();
    let size = libc::winsize { ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: a live descriptor and a live size.
    assert_eq!(unsafe { libc::ioctl(near.as_raw_fd(), libc::TIOCSWINSZ, &size) }, 0);
    command.stdin(near.try_clone().unwrap()).stdout(near.try_clone().unwrap()).stderr(near.try_clone().unwrap());
    // SAFETY: only calls that are safe after fork: its own session, with this as its terminal.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = Running(command.spawn().unwrap());
    drop(near);
    let (mut out, mut scanned, mut typed) = (Vec::new(), 0, false);
    let mut chunk = [0; 4096];
    let give_up = Instant::now() + Duration::from_secs(60);
    loop {
        assert!(Instant::now() < give_up, "still running:\n{}", String::from_utf8_lossy(&out));
        if !readable(far.as_raw_fd(), Duration::from_millis(50)).unwrap() {
            if let Some(status) = child.0.try_wait().unwrap() {
                return (out, status);
            }
            continue;
        }
        match far.read(&mut chunk) {
            Ok(0) | Err(_) => return (out, child.0.wait().unwrap()),
            Ok(n) => out.extend_from_slice(&chunk[..n]),
        }
        while scanned < out.len() {
            let seen = &out[scanned..];
            let query = terminal.answers().into_iter().find(|(q, _)| seen.starts_with(q));
            if let Some((query, reply)) = query {
                far.write_all(&reply).unwrap();
                scanned += query.len();
            } else if terminal.answers().iter().any(|(q, _)| q.starts_with(seen)) {
                break; // The rest of it is still on its way.
            } else {
                scanned += 1;
            }
        }
        if !typed && out.windows(ready.len()).any(|w| w == ready) {
            if keys.is_empty() {
                drop(far);
                let started = Instant::now();
                while started.elapsed() < Duration::from_secs(20) {
                    if let Some(status) = child.0.try_wait().unwrap() {
                        return (out, status);
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                panic!("still running after its terminal went away");
            }
            far.write_all(keys).unwrap();
            typed = true;
        }
    }
}

/// Not a test of its own: what [`prop_the_probe_asks_a_real_terminal`] runs on a pty, in a process
/// of its own as that's what a terminal belongs to.
#[test]
fn probe_alone() {
    let Some(asked) = std::env::var_os("LOCALLIBRARY_TEST_PROBE") else { return };
    theme::init();
    icons::init();
    let started = Instant::now();
    let answers = probe(asked != "off", theme::tier_forced());
    theme::answered(&answers);
    let (ms, mode, tier) = (started.elapsed().as_millis(), theme::mode(), theme::tier());
    println!("\nsaid {{{}}} {ms} ms {mode:?} {tier:?} {:?} {:?}", answers.truecolor, theme::page(), icons::set());
}

fn probe_alone_command(env: &[(&str, &str)]) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["tui::terminal::tests::probe_alone", "--exact", "--nocapture"]);
    command.env_remove("COLORTERM").env_remove("LOCALLIBRARY_THEME").env_remove("LOCALLIBRARY_COLOR");
    command.env("LOCALLIBRARY_TEST_PROBE", "1").env("TERM", "xterm").envs(env.iter().copied());
    command
}

fn probed(terminal: Terminal, env: &[(&str, &str)]) -> String {
    let (out, status) = on_a_pty(probe_alone_command(env), terminal, b"\xff", b"unused");
    let out = String::from_utf8_lossy(&out).into_owned();
    assert!(status.success(), "{out}");
    let said = out.lines().find_map(|l| l.trim().strip_prefix("said ")).unwrap_or_else(|| panic!("{out}"));
    said.to_string()
}

/// On a real pty, what the terminal says is what's believed: 24-bit colour if it says so by either
/// way of asking, its background for the mode unless one's named, and nothing lost if it knows
/// nothing but DA1. A terminal that knows even less costs the timeout and no more.
#[hegel::test(test_cases = 6)]
fn prop_the_probe_asks_a_real_terminal(tc: TestCase) {
    let capability = tc.draw(gs::booleans());
    let read_back = tc.draw(gs::booleans());
    let colours = tc.draw(gs::booleans());
    let dark = tc.draw(gs::booleans());
    let terminal = Terminal { capability, read_back, colours, dark, device: true };
    let said = probed(terminal, &[]);
    let truecolor = capability || read_back;
    assert!(said.starts_with(&format!("{{{truecolor}}}")), "{said}");
    assert!(said.contains(if truecolor { "Truecolor" } else { "Colors16" }), "{said}");
    assert_eq!(said.contains("Light"), colours && !dark, "{said}");
    assert_eq!(said.contains("[239, 241, 245]"), colours && !dark, "{said}");
    assert_eq!(said.contains("[30, 30, 46]"), colours && dark, "{said}");
    let on_dark = probed(Terminal { colours: true, dark: true, ..terminal }, &[]);
    assert!(on_dark.contains("Dark"), "{on_dark}");

    let named = probed(terminal, &[("LOCALLIBRARY_THEME", "dark")]);
    assert!(named.contains("Dark"), "{named}");
    let forced = probed(terminal, &[("LOCALLIBRARY_COLOR", "16"), ("LOCALLIBRARY_ICONS", "nerd")]);
    assert!(forced.starts_with("{false}") && forced.contains("Colors16"), "not asked: {forced}");
    assert!(forced.contains("Nerd"), "{forced}");
}

/// A terminal that knows even less than DA1 costs the timeout and no more, whichever question it
/// was left silent on.
#[hegel::test(test_cases = 4)]
fn prop_a_terminal_that_knows_nothing_costs_the_timeout_and_no_more(tc: TestCase) {
    let env: &[(&str, &str)] = if tc.draw(gs::booleans()) {
        &[("LOCALLIBRARY_ICONS", "ascii"), ("LOCALLIBRARY_COLOR", "nonsense")]
    } else {
        &[("LOCALLIBRARY_ICONS", "ascii"), ("LOCALLIBRARY_COLOR", "256")]
    };
    let said = probed(SILENT, env);
    let ms: u128 = said.split(' ').nth(1).unwrap().parse().unwrap();
    assert!(said.contains("Dark") && said.contains("Ascii"), "{said}");
    assert!(ms >= TIMEOUT.as_millis() && ms < 2_000, "{said}");
}

/// The player, on a world of nothing in `dir`.
fn player(dir: &std::path::Path) -> Command {
    let mut command = Command::new(crate::plugins::exe().unwrap());
    command.arg("tui").current_dir(dir).env("HOME", dir);
    command.env("LOCALLIBRARY_STORE", dir.join("store")).env("LOCALLIBRARY_AUDIO", "silent");
    command.env("LOCALLIBRARY_VOLUMES", dir.join("none")).env("LOCALLIBRARY_NO_OPEN", "1");
    command.env_remove("NO_COLOR").env_remove("LOCALLIBRARY_THEME").env_remove("LOCALLIBRARY_COLOR");
    command
}

/// The player on a terminal, start to end: it asks before it draws, each frame comes between the
/// synchronized output marks, `q` quits it, and what it asked leaves nothing behind.
#[hegel::test(test_cases = 2)]
fn prop_the_player_starts_asks_draws_and_quits(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let late = tc.draw(gs::booleans());
    let terminal = Terminal { capability: true, read_back: false, colours: true, dark: false, device: !late };
    let mut command = player(dir.path());
    command.env("TERM", "xterm-256color");
    let (out, status) = on_a_pty(command, terminal, b"\x1b[?2026l", b"Q");
    let shown = String::from_utf8_lossy(&out);
    assert!(status.success(), "{shown}");
    let at = |needle: &[u8]| out.windows(needle.len()).position(|w| w == needle);
    let (asked, screen, begin, end) = (at(b"\x1bP+q"), at(b"\x1b[?1049h"), at(b"\x1b[?2026h"), at(b"\x1b[?2026l"));
    assert!(asked.unwrap() < screen.unwrap(), "asked before the screen is taken over");
    assert!(screen.unwrap() < begin.unwrap() && begin.unwrap() < end.unwrap(), "{shown}");
    let drawn = out[begin.unwrap()..end.unwrap()].windows(10).any(|w| w == b"locallibra");
    assert!(drawn, "the frame is between the marks");
}

/// The terminal goes away while it's being asked, as a closed window or a dropped SSH connection
/// does: the asking ends, not the wait for an answer that won't come, and the player then ends as
/// it always has with no terminal.
#[hegel::test(test_cases = 3)]
fn prop_a_terminal_that_goes_away_mid_question_ends_the_player(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let at = tc.draw(gs::sampled_from(vec![&b"\x1bP+q"[..], b"\x1b[c"]));
    let mut command = player(dir.path());
    command.env("TERM", "xterm-256color");
    let (out, status) = on_a_pty(command, SILENT, at, b"");
    assert!(status.code().is_some() || status.signal().is_some(), "{out:?}");
}

/// The far end closes with the question asked: an end at once, not at the deadline. And a terminal
/// that isn't one any more says so.
#[hegel::test(test_cases = 3)]
fn prop_asking_a_terminal_that_goes_away_ends_at_once(_tc: TestCase) {
    let closed = File::open("/dev/null").unwrap().as_raw_fd();
    assert!(readable(closed, Duration::from_millis(1)).is_err(), "not open now");
    let (mut far, mut near) = raw_pty();
    let closing = std::thread::spawn(move || {
        far.read_exact(&mut vec![0; ASK.len()]).unwrap();
    });
    let started = Instant::now();
    let asked = ask(&mut near, started + Duration::from_secs(30));
    closing.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(5), "{asked:?}");
    assert!(!matches!(asked, Ok((_, true))), "{asked:?}");
}

/// What a frame writes, kept where the test can read it.
#[derive(Clone, Default)]
struct Written(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl Write for Written {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A frame goes out between the marks, the whole of it.
#[test]
fn a_frame_is_drawn_synchronized() {
    let (mut app, _dir) = blank(View::Library);
    let written = Written::default();
    let backend = CrosstermBackend::new(written.clone());
    let viewport = Viewport::Fixed(Rect::new(0, 0, 80, 24));
    let mut terminal = ratatui::Terminal::with_options(backend, TerminalOptions { viewport }).unwrap();
    draw_synchronized(&mut terminal, &mut app).unwrap();
    let out = written.0.lock().unwrap();
    assert!(out.starts_with(b"\x1b[?2026h"), "{:?}", String::from_utf8_lossy(&out[..20]));
    assert!(out.ends_with(b"\x1b[?2026l"));
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains('l') && text.contains("Library"), "{text}");
}

/// Through the player: what starts as an answer isn't typing, and what's typed after it is.
#[hegel::test(test_cases = 3)]
fn prop_a_late_answer_does_nothing_in_the_player(_tc: TestCase) {
    let (mut app, _dir) = blank(View::Library);
    app.expect_late(&Answers { late: true, ..Answers::default() });
    for key in keys(b"\x1b]11;rgb:1e1e/1e1e/2e2e?:\x1b\\") {
        app.on_key(key);
    }
    assert!(app.modal.is_none() && !app.help, "none of it was keys");
    app.on_key(key(KeyCode::Char('?'), KeyModifiers::NONE));
    assert!(app.help, "and this was");
}

/// The `Tier` a terminal asked for reaches frames only as it should: the environment's beats the
/// settings', which beats what's detected.
#[test]
fn a_tier_chosen_outranks_what_the_terminal_says() {
    let detected = theme::tier();
    theme::choose_tier(Some(Tier::Colors16));
    assert_eq!(theme::tier(), Tier::Colors16);
    assert_eq!(theme::chosen_tier(), Some(Tier::Colors16));
    theme::choose_tier(None);
    assert_eq!(theme::chosen_tier(), None);
    assert_eq!(theme::tier(), detected);
}
