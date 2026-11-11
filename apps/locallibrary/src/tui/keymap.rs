//! Every key the player answers to, in one table: which key, where it applies, what it does,
//! how the help, the footer and the command palette name it. Keys are looked up here and then
//! done by [`App::perform`](super::app::App::perform); the help (`?`) and the footer's hints are
//! drawn from the same rows, so they can't drift from what the keys do. Dialogs (the actions
//! menu, a confirmation, a prompt, the help, typing a search or a path) keep their own keys while
//! open.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthStr;

use super::app::{App, DrivePane, Focus, View};
use super::choices::Hints;
use super::deck::BandMode;
use super::keys::{KeyPalette, Notation};

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
    /// The digit row plays hot cues (the Deck layout, or with the pads on).
    Pads,
    /// The Deck layout.
    Deck,
    /// What follows `g`.
    Goto,
    Prepare,
    Drives,
    Snapshots,
    /// The Drives view's list of what players recorded.
    History,
    Profile,
    Mixes,
    /// The offer to replay edits after a rekordbox export is showing.
    Replay,
}

/// A key a binding answers to.
#[derive(Debug, Clone, Copy)]
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
    fn matches(self, key: &KeyEvent) -> bool {
        let ctrl = key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match self {
            K::Ch(c) => !ctrl && key.code == KeyCode::Char(c),
            K::Code(c) => !ctrl && key.code == c,
            K::Shift(c) => !ctrl && key.code == c && key.modifiers.contains(KeyModifiers::SHIFT),
            K::Ctrl(c) => key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char(c),
            K::Digits(a, b) => !ctrl && matches!(key.code, KeyCode::Char(d) if (a..=b).contains(&d)),
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
    /// A pane, by the digit pressed.
    PaneKey,
    /// `g`: what follows goes somewhere.
    Goto,
    ShowView(View),
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
    /// The hot cue of the digit pressed.
    HotCue,
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
    /// Browse, over the Deck layout.
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
    CrateAdd,
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
    ProfilePin,
    ProfileHide,
    ProfileHideArtist,
    ProfileDocument,
    ProfileWrite,
    ProfileScope,
    ProfileRounding,
    ProfileAccount,
    // Mixes
    MixOpen,
    MixZoom,
    MixExport,
    MixReport,
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
    /// Open a folder holding an export, by a path typed.
    OpenFolder,
    /// The Drives view, on the store's snapshots.
    Snapshots,
    /// The Publish view, signing in.
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
            Group::Crate => "Marks and the crate",
            Group::Changing => "Changing the drive",
            Group::Prepare => "Prepare",
            Group::Drives => "Drives",
            Group::Snapshots => "Snapshots",
            Group::History => "History",
            Group::Profile => "Publish",
            Group::Mixes => "Recordings",
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
    /// The footer's label for it, and how long it stays there as the footer narrows (higher
    /// stays longer).
    pub hint: Option<(&'static str, u8)>,
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
    Binding { scope, keys, shown, act, name, help, group, hint: None }
}

/// A help line that isn't a key of its own (or one bound elsewhere, explained for this view).
const fn note(scope: Scope, shown: &'static str, help: &'static str, group: Group) -> Binding {
    key(scope, &[], shown, Act::None, "", help, group)
}

/// A footer hint for a key bound elsewhere, as it reads in this scope.
const fn tip(scope: Scope, shown: &'static str, label: &'static str, prio: u8) -> Binding {
    key(scope, &[], shown, Act::None, "", "", Group::Player).hint(label, prio)
}

impl Binding {
    const fn hint(mut self, label: &'static str, prio: u8) -> Binding {
        self.hint = Some((label, prio));
        self
    }
}

use Act as A;
use Group as G;
use KeyCode as C;
use Scope as S;

/// `L` in the Profile view: signing in to the Atmosphere account and out.
#[cfg(feature = "account")]
#[rustfmt::skip]
const ACCOUNT_KEY: Binding = key(S::Profile, &[K::Ch('L')], "L", A::ProfileAccount, "sign in to your Atmosphere account (or out)", "sign in with your Atmosphere account (the one for Bluesky and other AT Protocol apps; alpha); signed in, press twice to sign out", G::Profile)
    .hint("sign in/out", 4);

/// Built without the account (the `account` feature), `L` only says so.
#[cfg(not(feature = "account"))]
const ACCOUNT_KEY: Binding = key(
    S::Profile,
    &[K::Ch('L')],
    "L",
    A::ProfileAccount,
    "the Atmosphere account (not in this build)",
    "the Atmosphere account: this locallibrary was built without it",
    G::Profile,
);

/// The table. Order matters: within a scope the first row a key matches wins, and the help and
/// the footer list rows in this order.
#[rustfmt::skip]
pub static KEYMAP: &[Binding] = &[
    // -----------------------------------------------------------------------------------------
    // The Library view
    // -----------------------------------------------------------------------------------------
    // The offer to replay edits rekordbox exported over, while it's up.
    key(S::Replay, &[K::Ch('y')], "y", A::Replay, "", "replay edits rekordbox overwrote (while it says so)", G::Changing),
    tip(S::Replay, "y", "replay edits", 10),
    tip(S::Replay, "esc", "not now", 9),
    // What the footer says in each pane (the keys are below).
    tip(S::Steps, "enter", "run", 9),
    tip(S::Steps, "↑↓", "select", 8),
    tip(S::Tracks, "enter", "play", 9),
    tip(S::Tracks, "space", "pause", 8),
    tip(S::Tracks, "/", "search", 8),
    tip(S::Tracks, "b", "sort", 5),
    tip(S::Tracks, "m", "mark", 6),
    tip(S::Tracks, "a", "to crate", 6),
    tip(S::Tracks, "e", "actions", 7),
    tip(S::Tracks, "i", "info", 4),
    tip(S::Tracks, "t", "crate", 3),
    tip(S::Tracks, "T", "My Tag", 3),
    tip(S::Tracks, "f", "deck", 2),
    tip(S::Tracks, "tab", "focus", 5),
    tip(S::Tree, "enter", "open", 9),
    tip(S::Tree, "e", "actions", 7),
    tip(S::Tree, "del", "delete", 5),
    tip(S::Tree, "/", "search", 6),
    tip(S::Tree, "tab", "focus", 5),
    tip(S::Details, "enter", "play", 9),
    tip(S::Details, "a", "to crate", 6),
    tip(S::Details, "e", "actions", 7),
    tip(S::Details, "i", "close", 5),
    tip(S::Details, "tab", "focus", 5),
    tip(S::Crate, "enter", "play", 9),
    tip(S::Crate, "shift ↑↓", "move", 6),
    tip(S::Crate, "del", "remove", 6),
    tip(S::Crate, "w", "save to drive", 7),
    tip(S::Crate, "X", "clear", 4),
    tip(S::Crate, "t", "close", 5),
    tip(S::Crate, "tab", "focus", 5),
    tip(S::DriveList, "enter", "browse", 9),
    tip(S::DriveList, "space", "pause", 8),
    tip(S::DriveList, "/", "search", 6),
    tip(S::DriveList, "tab", "focus", 5),
    // The crate's own keys, while it has focus.
    key(S::Crate, &[K::Shift(C::Up), K::Ch('K')], "shift ↑↓ K J", A::CrateMove(-1), "move up in the crate", "move a track up / down the crate", G::Crate),
    key(S::Crate, &[K::Shift(C::Down), K::Ch('J')], "", A::CrateMove(1), "move down in the crate", "", G::Crate),
    key(S::Crate, &[K::Code(C::Delete), K::Code(C::Backspace)], "del", A::Remove, "", "in the crate: take the track out", G::Crate),
    key(S::Crate, &[K::Ch('w')], "w", A::CrateSave, "save the crate to the drive as a playlist", "write the crate to the drive as a playlist", G::Crate),
    key(S::Crate, &[K::Ch('X')], "X", A::CrateClear, "clear the crate", "clear the crate (the drive isn't touched)", G::Crate),
    key(S::Library, &[K::Ch('/')], "/", A::Search, "search or filter the tracks", "search title, artist, album, genre, label, key, comment and path as you type", G::Browsing),
    note(S::Library, "", "or filter: bpm:120-128 key:8A~ rating:>=4 -genre:house tag:Deep", G::Browsing),
    note(S::Library, "", "in the box: enter keeps it, esc clears it, ctrl-u empties it, ctrl-w deletes a word, tab searches the whole drive or the list shown", G::Browsing),
    note(S::Library, "Browse", "at the top of the tree: by artist, album, genre, key, BPM, rating, colour, date added, My Tags, sets, history, problems", G::Browsing),
    key(S::Library, &[K::Ch('O')], "O", A::OpenFolder, "open a folder holding an export", "open a folder holding a rekordbox export, by its path", G::Browsing),
    key(S::Library, &[K::Ch('U')], "U", A::ResetPlayed, "forget what's been played", "bring back the tracks dimmed as played since starting", G::Browsing),
    key(S::Library, &[K::Ch('b')], "b  B", A::Sort, "sort by the next column", "sort by the next column / the other way (or click a header)", G::Browsing),
    key(S::Library, &[K::Ch('B')], "", A::SortReverse, "sort the other way", "", G::Browsing),
    key(S::Library, &[K::Ch('*')], "*", A::Ratings, "show or hide the rating column", "show / hide the rating column", G::Browsing),
    key(S::Library, &[K::Ch('#')], "#", A::TagsColumn, "show or hide the My Tags column", "show / hide the My Tags column", G::Browsing),
    note(S::Library, "", "green keys mix with the playing track, or the selected one (⇄ its key)", G::Browsing),
    key(S::Library, &[K::Ch('i')], "i", A::Details, "show or hide the track's details", "the selected track: details, playlists, what mixes into it", G::Browsing),
    key(S::Library, &[K::Ch('m'), K::Code(C::Insert)], "m  insert", A::Mark, "mark or unmark the track", "mark / unmark the selected track (in the tree: a playlist or folder, to pack)", G::Crate),
    key(S::Library, &[K::Ch('M')], "M", A::MarkAll, "mark everything shown, or clear the marks", "mark everything shown / clear the marks", G::Crate),
    key(S::Library, &[K::Ch('a')], "a", A::CrateAdd, "add to the crate", "add the marked (or selected) tracks to the crate", G::Crate),
    key(S::Library, &[K::Ch('t')], "t", A::CratePanel, "show or hide the crate", "show / hide the crate: tracks collected while browsing, a CDJ's TAG LIST", G::Crate),
    note(S::Library, "enter", "in the crate: play from there", G::Crate),
    note(S::Library, "", "The drive is backed up before a change is written. A change u can take back is made at once and says so; any other shows what it will do and asks first.", G::Changing),
    key(S::Library, &[K::Ch('e')], "e", A::Actions, "actions for the selection", "actions for the marked or selected tracks, the playlist shown and the crate, the likeliest first (enter runs it): rating, colour, add to a playlist, remove, new / rename / delete a playlist or folder, sort a playlist on the drive, plugins' operations", G::Changing),
    note(S::Library, "right-click", "the same, for what's under the pointer", G::Changing),
    key(S::Library, &[K::Ch('T')], "T", A::Tag, "give the tracks a My Tag, or take one off", "give the marked (or selected) tracks a My Tag, or take one off (asks first)", G::Changing),
    note(S::Library, "y", "after rekordbox exported over edits made here: replay them (it says so over the status line; esc puts it away)", G::Changing),
    key(S::Library, &[K::Code(C::Delete), K::Code(C::Backspace)], "del", A::Remove, "", "remove from the playlist shown; delete the selected playlist", G::Changing),
    // -----------------------------------------------------------------------------------------
    // The Prepare view
    // -----------------------------------------------------------------------------------------
    note(S::Prepare, "enter", "run the job selected, on the drive open in the library", G::Prepare),
    key(S::Prepare, &[K::Ch('e')], "e", A::PaneActions, "", "the same jobs by their keys", G::Prepare).hint("actions", 7),
    tip(S::Prepare, "enter", "run", 9),
    tip(S::Prepare, "↑↓", "select", 4),
    // -----------------------------------------------------------------------------------------
    // The Drives view
    // -----------------------------------------------------------------------------------------
    note(S::Drives, "enter", "open the drive in the library (a library: its newest clone)", G::Drives),
    key(S::Drives, &[K::Ch('e')], "e", A::DriveActions, "actions for the drive", "what can be done to the selected drive: save a snapshot, check, sync, pack, bring back a playlist, replay edits, undo, eject, plugins' commands; to a library's clones: sync, check, eject (each shows its plan and asks first)", G::Drives),
    tip(S::Drives, "enter", "browse", 9),
    tip(S::Drives, "↑↓", "select", 4),
    tip(S::Drives, "e", "actions", 8),
    tip(S::Drives, "u", "undo", 6),
    tip(S::Drives, "R", "refresh status", 5),
    note(S::Snapshots, "", "every library in the store, its snapshots newest first (locallibrary pull adds one)", G::Snapshots),
    note(S::Snapshots, "enter", "bring back a playlist from the selected snapshot onto the drive open in the library", G::Snapshots),
    key(S::Snapshots, &[K::Ch('e')], "e", A::PaneActions, "", "actions for the selected snapshot", G::Snapshots).hint("actions", 6),
    tip(S::Snapshots, "enter", "bring back a playlist", 7),
    tip(S::Snapshots, "↑↓", "scroll", 4),
    tip(S::Snapshots, "R", "refresh", 5),
    note(S::History, "enter", "open the list players recorded in the library", G::History),
    key(S::History, &[K::Ch('e')], "e", A::PaneActions, "", "actions for the selected list", G::History).hint("actions", 6),
    tip(S::History, "enter", "open", 7),
    tip(S::History, "↑↓", "select", 4),
    // -----------------------------------------------------------------------------------------
    // The Publish view
    // -----------------------------------------------------------------------------------------
    tip(S::Profile, "↑↓", "select", 3),
    key(S::Profile, &[K::Ch('e')], "e", A::PaneActions, "", "these actions, for the section selected", G::Profile).hint("actions", 7),
    key(S::Profile, &[K::Ch('i')], "i", A::ProfileInclude, "put the section in the document, or take it out", "put the section in the document, or take it out", G::Profile)
        .hint("in/out of document", 8),
    note(S::Profile, "tab  enter", "move between sections and their lists (esc goes back)", G::Profile),
    key(S::Profile, &[K::Ch('m')], "m", A::ProfilePin, "pin the track among signature tracks", "pin a track first among signature tracks", G::Profile)
        .hint("pin", 6),
    key(S::Profile, &[K::Ch('h')], "h  H", A::ProfileHide, "never name the track", "never name a track / its artist (still counted)", G::Profile)
        .hint("hide track/artist", 6),
    key(S::Profile, &[K::Ch('H')], "", A::ProfileHideArtist, "never name the artist", "", G::Profile),
    key(S::Profile, &[K::Ch('D')], "D", A::ProfileDocument, "show the profile document", "show the document a public profile would get", G::Profile)
        .hint("document", 7),
    key(S::Profile, &[K::Ch('w')], "w", A::ProfileWrite, "write the profile document to the store folder", "write the document as JSON in the store folder", G::Profile)
        .hint("write", 5),
    key(S::Profile, &[K::Ch('a')], "a", A::ProfileScope, "profile from every drive or the open one", "every drive and the store, or just the open drive", G::Profile)
        .hint("scope", 4),
    key(S::Profile, &[K::Ch('#')], "#", A::ProfileRounding, "rounded or exact numbers in the document", "rounded or exact numbers in the document", G::Profile)
        .hint("rounding", 3),
    tip(S::Profile, "R", "rebuild", 4),
    ACCOUNT_KEY,
    // -----------------------------------------------------------------------------------------
    // The Recordings view
    // -----------------------------------------------------------------------------------------
    note(S::Mixes, "", "A recorded set, and which of the open drive's tracks it plays (like locallibrary mix).", G::Mixes),
    note(S::Mixes, "enter", "on a recording: find its tracks; on a track: play the set from there", G::Mixes),
    key(S::Mixes, &[K::Ch('e')], "e", A::PaneActions, "", "actions for the selection", G::Mixes).hint("actions", 7),
    tip(S::Mixes, "enter", "open · play from", 9),
    note(S::Mixes, "space", "play the set, or pause it", G::Mixes),
    tip(S::Mixes, "space", "play", 8),
    note(S::Mixes, "n  p", "while the set plays: its next / previous track", G::Mixes),
    tip(S::Mixes, "n p", "next/prev track", 6),
    key(S::Mixes, &[K::Ch('O')], "O", A::MixOpen, "open a recording by path", "open a recording by path, or a folder of them (subfolders too) to keep listed", G::Mixes)
        .hint("open a file", 7),
    key(S::Mixes, &[K::Ch('z')], "z", A::MixZoom, "zoom the set's timeline", "zoom the timeline: the whole set, 2×, 4×, 8×", G::Mixes)
        .hint("zoom", 4),
    key(S::Mixes, &[K::Code(C::Delete), K::Code(C::Backspace)], "del", A::Remove, "", "on a recording from this computer: forget it, or the folder it was found in", G::Mixes),
    note(S::Mixes, "o", "go to the selected track in the library", G::Mixes),
    tip(S::Mixes, "o", "in library", 5),
    key(S::Mixes, &[K::Ch('E')], "E", A::MixExport, "export the set for Local Network", "export the set as data (.mix.json) for Local Network", G::Mixes)
        .hint("export", 4),
    key(S::Mixes, &[K::Ch('w')], "w", A::MixReport, "write the set's HTML report and open it", "write the HTML report and open it in a browser", G::Mixes)
        .hint("report", 4),
    note(S::Mixes, "click", "on the timeline: play from there, or pick the track under it", G::Mixes),
    tip(S::Mixes, "tab", "focus", 3),
    // -----------------------------------------------------------------------------------------
    // Full screen (hints: the keys are the deck's)
    // -----------------------------------------------------------------------------------------
    tip(S::Deck, "1-8", "hot cue", 9),
    tip(S::Deck, ", .", "beat", 8),
    tip(S::Deck, "< >", "bar", 6),
    tip(S::Deck, "Q", "quantize", 5),
    tip(S::Deck, "l", "loop", 7),
    tip(S::Deck, "c C", "cue", 6),
    tip(S::Deck, "N P", "phrase", 5),
    tip(S::Deck, "space", "pause", 8),
    tip(S::Deck, "n p", "track", 4),
    tip(S::Deck, "↑↓ enter", "play next", 7),
    tip(S::Deck, "S", "autoplay", 5),
    key(S::Deck, &[K::Ch('b')], "b", A::Browse, "", "browse the library over the deck; loading a track or esc comes back", G::Visuals).hint("browse", 9),
    key(S::Deck, &[K::Ch('w')], "w", A::NextBands, "", "the waveform's colours: BLUE, RGB, 3Band, as the drive says", G::Visuals).hint("colours", 3),
    tip(S::Deck, "f", "library", 7),
    key(S::Pads, &[K::Digits('1', '8')], "1 … 8", A::HotCue, "", "in the Deck layout, or with the pads on: hot cue A … H (jumps there and plays)", G::Deck),
    tip(S::Pads, "1-8", "hot cue", 9),
    // -----------------------------------------------------------------------------------------
    // After g
    // -----------------------------------------------------------------------------------------
    key(S::Goto, &[K::Ch('l')], "g l", A::ShowView(View::Library), "go to Library", "Library", G::Player),
    key(S::Goto, &[K::Ch('p')], "g p", A::ShowView(View::Prepare), "go to Prepare", "Prepare", G::Player),
    key(S::Goto, &[K::Ch('d')], "g d", A::ShowView(View::Drives), "go to Drives", "Drives, their snapshots and history", G::Player),
    key(S::Goto, &[K::Ch('r')], "g r", A::ShowView(View::Mixes), "go to Recordings", "Recordings", G::Player),
    key(S::Goto, &[K::Ch('u')], "g u", A::ShowView(View::Profile), "go to Publish", "Publish", G::Player),
    key(S::Goto, &[K::Ch('g')], "g g", A::End(-1), "", "the first row", G::Moving),
    // -----------------------------------------------------------------------------------------
    // Everywhere
    // -----------------------------------------------------------------------------------------
    key(S::Global, &[K::Code(C::Enter)], "enter", A::Activate, "", "play the selected track (queues the list it's in); open a playlist or folder; browse a drive", G::Playback),
    key(S::Global, &[K::Ch(' ')], "space", A::PlayPause, "play or pause", "pause / resume", G::Playback).hint("pause", 8),
    key(S::Global, &[K::Code(C::Left)], "← →", A::Seek(-1), "seek back", "seek 5 s (shift: 30 s)", G::Playback),
    key(S::Global, &[K::Code(C::Right)], "", A::Seek(1), "seek forward", "", G::Playback),
    key(S::Global, &[K::Ch('n')], "n  p", A::Next, "next track", "next / previous track", G::Playback),
    key(S::Global, &[K::Ch('p')], "", A::Prev, "previous track", "", G::Playback),
    key(S::Global, &[K::Ch('c')], "c  C", A::Cue(true), "next cue", "jump to the next / previous cue", G::Playback),
    key(S::Global, &[K::Ch('C')], "", A::Cue(false), "previous cue", "", G::Playback),
    key(S::Global, &[K::Ch('N')], "N  P", A::Phrase(true), "next phrase", "jump to the next / previous phrase", G::Playback),
    key(S::Global, &[K::Ch('P')], "", A::Phrase(false), "previous phrase", "", G::Playback),
    key(S::Global, &[K::Ch('+'), K::Ch('=')], "+  -", A::Volume(1), "volume up", "volume", G::Playback),
    key(S::Global, &[K::Ch('-'), K::Ch('_')], "", A::Volume(-1), "volume down", "", G::Playback),
    key(S::Global, &[K::Ch('s')], "s  r", A::Shuffle, "shuffle on or off", "shuffle / repeat (off, all, one)", G::Playback),
    key(S::Global, &[K::Ch('r')], "", A::Repeat, "repeat: off, all, one", "", G::Playback),
    key(S::Global, &[K::Ch('x')], "x", A::Stop, "stop", "stop", G::Playback),
    key(S::Global, &[K::Ch('o')], "o", A::GoTo, "go to the playing track", "go to the playing track (in Recordings, the selected track in the library)", G::Playback),
    key(S::Global, &[K::Ch('\'')], "'", A::Pads, "pads on or off", "pads on / off: the digit row plays hot cues instead of jumping to panes", G::Deck),
    key(S::Global, &[K::Ch(',')], ",  .", A::BeatJump(-1), "beat jump back", "beat jump: one beat back / forward", G::Deck),
    key(S::Global, &[K::Ch('.')], "", A::BeatJump(1), "beat jump forward", "", G::Deck),
    key(S::Global, &[K::Ch('<')], "<  >", A::BeatJump(-4), "a bar back", "one bar (4 beats) back / forward", G::Deck),
    key(S::Global, &[K::Ch('>')], "", A::BeatJump(4), "a bar forward", "", G::Deck),
    key(S::Global, &[K::Ch('{')], "{  }", A::BeatJump(-32), "8 bars back", "8 bars (32 beats, a phrase) back / forward", G::Deck),
    key(S::Global, &[K::Ch('}')], "", A::BeatJump(32), "8 bars forward", "", G::Deck),
    key(S::Global, &[K::Ch('Q')], "Q", A::Quantize, "quantize on or off", "quantize on / off: playing, cue, phrase and beat jumps keep the beat; paused, beat jumps land on one", G::Deck),
    key(S::Global, &[K::Ch('l')], "l", A::Loop, "loop here, or exit the loop", "loop: the saved loop the playhead is in, else 4 beats from the beat it's in; again to exit", G::Deck),
    key(S::Global, &[K::Ch('(')], "(  )", A::LoopSize(false), "halve the loop", "halve / double the loop", G::Deck),
    key(S::Global, &[K::Ch(')')], "", A::LoopSize(true), "double the loop", "", G::Deck),
    note(S::Global, "", "A hot cue saved as a loop plays the loop. Seeking out of a loop leaves it.", G::Deck),
    key(S::Global, &[K::Ch('S')], "S", A::Single, "autoplay on or off (single play)", "autoplay off (single play): stop after each track, the next one ready for space; again to play on", G::Deck),
    key(S::Global, &[K::Ch('A')], "A", A::AutoCue, "auto cue on or off", "auto cue: tracks start at their first memory cue, else their first beat", G::Deck),
    key(S::Global, &[K::Ch('d')], "d", A::Outputs, "the output to play through", "the output: the system's default, or another (headphones, an interface), remembered", G::Deck),
    key(S::Global, &[K::Ch('v')], "v", A::Visualiser, "the next visualiser", "spectrum · mirror · scope · none, in the Deck layout where there's room", G::Visuals),
    key(S::Global, &[K::Ch('[')], "[  ]", A::Zoom(false), "zoom the deck out", "zoom the deck's waveform out / in", G::Visuals),
    key(S::Global, &[K::Ch(']')], "", A::Zoom(true), "zoom the deck in", "", G::Visuals),
    key(S::Global, &[K::Ch('f')], "f", A::DeckLayout, "switch between the Library and Deck layouts", "the Library layout or the Deck layout, where the deck takes the screen", G::Visuals),
    key(S::Global, &[K::Code(C::Up), K::Ch('k')], "↑ ↓  j k", A::Move(-1), "", "move", G::Moving),
    key(S::Global, &[K::Code(C::Down), K::Ch('j')], "", A::Move(1), "", "", G::Moving),
    key(S::Global, &[K::Code(C::PageUp)], "pgup  pgdn", A::Page(-1), "", "a page up / down", G::Moving),
    key(S::Global, &[K::Code(C::PageDown)], "", A::Page(1), "", "", G::Moving),
    key(S::Global, &[K::Code(C::Home)], "home  G", A::End(-1), "", "the first / last row (end too)", G::Moving),
    key(S::Global, &[K::Code(C::End), K::Ch('G')], "", A::End(1), "", "", G::Moving),
    key(S::Global, &[K::Code(C::Tab)], "tab", A::Focus(false), "", "next pane (shift-tab: back)", G::Moving),
    key(S::Global, &[K::Code(C::BackTab)], "", A::Focus(true), "", "", G::Moving),
    key(S::Global, &[K::Code(C::Esc)], "esc", A::Escape, "", "stop a change before it writes; hide an error; clear the search, then the marks (press twice)", G::Moving),
    key(S::Global, &[K::Digits('1', '5')], "1 … 5", A::PaneKey, "", "the pane with that number in its title (while the pads are off)", G::Moving),
    key(S::Global, &[K::Ch('g')], "g", A::Goto, "", "then a letter, for a view (the letter beside its tab), or g for the first row", G::Moving),
    key(S::Global, &[K::Ch('R'), K::Code(C::F(5))], "R  F5", A::Refresh, "refresh: read the drive and the view again", "read again: the drive, the drives' status, the snapshots, the profile, the recordings", G::Player),
    key(S::Global, &[K::Ch(':')], ":", A::Palette, "", "commands: every action by name (type a few of its letters), the ones used lately first, plugins' commands and suggestions", G::Player).hint("commands", 6),
    key(S::Global, &[K::Ch('?')], "?", A::Help, "help: the keys", "this help: type to narrow it, enter runs the row selected", G::Player).hint("help", 5),
    key(S::Global, &[K::Ch('!')], "!", A::Messages, "messages since starting", "messages: everything the status line said since starting", G::Player),
    key(S::Global, &[K::Ch('&')], "&", A::Jobs, "jobs: what's running", "jobs: what's running, with a cancel for each that can still stop", G::Player),
    note(S::Global, "", "found a bug? locallibrary bug, in a terminal, reports it", G::Player),
    // Commands with no key of their own, for the drive open in the library (the selected one in
    // the Drives view).
    key(S::Global, &[], "pull", A::Pull, "save a snapshot of the drive", PULL, G::Commands),
    key(S::Global, &[], "check", A::Check, "check the drive for problems", CHECK, G::Commands),
    key(S::Global, &[], "set prep", A::SetPrep, "check all the library's drives", "check the drive and the other mounted drives of its library together, for a set", G::Commands),
    key(S::Global, &[], "sync", A::SyncGroup, "sync the library's drives", SYNC, G::Commands),
    key(S::Global, &[], "pack", A::Pack, "pack playlists for a set…", PACK, G::Commands),
    key(S::Global, &[], "restore", A::RestorePlaylist, "bring back a playlist from a snapshot…", "a playlist or folder as a snapshot in the store had it, onto the drive", G::Commands),
    key(S::Global, &[], "replay", A::Replay, "replay edits rekordbox overwrote", "make the drive's recorded edits again after rekordbox exported over them", G::Commands),
    key(S::Global, &[K::Ch('u')], "u", A::Undo, "undo the last change", "undo: take back the last change made, on the drive it was made to (before any, the open or selected drive's last), after showing what that puts back", G::Changing),
    key(S::Global, &[], "smart", A::SmartList, "smart playlists: the rules", "the rule-based playlists kept in the store", G::Commands),
    key(S::Global, &[], "smart apply", A::SmartApply, "fill the smart playlists on the drive", "fill the rule-based playlists on the drive from its own library (shows what changes first)", G::Commands),
    key(S::Global, &[], "smart new", A::SmartNew, "smart playlist from the search", "a rule-based playlist from the search box's filter and the list's sort", G::Commands),
    key(S::Global, &[], "export", A::ExportList, "export the list shown (M3U8, CSV, text)", "write the playlist, set, session or all tracks shown to a file on this computer", G::Commands),
    key(S::Global, &[], "eject", A::Eject, "eject the drive", EJECT, G::Commands),
    key(S::Global, &[], "g d 3", A::Snapshots, "browse snapshots", "the snapshots in the store, in the Drives view", G::Commands),
    key(S::Global, &[], "g u L", A::SignIn, "sign in", "sign in to your Atmosphere account, in the Publish view", G::Commands),
    key(S::Global, &[K::Ch('q'), K::Ctrl('c'), K::Ctrl('q')], "q", A::Quit, "quit", "quit (a write to the drive finishes first)", G::Player).hint("quit", 1),
    // -----------------------------------------------------------------------------------------
    // The drive's jobs, by key where they have a home (here so `about` finds the rows above)
    // -----------------------------------------------------------------------------------------
    key(S::Prepare, &[K::Ch('V')], "V", A::Check, "", CHECK, G::Prepare).hint("check", 6),
    key(S::Prepare, &[K::Ch('W')], "W", A::Pull, "", PULL, G::Prepare).hint("snapshot", 5),
    key(S::Prepare, &[K::Ch('Y')], "Y", A::SyncGroup, "", SYNC, G::Prepare).hint("sync", 5),
    key(S::Prepare, &[K::Ch('K')], "K", A::Pack, "", PACK, G::Prepare).hint("pack", 5),
    key(S::Prepare, &[K::Ch('E')], "E", A::Eject, "", EJECT, G::Prepare).hint("eject", 4),
    key(S::Drives, &[K::Ch('V')], "V", A::Check, "", CHECK, G::Drives).hint("check", 5),
    key(S::Drives, &[K::Ch('W')], "W", A::Pull, "", PULL, G::Drives).hint("snapshot", 4),
    key(S::Drives, &[K::Ch('Y')], "Y", A::SyncGroup, "", SYNC, G::Drives).hint("sync", 4),
    key(S::Drives, &[K::Ch('K')], "K", A::Pack, "", PACK, G::Drives).hint("pack", 3),
    key(S::Drives, &[K::Ch('E')], "E", A::Eject, "", EJECT, G::Drives).hint("eject", 3),
];

/// What the drive's jobs do, in the help under each of their homes.
const PULL: &str = "save the drive in the store, a snapshot of its library (shows what that adds first)";
const CHECK: &str = "check the drive against the player models remembered for it (results in a panel)";
const SYNC: &str = "copy the newest drive of the library over the others plugged in (when none is newest, says why)";
const PACK: &str = "the marked playlists, the playlist shown or the crate, from the open drive onto another";
const EJECT: &str = "unmount the drive and power it off (playing from it stops first)";

/// The scopes keys are looked up in now, most specific first.
pub fn scopes(app: &App) -> Vec<Scope> {
    let mut v = Vec::new();
    if app.replay_offer.is_some() {
        v.push(Scope::Replay);
    }
    if app.deck_showing() {
        v.push(Scope::Deck);
    }
    if app.view == View::Library {
        v.push(match app.focus {
            Focus::Drives => Scope::DriveList,
            Focus::Playlists => Scope::Tree,
            Focus::Tracks if app.empty() => Scope::Steps,
            Focus::Tracks => Scope::Tracks,
            Focus::Details => Scope::Details,
            Focus::Crate => Scope::Crate,
        });
    }
    if app.pads_on() {
        v.push(Scope::Pads);
    }
    match app.view {
        View::Library => v.push(Scope::Library),
        View::Prepare => v.push(Scope::Prepare),
        View::Drives => v.push(match app.drive_pane {
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

/// The key `act` answers to in `scope`, if it has one there.
pub fn key_in(scope: Scope, act: Act) -> Option<&'static K> {
    KEYMAP.iter().find(|b| b.scope == scope && b.act == act).and_then(|b| b.keys.first())
}

/// The letter after `g` that goes to `view`.
pub fn view_letter(view: View) -> &'static str {
    let row = KEYMAP.iter().find(|b| b.scope == Scope::Goto && b.act == Act::ShowView(view));
    row.map_or("", |b| &b.shown[2..])
}

/// The binding `key` means in `scopes` (most specific first).
pub fn lookup(scopes: &[Scope], key: &KeyEvent) -> Option<&'static Binding> {
    scopes.iter().find_map(|s| KEYMAP.iter().find(|b| b.scope == *s && b.keys.iter().any(|k| k.matches(key))))
}

/// The most hints the compact footer holds.
const COMPACT: usize = 5;

/// The footer's hints now: the view's own first, then those for everywhere, as many as the setting
/// and `width` columns allow (the least important go first).
pub fn hints(app: &App, width: usize) -> Vec<(String, &'static str)> {
    if app.hints == Hints::Off {
        return Vec::new();
    }
    let most = if app.hints == Hints::Compact { COMPACT } else { usize::MAX };
    let scopes = if app.deck_showing() { vec![Scope::Deck] } else { scopes(app) };
    let mut out: Vec<(String, &'static str, u8)> = Vec::new();
    let pick = |b: &Binding, out: &mut Vec<(String, &'static str, u8)>| {
        let Some((label, prio)) = b.hint else { return };
        let keys = b.shown.split_whitespace().collect::<Vec<_>>().join(" ");
        if !out.iter().any(|(k, ..)| *k == keys) {
            out.push((keys, label, prio));
        }
    };
    for s in &scopes {
        for b in KEYMAP.iter().filter(|b| b.scope == *s) {
            pick(b, &mut out);
        }
    }
    // Each hint takes its keys, a space, its label and two spaces; the footer starts with one.
    let cost = |(k, l, _): &(String, &str, u8)| k.width() + l.width() + 3;
    while out.len() > most || (!out.is_empty() && 1 + out.iter().map(cost).sum::<usize>() > width) {
        let lowest = out.iter().map(|h| h.2).min().unwrap_or(0);
        let i = out.iter().rposition(|h| h.2 == lowest).unwrap_or(0);
        out.remove(i);
    }
    out.into_iter().map(|(k, l, _)| (k, l)).collect()
}

/// One section of the key tables: its title and its lines (keys, what they do).
#[cfg(test)]
pub type HelpSection = (&'static str, Vec<(&'static str, &'static str)>);

#[cfg(test)]
fn sections(rows: impl Iterator<Item = &'static Binding>) -> Vec<HelpSection> {
    let mut out: Vec<HelpSection> = Vec::new();
    for b in rows.filter(|b| !b.help.is_empty()) {
        let title = b.group.title();
        match out.iter_mut().find(|(t, _)| *t == title) {
            Some((_, lines)) => lines.push((b.shown, b.help)),
            None => out.push((title, vec![(b.shown, b.help)])),
        }
    }
    out
}

/// A row of the help.
pub struct HelpRow {
    /// The section it's under.
    pub title: &'static str,
    pub binding: &'static Binding,
}

/// The rows of the help, as it opens: those of the pane with focus first, then the rest of the
/// view's, then those for everywhere. Typing narrows them to rows holding each word typed, a row
/// for the very keys typed first.
pub fn help_rows(app: &App, query: &str) -> Vec<HelpRow> {
    let focused = scopes(app);
    let own: &[Scope] = view_scopes(app.view);
    let pane = |s: Scope| focused.contains(&s) && !matches!(s, Scope::Library | Scope::Global);
    let rank = |s: Scope| {
        if pane(s) {
            0
        } else if own.contains(&s) {
            1
        } else {
            2
        }
    };
    let wanted = |s: Scope| focused.contains(&s) || own.contains(&s) || matches!(s, Scope::Pads | Scope::Goto);
    let mut rows: Vec<&'static Binding> = KEYMAP.iter().filter(|b| !b.help.is_empty() && wanted(b.scope)).collect();
    // The crate's own keys under its heading, after marking and adding to it.
    rows.sort_by_key(|b| (rank(b.scope), b.group as u8, b.scope == Scope::Crate));
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if !words.is_empty() {
        rows.retain(|b| {
            let text = format!("{} {} {}", b.shown, b.help, b.name).to_lowercase();
            words.iter().all(|w| text.contains(w.as_str()))
        });
        rows.sort_by_key(|b| b.shown.split_whitespace().all(|k| k != query.trim()));
    }
    rows.into_iter().map(|binding| HelpRow { title: binding.group.title(), binding }).collect()
}

/// Whether enter can run a help row: what needs the key itself, or moves what the help covers,
/// has to be pressed.
pub fn runs(act: Act) -> bool {
    !matches!(
        act,
        Act::None
            | Act::Help
            | Act::Goto
            | Act::PaneKey
            | Act::HotCue
            | Act::Move(_)
            | Act::Page(_)
            | Act::End(_)
            | Act::Focus(_)
            | Act::Escape
            | Act::Activate
            | Act::Remove
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
    // The crate's own keys under its heading, after marking and adding to it.
    let mut rows: Vec<&'static Binding> = KEYMAP.iter().filter(|b| own.contains(&b.scope)).collect();
    rows.sort_by_key(|b| (b.group as u8, b.scope == Scope::Crate));
    let everywhere = |b: &&Binding| matches!(b.scope, Scope::Global | Scope::Pads | Scope::Goto);
    let global = KEYMAP.iter().filter(everywhere);
    let mut global: Vec<&'static Binding> = global.collect();
    global.sort_by_key(|b| b.group as u8);
    (sections(rows.into_iter()), sections(global.into_iter()))
}

pub mod bound;
#[cfg(test)]
mod tests;
