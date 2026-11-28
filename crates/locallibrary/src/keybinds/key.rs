//! A key as `keybinds.json` and the keybinds record spell it, the same on every platform.

use std::fmt;

/// A key that has a name rather than being the character it types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Named {
    Space,
    Enter,
    Tab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
}

impl Named {
    pub const ALL: [Named; 14] = [
        Named::Space,
        Named::Enter,
        Named::Tab,
        Named::Backspace,
        Named::Delete,
        Named::Insert,
        Named::Home,
        Named::End,
        Named::PageUp,
        Named::PageDown,
        Named::Up,
        Named::Down,
        Named::Left,
        Named::Right,
    ];

    /// How it's spelled.
    pub fn name(self) -> &'static str {
        match self {
            Named::Space => "space",
            Named::Enter => "enter",
            Named::Tab => "tab",
            Named::Backspace => "backspace",
            Named::Delete => "delete",
            Named::Insert => "insert",
            Named::Home => "home",
            Named::End => "end",
            Named::PageUp => "pageup",
            Named::PageDown => "pagedown",
            Named::Up => "up",
            Named::Down => "down",
            Named::Left => "left",
            Named::Right => "right",
        }
    }

    fn parse(s: &str) -> Option<Named> {
        Named::ALL.into_iter().find(|n| n.name() == s)
    }
}

/// One key, as it's pressed. No `alt-`, and no sequences: terminals send alt as a separate key
/// first, which a list of keys can't tell from esc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// One character as typed: `a`, `A` (shift and the letter), `%`, `é`.
    Char(char),
    Named(Named),
    /// `shift-` and a named key: `shift-tab`, `shift-up`.
    Shift(Named),
    /// `ctrl-` and a lowercase letter.
    Ctrl(char),
    /// `f1` to `f12`.
    F(u8),
}

const SPELLINGS: &str = "one character, a key's name (enter, pageup, f5), shift- and a name, or ctrl- and a letter";

impl Key {
    /// `s` as a key: as [`Key`]'s `Display` writes it, around any blanks, with names in any case
    /// and `ctrl+` for `ctrl-` (as plugins' `keys.<name>` settings had them). Why not, when it
    /// isn't one or is [`Key::reserved`].
    pub fn parse(s: &str) -> Result<Key, String> {
        let s = s.trim();
        let lower = s.to_lowercase();
        let mut chars = s.chars();
        let one = chars.next().filter(|_| chars.next().is_none());
        let ctrl = lower.strip_prefix("ctrl-").or_else(|| lower.strip_prefix("ctrl+"));
        let f = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=12).contains(n));
        let key = match (one, ctrl, f) {
            (Some(c), ..) if !c.is_whitespace() && !c.is_control() => Key::Char(c),
            (_, Some(c), _) if c.len() == 1 && c.chars().all(|c| c.is_ascii_lowercase()) => {
                Key::Ctrl(c.chars().next().unwrap_or_default())
            }
            (_, _, Some(n)) if lower == format!("f{n}") => Key::F(n),
            _ => match (Named::parse(&lower), lower.strip_prefix("shift-").and_then(Named::parse)) {
                (Some(n), _) => Key::Named(n),
                (_, Some(n)) => Key::Shift(n),
                _ if lower.starts_with("alt-") || lower.starts_with("alt+") => {
                    return Err(format!("{s:?}: the player doesn't take alt- keys"));
                }
                _ if lower == "esc" || lower == "escape" => return Err("esc always closes".into()),
                _ => return Err(format!("{s:?} isn't a key: {SPELLINGS}")),
            },
        };
        match key.reserved() {
            Some(why) => Err(why.into()),
            None => Ok(key),
        }
    }

    /// Why no action can have this key, when none can: the player's fixed keys, and the ones a
    /// terminal sends as another key.
    pub fn reserved(&self) -> Option<&'static str> {
        match self {
            Key::Ctrl('c') => Some("ctrl-c always quits"),
            Key::Char('?') => Some("? always opens the help"),
            Key::Char(':') => Some(": always opens the commands"),
            Key::Ctrl('h') => Some("terminals send ctrl-h as backspace"),
            Key::Ctrl('i') => Some("terminals send ctrl-i as tab"),
            Key::Ctrl('m') => Some("terminals send ctrl-m as enter"),
            _ => None,
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Char(c) => write!(f, "{c}"),
            Key::Named(n) => f.write_str(n.name()),
            Key::Shift(n) => write!(f, "shift-{}", n.name()),
            Key::Ctrl(c) => write!(f, "ctrl-{c}"),
            Key::F(n) => write!(f, "f{n}"),
        }
    }
}
