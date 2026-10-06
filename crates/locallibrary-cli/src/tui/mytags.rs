//! My Tags in the player (rekordbox's own track tags, which players filter by): giving the marked
//! or selected tracks a tag or taking one off, through the library's tags plan; and the store's
//! rule-based playlists ("smart" playlists): listing the rules, filling them on the open drive,
//! and making one from the search box's filter. The My Tags column and Browse › My Tags are drawn
//! from the tags the library load reads (`app.rs`, `browse.rs`).

use std::collections::HashSet;
use std::path::PathBuf;

use locallibrary::api::{self, DriveRef, SetSmartPlaylist, TagAction, TagsEdit};
use locallibrary::query::{Filter, TrackSelector};
use locallibrary::{Error, Store, paths};

use super::app::{App, Level};
use super::browse::sort_spec;
use super::drives::{Report, Tone};
use super::edits::{Modal, Prompt, PromptFor};
use super::palette::{Item, Picker, Then};

/// A choice in the list of tags.
#[derive(Debug, Clone)]
pub enum TagChoice {
    Add(String),
    Remove(String),
    /// A tag that isn't on the drive yet: typed next.
    New,
}

impl App {
    /// `T`: give the marked (or selected) tracks a My Tag, or take one off. The drive's tags are
    /// read in the background, then listed to choose from.
    pub(super) fn tag_tracks(&mut self) {
        let Some(l) = self.loaded.as_ref() else {
            self.toast("open a drive first", Level::Info);
            return;
        };
        if l.stale {
            self.toast("the drive was unplugged", Level::Error);
            return;
        }
        let root = l.root().to_path_buf();
        let tracks: Vec<String> = self.targets().iter().map(|&i| l.library.tracks[i].file_path.clone()).collect();
        if tracks.is_empty() {
            self.toast("no track selected", Level::Info);
            return;
        }
        let drive = root.display().to_string();
        self.task("Reading My Tags".into(), move |_| {
            let tags = api::tags_list(&DriveRef { drive })?;
            Ok(Box::new(move |app: &mut App| {
                let chosen: HashSet<String> = tracks.iter().map(|p| paths::key(p).to_lowercase()).collect();
                // A new tag first: the list of existing ones can be long.
                let mut items = vec![Item::new("a new tag…", "Category/Tag")];
                let mut choices = vec![TagChoice::New];
                for t in tags.categories.iter().flat_map(|c| &c.tags) {
                    let on =
                        t.tracks.iter().filter(|r| chosen.contains(&paths::key(&r.file_path).to_lowercase())).count();
                    let detail = format!("{} track{}", t.tracks.len(), if t.tracks.len() == 1 { "" } else { "s" });
                    if on == chosen.len() {
                        items.push(Item::new(format!("take {} off", t.path), detail));
                        choices.push(TagChoice::Remove(t.path.clone()));
                    } else {
                        items.push(Item::new(t.path.clone(), detail));
                        choices.push(TagChoice::Add(t.path.clone()));
                    }
                }
                let n = tracks.len();
                let title = format!("My Tag for {n} track{}", if n == 1 { "" } else { "s" });
                app.modal = Some(Modal::Pick(Picker::new(title, items, Then::Tag { root, tracks, choices })));
            }))
        });
    }

    /// A tag was chosen (or a new one is to be typed).
    pub(super) fn tag_chosen(&mut self, root: PathBuf, tracks: Vec<String>, choice: TagChoice) {
        let (tag, add) = match choice {
            TagChoice::Add(t) => (t, true),
            TagChoice::Remove(t) => (t, false),
            TagChoice::New => {
                let n = tracks.len();
                self.modal = Some(Modal::Prompt(Prompt {
                    label: format!(
                        "New My Tag for {n} track{} (Genre/…, Components/…, Situation/…)",
                        if n == 1 { "" } else { "s" }
                    ),
                    text: "Genre/".into(),
                    purpose: PromptFor::NewTag { tracks },
                    root,
                }));
                return;
            }
        };
        self.plan_tag(root, tracks, tag, add);
    }

    /// Plan giving `tracks` the tag (`add`) or taking it off.
    pub(super) fn plan_tag(&mut self, root: PathBuf, tracks: Vec<String>, tag: String, add: bool) {
        let drive = root.display().to_string();
        let tracks = TrackSelector::paths(tracks);
        let action = if add { TagAction::Add { tag, tracks } } else { TagAction::Remove { tag, tracks } };
        self.plan(root, move |_| Ok(Box::new(api::tags_plan(&TagsEdit { drive, action })?)));
    }

    // -----------------------------------------------------------------------------------------
    // Rule-based playlists
    // -----------------------------------------------------------------------------------------

    /// The store's rule-based playlists, in a panel.
    pub(super) fn smart_list(&mut self) {
        let store_root = self.store_root.clone();
        self.task("Reading the rules".into(), move |_| {
            let store =
                Store::open(&store_root).map_err(|_| Error::NotFound("a store: pull a drive to make one".into()))?;
            let all = api::smart_list(&store)?;
            let mut lines = Vec::new();
            if all.playlists.is_empty() {
                lines.push((Tone::Text, "No rules yet.".to_string()));
                lines.push((
                    Tone::Faint,
                    "Search with a filter (/ bpm:124-128 key:8A~), then : and \"smart playlist from the search\"."
                        .to_string(),
                ));
            }
            for p in &all.playlists {
                lines.push((Tone::Heading, p.path.clone()));
                let mut extra = vec![p.filter.clone()];
                if let Some(s) = &p.sort {
                    extra.push(format!("sorted by {s}"));
                }
                if let Some(n) = p.limit {
                    extra.push(format!("at most {n}"));
                }
                extra.push(match p.drives.len() {
                    0 => "every drive".to_string(),
                    n => format!("{n} drive{} only", if n == 1 { "" } else { "s" }),
                });
                lines.push((Tone::Text, extra.join("  ·  ")));
            }
            if !all.playlists.is_empty() {
                lines.push((Tone::Text, String::new()));
                lines.push((
                    Tone::Faint,
                    "Drives get ordinary playlists: : and \"fill the smart playlists\" (re)fills them on the drive."
                        .to_string(),
                ));
            }
            Ok(Box::new(move |app: &mut App| {
                app.modal = Some(Modal::Report(Report { title: "Smart playlists".into(), lines, scroll: 0 }));
            }))
        });
    }

    /// Fill the rule-based playlists on the drive (the open one, or the one selected in the
    /// Drives view), once the plan is confirmed.
    pub(super) fn smart_apply(&mut self) {
        let Some(root) = self.action_drive() else {
            self.toast("open a drive first", Level::Info);
            return;
        };
        let (drive, store_root) = (root.display().to_string(), self.store_root.clone());
        self.plan(root, move |ctx| {
            let store = Store::open(&store_root).map_err(|_| Error::NotFound("a store with rules in it".into()))?;
            Ok(Box::new(api::smart_apply_plan(&store, &DriveRef { drive }, ctx)?))
        });
    }

    /// A rule from the search box's filter (and the list's sort): asks where the playlist goes,
    /// saves the rule in the store, then shows what filling it on the open drive does.
    pub(super) fn smart_from_search(&mut self) {
        let query = self.query.trim().to_string();
        if query.is_empty() {
            self.toast("search first (/): the rule fills the playlist with what the search finds", Level::Info);
            return;
        }
        if let Err(e) = Filter::parse(&query) {
            self.toast(format!("the search isn't a filter a rule can keep: {e}"), Level::Error);
            return;
        }
        let Some(root) = self.loaded.as_ref().map(|l| l.root().to_path_buf()) else { return };
        let name: String = query.chars().map(|c| if c == '/' { '-' } else { c }).collect();
        self.modal = Some(Modal::Prompt(Prompt {
            label: "Smart playlist at".into(),
            text: format!("Smart/{name}"),
            purpose: PromptFor::SmartRule { filter: query, sort: self.sort.map(sort_spec) },
            root,
        }));
    }

    /// Save the rule for the playlist at `path`, then plan filling it on the drive at `root`.
    pub(super) fn save_smart_rule(&mut self, root: PathBuf, path: String, filter: String, sort: Option<String>) {
        let store_root = self.store_root.clone();
        let drive = root.display().to_string();
        let shown = path.clone();
        self.task("Saving the rule".into(), move |_| {
            let store = Store::open_or_init(&store_root)?;
            let store_root = store_root.clone();
            api::smart_set(&store, &SetSmartPlaylist { path, filter, sort, limit: None, drives: Vec::new() })?;
            Ok(Box::new(move |app: &mut App| {
                app.toast(format!("rule saved for {shown}: here's what filling it does"), Level::Info);
                let root = PathBuf::from(drive);
                app.plan(root.clone(), move |ctx| {
                    let store = Store::open(&store_root)?;
                    let drive = root.display().to_string();
                    Ok(Box::new(api::smart_apply_plan(&store, &DriveRef { drive }, ctx)?))
                });
            }))
        });
    }
}
