//! Publishing a set at a plugin's set destination (`set/publish`): planned with a dry run, which
//! says what would happen and publishes nothing, then applied once the user confirms (publishing
//! reaches outside this computer, so it's always asked). The plugin answers where the set can be
//! heard, and the host keeps that as the set's link, with the link's name and the Atmosphere
//! record holding the recording when the plugin gives them.

use std::path::PathBuf;
use std::sync::{Arc, Weak};

use chrono::{DateTime, Utc};
use jacquard_common::SmolStr;
use jacquard_common::types::aturi::AtUri;
use jacquard_common::types::cid::Cid;
use loclib_plugin::{DestinationKind, PublishSet, Published, PublishedSet, StrongRef, TracklistEntry, method};
use schemars::JsonSchema;
use serde::Serialize;

use super::super::Plugins;
use super::super::process::Slot;
use crate::op::{Operation, Planned, Probe, Touched};
use crate::profile::{PREFS_FILE, ProfilePrefs};
use crate::progress::Ctx;
use crate::sets::{Set, SetBook, Unnamed};
use crate::store::Store;
use crate::{Error, Result};

/// A set a plugin will publish, as its dry run said.
#[derive(Serialize, JsonSchema)]
pub struct SetPublish {
    pub plugin: String,
    pub destination: String,
    /// The set, by id.
    pub set: String,
    /// Its recording, on this computer, which goes too.
    pub recording: PathBuf,
    pub tracks: usize,
    /// Where it would be, when the dry run said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The link the set has now, which publishing replaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
    #[serde(skip)]
    #[schemars(skip)]
    publish: PublishSet,
    /// When the set was last changed: a set changed since is planned again.
    #[serde(skip)]
    #[schemars(skip)]
    updated_at: DateTime<Utc>,
    /// The plugin, while it's hosted: a plan kept doesn't keep it.
    #[serde(skip)]
    #[schemars(skip)]
    slot: Weak<Slot>,
}

/// What publishing a set did.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SetPublished {
    pub set: String,
    /// Where it can be heard, which is the set's link now; none when the plugin didn't say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// What the link is (`plyr.fm`), when the plugin named it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The Atmosphere record holding the recording, when the plugin keeps it as one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<StrongRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// The longest name a link has, in characters: the set record's Lexicon takes 64 graphemes, and
/// a grapheme is at least one character.
const LINK_NAME_CHARS: usize = 64;

/// The set as it's published: its details and tracklist, never its notes, and the tracks at
/// `unnamed`'s positions as only their place.
fn published(set: &Set, unnamed: &Unnamed) -> (PublishedSet, Vec<TracklistEntry>) {
    let details = PublishedSet {
        id: set.id.clone(),
        date: set.date.to_string(),
        title: set.title.clone(),
        kind: set.kind.clone(),
        venue: set.venue.clone(),
        city: set.city.clone(),
        country: set.country.clone(),
    };
    let tracklist = (1..)
        .zip(&set.tracks)
        .map(|(n, e)| {
            let named = !unnamed.positions.contains(&n);
            TracklistEntry {
                artist: named.then(|| e.artist.clone()),
                title: named.then(|| e.title.clone()),
                start_seconds: e.start_seconds,
            }
        })
        .collect();
    (details, tracklist)
}

impl Plugins {
    /// Plans publishing the set `set` names (see [`SetBook::find`]) with its recording at the
    /// set destination `spec` names (`<plugin>:<destination>`): the plugin's dry run says what
    /// would happen, publishing nothing. Tracks the set marks unnamed, those at the positions
    /// (from 1) in `hide` and those the DJ's profile hides go without their names (see
    /// [`Set::unnamed`]), and the plan says which. The plan (operation
    /// `<plugin>.publish.<destination>`) is stale once the set, its recording or the profile's
    /// choices change, and destructive when the set has a link already, which publishing replaces.
    pub fn publish_plan(
        &self,
        spec: &str,
        set: &str,
        hide: &[usize],
        store: &Store,
        ctx: &Ctx<'_>,
    ) -> Result<Planned<SetPublish>> {
        let (slot, destination) = self.destination(spec, DestinationKind::Set)?;
        let book = SetBook::load(store)?;
        let set = book.find(set)?;
        let recording = set.recording.as_ref().map(|r| r.path.clone()).ok_or_else(|| {
            Error::invalid(format!("{} has no recording to publish: a set destination publishes one", set.heading()))
        })?;
        if !recording.is_file() {
            return Err(Error::NotFound(format!("{}, the set's recording", recording.display())));
        }
        let unnamed = set.unnamed(hide, &ProfilePrefs::load(store)?)?;
        let (details, tracklist) = published(set, &unnamed);
        let (context, _) = slot.context(loclib_plugin::Context::default(), Some(store))?;
        let mut publish =
            PublishSet { destination: destination.clone(), set: details, recording, tracklist, dry_run: true, context };
        let Published { url, warnings, .. } = slot.call(method::SET_PUBLISH, &publish, Some(store), ctx)?;
        publish.dry_run = false;
        let title =
            &slot.manifest.contributes.destinations.iter().find(|d| d.name == destination).expect("declared").title;
        let mut summary =
            vec![format!("publish {} ({} tracks) and its recording at {title}", set.heading(), set.tracks.len())];
        summary.extend(unnamed.notes);
        summary.extend(url.as_ref().map(|u| format!("it would be at {u}")));
        let replaces = set.link.clone();
        summary.extend(replaces.as_ref().map(|l| format!("replace its link {l}")));
        let file = publish.recording.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let folder = publish.recording.parent().unwrap_or(&publish.recording).to_path_buf();
        let plan = SetPublish {
            plugin: slot.manifest.id.clone(),
            destination: destination.clone(),
            set: set.id.clone(),
            recording: publish.recording.clone(),
            tracks: set.tracks.len(),
            url,
            replaces,
            publish,
            updated_at: set.updated_at,
            slot: Arc::downgrade(slot),
        };
        let destructive = plan.replaces.is_some();
        Ok(Planned::new(format!("{}.publish.{destination}", plan.plugin), plan, ())
            .summary(summary)
            .warnings(warnings)
            .destructive(destructive)
            .touches(Touched::path(&folder, false, vec![Probe::Stamp(vec![file])]))
            .touches(Touched::path(store.root(), false, vec![Probe::Content(vec![PREFS_FILE.into()])])))
    }
}

impl Operation for SetPublish {
    type Request = ();
    type Report = SetPublished;

    /// Has the plugin publish the set, then keeps where it says it can be heard as the set's
    /// link (an https address, or none) with the link's name, and the record it says holds the
    /// recording (an at:// URI and a CID). An answer that isn't those is the plugin misbehaving,
    /// and nothing of it is kept.
    fn apply(planned: &Planned<Self>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<SetPublished> {
        let p = &planned.plan;
        let store = store.ok_or_else(|| Error::invalid("publishing a set needs the store"))?;
        let unchanged = SetBook::load(store)?.find(&p.set).is_ok_and(|s| s.updated_at == p.updated_at);
        if !unchanged {
            return Err(Error::stale(format!("set {}", p.set), "changed"));
        }
        let slot = p.slot.upgrade().ok_or_else(|| Error::stale(format!("the {} plugin", p.plugin), "stopped"))?;
        // Lent its recording: what it sends of it is the publishing's progress.
        let lent = [p.publish.recording.as_path()];
        let Published { url, name, record, warnings } =
            slot.call_lending(method::SET_PUBLISH, &p.publish, &lent, Some(store), ctx)?;
        if let Some(url) = &url {
            crate::opener::web_address(url).map_err(|_| {
                slot.misbehaved(format!("it says the set is at {url:?}, which isn't an https:// address"))
            })?;
        }
        // A name goes with the link it names.
        let name = url.as_ref().and(name.map(|n| n.trim().to_string())).filter(|n| !n.is_empty());
        if let Some(name) = &name
            && name.chars().count() > LINK_NAME_CHARS
        {
            return Err(
                slot.misbehaved(format!("it names the link {name:?}, longer than {LINK_NAME_CHARS} characters"))
            );
        }
        if let Some(r) = &record
            && !(AtUri::<SmolStr>::new_owned(&r.uri).is_ok() && Cid::str(&r.cid).is_valid())
        {
            return Err(slot.misbehaved(format!(
                "it says the recording is the record {:?} ({:?}), which isn't an at:// URI and a CID",
                r.uri, r.cid
            )));
        }
        if url.is_some() || record.is_some() {
            let _lock = store.lock()?;
            let mut book = SetBook::load(store)?;
            let set = book.find_mut(&p.set)?;
            if url.is_some() {
                (set.link, set.link_name) = (url.clone(), name.clone());
            }
            if record.is_some() {
                set.recording_record = record.clone();
            }
            set.updated_at = Utc::now();
            book.save(store)?;
        }
        Ok(SetPublished { set: p.set.clone(), url, name, record, warnings })
    }
}
