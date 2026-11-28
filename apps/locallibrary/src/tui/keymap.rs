//! Every key the player answers to, in one table: which key, where it applies, what it does,
//! how the help, the footer and the command palette name it. Keys are looked up here and then
//! done by [`App::perform`](super::app::App::perform), or by the dialog that's open; the help
//! (`?`), the footer's hints and each dialog's bottom line are drawn from the same rows, so they
//! can't drift from what the keys do. A hint names what it does, and its key is the one that does
//! it where it's shown.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthStr;

use super::app::{App, DrivePane, Focus, View};
use super::choices::Hints;
use super::cli::Cli;
use super::deck::BandMode;
use super::keys::{KeyPalette, Notation};
use locallibrary::keybinds::{Key, Named};

/// Where a binding applies. The keys of the most specific scope win: a key bound in the crate
/// panel means that there, and what it means elsewhere everywhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Every view: playback, the deck, moving about, the views themselves.
    Global,
    /// The Library view, whatever has focus in it.
    Library,
    /// The Library view's drive list.
    DriveList,
    /// The Library view's next steps, with no drive open.
    Steps,
    /// The playlist tree.
    Tree,
    /// The track list.
    Tracks,
    /// The selected track's details panel.
    Details,
    /// The crate panel.
    Crate,
    /// The digit row plays hot cues (player mode, or with the pads on), not the views.
    Pads,
    /// Player mode.
    Deck,
    Prepare,
    Drives,
    Snapshots,
    /// The Drives view's list of what players recorded.
    History,
    Profile,
    Mixes,
    /// The offer to replay edits after a rekordbox export is showing.
    Replay,
    // Dialogs, each with the keys every dialog has (`Dialog`) and those for moving through its
    // list (`List`, or `Typed` where letters are typed). They answer before the view's keys.
    /// Every dialog: closing it, and doing what's selected.
    Dialog,
    /// A list in a dialog: the arrows, j k, a page, the ends.
    List,
    /// A list in a dialog typed into: the keys that type nothing.
    Typed,
    /// The actions menu (`e`), in every view: its letters.
    Actions,
    /// The rating or colour under the actions menu.
    SubMenu,
    /// A plan to confirm before it writes.
    Confirm,
    /// A question to answer yes or no.
    Ask,
    /// Results to read.
    Report,
    /// A form of fields.
    Form,
    /// The command palette and the other lists chosen from by typing.
    Picker,
    /// The jobs under way (`&`).
    Jobs,
    /// The help (`?`).
    Help,
    /// The messages since starting (by name in `:`).
    Messages,
    /// The outputs to play through (by name in `:`).
    Outputs,
    /// Typing a search (`/`).
    Search,
    /// A name being typed.
    Prompt,
    /// Picking the playlist to add to.
    Picking,
    /// The actions menu while a filter is typed: esc clears it before it closes the menu.
    Filter,
    /// The keys (`change the keys`, by name in `:`): typing narrows the actions.
    Keys,
    /// What a key pressed there would do, before it's kept: enter keeps it.
    KeysAnswer,
    // The Keys screen's other stages, for their hints: their keys are the answer's, or none.
    /// Waiting for the key to give an action: every key but esc is the answer.
    KeysCapture,
    /// An answer that puts an action's own keys back.
    KeysReset,
    /// An answer that swaps keys with another action.
    KeysSwap,
    /// An answer that can't be kept: enter waits for another key.
    KeysRefused,
}

impl Scope {
    /// Whether it's a dialog's: its keys answer only while that dialog is open.
    pub fn dialog(self) -> bool {
        self as u8 >= Scope::Dialog as u8
    }
}

/// The scopes a dialog looks its keys up in, most specific first.
pub const fn dialog_scopes(own: Scope, typed: bool) -> [Scope; 3] {
    [own, if typed { Scope::Typed } else { Scope::List }, Scope::Dialog]
}

/// The scopes each dialog looks its keys up in, and draws its hints from.
pub mod dialogs {
    use super::{Scope, dialog_scopes};

    pub const ACTIONS: [Scope; 3] = dialog_scopes(Scope::Actions, false);
    /// The actions menu while a filter is typed: its letters are typed, not chosen.
    pub const FILTERING: [Scope; 3] = dialog_scopes(Scope::Filter, true);
    pub const SUB_MENU: [Scope; 3] = dialog_scopes(Scope::SubMenu, false);
    pub const CONFIRM: [Scope; 3] = dialog_scopes(Scope::Confirm, false);
    pub const ASK: [Scope; 3] = dialog_scopes(Scope::Ask, false);
    pub const REPORT: [Scope; 3] = dialog_scopes(Scope::Report, false);
    pub const FORM: [Scope; 3] = dialog_scopes(Scope::Form, true);
    pub const PICKER: [Scope; 3] = dialog_scopes(Scope::Picker, true);
    pub const JOBS: [Scope; 3] = dialog_scopes(Scope::Jobs, false);
    pub const HELP: [Scope; 3] = dialog_scopes(Scope::Help, true);
    pub const MESSAGES: [Scope; 3] = dialog_scopes(Scope::Messages, false);
    pub const OUTPUTS: [Scope; 3] = dialog_scopes(Scope::Outputs, false);
    pub const SEARCH: [Scope; 3] = dialog_scopes(Scope::Search, true);
    pub const PROMPT: [Scope; 3] = dialog_scopes(Scope::Prompt, true);
    pub const PICKING: [Scope; 3] = dialog_scopes(Scope::Picking, false);
    pub const KEYS: [Scope; 3] = dialog_scopes(Scope::Keys, true);
    pub const KEYS_ANSWER: [Scope; 3] = dialog_scopes(Scope::KeysAnswer, false);
    pub const KEYS_CAPTURE: [Scope; 3] = dialog_scopes(Scope::KeysCapture, false);
    pub const KEYS_RESET: [Scope; 3] = dialog_scopes(Scope::KeysReset, false);
    pub const KEYS_SWAP: [Scope; 3] = dialog_scopes(Scope::KeysSwap, false);
    pub const KEYS_REFUSED: [Scope; 3] = dialog_scopes(Scope::KeysRefused, false);
}

/// A key a binding answers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum K {
    /// A character, typed as it is (uppercase letters come with shift).
    Ch(char),
    /// A key, with shift or without.
    Code(KeyCode),
    /// A key held with shift.
    Shift(KeyCode),
    /// A letter with control held.
    Ctrl(char),
    /// Any digit from the first to the second.
    Digits(char, char),
}

impl K {
    pub fn matches(self, key: &KeyEvent) -> bool {
        let ctrl = key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match self {
            K::Ch(c) => !ctrl && key.code == KeyCode::Char(c),
            K::Code(c) => !ctrl && key.code == c,
            K::Shift(c) => !ctrl && key.code == c && key.modifiers.contains(KeyModifiers::SHIFT),
            K::Ctrl(c) => key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char(c),
            K::Digits(a, b) => !ctrl && matches!(key.code, KeyCode::Char(d) if (a..=b).contains(&d)),
        }
    }

    /// How it's written in the footer, the palette and the menus.
    pub fn name(&self) -> String {
        let code = |c: &KeyCode| match c {
            KeyCode::Up => "↑".to_string(),
            KeyCode::Down => "↓".into(),
            KeyCode::Left => "←".into(),
            KeyCode::Right => "→".into(),
            KeyCode::PageUp => "pgup".into(),
            KeyCode::PageDown => "pgdn".into(),
            KeyCode::Delete => "del".into(),
            KeyCode::BackTab => "shift-tab".into(),
            KeyCode::F(n) => format!("f{n}"),
            other => format!("{other:?}").to_lowercase(),
        };
        match self {
            K::Ch(' ') => "space".into(),
            K::Ch(c) => c.to_string(),
            K::Code(c) => code(c),
            K::Shift(c) => format!("shift {}", code(c)),
            K::Ctrl(c) => format!("ctrl-{c}"),
            K::Digits(a, b) => format!("{a}-{b}"),
        }
    }

    /// The key as a terminal sends it, for a hint that's clicked.
    pub fn pressed(self) -> KeyEvent {
        match self {
            K::Ch(c) => KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            K::Ctrl(c) => KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL),
            K::Shift(c) => KeyEvent::new(c, KeyModifiers::SHIFT),
            K::Code(c) => KeyEvent::new(c, KeyModifiers::NONE),
            K::Digits(a, _) => KeyEvent::new(KeyCode::Char(a), KeyModifiers::NONE),
        }
    }

    /// The key that was pressed, as a binding would name it: a character as typed (shift makes the
    /// capital already), ctrl and a letter, or a key by its code, shift said for the named ones.
    /// None for what no binding can be (alt held, a key with no name).
    pub fn of(key: &KeyEvent) -> Option<K> {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            _ if key.modifiers.contains(KeyModifiers::ALT) => None,
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => Some(K::Ctrl(c.to_ascii_lowercase())),
            KeyCode::Char(c) => Some(K::Ch(c)),
            KeyCode::BackTab => Some(K::Code(KeyCode::BackTab)),
            code @ KeyCode::F(_) => Some(K::Code(code)),
            code if named(code).is_some() && shift => Some(K::Shift(code)),
            code if named(code).is_some() || code == KeyCode::Esc => Some(K::Code(code)),
            _ => None,
        }
    }

    /// As `keybinds.json` spells it: none for what it can't (esc, a run of digits).
    pub fn portable(self) -> Option<Key> {
        match self {
            K::Ch(' ') => Some(Key::Named(Named::Space)),
            K::Ch(c) => Some(Key::Char(c)),
            K::Code(KeyCode::BackTab) | K::Shift(KeyCode::BackTab) => Some(Key::Shift(Named::Tab)),
            K::Code(KeyCode::F(n)) => Some(Key::F(n)),
            K::Code(c) => named(c).map(Key::Named),
            K::Shift(c) => named(c).map(Key::Shift),
            K::Ctrl(c) => Some(Key::Ctrl(c)),
            K::Digits(..) => None,
        }
    }
}

/// The name `keybinds.json` has for a key the terminal sends as `code`.
fn named(code: KeyCode) -> Option<Named> {
    Named::ALL.into_iter().find(|n| code_of(*n) == code)
}

/// The code a terminal sends for the key `n`.
fn code_of(n: Named) -> KeyCode {
    match n {
        Named::Space => KeyCode::Char(' '),
        Named::Enter => KeyCode::Enter,
        Named::Tab => KeyCode::Tab,
        Named::Backspace => KeyCode::Backspace,
        Named::Delete => KeyCode::Delete,
        Named::Insert => KeyCode::Insert,
        Named::Home => KeyCode::Home,
        Named::End => KeyCode::End,
        Named::PageUp => KeyCode::PageUp,
        Named::PageDown => KeyCode::PageDown,
        Named::Up => KeyCode::Up,
        Named::Down => KeyCode::Down,
        Named::Left => KeyCode::Left,
        Named::Right => KeyCode::Right,
    }
}

impl From<Key> for K {
    fn from(key: Key) -> K {
        match key {
            Key::Char(c) => K::Ch(c),
            Key::Named(Named::Space) => K::Ch(' '),
            Key::Named(n) => K::Code(code_of(n)),
            Key::Shift(Named::Tab) => K::Code(KeyCode::BackTab),
            Key::Shift(n) => K::Shift(code_of(n)),
            Key::Ctrl(c) => K::Ctrl(c),
            Key::F(n) => K::Code(KeyCode::F(n)),
        }
    }
}

/// What a key (or a command in the palette) does. Some take what they need from the key that
/// was pressed (which digit, whether shift was held).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Nothing: a row for the help or the footer only.
    None,
    // The player
    Quit,
    Help,
    Palette,
    Messages,
    /// The job list.
    Jobs,
    ShowView(View),
    /// One of the library's actions menu's, from the palette, on what's selected.
    Edit(super::edits::Action),
    Refresh,
    // Playback
    PlayPause,
    /// Back (-1) or forward (1): 5 s, 30 s with shift.
    Seek(i8),
    Next,
    Prev,
    Stop,
    /// The next (true) or previous cue.
    Cue(bool),
    /// The next (true) or previous phrase.
    Phrase(bool),
    /// Up (1) or down (-1).
    Volume(i8),
    Shuffle,
    Repeat,
    /// Go to the playing track (in Mixes, the selected one).
    GoTo,
    // The deck
    Pads,
    /// The hot cue of the digit pressed: jump to it, or set it where it's empty (delete it after
    /// [`Act::DeleteCues`]).
    HotCue,
    /// `del` in player mode: the next pad pressed deletes its hot cue (pressed again, it doesn't).
    DeleteCues,
    /// Set a hot cue where the playhead is, on the first empty pad.
    SetHotCue,
    /// So many beats back (negative) or forward.
    BeatJump(i8),
    Quantize,
    Single,
    AutoCue,
    Outputs,
    Loop,
    /// Double (true) or halve the loop.
    LoopSize(bool),
    // Visuals
    Visualiser,
    /// The Library view's other layout.
    DeckLayout,
    /// Browse, over player mode.
    Browse,
    /// The next way of colouring waveforms.
    NextBands,
    /// How waveforms are coloured (`None`: as the drive says).
    Bands(Option<BandMode>),
    KeyNotation(Notation),
    KeyPalette(KeyPalette),
    /// Seconds before a track's end the deck warns of it (0: not at all).
    EndWarning(u16),
    /// Cover art on or off.
    Art(bool),
    /// Cover art in half blocks (`true`), or in the terminal's graphics when it has them.
    ArtHalfblocks(bool),
    /// Forget what's been played this session.
    ResetPlayed,
    /// Zoom in (true) or out.
    Zoom(bool),
    // Moving about
    /// One row up (-1) or down.
    Move(i8),
    /// A page up (-1) or down.
    Page(i8),
    /// The first (-1) or last row.
    End(i8),
    Activate,
    Escape,
    /// The next pane (the previous one with true).
    Focus(bool),
    // Library
    Search,
    Sort,
    SortReverse,
    Ratings,
    /// The My Tags column.
    TagsColumn,
    /// Give the tracks a My Tag, or take one off.
    Tag,
    Details,
    CratePanel,
    Mark,
    MarkAll,
    /// Add to the end of the queue (`a`).
    CrateAdd,
    /// Put at the front of the queue's picks, to play next.
    QueueNext,
    /// Fill the queue from matches when it runs out, on or off.
    Fill,
    Actions,
    /// `e` in a pane with no menu of its own: what enter does there, then the pane's keys.
    PaneActions,
    Remove,
    /// Move the selected crate track up (-1) or down.
    CrateMove(i8),
    CrateSave,
    CrateClear,
    /// Take back the last change made to the drive.
    Undo,
    // Drives (the open drive in the library, the selected one in the Drives view)
    DriveActions,
    Pull,
    Check,
    SetPrep,
    SyncGroup,
    Pack,
    RestorePlaylist,
    Replay,
    Eject,
    SmartList,
    SmartApply,
    SmartNew,
    ExportList,
    // Profile
    ProfileInclude,
    ProfileHideArtist,
    ProfileDocument,
    ProfileWrite,
    ProfileRounding,
    ProfileAccount,
    // Mixes
    MixOpen,
    MixZoom,
    MixExport,
    MixReport,
    /// Play the set, or pause it.
    MixPlay,
    /// The set's next (true) or previous track, while it plays.
    MixTrack(bool),
    /// The selected track, in the library.
    MixGoTo,
    /// Forget a recording from this computer, or the folder it was found in.
    MixForget,
    // Dialogs
    /// Yes (true) or no to what's asked.
    Answer(bool),
    /// The next (1) or previous (-1) of a field's choices.
    Change(i8),
    /// Cancel the job selected.
    CancelJob,
    /// Search the whole drive, or the list shown.
    SearchWhere,
    /// A plugin's command, by the plugin's id and its name (from the palette and the drive
    /// menu, never a key).
    Plugin(&'static str, &'static str),
    /// Run a plugin's analyzer (the plugin's id, the analyzer's name) on the drive's tracks.
    Analyze(&'static str, &'static str),
    /// Write the key and BPM a plugin's analyzer (the plugin's id, the analyzer's name) worked
    /// out to the drive's tracks.
    ApplyFields(&'static str, &'static str),
    /// Show or hide the column of a plugin's field.
    FieldColumn(locallibrary::query::FieldName),
    /// A plugin's settings (the plugin's id).
    PluginSettings(&'static str),
    /// Sign in to an account of a type in the browser (the type, the account plugin's id).
    AccountSignIn(&'static str, &'static str),
    /// Sign out of an account the store keeps, by its mark (see `tui::plugins::accounts::mark`).
    AccountSignOut(u64),
    /// Choose the accounts a plugin uses (the plugin's id).
    PluginAccess(&'static str),
    /// Plan what a plugin suggested, which it no longer keeps: the one waiting to be reviewed
    /// with this mark (see `tui::plugins::events::mark`), so a palette open as others come and go
    /// still plans the one picked.
    Review(u64),
    /// No longer keep what a plugin suggested, by its mark (as [`Act::Review`]).
    Dismiss(u64),
    /// The settings of every provider: when a candidate is picked for the user.
    ProviderSettings,
    /// Which plugins' checks run, and how long one has to answer.
    CheckSettings,
    /// Plan a fix the last check found (its place in [`super::app::App::fixes`]).
    Fix(usize),
    /// Draw in a theme, by its key (`<plugin>.<name>`; empty for the default).
    Theme(&'static str),
    /// Draw in a colour tier (`None`: the terminal's).
    Colours(Option<super::theme::Tier>),
    /// Draw in a set of icons (`None`: what suits the terminal).
    Icons(Option<super::icons::IconSet>),
    /// How much the footer says.
    Hints(Hints),
    /// The footer's `? help` with no hints, or not.
    HelpHint(bool),
    /// Open a folder holding an export, by a path typed.
    OpenFolder,
    /// The Drives view, on the store's snapshots.
    Snapshots,
    /// The localnetwork view, signing in.
    SignIn,
    /// Export the drive, or the set open, in a plugin's format (the plugin's id, the format, and
    /// which of the two).
    Export(&'static str, &'static str, loclib_plugin::ExportFrom),
    /// Import a file into the drive with a plugin's importer (the plugin's id, the format).
    Import(&'static str, &'static str),
    /// Build the drive from a plugin's library source (the plugin's id, the source).
    Source(&'static str, &'static str),
    /// Fetch what plugins' providers of a kind find for the tracks selected.
    Fetch(loclib_plugin::ProviderKind),
    /// One of the command line's jobs, run on the drive (see `tui::cli`).
    Run(super::cli::Cli),
    /// Show a command line to copy, for what the player doesn't do itself: `DRIVE` stands for
    /// the drive (see `tui::cli`).
    Copy(&'static str),
    /// The keys: change any of them (see `tui::keybinds`).
    Keys,
    /// Use keys someone published, by their handle.
    KeysImport,
    /// Every key back to its default.
    KeysReset,
    /// Keys made for a kind of keyboard (see `tui::keybinds::presets`).
    KeysPreset(Preset),
}

/// Keys made for a kind of keyboard. The defaults are [`Act::KeysReset`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// The digit row's keys unshifted on a French keyboard: & é " ' ( - è _.
    Azerty,
}

/// The help's sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Playback,
    Deck,
    Visuals,
    Moving,
    Player,
    Browsing,
    Crate,
    Changing,
    Prepare,
    Drives,
    Snapshots,
    History,
    Profile,
    Mixes,
    Dialogs,
    Commands,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Group::Playback => "Playback",
            Group::Deck => "Deck",
            Group::Visuals => "Visuals",
            Group::Moving => "Moving about",
            Group::Player => "The player",
            Group::Browsing => "Browsing",
            Group::Crate => "Marks and the queue",
            Group::Changing => "Changing the drive",
            Group::Prepare => "Prepare",
            Group::Drives => "Drives",
            Group::Snapshots => "Snapshots",
            Group::History => "History",
            Group::Profile => "localnetwork",
            Group::Mixes => "Recordings",
            Group::Dialogs => "In a dialog",
            Group::Commands => "Commands (type : and a few letters)",
        }
    }
}

/// A row of the table.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub scope: Scope,
    /// What it answers to; none for a row that only explains (help) or hints (footer).
    pub keys: &'static [K],
    /// The keys as the help and the footer write them (`n  p` for a pair, on the first of it).
    pub shown: &'static str,
    pub act: Act,
    /// What it does, for the command palette; empty to leave it out (the second of a pair, or a
    /// key that needs pressing, like a hot cue's digit).
    pub name: &'static str,
    /// The help's line; empty to leave it out.
    pub help: &'static str,
    pub group: Group,
    /// What it's called in a hint: on the first row of an act that has one, for every hint of
    /// that act; on a later row, only for that row (what enter does in this pane).
    pub label: &'static str,
    /// In the footer (or the dialog's bottom line), and how long it stays as it narrows (higher
    /// stays longer).
    pub hint: Option<u8>,
    /// More words the palette finds it by, comma-separated: what it's for, as someone looking for
    /// it might put it ("duplicates", "MP3 to FLAC").
    pub words: &'static str,
}

const fn key(
    scope: Scope,
    keys: &'static [K],
    shown: &'static str,
    act: Act,
    name: &'static str,
    help: &'static str,
    group: Group,
) -> Binding {
    Binding { scope, keys, shown, act, name, help, group, label: "", hint: None, words: "" }
}

/// A help line that isn't a key of its own (or one bound elsewhere, explained for this view).
const fn note(scope: Scope, shown: &'static str, help: &'static str, group: Group) -> Binding {
    key(scope, &[], shown, Act::None, "", help, group)
}

/// A hint here for `act`, bound in a wider scope: its key is the one that does it here, unless
/// `shown` writes a pair (`↑↓`, `n p`).
const fn tip(scope: Scope, shown: &'static str, act: Act, prio: u8) -> Binding {
    key(scope, &[], shown, act, "", "", Group::Player).hint(prio)
}

impl Binding {
    const fn hint(mut self, prio: u8) -> Binding {
        self.hint = Some(prio);
        self
    }

    const fn label(mut self, label: &'static str) -> Binding {
        self.label = label;
        self
    }

    /// More words the palette finds it by (see [`Binding::words`]).
    const fn words(mut self, words: &'static str) -> Binding {
        self.words = words;
        self
    }
}

use Act as A;
use Group as G;
use KeyCode as C;
use Scope as S;

/// Sign in/out in the Profile view (in `e` there, or `:`): signing in to the Atmosphere account and out.
#[cfg(feature = "account")]
#[rustfmt::skip]
const ACCOUNT_KEY: Binding = key(S::Profile, &[], "", A::ProfileAccount, "sign in to your Atmosphere account (or out)", "sign in with your Atmosphere account (the one for Bluesky and other AT Protocol apps; alpha); signed in, sign out (asks first)", G::Profile)
    .label("sign in/out").words("log in, logout, bluesky");

/// Built without the account (the `account` feature), it only says so.
#[cfg(not(feature = "account"))]
const ACCOUNT_KEY: Binding = key(
    S::Profile,
    &[],
    "",
    A::ProfileAccount,
    "the Atmosphere account (not in this build)",
    "the Atmosphere account: this locallibrary was built without it",
    G::Profile,
);

/// The table. Order matters: within a scope the first row a key matches wins, the first label an
/// act has is what its hints say, and the help and the footer list rows in this order.
#[rustfmt::skip]
pub static KEYMAP: &[Binding] = &[
    // -----------------------------------------------------------------------------------------
    // The Library view
    // -----------------------------------------------------------------------------------------
    // The offer to replay edits rekordbox exported over, while it's up.
    key(S::Replay, &[K::Ch('y')], "y", A::Replay, "", "replay edits rekordbox overwrote (while it says so)", G::Changing).label("replay edits").hint(10),
    tip(S::Replay, "", A::Escape, 9).label("not now"),
    // What the footer says in each pane (the keys are below).
    tip(S::Steps, "", A::Activate, 9).label("run"),
    tip(S::Tracks, "", A::Activate, 9).label("play"),
    tip(S::Tracks, "", A::Search, 8),
    tip(S::Tracks, "", A::Actions, 8),
    tip(S::Tracks, "", A::Mark, 7),
    tip(S::Tracks, "", A::CrateAdd, 7),
    tip(S::Tracks, "", A::Details, 5),
    tip(S::Tracks, "", A::CratePanel, 4),
    tip(S::Tracks, "", A::QueueNext, 3),
    tip(S::Tracks, "", A::Focus(false), 3),
    tip(S::Tracks, "", A::DeckLayout, 2),
    tip(S::Tree, "", A::Activate, 9).label("open"),
    tip(S::Tree, "", A::Actions, 8),
    tip(S::Tree, "", A::Remove, 6).label("delete"),
    tip(S::Tree, "", A::Search, 6),
    tip(S::Tree, "", A::Focus(false), 3),
    tip(S::Details, "", A::Activate, 9).label("play"),
    tip(S::Details, "", A::Actions, 8),
    tip(S::Details, "", A::CrateAdd, 7),
    tip(S::Details, "", A::Escape, 6),
    tip(S::Details, "", A::Focus(false), 3),
    tip(S::Crate, "", A::Activate, 9).label("play"),
    tip(S::Crate, "", A::QueueNext, 7),
    tip(S::Crate, "shift ↑↓", A::CrateMove(-1), 6),
    tip(S::Crate, "", A::Remove, 6),
    tip(S::Crate, "", A::Escape, 5),
    tip(S::Crate, "", A::Focus(false), 3),
    tip(S::DriveList, "", A::Activate, 9).label("browse"),
    tip(S::DriveList, "", A::Search, 6),
    tip(S::DriveList, "", A::Focus(false), 3),
    // The queue's own keys, while it has focus.
    key(S::Crate, &[K::Shift(C::Up)], "shift ↑↓", A::CrateMove(-1), "move up in the queue", "move a track up / down the queue", G::Crate).label("move"),
    key(S::Crate, &[K::Shift(C::Down)], "", A::CrateMove(1), "move down in the queue", "", G::Crate),
    key(S::Crate, &[K::Code(C::Delete), K::Code(C::Backspace)], "del", A::Remove, "", "in the queue: take the track out", G::Crate).label("remove"),
    key(S::Crate, &[], "", A::CrateSave, "save the queue to the drive as a playlist", "write the queue to the drive as a playlist (played tracks too)", G::Crate).label("save to drive").words("save the queue, queue to a playlist"),
    key(S::Crate, &[], "", A::CrateClear, "clear the queue", "clear the queue (the drive isn't touched; the list it plays on with stays)", G::Crate).label("clear").words("empty the queue"),
    key(S::Library, &[K::Ch('/')], "/", A::Search, "search or filter the tracks", "search title, artist, album, genre, label, key, comment and path as you type", G::Browsing).label("search"),
    note(S::Library, "", "or filter: bpm:120-128 key:8A~ rating:>=4 -genre:house tag:Deep", G::Browsing),
    note(S::Library, "", "in the box: ↑↓ and pgup pgdn move the list, enter keeps it, esc clears it, ctrl-u empties it, ctrl-w deletes a word, tab searches the whole drive or the list shown", G::Browsing),
    note(S::Library, "Browse", "at the top of the tree: by artist, album, genre, key, BPM, rating, colour, date added, My Tags, sets, history, problems", G::Browsing),
    key(S::Library, &[], "", A::OpenFolder, "open a folder holding an export", "open a folder holding a rekordbox export, by its path", G::Browsing).label("open…").words("open an export, load a folder"),
    key(S::Library, &[], "", A::ResetPlayed, "forget what's been played", "bring back the tracks dimmed as played since starting", G::Browsing).words("unplayed, forget played"),
    key(S::Library, &[], "", A::Sort, "sort by the next column", "sort by the next column / the other way (or click a header)", G::Browsing).label("sort").words("sort by, order"),
    key(S::Library, &[], "", A::SortReverse, "sort the other way", "", G::Browsing).words("reverse sort, descending"),
    key(S::Library, &[], "", A::Ratings, "show or hide the rating column", "show / hide the rating column", G::Browsing),
    key(S::Library, &[], "", A::TagsColumn, "show or hide the My Tags column", "show / hide the My Tags column", G::Browsing),
    note(S::Library, "", "green keys mix with the playing track, or the selected one (⇄ its key)", G::Browsing),
    key(S::Library, &[K::Ch('i')], "i", A::Details, "show or hide the track's details", "the selected track: details, playlists, what mixes into it (esc closes it)", G::Browsing).label("info").words("info"),
    key(S::Library, &[K::Ch('m'), K::Code(C::Insert)], "m  insert", A::Mark, "mark or unmark the track", "mark / unmark the selected track (in the tree: a playlist or folder, to pack)", G::Crate).label("mark"),
    key(S::Library, &[], "", A::MarkAll, "mark everything shown, or clear the marks", "mark everything shown / clear the marks", G::Crate).words("select all, mark all"),
    key(S::Library, &[K::Ch('a')], "a", A::CrateAdd, "add to the queue", "add the marked (or selected) tracks to the end of the queue", G::Crate).label("add to queue").words("tag list, crate"),
    key(S::Library, &[K::Ch('t')], "t", A::CratePanel, "show or hide the queue", "show / hide the queue (esc closes it)", G::Crate).label("queue").words("tag list, crate"),
    note(S::Library, "enter", "in the queue: play that track now (the others stay to play)", G::Crate),
    note(S::Library, "", "The drive is backed up before a change is written. A change that u takes back is made at once and says so; any other shows what it will do and asks first.", G::Changing),
    key(S::Library, &[K::Ch('e')], "e", A::Actions, "actions for the selection", "actions for the marked or selected tracks, the playlist shown and the queue, the likeliest first (enter runs it): rating, colour, add to a playlist, remove, new / rename / delete a playlist or folder, sort a playlist on the drive, plugins' operations", G::Changing).label("actions"),
    note(S::Library, "right-click", "the same, for what's under the pointer", G::Changing),
    key(S::Library, &[], "", A::Tag, "give the tracks a My Tag, or take one off", "give the marked (or selected) tracks a My Tag, or take one off (asks first)", G::Changing).label("My Tag").words("my tag, tag the tracks"),
    note(S::Library, "y", "after rekordbox exported over edits made here: replay them (it says so over the status line; esc puts it away)", G::Changing),
    key(S::Library, &[K::Code(C::Delete), K::Code(C::Backspace)], "del", A::Remove, "", "remove from the playlist shown; delete the selected playlist", G::Changing),
    // -----------------------------------------------------------------------------------------
    // The Prepare view
    // -----------------------------------------------------------------------------------------
    note(S::Prepare, "enter", "run the job selected, on the drive open in the library", G::Prepare),
    key(S::Prepare, &[K::Ch('e')], "e", A::PaneActions, "", "the same jobs, each with its letter", G::Prepare).label("actions"),
    tip(S::Prepare, "", A::Activate, 9).label("run"),
    tip(S::Prepare, "", A::PaneActions, 8),
    tip(S::Prepare, "", A::Refresh, 3),
    // -----------------------------------------------------------------------------------------
    // The Drives view
    // -----------------------------------------------------------------------------------------
    note(S::Drives, "enter", "open the drive in the library (a library: its newest clone)", G::Drives),
    key(S::Drives, &[K::Ch('e')], "e", A::DriveActions, "actions for the drive", "what can be done to the selected drive: save a snapshot, check, sync, pack, bring back a playlist, replay edits, undo, eject, plugins' commands; to a library's clones: sync, check, eject (each shows its plan and asks first)", G::Drives).label("actions"),
    tip(S::Drives, "", A::Activate, 9).label("browse"),
    tip(S::Drives, "", A::DriveActions, 8),
    tip(S::Drives, "", A::Undo, 6),
    tip(S::Drives, "", A::Refresh, 4),
    tip(S::Drives, "", A::Focus(false), 3),
    note(S::Snapshots, "", "every library in the store, its snapshots newest first (locallibrary pull adds one)", G::Snapshots),
    note(S::Snapshots, "enter", "bring back a playlist from the selected snapshot onto the drive open in the library", G::Snapshots),
    key(S::Snapshots, &[K::Ch('e')], "e", A::PaneActions, "", "actions for the selected snapshot", G::Snapshots),
    tip(S::Snapshots, "", A::Activate, 9).label("bring back"),
    tip(S::Snapshots, "", A::PaneActions, 8),
    tip(S::Snapshots, "", A::Refresh, 4),
    tip(S::Snapshots, "", A::Escape, 3).label("back"),
    note(S::History, "enter", "open the list players recorded in the library", G::History),
    key(S::History, &[K::Ch('e')], "e", A::PaneActions, "", "actions for the selected list", G::History),
    tip(S::History, "", A::Activate, 9).label("open"),
    tip(S::History, "", A::PaneActions, 8),
    tip(S::History, "", A::Escape, 3).label("back"),
    // -----------------------------------------------------------------------------------------
    // The localnetwork view
    // -----------------------------------------------------------------------------------------
    key(S::Profile, &[K::Ch('e')], "e", A::PaneActions, "", "these actions, for the section selected, with hiding an artist and rounding", G::Profile),
    key(S::Profile, &[], "", A::ProfileInclude, "put the section in the document, or take it out", "put the section in the document, or take it out", G::Profile).label("in document"),
    note(S::Profile, "tab  enter", "move between sections and their lists (esc goes back)", G::Profile),
    key(S::Profile, &[], "", A::ProfileDocument, "show the profile document", "show the document a public profile would get", G::Profile).label("document"),
    key(S::Profile, &[], "", A::ProfileWrite, "write the profile document to the store folder", "write the document as JSON in the store folder", G::Profile).label("save document"),
    ACCOUNT_KEY,
    // In the actions menu (`e`) and the palette.
    key(S::Profile, &[], "hide artist", A::ProfileHideArtist, "never name the artist", "never name an artist under Artists (still counted)", G::Commands),
    key(S::Profile, &[], "rounding", A::ProfileRounding, "rounded or exact numbers in the document", "rounded or exact numbers in the document", G::Commands),
    tip(S::Profile, "", A::Activate, 9).label("open"),
    tip(S::Profile, "", A::PaneActions, 8),
    tip(S::Profile, "", A::Refresh, 3),
    tip(S::Profile, "", A::Focus(false), 3),
    // -----------------------------------------------------------------------------------------
    // The Recordings view
    // -----------------------------------------------------------------------------------------
    note(S::Mixes, "", "A recorded set, and which of the open drive's tracks it plays (like locallibrary mix).", G::Mixes),
    note(S::Mixes, "enter", "on a recording: find its tracks; on a track: play the set from there", G::Mixes),
    key(S::Mixes, &[K::Ch('e')], "e", A::PaneActions, "", "actions for the selection, with exporting the set", G::Mixes),
    key(S::Mixes, &[K::Ch(' ')], "space", A::MixPlay, "play the set, or pause it", "play the set, or pause it", G::Mixes).label("play"),
    key(S::Mixes, &[K::Ch('n')], "n  p", A::MixTrack(true), "the set's next track", "while the set plays: its next / previous track", G::Mixes).label("next/prev track"),
    key(S::Mixes, &[K::Ch('p')], "", A::MixTrack(false), "the set's previous track", "", G::Mixes),
    key(S::Mixes, &[], "", A::MixOpen, "open a recording by path", "open a recording by path, or a folder of them (subfolders too) to keep listed", G::Mixes).label("open…").words("open a recording, load a set"),
    key(S::Mixes, &[], "", A::MixZoom, "zoom the set's timeline", "zoom the timeline: the whole set, 2×, 4×, 8×", G::Mixes).label("zoom").words("zoom the timeline"),
    key(S::Mixes, &[K::Code(C::Delete), K::Code(C::Backspace)], "del", A::MixForget, "", "on a recording from this computer: forget it, or the folder it was found in", G::Mixes).label("forget"),
    key(S::Mixes, &[], "", A::MixGoTo, "the selected track in the library", "go to the selected track in the library", G::Mixes).label("show in library").words("show in the library"),
    key(S::Mixes, &[], "", A::MixReport, "write the set's HTML report and open it", "write the HTML report and open it in a browser", G::Mixes).label("save report").words("html report"),
    note(S::Mixes, "click", "on the timeline: play from there, or pick the track under it", G::Mixes),
    // In the actions menu (`e`) and the palette.
    key(S::Mixes, &[], "export set", A::MixExport, "export the set for Local Network", "export the set as data (.mix.json) for Local Network", G::Commands),
    tip(S::Mixes, "", A::Activate, 9).label("open · play from"),
    tip(S::Mixes, "", A::PaneActions, 8),
    tip(S::Mixes, "", A::MixPlay, 7),
    tip(S::Mixes, "n p", A::MixTrack(true), 6),
    tip(S::Mixes, "", A::Focus(false), 3),
    tip(S::Mixes, "", A::MixForget, 2),
    // -----------------------------------------------------------------------------------------
    // Player mode (hints: the few a set needs; `?` has the rest)
    // -----------------------------------------------------------------------------------------
    tip(S::Deck, "1-8", A::HotCue, 9),
    tip(S::Deck, "", A::Loop, 8),
    tip(S::Deck, "", A::PlayPause, 8),
    tip(S::Deck, "↑↓ enter", A::Activate, 7).label("play next"),
    key(S::Deck, &[K::Ch('w')], "w", A::NextBands, "the waveform's next colours", "in player mode: the waveform's colours, BLUE, RGB, 3Band, as the drive says", G::Visuals),
    key(S::Deck, &[K::Ch('b')], "b", A::Browse, "", "browse the library over player mode; loading a track or esc comes back", G::Visuals).label("browse").hint(6),
    tip(S::Deck, "", A::Escape, 5),
    key(S::Deck, &[K::Code(C::Delete), K::Code(C::Backspace)], "del", A::DeleteCues, "delete a hot cue", "in player mode: the next pad pressed deletes its hot cue (the hot cues turn red; del again or esc doesn't)", G::Deck).label("delete cue").hint(4).words("hot cue, remove, clear"),
    key(S::Pads, &[K::Digits('1', '8')], "1 … 8", A::HotCue, "", "in player mode (browsing over it too), or with the pads on: hot cue A … H (jumps there and plays; an empty pad sets it where the playhead is)", G::Deck).label("hot cue"),
    tip(S::Pads, "1-8", A::HotCue, 9),
    // -----------------------------------------------------------------------------------------
    // Everywhere
    // -----------------------------------------------------------------------------------------
    key(S::Global, &[K::Code(C::Enter)], "enter", A::Activate, "", "play the selected track (queues the list it's in); open a playlist or folder; browse a drive", G::Playback),
    key(S::Global, &[K::Ch(' ')], "space", A::PlayPause, "play or pause", "pause / resume", G::Playback).label("pause"),
    key(S::Global, &[K::Code(C::Left)], "← →", A::Seek(-1), "seek back", "seek 5 s (shift: 30 s)", G::Playback).words("rewind"),
    key(S::Global, &[K::Code(C::Right)], "", A::Seek(1), "seek forward", "", G::Playback),
    key(S::Global, &[K::Ch('n')], "n  p", A::Next, "next track", "next / previous track", G::Playback).words("skip"),
    key(S::Global, &[K::Ch('p')], "", A::Prev, "previous track", "", G::Playback).words("skip back"),
    key(S::Global, &[K::Ch('c')], "c  h", A::Cue(true), "next cue", "jump to the next / previous cue", G::Playback),
    key(S::Global, &[K::Ch('h')], "", A::Cue(false), "previous cue", "", G::Playback).words("back to the cue, cue back, last cue"),
    key(S::Global, &[], "", A::Phrase(true), "next phrase", "jump to the next / previous phrase", G::Playback).words("jump a phrase, phrase forward"),
    key(S::Global, &[], "", A::Phrase(false), "previous phrase", "", G::Playback).words("phrase back"),
    key(S::Global, &[K::Ch('+'), K::Ch('=')], "+  -", A::Volume(1), "volume up", "volume", G::Playback),
    key(S::Global, &[K::Ch('-'), K::Ch('_')], "", A::Volume(-1), "volume down", "", G::Playback),
    key(S::Global, &[], "", A::Shuffle, "shuffle on or off", "shuffle / repeat (off, all, one)", G::Playback).words("random, shuffle play"),
    key(S::Global, &[], "", A::Repeat, "repeat: off, all, one", "", G::Playback).words("repeat one, repeat all"),
    key(S::Global, &[], "", A::Stop, "stop", "stop", G::Playback).words("stop playing"),
    key(S::Global, &[], "", A::GoTo, "go to the playing track", "go to the playing track", G::Playback).words("now playing, find the playing track"),
    key(S::Global, &[], "", A::Pads, "pads on or off", "pads on / off: the digit row plays hot cues instead of going to the views", G::Deck).words("hot cue pads, performance pads"),
    key(S::Global, &[K::Ch(',')], ",  .", A::BeatJump(-1), "beat jump back", "beat jump: one beat back / forward", G::Deck),
    key(S::Global, &[K::Ch('.')], "", A::BeatJump(1), "beat jump forward", "", G::Deck),
    key(S::Global, &[], "", A::BeatJump(-4), "a bar back", "one bar (4 beats) back / forward", G::Deck),
    key(S::Global, &[], "", A::BeatJump(4), "a bar forward", "", G::Deck),
    key(S::Global, &[], "", A::BeatJump(-32), "8 bars back", "8 bars (32 beats, a phrase) back / forward", G::Deck),
    key(S::Global, &[], "", A::BeatJump(32), "8 bars forward", "", G::Deck),
    key(S::Global, &[], "", A::Quantize, "quantize on or off", "quantize on / off: playing, cue, phrase and beat jumps keep the beat; paused, beat jumps land on one", G::Deck).words("snap to the beat, grid"),
    key(S::Global, &[K::Ch('l')], "l", A::Loop, "loop here, or exit the loop", "loop: the saved loop the playhead is in, else 4 beats from the nearest bar; again to exit", G::Deck).label("loop"),
    key(S::Global, &[], "", A::LoopSize(false), "halve the loop", "halve / double the loop", G::Deck).words("shorter loop, loop length"),
    key(S::Global, &[], "", A::LoopSize(true), "double the loop", "", G::Deck).words("longer loop, loop length"),
    note(S::Global, "", "A hot cue saved as a loop plays the loop. Seeking out of a loop leaves it.", G::Deck),
    key(S::Global, &[], "", A::Single, "autoplay on or off (single play)", "autoplay off (single play): stop after each track, the next one ready for space; again to play on", G::Deck).words("autoplay, single play, stop after each track"),
    key(S::Global, &[], "", A::AutoCue, "auto cue on or off", "auto cue: tracks start at their first memory cue, else their first beat", G::Deck).words("start at the first cue"),
    key(S::Global, &[], "", A::Outputs, "the output to play through", "the output: the system's default, or another (headphones, an interface), remembered", G::Deck).words("headphones, audio device, sound card, interface, speakers, output"),
    key(S::Global, &[], "", A::Visualiser, "the next visualiser", "spectrum · mirror · scope · none, in player mode where there's room", G::Visuals).words("spectrum, scope, mirror, visualizer"),
    key(S::Global, &[], "waveform colours", A::NextBands, "the waveform's next colours", "the waveform's colours: BLUE, RGB, 3Band, as the drive says", G::Commands),
    key(S::Global, &[], "", A::Zoom(false), "zoom the waveform out", "zoom the detail waveform out / in", G::Visuals).words("zoom out"),
    key(S::Global, &[], "", A::Zoom(true), "zoom the waveform in", "", G::Visuals).words("zoom in"),
    key(S::Global, &[K::Ch('f')], "f", A::DeckLayout, "open or close player mode", "player mode: the playing track on the whole screen, from any tab; again (or esc) to go back", G::Visuals).label("player mode").words("full screen, fullscreen"),
    key(S::Global, &[K::Code(C::Up), K::Ch('k')], "↑ ↓  j k", A::Move(-1), "", "move", G::Moving).label("move"),
    key(S::Global, &[K::Code(C::Down), K::Ch('j')], "", A::Move(1), "", "", G::Moving),
    key(S::Global, &[K::Code(C::PageUp)], "pgup  pgdn", A::Page(-1), "", "a page up / down", G::Moving),
    key(S::Global, &[K::Code(C::PageDown)], "", A::Page(1), "", "", G::Moving),
    key(S::Global, &[K::Code(C::Home)], "home  end", A::End(-1), "", "the first / last row", G::Moving),
    key(S::Global, &[K::Code(C::End)], "", A::End(1), "", "", G::Moving),
    key(S::Global, &[K::Code(C::Tab)], "tab", A::Focus(false), "", "next pane (shift-tab: back)", G::Moving).label("next pane"),
    key(S::Global, &[K::Code(C::BackTab)], "", A::Focus(true), "", "", G::Moving),
    key(S::Global, &[K::Code(C::Esc)], "esc", A::Escape, "", "back one step: leave player mode, stop a change before it writes, hide an error, clear the search, close the details or the queue, then clear the marks (twice); with nothing left, twice quits", G::Moving).label("close"),
    key(S::Global, &[K::Code(C::F(5))], "f5", A::Refresh, "refresh: read the drive and the view again", "read again: the drive, the drives' status, the snapshots, the profile, the recordings", G::Player).label("refresh"),
    key(S::Global, &[K::Ch('q')], "q", A::QueueNext, "play next", "play the marked (or selected) tracks next, before the other picks (in player mode, the match picked)", G::Playback).label("play next"),
    key(S::Global, &[K::Ch('1')], "1", A::ShowView(View::Library), "go to Library", "Library (in player mode the digits are hot cues)", G::Player),
    key(S::Global, &[K::Ch('2')], "2", A::ShowView(View::Prepare), "go to Prepare", "Prepare", G::Player),
    key(S::Global, &[K::Ch('3')], "3", A::ShowView(View::Drives), "go to Drives", "Drives, their snapshots and history", G::Player),
    key(S::Global, &[K::Ch('4')], "4", A::ShowView(View::Mixes), "go to Recordings", "Recordings", G::Player),
    key(S::Global, &[K::Ch('5')], "5", A::ShowView(View::Profile), "go to localnetwork", "localnetwork", G::Player).words("profile, publish"),
    key(S::Global, &[K::Ch(':')], ":", A::Palette, "", "commands: every action by name (type a few of its letters), the ones used lately first, plugins' commands and suggestions", G::Player).label("commands").hint(1),
    key(S::Global, &[K::Ch('?')], "?", A::Help, "help: the keys", "this help: type to narrow it, enter runs the row selected", G::Player).label("help").hint(1),
    note(S::Global, "", "with no hints the footer still says ? help; \"no ? help with no hints\" in : turns that off too", G::Player),
    key(S::Global, &[], "", A::Messages, "messages since starting", "messages: everything the status line said since starting", G::Player).words("log, errors, status"),
    key(S::Global, &[K::Ch('&')], "&", A::Jobs, "jobs: what's running", "jobs: what's running, with a cancel for each that can still stop", G::Player).words("cancel"),
    key(S::Global, &[], "", A::Keys, "change the keys", "change any key, or give an action one: pick it and press the new key", G::Player).words("keybinds, shortcuts, hotkeys, remap, rebind"),
    key(S::Global, &[], "", A::KeysImport, "import someone's keys…", "use keys someone published, by their handle (shows what changes first)", G::Commands).words("keybinds, import keys, copy keys"),
    key(S::Global, &[], "", A::KeysReset, "all keys back to their defaults", "every key as it came (asks first)", G::Commands).words("reset keys, default keys"),
    key(S::Global, &[], "", A::KeysPreset(Preset::Azerty), "keys for an AZERTY keyboard", "the views and hot cues on & é \" ' ( - è _ (shows what changes first)", G::Commands).words("french keyboard, azerty"),
    // Commands with no key of their own, for the drive open in the library (the selected one in
    // the Drives view).
    key(S::Global, &[], "pull", A::Pull, "save a snapshot of the drive", PULL, G::Commands).label("snapshot"),
    key(S::Global, &[], "", A::SetHotCue, "set a hot cue here", "a hot cue where the playhead is, on the first empty pad", G::Commands)
        .words("hot cue, pad, mark"),
    key(S::Global, &[], "check", A::Check, "check the drive for problems", CHECK, G::Commands).label("check"),
    key(S::Global, &[], "set prep", A::SetPrep, "check all the library's drives", "check the drive and the other mounted drives of its library together, for a set", G::Commands),
    key(S::Global, &[], "sync", A::SyncGroup, "sync the library's drives", SYNC, G::Commands).label("sync"),
    key(S::Global, &[], "pack", A::Pack, "pack playlists for a set…", PACK, G::Commands).label("pack"),
    key(S::Global, &[], "restore", A::RestorePlaylist, "bring back a playlist from a snapshot…", "a playlist or folder as a snapshot in the store had it, onto the drive", G::Commands),
    key(S::Global, &[], "fill", A::Fill, "fill the queue from matches, on or off", "when the queue runs out, play on with the best match for the last track that hasn't played (off until turned on)", G::Commands),
    key(S::Global, &[], "replay", A::Replay, "replay edits rekordbox overwrote", "make the drive's recorded edits again after rekordbox exported over them", G::Commands),
    key(S::Global, &[K::Ch('u')], "u", A::Undo, "undo the last change", "undo: take back the last change made, on the drive it was made to (before any, the open or selected drive's last), after showing what that puts back", G::Changing).label("undo"),
    key(S::Global, &[], "smart", A::SmartList, "smart playlists: the rules", "the rule-based playlists kept in the store", G::Commands),
    key(S::Global, &[], "smart apply", A::SmartApply, "fill the smart playlists on the drive", "fill the rule-based playlists on the drive from its own library (shows what changes first)", G::Commands),
    key(S::Global, &[], "smart new", A::SmartNew, "smart playlist from the search", "a rule-based playlist from the search box's filter and the list's sort", G::Commands),
    key(S::Global, &[], "export", A::ExportList, "export the list shown (M3U8, CSV, text)", "write the playlist, set, session or all tracks shown to a file on this computer", G::Commands),
    key(S::Global, &[], "eject", A::Eject, "eject the drive", EJECT, G::Commands).label("eject"),
    key(S::Global, &[], "", A::Snapshots, "browse snapshots", "the snapshots in the store, in the Drives view", G::Commands),
    key(S::Global, &[], "", A::SignIn, "sign in", "sign in to your Atmosphere account, in the localnetwork view", G::Commands),
    // The command line's jobs, run on the drive: reports in a panel, changes planned and confirmed.
    key(S::Global, &[], "doctor", A::Run(Cli::Doctor), "health report: duplicates, missing and orphaned files", "duplicates, missing and orphaned files, tracks in no playlist, files too big for FAT32 (results in a panel)", G::Commands)
        .words("duplicates, missing files, orphaned files, broken tracks, unfiled, health, problems"),
    key(S::Global, &[], "doctor --by-sound", A::Run(Cli::BySound), "find duplicates by their sound", "the same recording in other files and encodings, by its sound (decodes every track the first time)", G::Commands)
        .words("duplicates, same recording, FLAC and MP3, fingerprint"),
    key(S::Global, &[], "track dedupe", A::Run(Cli::Dedupe), "replace duplicates with the copy to keep", "replace each copy of a recording with the one doctor keeps, its playlists and My Tags with it (shows what changes first)", G::Commands)
        .words("duplicates, copies, merge duplicates, remove duplicates"),
    key(S::Global, &[], "track relocate", A::Run(Cli::Relocate), "find the tracks' missing files…", "point tracks whose file is missing at it, where it is on the drive or in a folder you name (shows what changes first)", G::Commands)
        .words("missing files, moved files, broken tracks, relink, locate"),
    key(S::Global, &[], "track upgrade", A::Run(Cli::Upgrade), "upgrade tracks to lossless files…", "point lossy tracks at lossless files of their recording in a folder you name, cues and beat grid kept (shows what changes first)", G::Commands)
        .words("MP3 to FLAC, lossless, FLAC, AIFF, WAV, better quality, replace lossy"),
    key(S::Global, &[], "refresh", A::Run(Cli::Refresh), "after a rekordbox export: put back what it replaced", "after rekordbox exported to the drive: remove its demo tracks, replay edits, fill smart playlists, save it (shows the steps first)", G::Commands)
        .words("rekordbox export, exported, replay edits, demo tracks"),
    key(S::Global, &[], "drive space", A::Run(Cli::Space), "what's using the drive's space", "what's using the drive's space, and when it will be full (results in a panel)", G::Commands)
        .words("free space, full, disk usage, size, room left"),
    key(S::Global, &[], "plays stats", A::Run(Cli::PlayStats), "play counts: most and never played", "the drive's most played tracks, and those never played (results in a panel)", G::Commands)
        .words("most played, never played, play count, statistics, history"),
    // What the player doesn't do itself: the command to run in a terminal, the drive filled in.
    key(S::Global, &[], "drive format", A::Copy("drive format DRIVE"), "format a drive (in a terminal)", "the command that erases and formats the drive", G::Commands)
        .words("erase, wipe, FAT32, exFAT, new drive"),
    key(S::Global, &[], "drive fsck", A::Copy("drive fsck DRIVE"), "check the drive's file system (in a terminal)", "the command that checks the drive's file system (--repair fixes it)", G::Commands)
        .words("file system, repair, corrupt, disk utility, chkdsk"),
    key(S::Global, &[], "drive firmware", A::Copy("drive firmware DRIVE <file.UPD>"), "put a player firmware update on the drive (in a terminal)", "the command that copies a player's firmware update onto the drive", G::Commands)
        .words("update, firmware update, UPD, CDJ update"),
    key(S::Global, &[], "provision", A::Copy("provision <new drive> --like DRIVE"), "make a new drive a copy of the library (in a terminal)", "the command that turns a blank drive into another copy of the drive's library", G::Commands)
        .words("new drive, blank drive, clone, set up a drive, another copy"),
    key(S::Global, &[], "store gc", A::Copy("store gc --dry-run"), "delete what nothing in the store uses (in a terminal)", "the command that frees the store's files no snapshot uses (--dry-run shows them)", G::Commands)
        .words("store size, free space, garbage, tidy the store"),
    key(S::Global, &[], "store verify", A::Copy("store verify"), "re-read every file in the store (in a terminal)", "the command that hashes every stored file again", G::Commands)
        .words("integrity, corrupt, check the store"),
    key(S::Global, &[], "store prune", A::Copy("store prune --dry-run"), "remove old snapshots (in a terminal)", "the command that removes old snapshots, keeping each library's latest and labelled ones (--dry-run shows them)", G::Commands)
        .words("old snapshots, store size, free space"),
    key(S::Global, &[], "store copy", A::Copy("store copy <folder>"), "copy the store to another folder (in a terminal)", "the command that copies the store to a NAS, another disk or a cloud-synced folder", G::Commands)
        .words("backup, back up, NAS, cloud, offsite, mirror"),
    key(S::Global, &[], "bug", A::Copy("bug"), "report a bug (in a terminal)", "the command that reports a bug, with this version and system filled in", G::Commands)
        .words("problem, issue, crash, feedback"),
    key(S::Global, &[K::Ctrl('c'), K::Ctrl('q')], "ctrl-c", A::Quit, "quit", "quit, also esc twice with nothing open, or ctrl-q (a write to the drive finishes first)", G::Player)
        .words("exit, close, leave"),
    // -----------------------------------------------------------------------------------------
    // The actions menu's letters (`e`): each runs its action when the menu offers it. A view's
    // own key for an action is the same letter here.
    // -----------------------------------------------------------------------------------------
    key(S::Actions, &[K::Ch('/')], "/", A::Search, "", "in the actions menu (e): each action's letter runs it, and / filters it by what's typed", G::Dialogs).label("filter").hint(3),
    key(S::Actions, &[K::Ch('e')], "", A::Escape, "", "", G::Dialogs),
    tip(S::Actions, "", A::Activate, 2).label("run"),
    tip(S::Actions, "esc", A::Escape, 1),
    key(S::Actions, &[K::Ch('c')], "", A::Check, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('a')], "", A::SetPrep, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('g')], "", A::SyncGroup, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('p')], "", A::Pack, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('s')], "", A::Pull, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('b')], "", A::RestorePlaylist, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('y')], "", A::Replay, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('u')], "", A::Undo, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('d')], "", A::Eject, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('r')], "", A::Refresh, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('m')], "", A::ProfileInclude, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('i')], "", A::ProfileDocument, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('h')], "", A::ProfileHideArtist, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('#')], "", A::ProfileRounding, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('w')], "", A::ProfileWrite, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('l')], "", A::ProfileAccount, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('f')], "", A::MixOpen, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('z')], "", A::MixZoom, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('o')], "", A::MixGoTo, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('x')], "", A::MixExport, "", "", G::Dialogs),
    key(S::Actions, &[K::Ch('w')], "", A::MixReport, "", "", G::Dialogs),
    key(S::SubMenu, &[K::Code(C::Left), K::Code(C::Backspace)], "", A::Escape, "", "", G::Dialogs),
    tip(S::SubMenu, "", A::Activate, 2).label("set"),
    tip(S::SubMenu, "esc", A::Escape, 1).label("back"),
    // -----------------------------------------------------------------------------------------
    // Dialogs: every one closes on esc and does what's selected on enter; lists move alike
    // -----------------------------------------------------------------------------------------
    key(S::Confirm, &[K::Ch('y')], "y  n", A::Answer(true), "", "in a plan or a question: yes (once it's been up long enough to read) / no", G::Dialogs).label("yes").hint(9),
    key(S::Confirm, &[K::Ch('n')], "", A::Answer(false), "", "", G::Dialogs).label("no").hint(8),
    key(S::Confirm, &[K::Ch(' ')], "", A::Page(1), "", "", G::Dialogs),
    tip(S::Confirm, "esc", A::Escape, 1).label("cancel"),
    key(S::Ask, &[K::Ch('y')], "", A::Answer(true), "", "", G::Dialogs).hint(9),
    key(S::Ask, &[K::Ch('n')], "", A::Answer(false), "", "", G::Dialogs).hint(8),
    tip(S::Ask, "esc", A::Escape, 1).label("cancel"),
    key(S::Report, &[K::Ch(' ')], "space", A::Page(1), "", "in results or the messages: a page down", G::Dialogs),
    key(S::Report, &[K::Code(C::Enter)], "", A::Escape, "", "", G::Dialogs),
    tip(S::Report, "↑↓", A::Move(-1), 8).label("scroll"),
    tip(S::Report, "esc", A::Escape, 1),
    key(S::Messages, &[K::Code(C::Enter)], "", A::Escape, "", "", G::Dialogs),
    key(S::Messages, &[K::Ch(' ')], "", A::Page(1), "", "", G::Dialogs),
    tip(S::Messages, "↑↓", A::Move(-1), 8).label("scroll"),
    tip(S::Messages, "esc", A::Escape, 1),
    key(S::Form, &[K::Code(C::Tab)], "tab", A::Move(1), "", "in a form: the next field (shift-tab: back), ←→ or space change a choice", G::Dialogs),
    key(S::Form, &[K::Code(C::BackTab)], "", A::Move(-1), "", "", G::Dialogs),
    key(S::Form, &[K::Code(C::Right), K::Ch(' ')], "", A::Change(1), "", "", G::Dialogs).label("change"),
    key(S::Form, &[K::Code(C::Left)], "", A::Change(-1), "", "", G::Dialogs),
    tip(S::Form, "↑↓", A::Move(-1), 7).label("field"),
    tip(S::Form, "←→", A::Change(1), 6),
    tip(S::Form, "", A::Activate, 2),
    tip(S::Form, "esc", A::Escape, 1).label("cancel"),
    key(S::Picker, &[K::Ch(' ')], "space", A::Mark, "", "in a list some of which may be chosen: mark one", G::Dialogs).hint(3),
    tip(S::Picker, "", A::Activate, 2),
    tip(S::Picker, "esc", A::Escape, 1),
    key(S::Jobs, &[K::Ch('x'), K::Code(C::Delete)], "x", A::CancelJob, "", "in the jobs: cancel the one selected", G::Dialogs).label("cancel job").hint(8),
    key(S::Jobs, &[K::Ch('&')], "", A::Escape, "", "", G::Dialogs),
    tip(S::Jobs, "esc", A::Escape, 1),
    key(S::Help, &[K::Ch('?')], "", A::Escape, "", "", G::Dialogs),
    tip(S::Help, "", A::Activate, 2).label("run it"),
    tip(S::Help, "esc", A::Escape, 1),
    tip(S::Outputs, "", A::Activate, 2).label("play through it"),
    tip(S::Outputs, "esc", A::Escape, 1),
    key(S::Search, &[K::Code(C::Tab), K::Code(C::BackTab)], "", A::SearchWhere, "", "", G::Dialogs).label("whole drive").hint(7),
    tip(S::Search, "", A::Activate, 2).label("keep"),
    tip(S::Search, "esc", A::Escape, 1).label("clear"),
    tip(S::Prompt, "", A::Activate, 2).label("ok"),
    tip(S::Prompt, "esc", A::Escape, 1).label("cancel"),
    tip(S::Picking, "", A::Activate, 2).label("choose"),
    tip(S::Picking, "esc", A::Escape, 1).label("cancel"),
    tip(S::Filter, "esc", A::Escape, 1).label("clear"),
    tip(S::Keys, "", A::Activate, 2).label("change"),
    tip(S::Keys, "esc", A::Escape, 1).label("close"),
    tip(S::KeysAnswer, "", A::Activate, 2).label("save"),
    tip(S::KeysAnswer, "esc", A::Escape, 1).label("cancel"),
    tip(S::KeysCapture, "esc", A::Escape, 1).label("cancel"),
    tip(S::KeysReset, "", A::Activate, 2).label("reset"),
    tip(S::KeysReset, "esc", A::Escape, 1).label("cancel"),
    tip(S::KeysSwap, "", A::Activate, 2).label("swap"),
    tip(S::KeysSwap, "esc", A::Escape, 1).label("cancel"),
    tip(S::KeysRefused, "", A::Activate, 2).label("try another"),
    tip(S::KeysRefused, "esc", A::Escape, 1).label("cancel"),
    key(S::List, &[K::Code(C::Up), K::Ch('k')], "↑ ↓  j k", A::Move(-1), "", "move; pgup pgdn a page, home end the ends (in a list you type into, the arrows and those keys only)", G::Dialogs),
    tip(S::List, "↑↓", A::Move(-1), 5),
    key(S::List, &[K::Code(C::Down), K::Ch('j')], "", A::Move(1), "", "", G::Dialogs),
    key(S::List, &[K::Code(C::PageUp)], "", A::Page(-1), "", "", G::Dialogs),
    key(S::List, &[K::Code(C::PageDown)], "", A::Page(1), "", "", G::Dialogs),
    key(S::List, &[K::Code(C::Home)], "", A::End(-1), "", "", G::Dialogs),
    key(S::List, &[K::Code(C::End)], "", A::End(1), "", "", G::Dialogs),
    key(S::Typed, &[K::Code(C::Up)], "", A::Move(-1), "", "", G::Dialogs),
    tip(S::Typed, "↑↓", A::Move(-1), 5),
    key(S::Typed, &[K::Code(C::Down)], "", A::Move(1), "", "", G::Dialogs),
    key(S::Typed, &[K::Code(C::PageUp)], "", A::Page(-1), "", "", G::Dialogs),
    key(S::Typed, &[K::Code(C::PageDown)], "", A::Page(1), "", "", G::Dialogs),
    key(S::Typed, &[K::Code(C::Home)], "", A::End(-1), "", "", G::Dialogs),
    key(S::Typed, &[K::Code(C::End)], "", A::End(1), "", "", G::Dialogs),
    key(S::Dialog, &[K::Code(C::Esc)], "esc", A::Escape, "", "close a dialog (cancel a plan, a prompt, a search)", G::Dialogs).hint(1),
    key(S::Dialog, &[K::Code(C::Enter)], "enter", A::Activate, "", "do what's selected", G::Dialogs).label("ok"),
];

/// What the drive's jobs do, in the help under each of their homes.
const PULL: &str = "save the drive in the store, a snapshot of its library (shows what that adds first)";
const CHECK: &str = "check the drive against the player models remembered for it (results in a panel)";
const SYNC: &str = "copy the newest drive of the library over the others plugged in (when none is newest, says why)";
const PACK: &str = "the marked playlists, the playlist shown or the queue, from the open drive onto another";
const EJECT: &str = "unmount the drive and power it off (playing from it stops first)";

/// What decides the scopes keys are looked up in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Situation {
    /// The offer to replay edits is up.
    pub replay: bool,
    /// The deck has the screen (player mode, Browse closed).
    pub deck: bool,
    pub view: View,
    pub focus: Focus,
    /// The library has nothing open: the track list shows the next steps.
    pub empty: bool,
    /// The digit row plays hot cues.
    pub pads: bool,
    pub drive_pane: DrivePane,
}

/// The scopes keys are looked up in now, most specific first.
pub fn scopes(app: &App) -> Vec<Scope> {
    scopes_of(&situation(app))
}

/// What decides the scopes keys are looked up in now: what's kept drawn from the keys is kept for it.
pub fn situation(app: &App) -> Situation {
    Situation {
        replay: app.replay_offer.is_some(),
        deck: app.deck_showing(),
        view: app.view,
        focus: app.focus,
        empty: app.empty(),
        pads: app.pads_on(),
        drive_pane: app.drive_pane,
    }
}

/// The scopes keys are looked up in, in situation `s`, most specific first.
pub fn scopes_of(s: &Situation) -> Vec<Scope> {
    let mut v = Vec::new();
    if s.replay {
        v.push(Scope::Replay);
    }
    if s.deck {
        v.push(Scope::Deck);
    }
    if s.view == View::Library {
        v.push(match s.focus {
            Focus::Drives => Scope::DriveList,
            Focus::Playlists => Scope::Tree,
            Focus::Tracks if s.empty => Scope::Steps,
            Focus::Tracks => Scope::Tracks,
            Focus::Details => Scope::Details,
            Focus::Crate => Scope::Crate,
        });
    }
    if s.pads {
        v.push(Scope::Pads);
    }
    match s.view {
        View::Library => v.push(Scope::Library),
        View::Prepare => v.push(Scope::Prepare),
        View::Drives => v.push(match s.drive_pane {
            DrivePane::List | DrivePane::Info => Scope::Drives,
            DrivePane::Snapshots => Scope::Snapshots,
            DrivePane::History => Scope::History,
        }),
        View::Profile => v.push(Scope::Profile),
        View::Mixes => v.push(Scope::Mixes),
    }
    v.push(Scope::Global);
    v
}

/// What `act` is called in the palette and the menus that offer it.
pub fn name(act: Act) -> &'static str {
    KEYMAP.iter().find(|b| b.act == act && !b.name.is_empty()).map_or("", |b| b.name)
}

/// What the help says of `act`.
pub fn about(act: Act) -> &'static str {
    KEYMAP.iter().find(|b| b.act == act && !b.help.is_empty()).map_or("", |b| b.help)
}

/// The key `act` answers to in the first of `scopes` (most specific first) that has one.
pub fn key_for(scopes: &[Scope], act: Act) -> Option<K> {
    let e = effective::current();
    let found = e.key_row(scopes, act).map(|i| e.rows()[i].keys[0]);
    #[cfg(test)]
    tests::looked_up(scopes, act, found);
    found
}

/// The key `act` answers to in `scopes`, written as text that names it: none when it has none
/// there.
pub fn key_name(scopes: &[Scope], act: Act) -> Option<String> {
    key_for(scopes, act).map(|k| k.name())
}

/// What to type in `:` to find `act`, for text that names it where it has no key: its label, or
/// its name when it has no label. Nothing when `:` doesn't list it.
pub fn typed_name(act: Act) -> Option<&'static str> {
    let label = label(act);
    match name(act) {
        "" => None,
        name => Some(if label.is_empty() { name } else { label }),
    }
}

/// The key that goes to `view`: empty when it has none.
pub fn view_key(view: View) -> String {
    key_name(&[Scope::Global], Act::ShowView(view)).unwrap_or_default()
}

/// What a key does, and the scope it does it in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub act: Act,
    pub scope: Scope,
}

/// What `key` does in `scopes` (most specific first).
pub fn lookup(scopes: &[Scope], key: &KeyEvent) -> Option<Hit> {
    let e = effective::current();
    e.hit(scopes, key).map(|i| Hit { act: e.rows()[i].act, scope: e.rows()[i].scope })
}

/// What `key` does in the dialog whose scopes are `scopes` (see [`dialog_scopes`]).
pub fn dialog_act(scopes: &[Scope], key: &KeyEvent) -> Option<Act> {
    lookup(scopes, key).map(|h| h.act)
}

/// Whether `key` quits from anywhere, a dialog open or not: ctrl-c, and the quit's other keys
/// that type nothing (ctrl and a letter, an f key).
pub fn quits(key: &KeyEvent) -> bool {
    let types_nothing = |k: &K| matches!(k, K::Ctrl(_) | K::Code(KeyCode::F(_)));
    let keys = effective::current().keys(Act::Quit);
    K::Ctrl('c').matches(key) || keys.iter().any(|k| types_nothing(k) && k.matches(key))
}

/// Which hot cue `key` plays: its place among the hot cues' keys.
pub fn pad_of(key: &KeyEvent) -> Option<u8> {
    let keys = effective::current().keys(Act::HotCue);
    keys.iter().position(|k| k.matches(key)).and_then(|i| u8::try_from(i).ok())
}

/// `act`'s letter in an actions menu as it came: its row among the menu's letters.
fn default_letter(act: Act) -> String {
    let row = KEYMAP.iter().find(|b| b.scope == Scope::Actions && b.act == act && !b.keys.is_empty());
    row.map_or(String::new(), |b| b.keys[0].name())
}

/// The letters an actions menu offering `menu` (each act with the letter it came with) writes
/// beside them: an act's own key where it's been changed to a character, else the letter it came
/// with. A changed key claims its letter first, so another act that came with it shows none.
pub fn letters(menu: &[(Act, String)]) -> Vec<String> {
    let e = effective::current();
    let own = |act: Act| match e.keys(act).first() {
        Some(K::Ch(c)) if e.changed(act) && *c != ' ' => Some(c.to_string()),
        _ => None,
    };
    let claimed: Vec<String> = menu.iter().filter_map(|(act, _)| own(*act)).collect();
    let letter = |(act, came): &(Act, String)| match own(*act) {
        Some(l) => l,
        None if claimed.contains(came) => String::new(),
        None => came.clone(),
    };
    menu.iter().map(letter).collect()
}

/// The letters of an actions menu offering `acts` (see [`letters`]).
pub fn menu_letters(acts: &[Act]) -> Vec<String> {
    letters(&acts.iter().map(|a| (*a, default_letter(*a))).collect::<Vec<_>>())
}

/// Which of `offered` an actions menu's letter `key` runs: the act [`menu_letters`] gives it, so
/// two views' menus may give one letter to their own acts (`w` saves the profile in localnetwork
/// and the report in Recordings).
pub fn menu_act(offered: &[Act], key: &KeyEvent) -> Option<Act> {
    let Some(K::Ch(c)) = K::of(key) else { return None };
    let typed = c.to_string();
    offered.iter().zip(menu_letters(offered)).find(|(_, l)| *l == typed).map(|(a, _)| *a)
}

/// A row that only hints at a key bound in a wider scope.
fn is_tip(b: &Binding) -> bool {
    b.keys.is_empty() && b.name.is_empty() && b.help.is_empty()
}

/// What `act` is called in a hint: the first label a row of its own gives it.
pub fn label(act: Act) -> &'static str {
    KEYMAP.iter().find(|b| b.act == act && !b.label.is_empty() && !is_tip(b)).map_or("", |b| b.label)
}

/// A hint: the keys as written, what it's called and what it does (a click does that).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub keys: String,
    pub label: &'static str,
    pub act: Act,
    prio: u8,
}

/// Row `i`'s hint where `scopes` apply: its own key, its pair as written, or the key that does its
/// act there (with its pair's, or the first and last hot cue's, once one of them has changed).
fn hint_of(e: &Effective, i: usize, scopes: &[Scope]) -> Option<Hint> {
    let row = &e.rows()[i];
    let b = row.b?;
    let prio = b.hint?;
    let keys = match (row.keys.first(), b.shown) {
        (Some(k), _) => k.name(),
        (None, shown) if !shown.is_empty() && !e.pair_changed(scopes, row.act) => {
            shown.split_whitespace().collect::<Vec<_>>().join(" ")
        }
        (None, shown) if !shown.is_empty() => e.pair_written(scopes, row.act, " ")?,
        (None, _) => e.rows()[e.key_row(scopes, row.act)?].keys[0].name(),
    };
    let label = if b.label.is_empty() { label(row.act) } else { b.label };
    Some(Hint { keys, label, act: row.act, prio })
}

/// The hints of the rows in `scopes` (most specific first), each key and act once.
fn hints_in(scopes: &[Scope]) -> Vec<Hint> {
    let e = effective::current();
    let mut out: Vec<Hint> = Vec::new();
    for s in scopes {
        for h in e.in_scope(*s).filter_map(|i| hint_of(&e, i, scopes)) {
            if !out.iter().any(|o| o.keys == h.keys || o.act == h.act) {
                out.push(h);
            }
        }
    }
    out
}

/// `act`'s hint where `scopes` apply, called `label`: what a line of the screen says beside its key.
pub fn hint_for(scopes: &[Scope], act: Act, label: &'static str) -> Option<Hint> {
    Some(Hint { keys: key_for(scopes, act)?.name(), label, act, prio: 0 })
}

/// Where a hint goes in the footer: what enter does first, then the actions, then the rest as the
/// table has them.
fn place_of(act: Act) -> u8 {
    match act {
        Act::Activate => 0,
        Act::Actions | Act::PaneActions | Act::DriveActions => 1,
        _ => 2,
    }
}

/// The most hints the compact footer holds.
const COMPACT: usize = 5;

/// What the footer always ends with, whatever the width.
const PINNED: [Act; 2] = [Act::Palette, Act::Help];

/// The footer's hints now: the view's own first, as many as the setting and `width` columns allow
/// (the least important go first), then the commands and the help, always.
pub fn hints(app: &App, width: usize) -> Vec<Hint> {
    if app.hints == Hints::Off {
        return Vec::new();
    }
    let most = if app.hints == Hints::Compact { COMPACT } else { usize::MAX };
    // Player mode hints its own keys, the pads' among them: the hot cues' hint is written from the
    // keys that play them there.
    let scopes = if app.deck_showing() { vec![Scope::Deck, Scope::Pads, Scope::Global] } else { scopes(app) };
    let (pinned, mut out): (Vec<Hint>, Vec<Hint>) =
        hints_in(&scopes).into_iter().partition(|h| PINNED.contains(&h.act));
    // Player mode keeps its own order: the hot cues lead.
    if !app.deck_showing() {
        out.sort_by_key(|h| place_of(h.act));
    }
    // Each hint takes its keys, a space, its label and two spaces; the footer starts with one.
    let cost = |h: &Hint| h.keys.width() + h.label.width() + 3;
    let fixed = 1 + pinned.iter().map(cost).sum::<usize>();
    let over = |out: &[Hint]| out.len() + pinned.len() > most || fixed + out.iter().map(cost).sum::<usize>() > width;
    while !out.is_empty() && over(&out) {
        let lowest = out.iter().map(|h| h.prio).min().unwrap_or(0);
        let i = out.iter().rposition(|h| h.prio == lowest).unwrap_or(0);
        out.remove(i);
    }
    out.extend(pinned.into_iter().filter(|_| fixed <= width));
    out
}

/// A dialog's hints in `scopes` (see [`dialog_scopes`]), the most important first, esc last.
pub fn dialog_hints(scopes: &[Scope]) -> Vec<Hint> {
    let mut out = hints_in(scopes);
    out.sort_by_key(|h| std::cmp::Reverse(h.prio));
    out
}

/// One section of the key tables: its title and its lines (keys, what they do).
#[cfg(test)]
pub type HelpSection = (&'static str, Vec<(&'static str, &'static str)>);

#[cfg(test)]
fn sections(rows: impl Iterator<Item = &'static Binding>) -> Vec<HelpSection> {
    let mut out: Vec<HelpSection> = Vec::new();
    for b in rows.filter(|b| !b.help.is_empty()) {
        let title = help_title(by_name(b), b.group);
        match out.iter_mut().find(|(t, _)| *t == title) {
            Some((_, lines)) => lines.push((written(b), b.help)),
            None => out.push((title, vec![(written(b), b.help)])),
        }
    }
    out
}

/// What has no key of its own: found by name in the palette (`:`).
fn by_name(b: &Binding) -> bool {
    b.keys.is_empty() && b.shown.is_empty() && b.act != Act::None
}

/// How the help writes what's found by name, in place of its keys.
const BY_NAME: &str = ":";

/// The heading a row of the help is under: what it's for, or, with no key, that it's found by name.
fn help_title(by_name: bool, group: Group) -> &'static str {
    if by_name { BY_NAME_TITLE } else { group.title() }
}

/// The heading of what's found by name.
const BY_NAME_TITLE: &str = "By name, in :";

/// How the help writes a row's keys as they came: `:` for what's found by name in the palette.
pub fn written(b: &Binding) -> &'static str {
    if by_name(b) { BY_NAME } else { b.shown }
}

/// A row of the help.
pub struct HelpRow {
    /// The section it's under.
    pub title: &'static str,
    pub binding: &'static Binding,
    /// Its keys as they are now (see [`Effective::written`]).
    pub keys: String,
}

/// The rows of the help, as it opens: the keys, those of the pane with focus first, then the rest
/// of the view's, then those for everywhere; then what has no key, found by name in `:`. Typing
/// narrows them to rows holding each word typed, a row for the very keys typed first.
pub fn help_rows(app: &App, query: &str) -> Vec<HelpRow> {
    let focused = scopes(app);
    let own: &[Scope] = view_scopes(app.view);
    let pane = |s: Scope| focused.contains(&s) && !matches!(s, Scope::Library | Scope::Global);
    let rank = |s: Scope| {
        if pane(s) {
            0
        } else if own.contains(&s) {
            1
        } else if !s.dialog() {
            2
        } else {
            3
        }
    };
    let wanted = |s: Scope| focused.contains(&s) || own.contains(&s) || s.dialog() || s == Scope::Pads;
    let e = effective::current();
    let mut rows: Vec<HelpRow> = (e.rows().iter().enumerate())
        .filter_map(|(i, r)| r.b.filter(|b| in_help(b) && wanted(r.scope)).map(|b| (i, b)))
        .map(|(i, binding)| HelpRow { title: help_title(e.by_name(i), binding.group), binding, keys: e.written(i) })
        .collect();
    // The keys first, those found by name after; the crate's own under its heading, after marking
    // and adding to it.
    let order = |r: &HelpRow| {
        (r.title == BY_NAME_TITLE, rank(r.binding.scope), r.binding.group as u8, r.binding.scope == Scope::Crate)
    };
    rows.sort_by_key(order);
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if !words.is_empty() {
        rows.retain(|r| {
            let b = r.binding;
            let text = format!("{} {} {} {}", r.keys, b.help, b.name, b.words).to_lowercase();
            words.iter().all(|w| text.contains(w.as_str()))
        });
        rows.sort_by_key(|r| r.keys.split_whitespace().all(|k| k != query.trim()));
    }
    rows
}

/// Whether a row is in the help, which lists keys: a command with none is the palette's alone.
fn in_help(b: &Binding) -> bool {
    !b.help.is_empty() && b.group != Group::Commands
}

/// Whether a hint runs what it does when clicked: what needs the key pressed (a digit) can't.
pub fn clicks(act: Act) -> bool {
    // A pair (`shift ↑↓`, `n p`) would only do its first half.
    !matches!(act, Act::None | Act::HotCue | Act::CrateMove(_) | Act::MixTrack(_))
}

/// Whether enter can run a help row: what needs the key itself, moves what the help covers, or is
/// a dialog's (which isn't open), has to be pressed.
pub fn runs(b: &Binding) -> bool {
    !b.scope.dialog()
        && !matches!(
            b.act,
            Act::None
                | Act::Help
                | Act::HotCue
                | Act::Move(_)
                | Act::Page(_)
                | Act::End(_)
                | Act::Focus(_)
                | Act::Escape
                | Act::Activate
                | Act::Remove
                | Act::MixForget
        )
}

fn view_scopes(view: View) -> &'static [Scope] {
    match view {
        View::Library => &[Scope::Library, Scope::Tree, Scope::Tracks, Scope::Details, Scope::Crate, Scope::DriveList],
        View::Prepare => &[Scope::Prepare],
        View::Drives => &[Scope::Drives, Scope::Snapshots, Scope::History],
        View::Profile => &[Scope::Profile],
        View::Mixes => &[Scope::Mixes],
    }
}

/// The key tables for `view`: its own keys first, then the ones that work everywhere.
#[cfg(test)]
pub fn help(view: View) -> (Vec<HelpSection>, Vec<HelpSection>) {
    let own = view_scopes(view);
    // The keys first, those found by name after; the crate's own under its heading, after marking
    // and adding to it.
    let mut rows: Vec<&'static Binding> = KEYMAP.iter().filter(|b| own.contains(&b.scope) && in_help(b)).collect();
    rows.sort_by_key(|b| (by_name(b), b.group as u8, b.scope == Scope::Crate));
    let everywhere =
        |b: &&Binding| (b.scope.dialog() || matches!(b.scope, Scope::Global | Scope::Pads | Scope::Deck)) && in_help(b);
    let global = KEYMAP.iter().filter(everywhere);
    let mut global: Vec<&'static Binding> = global.collect();
    global.sort_by_key(|b| (by_name(b), b.group as u8));
    (sections(rows.into_iter()), sections(global.into_iter()))
}

pub mod conflict;
pub mod effective;
pub mod ids;
#[cfg(test)]
mod tests;

use effective::Effective;
