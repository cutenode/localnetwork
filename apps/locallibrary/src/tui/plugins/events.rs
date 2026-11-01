//! Plugins' events and services in the player: what they tell the user is a message, and what
//! they suggest (one of their operations, or one of the player's own flows on their
//! contributions) waits in the palette to be reviewed (started as the user would, then confirmed
//! as any plan) or dismissed, kept in the store until it is, with what the `watch` agent kept for
//! it. The services the user turned on run while the player does.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};

use locallibrary::plugin::{Cli, Notices, PluginNotice, forget_suggested, keep_suggested, suggested, tracks_on};
use locallibrary::{Result, Store};
use loclib_plugin::{Context, HostAction, Manifest, Suggested, Suggestion};

use super::{Run, named};
use crate::tui::app::{App, Level, Msg};
use crate::tui::edits::Modal;
use crate::tui::keymap::Act;
use crate::tui::palette::Item;

/// The plugin's name, as its manifest has it (its id, if it's gone).
fn name(plugin: &str) -> String {
    crate::plugins::host().and_then(|p| p.manifest(plugin)).map_or_else(|_| plugin.into(), |m| m.name.clone())
}

/// How the palette names a suggestion: the same for the same one, whatever else is kept.
fn mark(notice: &PluginNotice) -> u64 {
    let mut h = DefaultHasher::new();
    serde_json::to_string(notice).unwrap_or_default().hash(&mut h);
    h.finish()
}

/// Held while the player changes the suggestions kept and says what they are now: what it's
/// told last is what's kept.
pub(super) static CHANGING: Mutex<()> = Mutex::new(());

/// `change`s the suggestions kept in the store at `store_root`, and tells the player what they
/// are then (or what went wrong).
fn change_kept(tx: &Sender<Msg>, store_root: &Path, change: impl FnOnce(&Path) -> Result<Vec<PluginNotice>>) {
    let _one = CHANGING.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = tx.send(match change(store_root) {
        Ok(kept) => Msg::Suggested(kept),
        Err(e) => Msg::Note(format!("suggestions: {e}"), Level::Error),
    });
}

impl App {
    /// Where what plugins tell the user goes while the player runs: its messages, and what they
    /// suggest kept in the store (on the thread reading the plugin, so in the order it's told).
    pub(in crate::tui) fn plugin_notices(&self) -> Notices {
        let (tx, store_root) = (self.sender(), self.store_root.clone());
        Arc::new(move |notice: PluginNotice| {
            if notice.suggests.is_some() {
                change_kept(&tx, &store_root, |root| keep_suggested(&Store::open_or_init(root)?, &notice));
            }
            let _ = tx.send(Msg::Notice(notice));
        })
    }

    /// [`change_kept`], in the background.
    fn change_kept(&self, change: impl FnOnce(&Path) -> Result<Vec<PluginNotice>> + Send + 'static) {
        let (tx, store_root) = (self.sender(), self.store_root.clone());
        std::thread::spawn(move || change_kept(&tx, &store_root, change));
    }

    /// Offers what plugins suggested before (in the player or the `watch` agent), saying how many
    /// it can't (another version's), and runs the services the user turned on, in the background.
    pub(in crate::tui) fn start_plugins(&self) {
        let tx = self.sender();
        self.change_kept(move |root| {
            let Ok(store) = Store::open(root) else { return Ok(Vec::new()) };
            let kept = suggested(&store)?;
            if kept.unread > 0 {
                let unread = crate::output::plural(kept.unread, "suggestion");
                let _ = tx.send(Msg::Note(format!("{unread} from another version can't be offered here"), Level::Info));
            }
            Ok(kept.notices)
        });
        let store_root = self.store_root.clone();
        std::thread::spawn(move || {
            if let Ok(plugins) = crate::plugins::host() {
                plugins.run_services(Store::open(&store_root).ok().as_ref());
            }
        });
    }

    /// What a plugin tells the user, as a message, saying when it suggests something to review.
    pub(in crate::tui) fn told(&mut self, notice: PluginNotice) {
        let by = name(&notice.plugin);
        let review = if notice.suggests.is_some() { " (: to review)" } else { "" };
        self.toast(format!("{by}: {}{review}", notice.message), Level::Info);
    }

    /// What plugins suggested, as the palette lists them, newest first: to review, or dismiss.
    pub(in crate::tui) fn suggested_items(&self) -> Vec<(Act, Item)> {
        let items = |n: &PluginNotice| {
            let by = name(&n.plugin);
            [
                (Act::Review(mark(n)), Item::new(format!("review: {}", n.message), &by)),
                (Act::Dismiss(mark(n)), Item::new(format!("dismiss: {}", n.message), &by)),
            ]
        };
        self.suggested.iter().rev().flat_map(items).collect()
    }

    /// The suggestion marked `marked`, no longer offered, and no longer kept once the store
    /// forgets it in the background: none when it went since.
    fn forget(&mut self, marked: u64) -> Option<PluginNotice> {
        let i = self.suggested.iter().position(|n| mark(n) == marked)?;
        let notice = self.suggested.remove(i);
        self.forgetting.push(marked);
        let (tx, forgotten) = (self.sender(), notice.clone());
        self.change_kept(move |root| {
            let kept = Store::open(root).and_then(|store| forget_suggested(&store, &forgotten));
            let _ = tx.send(Msg::Forgotten(marked));
            kept
        });
        Some(notice)
    }

    /// What's kept now, told after a change to it: without what was reviewed or dismissed since
    /// (the store forgets those in turn, and what it kept before it did would offer them again).
    pub(in crate::tui) fn kept(&mut self, kept: Vec<PluginNotice>) {
        self.suggested = kept.into_iter().filter(|n| !self.forgetting.contains(&mark(n))).collect();
    }

    /// The store has forgotten the suggestion marked `marked` (or failed to).
    pub(in crate::tui) fn forgotten(&mut self, marked: u64) {
        if let Some(i) = self.forgetting.iter().position(|m| *m == marked) {
            self.forgetting.swap_remove(i);
        }
    }

    /// Starts what a plugin suggested (the one marked `marked`, see [`Act::Review`]) as the user
    /// would, its params filled in: what it plans is confirmed as any plan is, or it asks first.
    /// One gone since is not.
    pub(in crate::tui) fn review(&mut self, marked: u64) {
        let Some(notice) = self.forget(marked) else { return };
        let Ok(m) = crate::plugins::host().and_then(|p| p.manifest(&notice.plugin)) else { return };
        match notice.suggests {
            Some(Suggestion::Operation(o)) => self.review_operation(m, o),
            Some(Suggestion::Action(a)) => self.review_action(m, a),
            None => {}
        }
    }

    /// Plans one of `m`'s operations, on the drive actions work on.
    fn review_operation(&mut self, m: &'static Manifest, suggested: Suggested) {
        let Some(op) = m.contributes.operations.iter().find(|o| o.name == suggested.name) else { return };
        let root = self.action_drive();
        let context = Context { drive: root.as_deref().and_then(|r| self.plugin_drive(r)), ..Context::default() };
        let run = Run { plugin: &m.id, hook: Cli::Operation(op), root, context };
        self.run_plugin(run, suggested.params);
    }

    /// Opens the player's own flow for one of the host's actions, on `m`'s contributions and the
    /// drive it names, plugged in (the drive actions work on, for a set's export naming none), and
    /// the tracks it names that the drive has, each once: fetching from its providers and
    /// exporting in its format open their forms filled in, saying what they're for, and analyzing
    /// asks first.
    fn review_action(&mut self, m: &'static Manifest, mut action: HostAction) {
        let root = match action.drive().map(|d| self.named_drive(d)) {
            Some(Ok(root)) => Some(root),
            Some(Err(why)) => return self.toast(why, Level::Info),
            None => self.action_drive(),
        };
        let Some(root) = self.drive_open(root) else { return };
        let label = self.drive_label(&root);
        if let (_, Some(tracks)) = action.scope_mut()
            && !tracks.is_empty()
        {
            let counted = match self.loaded.as_ref().filter(|l| l.root() == root) {
                Some(l) => Ok(tracks_on(&l.library, tracks)),
                None => locallibrary::Device::open(&root).and_then(|d| d.library()).map(|l| tracks_on(&l, tracks)),
            };
            match counted {
                Err(e) => return self.toast(e.to_string(), Level::Error),
                Ok((on, _)) if on.is_empty() => {
                    return self.toast(format!("none of the tracks it suggests are on {label}"), Level::Info);
                }
                Ok((on, missing)) => {
                    if missing > 0 {
                        let left = crate::output::plural(missing, "track");
                        self.toast(format!("left out {left} {label} doesn't have"), Level::Info);
                    }
                    *tracks = on;
                }
            }
        }
        let scope = |tracks: &[u32]| match tracks.len() {
            0 => format!("every track on {label}"),
            n => format!("{} on {label}", crate::output::plural(n, "track")),
        };
        match action {
            HostAction::Export(e) => {
                let Some(x) = m.contributes.exporters.iter().find(|x| x.format == e.format) else { return };
                let Some(from) = e.from().filter(|f| x.takes(*f)) else { return };
                self.export_form(&m.id, &x.format, from, Some(root), Some(&e));
            }
            HostAction::Analyze(a) => {
                let Some(x) = m.contributes.analyzers.iter().find(|x| x.name == a.analyzer) else { return };
                let question = format!("Analyze {} with {}?", scope(&a.tracks), named(&x.title));
                let tracks = (!a.tracks.is_empty()).then_some(a.tracks);
                let (plugin, analyzer) = (m.id.as_str(), x.name.as_str());
                let yes = move |app: &mut App| app.analyze(plugin, analyzer, Some(root), tracks);
                self.modal = Some(Modal::Ask(question, Box::new(yes)));
            }
            fetch => {
                let Some((kind, f)) = fetch.fetch() else { return };
                let from = f.provider.as_ref().map_or_else(|| m.id.clone(), |p| format!("{}.{p}", m.id));
                let title = format!("Fetch {}: {}", kind.found(), scope(&f.tracks));
                self.fetch_form_from(kind, title, root, f.tracks.clone(), Some(from));
            }
        }
    }

    /// No longer offers what a plugin suggested (the one marked `marked`).
    pub(in crate::tui) fn dismiss(&mut self, marked: u64) {
        if self.forget(marked).is_some() {
            self.toast("suggestion dismissed", Level::Info);
        }
    }
}
