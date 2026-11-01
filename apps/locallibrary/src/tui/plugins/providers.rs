//! Providers in the player: the cover art and tags they find for the tracks selected, offered
//! track by track (one alone sure enough picked already), the edits planned and confirmed, and
//! when a candidate is picked for the user, a form in the palette.

use std::collections::VecDeque;
use std::path::PathBuf;

use indexmap::IndexMap;
use locallibrary::Store;
use locallibrary::plugin::{
    AUTO_PICK, Offer, Pick, PlanProvided, Plugins, Provide, Provided, auto_pick, plan_provided, set_auto_pick,
};
use locallibrary::query::TrackSelector;
use loclib_plugin::ProviderKind;
use serde_json::{Map, Value, json};

use super::Asking;
use crate::tui::app::{App, Level};
use crate::tui::edits::{Modal, Target};
use crate::tui::form::{For, Form};

/// Fetching from providers for the tracks of the drive at `root`: the picks so far, the tracks
/// left to ask about, and the images found, by id.
pub struct Fetching {
    kind: ProviderKind,
    root: PathBuf,
    replace: bool,
    picks: Vec<Pick>,
    left: VecDeque<Offer>,
    images: IndexMap<String, String>,
}

/// What providers fetch for the tracks selected, as a menu offers it on a drive (see
/// [`super::offered`]): each kind some plugin provides, with its providers' plugins' names, and
/// why it can't run here, if it can't.
pub fn fetches(drive: bool) -> Vec<(ProviderKind, String, String, Option<String>)> {
    let Ok(plugins) = crate::plugins::host() else { return Vec::new() };
    let why = (!drive).then_some("open a drive first");
    let kinds = [ProviderKind::Artwork, ProviderKind::Metadata];
    let found = kinds.into_iter().filter_map(|kind| {
        let by = providing(plugins, Some(kind))?;
        Some((kind, format!("fetch {}…", kind.found()), by, why.map(String::from)))
    });
    found.collect()
}

/// The names of the plugins with a provider (of `kind`, when it says), each once: none when
/// there's none.
pub(super) fn providing(plugins: &Plugins, kind: Option<ProviderKind>) -> Option<String> {
    let mut by: Vec<&str> = Vec::new();
    for (plugin, _) in plugins.providers(kind) {
        let name = plugins.manifest(plugin).map_or(plugin, |m| m.name.as_str());
        if !by.contains(&name) {
            by.push(name);
        }
    }
    (!by.is_empty()).then(|| by.join(", "))
}

/// The providers' settings form's field for the score a candidate is picked for the user at.
const AUTO_PICK_FIELD: &str = "auto_pick";

impl App {
    /// The providers' settings in a form, as they're set now: the score a candidate alone at or
    /// above is picked for the user with (`plugin providers --auto-pick`).
    pub(in crate::tui) fn provider_settings(&mut self) {
        let store = Store::open(&self.store_root).ok();
        let description = format!("Pick a candidate alone scoring this or more, 0 to 1 (empty for {AUTO_PICK})");
        let field = json!({ "type": "number", "minimum": 0, "description": description });
        let schema = json!({ "type": "object", "properties": { AUTO_PICK_FIELD: field } });
        let filled = Map::from_iter([(AUTO_PICK_FIELD.to_string(), Value::from(auto_pick(store.as_ref())))]);
        let form = Form::with("Provider settings".into(), Some(schema), &filled, For::ProviderSettings);
        self.modal = Some(Modal::Form(form));
    }

    /// Keep the providers' settings as the form has them (`given`), in the background, kept where
    /// `plugin providers` keeps them; left empty, back to the default.
    pub(in crate::tui) fn save_provider_settings(&mut self, given: Value) {
        let store_root = self.store_root.clone();
        let score = given[AUTO_PICK_FIELD].as_f64();
        self.task("Saving settings".into(), move |_| {
            let store = Store::open_or_init(&store_root)?;
            let said = match score.unwrap_or(AUTO_PICK) == auto_pick(Some(&store)) {
                true => "nothing changed",
                false => {
                    set_auto_pick(&store, score)?;
                    "provider settings saved"
                }
            };
            Ok(Box::new(move |app: &mut App| app.toast(said, Level::Info)))
        });
    }

    /// What providers of `kind` find for the tracks `target` selects on the drive at `root`: a
    /// form asks which provider (any, to start with) and whether over what tracks have first.
    pub(in crate::tui) fn fetch_form(&mut self, kind: ProviderKind, root: Option<PathBuf>, target: Option<&Target>) {
        let Some(root) = self.drive_open(root) else { return };
        let tracks = target.map(|t| self.context(t).tracks).unwrap_or_default();
        if tracks.is_empty() {
            return self.toast("no track selected", Level::Info);
        }
        self.fetch_form_from(kind, format!("Fetch {}", kind.found()), root, tracks, None);
    }

    /// [`App::fetch_form`] for `tracks` (every one, when none), titled `title`, only from `from` to
    /// start with when it says (`<plugin>` or `<plugin>.<provider>`, as a plugin's suggestion has it).
    pub(super) fn fetch_form_from(
        &mut self,
        kind: ProviderKind,
        title: String,
        root: PathBuf,
        tracks: Vec<u32>,
        from: Option<String>,
    ) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let mut names: Vec<String> =
            plugins.providers(Some(kind)).into_iter().map(|(plugin, p)| format!("{plugin}.{}", p.name)).collect();
        names.extend(from.clone().filter(|f| !names.contains(f)));
        let schema = json!({
            "type": "object",
            "properties": {
                "from": { "type": "string", "enum": names, "description": "Only from this provider" },
                "replace": { "type": "boolean", "description": "Also over what tracks have" }
            }
        });
        let filled = Map::from_iter(from.map(|f| ("from".to_string(), Value::from(f))));
        let form = Form::with(title, Some(schema), &filled, For::Fetch { kind, root, tracks });
        self.modal = Some(Modal::Form(form));
    }

    /// Ask the providers of `kind` (the one `given` names, if it does) about `tracks` on the drive
    /// at `root`, in the background: what they find is offered track by track.
    pub(in crate::tui) fn fetch(&mut self, kind: ProviderKind, root: PathBuf, tracks: Vec<u32>, given: Value) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let store_root = self.store_root.clone();
        let replace = given["replace"] == true;
        let req = Provide {
            kind,
            drive: root.display().to_string(),
            tracks: TrackSelector { ids: tracks, ..TrackSelector::default() },
            from: given["from"].as_str().map(String::from),
            replace,
        };
        self.task(format!("Fetching {}", kind.found()), move |ctx| {
            let provided = plugins.provide(&req, Store::open(&store_root).ok().as_ref(), ctx)?;
            Ok(Box::new(move |app: &mut App| app.fetched(root, replace, provided)))
        });
    }

    /// What the providers found: their warnings said, then each track's candidates asked about in
    /// turn, but those picked already.
    pub(in crate::tui) fn fetched(&mut self, root: PathBuf, replace: bool, provided: Provided) {
        let Provided { kind, asked, offers, images, warnings } = provided;
        for w in &warnings {
            self.toast(w.clone(), Level::Info);
        }
        let what = kind.found();
        let why = if warnings.is_empty() { "" } else { " (! says why)" };
        if asked == 0 {
            return self.toast(format!("the tracks selected have {what} already"), Level::Info);
        }
        if offers.is_empty() {
            return self
                .toast(format!("no {what} found for {}{why}", crate::output::plural(asked, "track")), Level::Info);
        }
        let (picked, left): (Vec<Offer>, Vec<Offer>) = offers.into_iter().partition(|o| o.picked.is_some());
        let picks = picked.iter().filter_map(Offer::pick).collect();
        self.next_offer(Fetching { kind, root, replace, picks, left: left.into(), images });
    }

    /// Go on with what was chosen for a track's candidates (`values`, an item's at most): then the
    /// next track's, then the plan.
    pub(super) fn answer_offer(&mut self, offer: Offer, mut fetching: Fetching, values: Vec<Value>) {
        fetching.picks.extend(values.first().and_then(|v| offer.chosen(v)));
        self.next_offer(fetching)
    }

    /// The next track to ask about, or once there's none the plan of the picks, to confirm.
    fn next_offer(&mut self, mut f: Fetching) {
        if let Some(offer) = f.left.pop_front() {
            let choice = offer.choice();
            return self.ask(Asking::Offer { offer, choice, fetching: f });
        }
        if f.picks.is_empty() {
            return self.toast(format!("no {} chosen", f.kind.found()), Level::Info);
        }
        let (drive, images) = (f.root.display().to_string(), f.images);
        let req = PlanProvided { kind: f.kind, drive, picks: f.picks, images, replace: f.replace };
        self.plan(f.root, move |ctx| Ok(Box::new(plan_provided(&req, ctx)?)));
    }
}
