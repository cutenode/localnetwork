//! The show a set becomes on Mixcloud: its name, where and when it was played as the description,
//! the `tags` setting's tags and the tracklist as the show's sections, as Mixcloud's upload form
//! takes them (mixcloud.com/developers, "Uploading shows"): a description of 1,000 characters at
//! most, five tags, and a section for each track, its artist and song, starting `start_time` whole
//! seconds into the show. Mixcloud says of no other limit.

use loclib_plugin::{FormPart, PublishedSet, TracklistEntry};

/// The longest description, in characters.
pub(crate) const MAX_DESCRIPTION: usize = 1000;
/// How many tags a show has at most.
pub(crate) const MAX_TAGS: usize = 5;

/// A set's show, and what was cut or left out to fit, in a line each.
#[derive(Debug, PartialEq)]
pub(crate) struct Show {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) tags: Vec<String>,
    pub(crate) sections: Vec<Section>,
    pub(crate) unlisted: bool,
    pub(crate) notes: Vec<String>,
}

/// A track of the show's tracklist.
#[derive(Debug, PartialEq)]
pub(crate) struct Section {
    pub(crate) artist: String,
    pub(crate) song: String,
    /// Seconds into the show, when it's known.
    pub(crate) start: Option<u32>,
}

/// `set`'s show: its title, else where and when it was played ("Warehouse, 2026-10-03"), the
/// comma-separated `tags` (each once), and a section for each track of `tracklist`. A track's time
/// is kept when it's after the time before it, so the sections are in order.
pub(crate) fn of(set: &PublishedSet, tracklist: &[TracklistEntry], tags: &str, unlisted: bool) -> Show {
    let mut notes = Vec::new();
    let place = given(&set.venue).or_else(|| given(&set.city)).unwrap_or("DJ set");
    let name = given(&set.title).map_or_else(|| format!("{place}, {}", set.date), String::from);
    let at: Vec<&str> = [&set.venue, &set.city, &set.country].into_iter().filter_map(given).collect();
    let mut description = match at.join(", ") {
        at if at.is_empty() => format!("Played on {}", set.date),
        at => format!("Played at {at} on {}", set.date),
    };
    if description.chars().count() > MAX_DESCRIPTION {
        description = description.chars().take(MAX_DESCRIPTION).collect();
        notes.push(format!("the description is cut to Mixcloud's {MAX_DESCRIPTION} characters"));
    }
    let mut kept: Vec<String> = Vec::new();
    for tag in tags.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if !kept.iter().any(|k| k.eq_ignore_ascii_case(tag)) {
            kept.push(tag.into());
        }
    }
    if kept.len() > MAX_TAGS {
        let left = kept.split_off(MAX_TAGS);
        notes.push(format!("Mixcloud takes {MAX_TAGS} tags: left out {}", left.join(", ")));
    }
    let named = |s: &Option<String>| given(s).unwrap_or("ID").to_string();
    let (mut sections, mut last, mut untimed) = (Vec::with_capacity(tracklist.len()), None, 0);
    for entry in tracklist {
        let start = entry.start_seconds.filter(|&s| last.is_none_or(|l| s > l));
        if start.is_none() && entry.start_seconds.is_some() {
            untimed += 1;
        }
        last = start.or(last);
        sections.push(Section { artist: named(&entry.artist), song: named(&entry.title), start });
    }
    if untimed > 0 {
        notes.push(format!("left the time off {untimed} of the tracks, each no later than a time before it"));
    }
    Show { name, description, tags: kept, sections, unlisted, notes }
}

/// `s` trimmed, when there's anything left of it.
fn given(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

impl Show {
    /// Its fields as the upload form has them, in this order: `name`, `description`,
    /// `tags-N-tag`, `sections-N-artist`, `sections-N-song` and `sections-N-start_time` (when it's
    /// known), and `unlisted` when it is, each `N` counting from 0.
    pub(crate) fn fields(&self) -> Vec<FormPart> {
        let field = |name: String, value: String| FormPart { name, value: Some(value), ..FormPart::default() };
        let mut fields =
            vec![field("name".into(), self.name.clone()), field("description".into(), self.description.clone())];
        fields.extend(self.tags.iter().enumerate().map(|(n, t)| field(format!("tags-{n}-tag"), t.clone())));
        for (n, s) in self.sections.iter().enumerate() {
            fields.push(field(format!("sections-{n}-artist"), s.artist.clone()));
            fields.push(field(format!("sections-{n}-song"), s.song.clone()));
            fields.extend(s.start.map(|at| field(format!("sections-{n}-start_time"), at.to_string())));
        }
        if self.unlisted {
            fields.push(field("unlisted".into(), "1".into()));
        }
        fields
    }
}
