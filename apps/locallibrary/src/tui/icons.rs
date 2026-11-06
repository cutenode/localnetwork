//! The player's symbols, each in three sets: a Nerd Font's, plain Unicode's and ASCII's. Which one
//! is drawn is `LOCALLIBRARY_ICONS`, else the settings', else what suits the terminal. Text
//! punctuation and what widgets draw isn't here: with ASCII, a last pass over the frame ([`fold`])
//! swaps whatever else is left, line drawing, blocks and typography, for its nearest in ASCII.

use std::sync::atomic::{AtomicU8, Ordering};

use ratatui::buffer::Buffer;

/// Which column of the table is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSet {
    Nerd,
    Unicode,
    Ascii,
}

impl IconSet {
    /// In the order of the `as u8` the player stores a set by.
    pub const ALL: [IconSet; 3] = [IconSet::Nerd, IconSet::Unicode, IconSet::Ascii];

    /// As `LOCALLIBRARY_ICONS` and the settings name it.
    pub fn parse(name: &str) -> Option<IconSet> {
        match name.trim().to_ascii_lowercase().as_str() {
            "nerd" => Some(IconSet::Nerd),
            "unicode" => Some(IconSet::Unicode),
            "ascii" => Some(IconSet::Ascii),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            IconSet::Nerd => "nerd",
            IconSet::Unicode => "unicode",
            IconSet::Ascii => "ascii",
        }
    }
}

/// `LOCALLIBRARY_ICONS`, the settings' and what's detected, by `IconSet as u8 + 1`.
static FORCED: AtomicU8 = AtomicU8::new(0);
static CHOSEN: AtomicU8 = AtomicU8::new(0);
static DETECTED: AtomicU8 = AtomicU8::new(IconSet::Unicode as u8 + 1);

#[cfg(test)]
thread_local! {
    /// A test's own set, so tests drawing in different sets can run side by side.
    static TEST_SET: std::cell::Cell<Option<IconSet>> = const { std::cell::Cell::new(None) };
}

fn code(set: Option<IconSet>) -> u8 {
    set.map_or(0, |s| s as u8 + 1)
}

/// What suits the terminal: ASCII for the Linux console, a real or emulated VT and a locale that
/// isn't UTF-8; a Nerd Font's only in the terminals that bring one along, whatever font they're
/// set to; else Unicode.
pub fn detect(var: impl Fn(&str) -> Option<String>) -> IconSet {
    let term = var("TERM").unwrap_or_default();
    let locale = ["LC_ALL", "LC_CTYPE", "LANG"].iter().find_map(|k| var(k).filter(|v| !v.is_empty()));
    let utf8 = locale.is_none_or(|l| l.to_lowercase().replace('-', "").contains("utf8"));
    if !utf8 || term == "linux" || term.starts_with("vt") {
        return IconSet::Ascii;
    }
    let bundled = matches!(var("TERM_PROGRAM").as_deref(), Some("ghostty" | "WezTerm"))
        || term == "xterm-kitty"
        || var("KITTY_WINDOW_ID").is_some();
    if bundled { IconSet::Nerd } else { IconSet::Unicode }
}

/// Read what to draw from the environment, as the player starts.
pub fn init() {
    let var = |k: &str| std::env::var(k).ok();
    FORCED.store(code(var("LOCALLIBRARY_ICONS").and_then(|s| IconSet::parse(&s))), Ordering::Relaxed);
    DETECTED.store(code(Some(detect(var))), Ordering::Relaxed);
}

/// Draw in the set the settings name, from the next frame (`None`: what suits the terminal).
pub fn choose(set: Option<IconSet>) {
    CHOSEN.store(code(set), Ordering::Relaxed);
}

/// The set the settings name.
pub fn chosen() -> Option<IconSet> {
    IconSet::ALL.get(usize::from(CHOSEN.load(Ordering::Relaxed)).checked_sub(1)?).copied()
}

/// The set drawn in.
pub fn set() -> IconSet {
    #[cfg(test)]
    if let Some(s) = TEST_SET.with(std::cell::Cell::get) {
        return s;
    }
    let forced = FORCED.load(Ordering::Relaxed);
    let chosen = CHOSEN.load(Ordering::Relaxed);
    let at = if forced != 0 {
        forced
    } else if chosen != 0 {
        chosen
    } else {
        DETECTED.load(Ordering::Relaxed)
    };
    IconSet::ALL[usize::from(at) - 1]
}

/// Draw this thread's frames in `set` (`None`: as the player would).
#[cfg(test)]
pub fn set_test_set(set: Option<IconSet>) {
    TEST_SET.with(|s| s.set(set));
}

/// A symbol in each set, alone, followed by a space, and with one on each side: a frame draws many,
/// and a string made for each would be an allocation in every row.
#[derive(Debug)]
pub struct Icon {
    alone: [&'static str; 3],
    spaced: [&'static str; 3],
    padded: [&'static str; 3],
}

impl Icon {
    pub fn get(&self) -> &'static str {
        self.alone[set() as usize]
    }

    pub fn spaced(&self) -> &'static str {
        self.spaced[set() as usize]
    }

    pub fn padded(&self) -> &'static str {
        self.padded[set() as usize]
    }
}

/// The table: a constant for each icon, and all of them for the tests.
macro_rules! icons {
    ($($(#[$doc:meta])* $name:ident: $nerd:literal $unicode:literal $ascii:literal,)*) => {
        $($(#[$doc])* pub const $name: Icon = Icon {
            alone: [$nerd, $unicode, $ascii],
            spaced: [concat!($nerd, " "), concat!($unicode, " "), concat!($ascii, " ")],
            padded: [concat!(" ", $nerd, " "), concat!(" ", $unicode, " "), concat!(" ", $ascii, " ")],
        };)*
        #[cfg(test)]
        pub const ALL: &[Icon] = &[$($name),*];
    };
}

icons! {
    PLAY: "\u{f04b}" "▶" ">",
    PAUSE: "\u{f04c}" "‖" "=",
    /// Stopped, and a colour's swatch in a legend.
    SQUARE: "\u{f04d}" "■" "#",
    /// A key of the deck strip that's off.
    HOLLOW: "\u{f096}" "□" ".",
    CHECK: "\u{f00c}" "✓" "+",
    CROSS: "\u{f00d}" "✖" "x",
    /// Keys that clash.
    CLASH: "\u{f00d}" "✕" "x",
    /// A check that stops a change.
    REFUSED: "\u{f00d}" "✗" "x",
    WARNING: "\u{f071}" "⚠" "!",
    /// Something that's on: a signed-in account, a drive plugged in.
    DOT: "\u{f111}" "●" "*",
    /// Something that's off.
    RING: "\u{f10c}" "○" "o",
    /// A message that wants an answer, a memory cue.
    DIAMOND: "\u{f219}" "◆" "*",
    STAR: "\u{f005}" "★" "*",
    /// A track kept out.
    BLOCKED: "\u{f05e}" "⊘" "/",
    /// Almost: a track with copies.
    NEAR: "≈" "≈" "~",
    /// Keys that mix; shuffle.
    MIX: "\u{f074}" "⇄" "~",
    REPEAT: "\u{f01e}" "↻" "@",
    /// Something under way.
    BUSY: "\u{f021}" "⟳" "*",
    /// The whole library.
    MUSIC: "\u{f001}" "♫" "*",
    /// A folder that's open, and one that's closed.
    OPEN: "\u{f0d7}" "▾" "v",
    CLOSED: "\u{f0da}" "▸" ">",
    LOGO: "\u{f192}" "◉" "@",
    /// A playlist, a list's point.
    BULLET: "\u{f111}" "•" "-",
    /// A drive that has gone its own way.
    FORK: "\u{f126}" "⑂" "Y",
    /// A drive ahead of the store, and one behind; a cue's marker.
    AHEAD: "\u{f0d8}" "▲" "^",
    BEHIND: "\u{f0d7}" "▼" "v",
    /// The selected row, and where typing goes.
    SELECTED: "▍" "▍" ">",
    CURSOR: "▏" "▏" "_",
    /// A phrase in a strip of them.
    PHRASE: "▮" "▮" "#",
    /// Something taken out.
    MINUS: "−" "−" "-",
}

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPINNER_ASCII: [&str; 4] = ["|", "/", "-", "\\"];

/// The frames of the spinner.
pub fn spinner() -> &'static [&'static str] {
    if set() == IconSet::Ascii { &SPINNER_ASCII } else { &SPINNER }
}

/// Whether the frame goes through [`fold`].
pub fn ascii() -> bool {
    set() == IconSet::Ascii
}

/// The nearest in ASCII of a character that isn't: a line for a line and a block for a block, so
/// frames and waveforms still look like them. Anything else, like a title's letters in a locale
/// that can't show them, is `?`.
fn ascii_of(c: char) -> char {
    match c {
        '─' | '━' | '–' | '−' | '·' | '•' | '▔' => '-',
        '│' | '┃' | '╷' | '╻' | '╵' | '╹' | '‖' | '▏' | '▍' => '|',
        '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '╭' | '╮' | '╯' | '╰' | '┏' | '┓' | '┗' | '┛' => {
            '+'
        }
        '▁' | '▂' | '▃' => '_',
        '▄' | '▅' | '▆' => '=',
        '▇' | '█' | '▉' | '▊' | '▋' | '▌' | '▐' | '▮' => '#',
        '▀' => '"',
        '░' => '.',
        '▒' => ':',
        '▓' => '%',
        '\u{2801}'..='\u{28ff}' => '*',
        '\u{2800}' => ' ',
        '●' | '◆' | '★' | '⟳' | '♫' => '*',
        '○' => 'o',
        '□' => '.',
        '■' => '#',
        '✓' => '+',
        '✖' | '✕' | '✗' => 'x',
        '⚠' => '!',
        '⊘' => '/',
        '↻' | '◉' => '@',
        '⑂' => 'Y',
        '…' | '≈' | '↔' | '⇄' => '~',
        '←' | '‹' => '<',
        '→' | '›' | '▶' | '▸' => '>',
        '↑' | '▲' => '^',
        '↓' | '▼' | '▾' => 'v',
        '“' | '”' => '"',
        '‘' | '’' => '\'',
        '×' => 'x',
        '±' => '+',
        '½' => '/',
        _ => '?',
    }
}

/// Swaps every cell of a finished frame that isn't ASCII for its nearest.
pub fn fold(buf: &mut Buffer) {
    for cell in &mut buf.content {
        if !cell.symbol().is_ascii() {
            let c = cell.symbol().chars().next().map_or('?', ascii_of);
            cell.set_char(c);
        }
    }
}

#[cfg(test)]
mod tests;
