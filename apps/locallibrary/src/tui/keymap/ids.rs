//! Every action a key can be bound to, by an id `keybinds.json` and the keybinds record name it
//! with: `area.name` (`playback.next`), and `plugin.<plugin id>.<name>` for a plugin's command or
//! operation. An id never changes once released, and one retired never comes back: `ids.txt`
//! lists every id ever issued, and a test holds this file to it.

use std::borrow::Cow;

use loclib_plugin::Manifest;

use super::super::cli::Cli;
use super::super::edits::Action;
use super::{Act, KEYMAP, Preset};
use crate::tui::app::View;

/// The library's actions menu's own actions a key can do, on what's selected (the rest duplicate
/// an act of the keymap, or come from plugins).
const EDITS: [Action; 25] = {
    use locallibrary::model::TrackColor as C;
    [
        Action::RatingMenu,
        Action::Rate(0),
        Action::Rate(1),
        Action::Rate(2),
        Action::Rate(3),
        Action::Rate(4),
        Action::Rate(5),
        Action::ColourMenu,
        Action::Colour(None),
        Action::Colour(Some(C::Pink)),
        Action::Colour(Some(C::Red)),
        Action::Colour(Some(C::Orange)),
        Action::Colour(Some(C::Yellow)),
        Action::Colour(Some(C::Green)),
        Action::Colour(Some(C::Aqua)),
        Action::Colour(Some(C::Blue)),
        Action::Colour(Some(C::Purple)),
        Action::AddToPlaylist,
        Action::NewPlaylistWith,
        Action::RemoveFromList,
        Action::SortList,
        Action::NewPlaylist,
        Action::NewFolder,
        Action::Rename,
        Action::Delete,
    ]
};

/// The library's own actions a key can do, as acts.
pub fn edits() -> impl Iterator<Item = Act> {
    EDITS.into_iter().map(Act::Edit)
}

/// `act`'s id, which never changes once released (see `ids.txt`). None: it can't be bound (a
/// fixed key, an act that carries what it's for, a look, or a plugin's list item).
pub fn id(act: Act) -> Option<Cow<'static, str>> {
    let s = |id: &'static str| Some(Cow::Borrowed(id));
    match act {
        Act::None | Act::Escape | Act::Help | Act::Palette => None,
        Act::Quit => s("player.quit"),
        Act::Messages => s("player.messages"),
        Act::Jobs => s("player.jobs"),
        Act::Refresh => s("player.refresh"),
        Act::ShowView(View::Library) => s("view.library"),
        Act::ShowView(View::Prepare) => s("view.prepare"),
        Act::ShowView(View::Drives) => s("view.drives"),
        Act::ShowView(View::Mixes) => s("view.recordings"),
        Act::ShowView(View::Profile) => s("view.localnetwork"),
        Act::Edit(a) => edit_id(a),
        Act::Activate => s("playback.activate"),
        Act::PlayPause => s("playback.play-pause"),
        Act::Seek(-1) => s("playback.seek-back"),
        Act::Seek(1) => s("playback.seek-forward"),
        Act::Next => s("playback.next"),
        Act::Prev => s("playback.previous"),
        Act::Stop => s("playback.stop"),
        Act::Cue(true) => s("playback.cue-next"),
        Act::Cue(false) => s("playback.cue-previous"),
        Act::Phrase(true) => s("playback.phrase-next"),
        Act::Phrase(false) => s("playback.phrase-previous"),
        Act::Volume(1) => s("playback.volume-up"),
        Act::Volume(-1) => s("playback.volume-down"),
        Act::Shuffle => s("playback.shuffle"),
        Act::Repeat => s("playback.repeat"),
        Act::GoTo => s("playback.go-to-playing"),
        Act::Pads => s("deck.pads"),
        Act::HotCue => s("deck.hot-cue"),
        Act::DeleteCues => s("deck.delete-hot-cue"),
        Act::SetHotCue => s("deck.set-hot-cue"),
        Act::BeatJump(-1) => s("deck.beat-back"),
        Act::BeatJump(1) => s("deck.beat-forward"),
        Act::BeatJump(-4) => s("deck.bar-back"),
        Act::BeatJump(4) => s("deck.bar-forward"),
        Act::BeatJump(-32) => s("deck.phrase-back"),
        Act::BeatJump(32) => s("deck.phrase-forward"),
        Act::Quantize => s("deck.quantize"),
        Act::Loop => s("deck.loop"),
        Act::LoopSize(false) => s("deck.loop-halve"),
        Act::LoopSize(true) => s("deck.loop-double"),
        Act::Single => s("deck.single"),
        Act::AutoCue => s("deck.auto-cue"),
        Act::Outputs => s("deck.outputs"),
        Act::Visualiser => s("visuals.visualiser"),
        Act::NextBands => s("visuals.waveform-colours"),
        Act::Zoom(false) => s("visuals.zoom-out"),
        Act::Zoom(true) => s("visuals.zoom-in"),
        Act::DeckLayout => s("visuals.player-mode"),
        Act::Browse => s("visuals.browse"),
        Act::Move(-1) => s("move.up"),
        Act::Move(1) => s("move.down"),
        Act::Page(-1) => s("move.page-up"),
        Act::Page(1) => s("move.page-down"),
        Act::End(-1) => s("move.first"),
        Act::End(1) => s("move.last"),
        Act::Focus(false) => s("move.next-pane"),
        Act::Focus(true) => s("move.previous-pane"),
        Act::Search => s("library.search"),
        Act::Actions => s("library.actions"),
        Act::Details => s("library.info"),
        Act::Mark => s("library.mark"),
        Act::MarkAll => s("library.mark-all"),
        Act::Sort => s("library.sort"),
        Act::SortReverse => s("library.sort-reverse"),
        Act::Ratings => s("library.ratings-column"),
        Act::TagsColumn => s("library.tags-column"),
        Act::Tag => s("library.my-tag"),
        Act::OpenFolder => s("library.open-folder"),
        Act::ResetPlayed => s("library.forget-played"),
        Act::Remove => s("library.remove"),
        Act::CrateAdd => s("queue.add"),
        Act::CratePanel => s("queue.show"),
        Act::QueueNext => s("queue.play-next"),
        Act::CrateMove(-1) => s("queue.move-up"),
        Act::CrateMove(1) => s("queue.move-down"),
        Act::CrateSave => s("queue.save"),
        Act::CrateClear => s("queue.clear"),
        Act::Fill => s("queue.fill"),
        Act::PaneActions => s("pane.actions"),
        Act::DriveActions => s("drive.actions"),
        Act::Undo => s("drive.undo"),
        Act::Pull => s("drive.snapshot"),
        Act::Check => s("drive.check"),
        Act::SetPrep => s("drive.set-prep"),
        Act::SyncGroup => s("drive.sync"),
        Act::Pack => s("drive.pack"),
        Act::RestorePlaylist => s("drive.restore-playlist"),
        Act::Replay => s("drive.replay-edits"),
        Act::Eject => s("drive.eject"),
        Act::SmartList => s("drive.smart-playlists"),
        Act::SmartApply => s("drive.smart-apply"),
        Act::SmartNew => s("drive.smart-new"),
        Act::ExportList => s("drive.export-list"),
        Act::Snapshots => s("drive.snapshots"),
        Act::Run(Cli::Doctor) => s("drive.doctor"),
        Act::Run(Cli::BySound) => s("drive.duplicates-by-sound"),
        Act::Run(Cli::Dedupe) => s("drive.dedupe"),
        Act::Run(Cli::Relocate) => s("drive.relocate"),
        Act::Run(Cli::Upgrade) => s("drive.upgrade"),
        Act::Run(Cli::Refresh) => s("drive.refresh-after-export"),
        Act::Run(Cli::Space) => s("drive.space"),
        Act::Run(Cli::PlayStats) => s("drive.play-stats"),
        Act::Copy(command) => copy_id(command),
        Act::ProfileInclude => s("localnetwork.in-document"),
        Act::ProfileHideArtist => s("localnetwork.hide-artist"),
        Act::ProfileDocument => s("localnetwork.show-document"),
        Act::ProfileWrite => s("localnetwork.save-document"),
        Act::ProfileRounding => s("localnetwork.rounding"),
        Act::ProfileAccount => s("localnetwork.account"),
        Act::SignIn => s("localnetwork.sign-in"),
        Act::MixPlay => s("recordings.play"),
        Act::MixTrack(true) => s("recordings.next-track"),
        Act::MixTrack(false) => s("recordings.previous-track"),
        Act::MixOpen => s("recordings.open"),
        Act::MixZoom => s("recordings.zoom"),
        Act::MixForget => s("recordings.forget"),
        Act::MixGoTo => s("recordings.show-in-library"),
        Act::MixReport => s("recordings.save-report"),
        Act::MixExport => s("recordings.export-set"),
        Act::Answer(true) => s("dialog.yes"),
        Act::Answer(false) => s("dialog.no"),
        Act::Change(1) => s("dialog.next-choice"),
        Act::Change(-1) => s("dialog.previous-choice"),
        Act::CancelJob => s("dialog.cancel-job"),
        Act::SearchWhere => s("dialog.search-where"),
        Act::Keys => s("keys.open"),
        Act::KeysImport => s("keys.import"),
        Act::KeysReset => s("keys.reset-all"),
        Act::KeysPreset(Preset::Azerty) => s("keys.azerty"),
        Act::Plugin(plugin, name) => Some(Cow::Owned(format!("plugin.{plugin}.{name}"))),
        // Values the keymap never uses.
        Act::Seek(_)
        | Act::Volume(_)
        | Act::BeatJump(_)
        | Act::Move(_)
        | Act::Page(_)
        | Act::End(_)
        | Act::CrateMove(_)
        | Act::Change(_) => None,
        // What carries a value found at runtime, a look, or a plugin's list item.
        Act::Bands(_)
        | Act::KeyNotation(_)
        | Act::KeyPalette(_)
        | Act::EndWarning(_)
        | Act::Art(_)
        | Act::ArtHalfblocks(_)
        | Act::Theme(_)
        | Act::Colours(_)
        | Act::Icons(_)
        | Act::Hints(_)
        | Act::HelpHint(_)
        | Act::AccountSignOut(_)
        | Act::Review(_)
        | Act::Dismiss(_)
        | Act::Fix(_)
        | Act::Analyze(..)
        | Act::ApplyFields(..)
        | Act::FieldColumn(_)
        | Act::PluginSettings(_)
        | Act::PluginAccess(_)
        | Act::AccountSignIn(..)
        | Act::ProviderSettings
        | Act::CheckSettings
        | Act::Export(..)
        | Act::Import(..)
        | Act::Source(..)
        | Act::Fetch(_) => None,
    }
}

/// The id of one of the library's actions: none for those that duplicate a keymap act (which has
/// the id) or come from plugins.
fn edit_id(a: Action) -> Option<Cow<'static, str>> {
    let s = |id: &'static str| Some(Cow::Borrowed(id));
    match a {
        Action::RatingMenu => s("edit.rating-menu"),
        Action::Rate(n) if n <= 5 => Some(Cow::Owned(format!("edit.rate-{n}"))),
        Action::ColourMenu => s("edit.colour-menu"),
        Action::Colour(None) => s("edit.colour-none"),
        Action::Colour(Some(c)) => Some(Cow::Owned(format!("edit.colour-{}", format!("{c:?}").to_lowercase()))),
        Action::AddToPlaylist => s("edit.add-to-playlist"),
        Action::NewPlaylistWith => s("edit.new-playlist-with"),
        Action::RemoveFromList => s("edit.remove-from-list"),
        Action::SortList => s("edit.sort-list"),
        Action::NewPlaylist => s("edit.new-playlist"),
        Action::NewFolder => s("edit.new-folder"),
        Action::Rename => s("edit.rename"),
        Action::Delete => s("edit.delete"),
        Action::Rate(_)
        | Action::CrateAdd
        | Action::SaveCrate
        | Action::ClearCrate
        | Action::Replay
        | Action::Restore
        | Action::Tag
        | Action::PlayNext
        | Action::Collab
        | Action::Plugin(..)
        | Action::Fetch(_) => None,
    }
}

/// The id of a command line to copy.
fn copy_id(command: &str) -> Option<Cow<'static, str>> {
    let id = match command.split(' ').take(2).collect::<Vec<_>>()[..] {
        ["drive", "format"] => "terminal.drive-format",
        ["drive", "fsck"] => "terminal.drive-fsck",
        ["drive", "firmware"] => "terminal.drive-firmware",
        ["provision", _] => "terminal.provision",
        ["store", "gc"] => "terminal.vault-gc",
        ["store", "verify"] => "terminal.vault-verify",
        ["store", "prune"] => "terminal.vault-prune",
        ["store", "copy"] => "terminal.vault-copy",
        ["bug"] => "terminal.bug",
        _ => return None,
    };
    Some(Cow::Borrowed(id))
}

/// Every act that can be bound now, each once: the keymap's, the library's own actions, then the
/// plugins' commands and operations.
pub fn bindable(plugins: &[&'static Manifest]) -> Vec<Act> {
    let plugin_acts = plugins.iter().flat_map(|m| {
        let c = &m.contributes;
        let names = c.commands.iter().map(|x| x.name.as_str()).chain(c.operations.iter().map(|x| x.name.as_str()));
        names.map(|name| Act::Plugin(&m.id, name))
    });
    let mut out: Vec<Act> = Vec::new();
    for act in KEYMAP.iter().map(|b| b.act).chain(edits()).chain(plugin_acts) {
        if id(act).is_some() && !out.contains(&act) {
            out.push(act);
        }
    }
    out
}

/// The act `id` names, among [`bindable`]; none for an id this version doesn't know.
pub fn act(id: &str, plugins: &[&'static Manifest]) -> Option<Act> {
    bindable(plugins).into_iter().find(|a| self::id(*a).as_deref() == Some(id))
}
