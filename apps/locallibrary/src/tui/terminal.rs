//! Asking the terminal what it is, once, as the player starts: whether it draws 24-bit colour
//! (XTGETTCAP, then setting a colour and reading it back with DECRQSS, as Neovim does) and its real
//! foreground and background (OSC 10 and 11, through terminal-colorsaurus). Each question ends with
//! a request every terminal answers (DA1), so a terminal that knows none of it is done with as soon
//! as a local one can reply; only one that says nothing at all, or a slow connection, takes up
//! [`TIMEOUT`]. It runs beside the player's start, so a first frame waits on it only for what's
//! left. Answers that come after that would read as keys: [`LateAnswers`] keeps them out.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The most the two questions take together: long enough for most SSH connections' round trip,
/// short enough not to be noticed as a pause before the first frame.
pub const TIMEOUT: Duration = Duration::from_millis(100);

/// How long after giving up answers are still expected.
const LATE: Duration = Duration::from_secs(3);

/// XTGETTCAP for `Tc` and `RGB`, one at a time as xterm stops at the first it doesn't know; a
/// colour set and read back with DECRQSS; the attributes put back; DA1.
#[cfg(unix)]
const ASK: &[u8] = b"\x1bP+q5463\x1b\\\x1bP+q524742\x1b\\\x1b[0m\x1b[48;2;1;2;3m\x1bP$qm\x1b\\\x1b[0m\x1b[c";

/// A terminal's own colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colours {
    pub background: [u8; 3],
    pub foreground: [u8; 3],
}

/// What the terminal said.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Answers {
    /// It says it draws 24-bit colour.
    pub truecolor: bool,
    pub colours: Option<Colours>,
    /// It didn't finish answering in time, so the rest may yet come.
    pub late: bool,
}

/// Whether the terminal's replies say 24-bit colour: a capability found, or the colour read back.
#[cfg(unix)]
fn says_truecolor(reply: &[u8]) -> bool {
    reply.split(|b| *b == 0x1b).any(|part| has_capability(part) || reads_colour_back(part))
}

/// `P1+r<name in hex>[=<value>]`: the terminal has `Tc` or `RGB`.
#[cfg(unix)]
fn has_capability(part: &[u8]) -> bool {
    let Some(rest) = part.strip_prefix(b"P1+r") else { return false };
    let name = rest.split(|b| *b == b'=').next().unwrap_or_default();
    name.eq_ignore_ascii_case(b"5463") || name.eq_ignore_ascii_case(b"524742")
}

/// `P1$r<attributes>m` with the colour [`ASK`] set among them, however it's separated.
#[cfg(unix)]
fn reads_colour_back(part: &[u8]) -> bool {
    let Some(rest) = part.strip_prefix(b"P1$r") else { return false };
    let params: Vec<&[u8]> = rest.split(|b| matches!(b, b';' | b':' | b'm')).filter(|p| !p.is_empty()).collect();
    params.windows(5).any(|w| w == [&b"48"[..], b"2", b"1", b"2", b"3"])
}

/// Whether DA1's answer (`ESC [ ? ... c`) has come: all the answers to what was asked before it have.
#[cfg(unix)]
fn finished(reply: &[u8]) -> bool {
    reply.windows(3).enumerate().filter(|(_, w)| *w == b"\x1b[?").any(|(at, _)| {
        let params = reply[at + 3..].iter().skip_while(|b| b.is_ascii_digit() || **b == b';');
        params.into_iter().next() == Some(&b'c')
    })
}

/// Asks, and reads the replies until DA1's answer or `deadline`: the replies, and whether it came.
#[cfg(unix)]
fn ask<T>(tty: &mut T, deadline: Instant) -> std::io::Result<(Vec<u8>, bool)>
where
    T: std::io::Read + std::io::Write + std::os::fd::AsRawFd,
{
    tty.write_all(ASK)?;
    tty.flush()?;
    let (mut reply, mut chunk) = (Vec::with_capacity(128), [0; 128]);
    while !finished(&reply) {
        let left = deadline.saturating_duration_since(Instant::now());
        if !readable(tty.as_raw_fd(), left)? {
            return Ok((reply, false));
        }
        match tty.read(&mut chunk)? {
            0 => return Ok((reply, false)),
            n => reply.extend_from_slice(&chunk[..n]),
        }
    }
    Ok((reply, true))
}

/// Waits for something to read, up to `wait`: select(2), as macOS can't poll a terminal device.
#[cfg(unix)]
fn readable(fd: std::os::fd::RawFd, wait: Duration) -> std::io::Result<bool> {
    let mut timeout =
        libc::timeval { tv_sec: wait.as_secs() as libc::time_t, tv_usec: wait.subsec_micros() as libc::suseconds_t };
    // SAFETY: a zeroed fd_set is an empty one, and `fd` is open for as long as its owner is borrowed.
    let ready = unsafe {
        let mut set: libc::fd_set = std::mem::zeroed();
        libc::FD_SET(fd, &mut set);
        libc::select(fd + 1, &mut set, std::ptr::null_mut(), std::ptr::null_mut(), &mut timeout)
    };
    match ready {
        -1 => Err(std::io::Error::last_os_error()),
        n => Ok(n > 0),
    }
}

/// Whether the terminal says it draws 24-bit colour, and whether it answered in time. Going wrong
/// partway (a signal in select, a terminal closing) counts as not in time: its answers may be on
/// their way.
#[cfg(unix)]
fn ask_truecolor(deadline: Instant) -> (bool, bool) {
    let asked = terminal_trx::terminal().and_then(|mut tty| ask(&mut tty.lock().enable_raw_mode()?, deadline));
    asked.map_or((false, false), |(reply, done)| (says_truecolor(&reply), done))
}

/// Windows Terminal says so in the environment.
#[cfg(not(unix))]
fn ask_truecolor(_: Instant) -> (bool, bool) {
    (false, true)
}

/// The terminal's colours, and whether it answered in time.
fn ask_colours(deadline: Instant) -> (Option<Colours>, bool) {
    use terminal_colorsaurus::{Error, QueryOptions, color_palette};
    let mut options = QueryOptions::default();
    options.timeout = deadline.saturating_duration_since(Instant::now());
    let eight_bit = |c: &terminal_colorsaurus::Color| {
        let (r, g, b) = c.scale_to_8bit();
        [r, g, b]
    };
    match color_palette(options) {
        Ok(p) => (Some(Colours { background: eight_bit(&p.background), foreground: eight_bit(&p.foreground) }), true),
        Err(Error::Timeout(_)) => (None, false),
        Err(_) => (None, true),
    }
}

/// Everything the terminal will say, in at most [`TIMEOUT`]. Nothing's sent unless both ends of
/// the player are on a terminal, and tiers aren't asked about when `tier_forced`.
pub fn probe(on_terminal: bool, tier_forced: bool) -> Answers {
    if !on_terminal {
        return Answers::default();
    }
    let deadline = Instant::now() + TIMEOUT;
    let (truecolor, in_time) = if tier_forced { (false, true) } else { ask_truecolor(deadline) };
    if !in_time {
        return Answers { truecolor, colours: None, late: true };
    }
    let (colours, in_time) = ask_colours(deadline);
    Answers { truecolor, colours, late: !in_time }
}

/// Answers that come after the player stopped waiting. The terminal's still sends them, into what
/// reads keys, where each starts with what looks like Alt and a letter and goes on as typing. For
/// a little while, what starts that way is dropped, up to its end.
#[derive(Debug, Default)]
pub struct LateAnswers {
    until: Option<Instant>,
    inside: bool,
}

impl LateAnswers {
    pub fn after(answers: &Answers, now: Instant) -> LateAnswers {
        LateAnswers { until: answers.late.then_some(now + LATE), inside: false }
    }

    /// Whether `key` is part of a late answer, not something typed.
    pub fn swallows(&mut self, key: &KeyEvent, now: Instant) -> bool {
        let Some(until) = self.until else { return false };
        if now >= until {
            self.until = None;
            return false;
        }
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let opens = alt && matches!(key.code, KeyCode::Char(']' | 'P'));
        let closes = (alt && key.code == KeyCode::Char('\\'))
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('g'));
        let inside = self.inside || opens;
        self.inside = inside && !closes;
        inside
    }
}

#[cfg(all(test, unix))]
mod tests;
