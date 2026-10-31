//! Human-readable rendering of results.

use indexmap::IndexMap;

use console::style;
use locallibrary::api::{DriveSettingsView, ListedDrive, StoreInfo, TrackDetails, TracksPage};
use locallibrary::compare::{Comparison, Favors, Relation, Suggestion};
use locallibrary::convert::{ConvertPlan, ConvertReport, Target};
use locallibrary::device::DeviceInfo;
use locallibrary::format::{Compatibility, DiskView, FormatPlan, FormatReport, Scheme, Verdict, Volume};
use locallibrary::harmony::Camelot;
use locallibrary::local_time;
use locallibrary::merge::{MergePlan, MergeReport};
use locallibrary::settings::{self, ApplyReport, SettingsProfile};
use locallibrary::store::SnapshotInfo;
use locallibrary::sync::{CopyReason, PullReport, SyncPlan, SyncReport};
use locallibrary::{Library, Track};

use crate::each_drive::DriveRead;
use crate::output::{self, bytes, dim, duration, heading, ok, warn};

/// The drives, under the library each is a clone of, those of none last.
pub fn drives(list: &[ListedDrive]) {
    if list.is_empty() {
        say!("No rekordbox drives mounted. {}", dim("(--all lists every volume)"));
        return;
    }
    let mut groups: IndexMap<(bool, String), (&str, Vec<&DeviceInfo>)> = IndexMap::new();
    for ListedDrive { drive: d, library } in list {
        let library = library.as_deref();
        let key = (library.is_none(), library.unwrap_or_default().to_lowercase());
        groups.entry(key).or_insert((library.unwrap_or("No library"), Vec::new())).1.push(d);
    }
    groups.sort_unstable_keys();
    for (library, drives) in groups.values() {
        heading(library);
        for d in drives {
            let name = d.name.clone().unwrap_or_else(|| d.volume_name.clone());
            let lib = match &d.summary {
                Some(s) => format!("{} tracks, {} playlists", s.tracks, s.playlists),
                None if d.has_export => "unreadable export".into(),
                None => "no export".into(),
            };
            let space = match (d.free_space, d.total_space) {
                (Some(f), Some(t)) => format!("{} free of {}", bytes(f), bytes(t)),
                _ => String::new(),
            };
            say!("  {name}  {}  {lib}  {}", d.root.display(), dim(space));
            if let Some(p) = &d.pending {
                warn(format!("  {name}: {}", interrupted(p)));
            }
        }
    }
}

/// What an interrupted operation was, and what finishes it.
fn interrupted(p: &locallibrary::device::PendingOp) -> String {
    format!("{}: {}", p.describe(), p.remedy())
}

/// Where a mounted drive stands against the store, in a few words.
pub fn drive_state(s: &locallibrary::api::DriveStatus) -> String {
    match (s.relation, &s.error) {
        (_, Some(e)) => style(format!("error: {e}")).red().to_string(),
        (None, _) if s.library.is_none() => dim("not in the store (locallibrary pull)").to_string(),
        (None, _) => dim("library has no snapshots").to_string(),
        (Some(Relation::Identical), _) => style("up to date").green().to_string(),
        (Some(Relation::RightNewer), _) => match s.back_from_set {
            Some(0) => style("back from a set (pull saves its changes)").yellow().to_string(),
            Some(n) => style(format!("back from a set: {} (pull saves them)", output::plural(n, "new session")))
                .yellow()
                .to_string(),
            None => style("has changes not in the store (locallibrary pull)").yellow().to_string(),
        },
        (Some(Relation::LeftNewer), _) => style("behind the store (locallibrary library push)").yellow().to_string(),
        (Some(Relation::Diverged), _) => style("diverged from the store").red().to_string(),
        (Some(other), _) => format!("{other:?}").to_lowercase(),
    }
}

pub fn info(d: &DeviceInfo) {
    heading(&d.name.clone().unwrap_or_else(|| d.volume_name.clone()));
    say!("  mount       {}", d.root.display());
    match d.id {
        Some(id) => say!("  id          {id}"),
        None => say!("  id          {}", dim("not adopted (locallibrary drive adopt)")),
    }
    if let (Some(f), Some(t)) = (d.free_space, d.total_space) {
        say!("  space       {} free of {}", bytes(f), bytes(t));
    }
    if let Some(s) = &d.summary {
        say!(
            "  library     {} tracks, {} playlists in {} folders, {} artists, {}",
            s.tracks,
            s.playlists,
            s.folders,
            s.artists,
            bytes(s.audio_bytes)
        );
        say!("  playtime    {}", duration(s.total_duration_secs));
        if s.tracks_without_artwork > 0 {
            say!("  artwork     {} track(s) without artwork", s.tracks_without_artwork);
        }
    } else if !d.has_export {
        say!("  library     {}", dim("none"));
    }
    if let Some(at) = d.exported_at {
        say!("  exported    {}", local_time(at, "%Y-%m-%d %H:%M"));
    }
    if let Some(h) = &d.pdb {
        say!("  export.pdb  save #{}", h.sequence);
    }
    match (d.has_device_library, d.has_library_plus) {
        (true, true) => say!("  also        OneLibrary (exportLibrary.db), for CDJ-3000X, OPUS-QUAD, XDJ-AZ..."),
        (false, true) => say!(
            "  format      OneLibrary only (exportLibrary.db): CDJ-3000X, OPUS-QUAD, XDJ-AZ and newer players only"
        ),
        _ => {}
    }
    if let Some(drift) = &d.drift {
        drift_lines(drift, &d.root.display().to_string());
    }
    for s in d.rekordbox_sync.iter().filter(|s| s.automatic) {
        let db = if s.library == "onelibrary" { "OneLibrary" } else { "export.pdb" };
        let from = s.collection.map(|c| format!(" from collection {c}")).unwrap_or_default();
        say!("  rekordbox   syncs {} playlist(s) to {db} automatically{from}", s.playlists);
    }
    say!(
        "  settings    {}",
        if d.settings_files.is_empty() { "none".to_string() } else { d.settings_files.join(", ") }
    );
    if let Some(last) = &d.last_known {
        say!(
            "  last seen   {:?} at {} ({} recorded states)",
            last.op,
            local_time(last.at, "%Y-%m-%d %H:%M"),
            d.lineage_len
        );
    }
    if let Some(p) = &d.pending {
        warn(interrupted(p));
    }
}

fn drift_lines(drift: &locallibrary::device::LibraryDrift, drive: &str) {
    if let Some(e) = &drift.error {
        warn(format!("can't compare export.pdb with OneLibrary: {e}"));
        return;
    }
    if drift.is_empty() {
        say!("  databases   export.pdb and OneLibrary agree");
        return;
    }
    warn("export.pdb and OneLibrary differ, so players don't all show the same library:");
    let sample = |v: &[locallibrary::model::TrackRef]| {
        let names: Vec<&str> = v.iter().take(3).map(|t| t.name.as_str()).collect();
        let more = if v.len() > 3 { format!(", +{} more", v.len() - 3) } else { String::new() };
        format!("{}{more}", names.join(", "))
    };
    if !drift.only_in_device_library.is_empty() {
        say!(
            "    {} track(s) only in export.pdb, hidden from CDJ-3000X/OPUS-QUAD/XDJ-AZ: {}",
            drift.only_in_device_library.len(),
            sample(&drift.only_in_device_library)
        );
    }
    if !drift.only_in_onelibrary.is_empty() {
        say!(
            "    {} track(s) only in OneLibrary, hidden from older players: {}",
            drift.only_in_onelibrary.len(),
            sample(&drift.only_in_onelibrary)
        );
    }
    if !drift.changed_tracks.is_empty() {
        say!("    {} track(s) with different details", drift.changed_tracks.len());
    }
    if !drift.playlists.is_empty() {
        let shown: Vec<&str> = drift.playlists.iter().take(5).map(String::as_str).collect();
        say!("    {} playlist(s) differ: {}", drift.playlists.len(), shown.join(", "));
    }
    say!(
        "    {}",
        dim(format!(
            "give each what the other has: locallibrary convert \"{drive}\" --to onelibrary, then --to device-library"
        ))
    );
}

/// A column of a track listing (`tracks --columns`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum TrackColumn {
    Id,
    /// "Artist - Title"
    Name,
    Title,
    Artist,
    Album,
    Genre,
    Label,
    Key,
    Bpm,
    Rating,
    Color,
    Year,
    Plays,
    Added,
    Time,
    Bitrate,
    Path,
    Comment,
}

/// What `tracks` shows when no `--columns` are given.
pub const DEFAULT_COLUMNS: &[TrackColumn] =
    &[TrackColumn::Id, TrackColumn::Name, TrackColumn::Bpm, TrackColumn::Key, TrackColumn::Time];

/// How a listing is laid out (`--format`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum TableFormat {
    /// Aligned columns with a header, long values cut short, for reading
    Table,
    /// Tab-separated values with a header row, nothing cut short, for other programs
    Tsv,
    /// Comma-separated values with a header row (RFC 4180 quoting), for spreadsheets
    Csv,
}

impl TrackColumn {
    fn header(self) -> &'static str {
        match self {
            TrackColumn::Id => "id",
            TrackColumn::Name => "name",
            TrackColumn::Title => "title",
            TrackColumn::Artist => "artist",
            TrackColumn::Album => "album",
            TrackColumn::Genre => "genre",
            TrackColumn::Label => "label",
            TrackColumn::Key => "key",
            TrackColumn::Bpm => "bpm",
            TrackColumn::Rating => "rating",
            TrackColumn::Color => "color",
            TrackColumn::Year => "year",
            TrackColumn::Plays => "plays",
            TrackColumn::Added => "added",
            TrackColumn::Time => "time",
            TrackColumn::Bitrate => "bitrate",
            TrackColumn::Path => "path",
            TrackColumn::Comment => "comment",
        }
    }

    /// The value, as text.
    fn value(self, t: &Track) -> String {
        let n = |v: u64| if v == 0 { String::new() } else { v.to_string() };
        match self {
            TrackColumn::Id => t.id.to_string(),
            TrackColumn::Name => t.display_name(),
            TrackColumn::Title => t.title.clone(),
            TrackColumn::Artist => t.artist.clone(),
            TrackColumn::Album => t.album.clone(),
            TrackColumn::Genre => t.genre.clone(),
            TrackColumn::Label => t.label.clone(),
            TrackColumn::Key => t.key.clone(),
            TrackColumn::Bpm if t.bpm > 0.0 => format!("{:.2}", t.bpm),
            TrackColumn::Bpm => String::new(),
            TrackColumn::Rating => n(u64::from(t.rating)),
            TrackColumn::Color => t.color.map(|c| format!("{c:?}").to_lowercase()).unwrap_or_default(),
            TrackColumn::Year => n(u64::from(t.year)),
            TrackColumn::Plays => t.play_count.to_string(),
            TrackColumn::Added => t.date_added.clone(),
            TrackColumn::Time => format!("{}:{:02}", t.duration_secs / 60, t.duration_secs % 60),
            TrackColumn::Bitrate => n(u64::from(t.bitrate)),
            TrackColumn::Path => t.file_path.clone(),
            TrackColumn::Comment => t.comment.clone(),
        }
    }

    /// Numbers line up on the right.
    fn numeric(self) -> bool {
        matches!(
            self,
            TrackColumn::Id
                | TrackColumn::Bpm
                | TrackColumn::Rating
                | TrackColumn::Year
                | TrackColumn::Plays
                | TrackColumn::Time
                | TrackColumn::Bitrate
        )
    }

    /// The most columns a table gives it; longer values are cut short.
    fn max_width(self) -> usize {
        match self {
            TrackColumn::Name | TrackColumn::Path => 50,
            TrackColumn::Title | TrackColumn::Artist | TrackColumn::Album | TrackColumn::Comment => 32,
            _ => 20,
        }
    }
}

pub fn tracks(page: &TracksPage, columns: &[TrackColumn], format: TableFormat) {
    let list: Vec<&Track> = page.tracks.iter().collect();
    track_table(&list, columns, format, page.total, page.total_duration_secs);
}

/// Tracks, one a line, and how many (`total` of them matched; `secs` long).
pub fn track_rows(list: &[&Track], total: usize, secs: u64) {
    track_table(list, DEFAULT_COLUMNS, TableFormat::Table, total, secs);
}

/// Tracks as a table (a header, aligned columns, and how many: `total` matched, `secs` long), or
/// as TSV or CSV with a header row.
pub fn track_table(list: &[&Track], columns: &[TrackColumn], format: TableFormat, total: usize, secs: u64) {
    use unicode_width::UnicodeWidthStr;
    let columns = if columns.is_empty() { DEFAULT_COLUMNS } else { columns };
    let rows: Vec<Vec<String>> = list.iter().map(|t| columns.iter().map(|c| c.value(t)).collect()).collect();
    match format {
        TableFormat::Tsv | TableFormat::Csv => delimited(format, columns.iter().map(|c| c.header()), &rows),
        TableFormat::Table => {
            let widths: Vec<usize> = columns
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let longest = rows.iter().map(|r| r[i].width()).max().unwrap_or(0);
                    longest.max(c.header().len()).min(c.max_width())
                })
                .collect();
            let cell = |i: usize, v: &str| {
                let fitted = crate::output::fit(v, widths[i]);
                match columns[i].numeric() {
                    true => format!("{:>w$}", fitted.trim_end(), w = widths[i]),
                    false => fitted,
                }
            };
            let header: Vec<String> = columns.iter().enumerate().map(|(i, c)| cell(i, c.header())).collect();
            say!("{}", style(header.join("  ").trim_end()).dim().bold());
            for row in &rows {
                let line: Vec<String> = row.iter().enumerate().map(|(i, v)| cell(i, v)).collect();
                say!("{}", line.join("  ").trim_end());
            }
            let shown = list.len();
            let count = match shown == total {
                true => format!("{shown} track(s), {}", duration(secs)),
                false => format!("{shown} of {total} track(s)"),
            };
            say!("{}", dim(count));
        }
    }
}

/// Several drives' tracks as one TSV or CSV listing, the drive's name in a column of its own.
pub fn drive_tracks(reads: &[DriveRead<TracksPage>], columns: &[TrackColumn], format: TableFormat) {
    let columns = if columns.is_empty() { DEFAULT_COLUMNS } else { columns };
    let rows: Vec<Vec<String>> = (reads.iter())
        .filter_map(|r| Some((&r.drive, r.result.as_ref()?)))
        .flat_map(|(drive, page)| {
            page.tracks
                .iter()
                .map(move |t| std::iter::once(drive.clone()).chain(columns.iter().map(|c| c.value(t))).collect())
        })
        .collect();
    delimited(format, std::iter::once("drive").chain(columns.iter().map(|c| c.header())), &rows);
}

/// Rows as TSV (tabs and line breaks in a value become spaces) or CSV (quoted as RFC 4180 says),
/// under a header row.
fn delimited<'a>(format: TableFormat, headers: impl Iterator<Item = &'a str>, rows: &[Vec<String>]) {
    let (separator, cell): (&str, fn(&str) -> String) = match format {
        TableFormat::Csv => (",", |v| match v.contains([',', '"', '\n', '\r']) {
            true => format!("\"{}\"", v.replace('"', "\"\"")),
            false => v.to_string(),
        }),
        _ => ("\t", |v| v.replace(['\t', '\n', '\r'], " ")),
    };
    println!("{}", headers.collect::<Vec<_>>().join(separator));
    for row in rows {
        println!("{}", row.iter().map(|v| cell(v)).collect::<Vec<_>>().join(separator));
    }
}

pub fn track_details(d: &TrackDetails) {
    let t = &d.track;
    heading(&format!("#{}  {}", t.id, if t.display_name().is_empty() { "Untitled".into() } else { t.display_name() }));
    let problem = |s: String| say!("  {} {s}", style("!").yellow().bold());
    if !d.audio_present {
        problem(format!("the audio file isn't on the drive: {}", t.file_path));
    }
    for c in &d.copies {
        problem(format!("same recording as #{} at {}", c.id, c.file_path));
    }
    let kv = |k: &str, v: String| say!("  {k:<11} {v}");
    let some = |k: &str, v: &str| {
        if !v.trim().is_empty() {
            kv(k, v.trim().to_string());
        }
    };
    let mut about: Vec<String> =
        [&t.album, &t.remixer, &t.mix_name].into_iter().filter(|s| !s.is_empty()).cloned().collect();
    if t.year > 0 {
        about.push(t.year.to_string());
    }
    some("album", &about.join(" · "));
    some("genre", &t.genre);
    some("label", &t.label);
    let mut tempo = Vec::new();
    if t.bpm > 0.0 {
        tempo.push(format!("{:.2} BPM", t.bpm));
    }
    match Camelot::parse(&t.key) {
        Some(k) => tempo.push(format!("{} {}", k.code(), k.note())),
        None if !t.key.is_empty() => tempo.push(t.key.clone()),
        None => {}
    }
    tempo.push(format!("{}:{:02}", t.duration_secs / 60, t.duration_secs % 60));
    kv("tempo", tempo.join(" · "));
    if t.rating > 0 {
        let stars = t.rating.min(5) as usize;
        kv("rating", format!("{}{}", "★".repeat(stars), "☆".repeat(5 - stars)));
    }
    if let Some(c) = t.color {
        kv("colour", format!("{c:?}").to_lowercase());
    }
    let credits: Vec<String> = [("composer", &t.composer), ("original", &t.original_artist), ("ISRC", &t.isrc)]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{k} {v}"))
        .collect();
    some("credits", &credits.join(" · "));
    some("comment", &t.comment);
    let mut format = vec![t.file_path.rsplit('.').next().unwrap_or("").to_uppercase()];
    if t.bitrate > 0 {
        format.push(format!("{} kbps", t.bitrate));
    }
    if t.sample_rate > 0 {
        format.push(format!("{:.1} kHz", t.sample_rate as f64 / 1000.0));
    }
    if t.sample_depth > 0 {
        format.push(format!("{}-bit", t.sample_depth));
    }
    if t.file_size > 0 {
        format.push(bytes(t.file_size));
    }
    kv("format", format.join(" · "));
    kv("file", t.file_path.clone());
    let played = if t.play_count > 0 { format!("played {}×", t.play_count) } else { "never played".into() };
    kv("plays", if t.date_added.is_empty() { played } else { format!("{played} · added {}", t.date_added) });
    let list = |k: &str, v: &[String], none: &str| match v.split_first() {
        None => kv(k, dim(none).to_string()),
        Some((first, rest)) => {
            kv(k, first.clone());
            for v in rest {
                kv("", v.clone());
            }
        }
    };
    list("playlists", &d.playlists, "none (unfiled)");
    if !d.history.is_empty() {
        list("history", &d.history, "");
    }
    if !d.tags.is_empty() {
        kv("my tags", d.tags.join(", "));
    }
    let a = &d.analysis;
    if !a.analysed {
        kv("analysis", dim("none on the drive").to_string());
        return;
    }
    if a.beats > 0 {
        let tempo = match a.tempo_range {
            Some(r) if r.high - r.low > 0.05 => format!("{:.1}–{:.1} BPM", r.low, r.high),
            _ => "steady tempo".into(),
        };
        kv("beat grid", format!("{} beats · {} bars · {tempo}", a.beats, a.bars));
    }
    let mut cues = vec![format!("{} hot", a.hot_cues), format!("{} memory", a.memory_cues)];
    if a.loops > 0 {
        cues.push(format!("{} loop(s)", a.loops));
    }
    kv("cues", cues.join(" · "));
    if !a.phrases.is_empty() {
        let mood = a.mood.map(|m| format!("{m:?} mood: ").to_lowercase()).unwrap_or_default();
        // Wrapped, a phrase never split, so a long track's phrases line up under the label.
        let mut lines = vec![mood];
        for p in &a.phrases {
            let phrase = format!("{} {}", p.kind, p.bars);
            let last = lines.last_mut().expect("starts with one line");
            if last.is_empty() || last.ends_with(": ") {
                last.push_str(&phrase);
            } else if last.chars().count() + phrase.len() + 3 > 64 {
                lines.push(phrase);
            } else {
                last.push_str(&format!(" · {phrase}"));
            }
        }
        lines.last_mut().expect("starts with one line").push_str(&format!(" {}", dim("(bars)")));
        list("phrases", &lines, "");
    }
    if !a.waveform {
        kv("waveform", dim("no scrolling waveform (older analysis)").to_string());
    }
}

pub fn playlist_tree(lib: &Library) {
    fn walk(lib: &Library, parent: Option<u32>, depth: usize) {
        for p in lib.children(parent) {
            let indent = "  ".repeat(depth);
            if p.is_folder {
                say!("{indent}{}/", style(&p.name).bold());
                walk(lib, Some(p.id), depth + 1);
            } else {
                say!("{indent}{} {}", p.name, dim(format!("({})", p.track_ids.len())));
            }
        }
    }
    if lib.playlists.is_empty() {
        say!("no playlists");
    }
    walk(lib, None, 0);
}

fn relation_line(c: &Comparison) -> String {
    let (l, r) = (&c.left.label, &c.right.label);
    match c.relation {
        Relation::Identical => format!("{l} and {r} are identical"),
        Relation::LeftNewer => format!("{} is newer than {r}", style(l).bold()),
        Relation::RightNewer => format!("{} is newer than {l}", style(r).bold()),
        Relation::Diverged => format!("{l} and {r} have both changed since they were last in sync"),
        Relation::Unrelated => format!("{l} and {r} hold different libraries ({:.0}% overlap)", c.overlap * 100.0),
        Relation::Undetermined => format!("can't tell whether {l} or {r} is newer"),
    }
}

pub fn comparison_short(c: &Comparison) {
    say!("{} {}", relation_line(c), dim(format!("[{:?} confidence]", c.confidence).to_lowercase()));
}

pub fn comparison(c: &Comparison) {
    comparison_short(c);
    for side in [&c.left, &c.right] {
        let mut flags = Vec::new();
        if side.changed_since_last_sync {
            flags.push("changed since last sync");
        }
        if side.interrupted_operation {
            flags.push("interrupted operation");
        }
        say!(
            "  {:<16} {} tracks, {} playlists{}{}",
            side.label,
            side.tracks,
            side.playlists,
            side.exported_at.map(|t| format!(", exported {}", local_time(t, "%Y-%m-%d %H:%M"))).unwrap_or_default(),
            if flags.is_empty() { String::new() } else { format!(" ({})", flags.join(", ")) }
        );
    }
    if !c.evidence.is_empty() {
        say!("{}", style("Evidence").bold());
        for e in &c.evidence {
            let favors = match e.favors {
                Favors::Left => format!(" (favours {})", c.left.label),
                Favors::Right => format!(" (favours {})", c.right.label),
                Favors::Neither => String::new(),
            };
            say!("  • {}{}", e.detail, dim(favors));
        }
    }
    let d = &c.diff;
    if !d.is_empty() {
        say!("{}", style("Differences (older → newer)").bold());
        let list = |label: &str, items: Vec<String>| {
            if !items.is_empty() {
                say!("  {label} ({}):", items.len());
                for i in items.iter().take(crate::output::cap(10)) {
                    say!("    {i}");
                }
                if items.len() > crate::output::cap(10) {
                    say!("    {}", dim(format!("... and {} more", items.len() - 10)));
                }
            }
        };
        list("+ tracks", d.added_tracks.iter().map(|t| t.name.clone()).collect());
        list("- tracks", d.removed_tracks.iter().map(|t| t.name.clone()).collect());
        list(
            "~ tracks",
            d.changed_tracks
                .iter()
                .map(|t| {
                    format!(
                        "{} ({})",
                        t.track.name,
                        t.fields.iter().map(|f| f.field.as_str()).collect::<Vec<_>>().join(", ")
                    )
                })
                .collect(),
        );
        list("+ playlists", d.added_playlists.clone());
        list("- playlists", d.removed_playlists.clone());
        list("~ playlists", d.changed_playlists.clone());
    }
    let hint = match c.suggestion {
        Suggestion::Nothing => None,
        Suggestion::UpdateRight | Suggestion::UpdateLeft => {
            Some("run `locallibrary sync` with the same two drives to update the older one".to_string())
        }
        Suggestion::ChooseOrMerge => Some(
            "use `locallibrary sync --from <source> <target>` to pick a winner, or `locallibrary merge` to combine"
                .to_string(),
        ),
        Suggestion::Merge => Some("use `locallibrary merge <source> <target>` to combine them".to_string()),
        _ => None,
    };
    if let Some(h) = hint {
        crate::output::hint(h);
    }
}

pub fn sync_plan(p: &SyncPlan) {
    let new = p.copies.iter().filter(|c| c.reason == CopyReason::New).count();
    let changed = p.copies.len() - new;
    say!("{} → {} {}", style(&p.source).bold(), style(&p.target_name).bold(), dim(p.target.display()));
    say!(
        "  copy   {} files ({} new, {} changed), {}  [{} audio, {} analysis, {} artwork, {} database]",
        p.copies.len(),
        new,
        changed,
        bytes(p.copy_bytes),
        p.copies_by_class.audio,
        p.copies_by_class.analysis,
        p.copies_by_class.artwork,
        p.copies_by_class.database
    );
    if !p.deletes.is_empty() {
        say!("  delete {} files, {}  [{} audio]", p.deletes.len(), bytes(p.delete_bytes), p.deletes_by_class.audio);
    }
    say!("  keep   {} unchanged files", p.unchanged);
    if let Some(free) = p.free_space {
        say!("  space  {} free on target", bytes(free));
    }
    for w in &p.warnings {
        warn(w);
    }
}

pub fn sync_report(r: &SyncReport) {
    ok(format!("copied {} files ({}), deleted {}", r.copied, bytes(r.copied_bytes), r.deleted));
}

pub fn merge_plan(p: &MergePlan) {
    say!("{} → {}", style(&p.source).bold(), p.target.display());
    say!("  {} new track(s), {} already present, {}", p.new_tracks.len(), p.matched_tracks, bytes(p.copy_bytes));
    for t in p.new_tracks.iter().take(15) {
        let note = if t.renamed { format!("  (as {})", t.audio) } else { String::new() };
        say!("    + {}{}", t.name, dim(note));
    }
    if p.new_tracks.len() > 15 {
        say!("    {}", dim(format!("... and {} more", p.new_tracks.len() - 15)));
    }
    for pl in &p.playlists {
        let verb = if pl.create { "create" } else { "extend" };
        say!("  {verb} playlist {} (+{} tracks)", pl.path, pl.tracks_added);
    }
    for h in &p.history {
        say!("  add history {h}");
    }
    if p.tagged_tracks > 0 {
        say!("  My Tags for {} track(s)", p.tagged_tracks);
    }
    for c in &p.new_tags {
        say!("  create My Tag {} in {}", c.path, c.database.file_name());
    }
    for w in &p.warnings {
        warn(w);
    }
}

pub fn merge_report(r: &MergeReport, target: &str) {
    ok(format!(
        "added {} track(s), {} playlist(s), {} playlist entries, {} artwork",
        r.tracks_added, r.playlists_created, r.playlist_entries_added, r.artwork_added
    ));
    if r.history_playlists_added > 0 || r.hot_cue_bank_lists_added > 0 {
        say!(
            "  and {} history playlist(s), {} hot cue bank list(s)",
            r.history_playlists_added,
            r.hot_cue_bank_lists_added
        );
    }
    if r.tagged_tracks > 0 {
        say!("  and My Tags on {} track(s)", r.tagged_tracks);
    }
    if let Some(b) = &r.backup {
        crate::output::hint(format!("undo with: locallibrary undo {target:?} (backup {b})"));
    }
}

pub fn convert_plan(p: &ConvertPlan) {
    let (to, from) = match p.to {
        Target::OneLibrary => ("OneLibrary (exportLibrary.db)", "export.pdb"),
        Target::DeviceLibrary => ("export.pdb", "OneLibrary (exportLibrary.db)"),
    };
    let what = match (p.adds, p.replaces) {
        (true, _) => format!("add to {to} what {from} has that it lacks"),
        (_, true) => format!("rebuild {to} from {from}"),
        _ => format!("create {to} from {from}"),
    };
    say!("{}: {what}", style(&p.drive).bold());
    let playlists = match p.adds {
        true => format!("{} playlist(s) new or gaining entries, {} new folder(s)", p.playlists, p.folders),
        false => format!("{} playlist(s) in {} folder(s)", p.playlists, p.folders),
    };
    say!(
        "  {} track(s), {playlists}, {} history playlist(s), {} artwork image(s)",
        p.tracks,
        p.history_playlists,
        p.artwork
    );
    if p.tagged_tracks > 0 {
        say!("  My Tags on {} track(s)", p.tagged_tracks);
    }
    for w in &p.warnings {
        warn(w);
    }
}

pub fn convert_report(r: &ConvertReport, drive: &str) {
    ok(format!(
        "converted {} track(s), {} playlist(s), {} history playlist(s), {} artwork image(s)",
        r.tracks, r.playlists, r.history_playlists, r.artwork
    ));
    if let Some(b) = &r.backup {
        crate::output::hint(format!("undo with: locallibrary undo {drive:?} (backup {b})"));
    }
}

pub fn pull(r: &PullReport) {
    match (&r.snapshot, r.up_to_date) {
        (Some(s), true) => ok(format!("{} is up to date (snapshot {}{})", r.library, s.short_id, labels(&s.labels))),
        (Some(s), false) => ok(format!(
            "snapshot {}{} of {}: {} tracks; stored {} new files ({}), {} already in the store",
            s.short_id,
            labels(&s.labels),
            r.library,
            s.summary.tracks,
            r.imported_files,
            bytes(r.imported_bytes),
            r.reused_files
        )),
        (None, _) => ok("nothing to do"),
    }
}

/// ` [before NYE] [Friday]`, or nothing.
pub fn labels(labels: &[String]) -> String {
    labels.iter().map(|l| format!(" {}", style(format!("[{l}]")).cyan())).collect()
}

pub fn log(all: &IndexMap<String, Vec<SnapshotInfo>>) {
    if all.is_empty() {
        say!("The store is empty. Snapshot a drive with `locallibrary pull <drive>`.");
    }
    for (lib, snaps) in all {
        heading(lib);
        for s in snaps {
            say!(
                "  {}{}{}  {}  {:>5} tracks  {:>9}  {}{}",
                style(&s.short_id).yellow(),
                labels(&s.labels),
                if s.beside { format!(" {}", dim("beside")) } else { String::new() },
                local_time(s.created, "%Y-%m-%d %H:%M"),
                s.summary.tracks,
                bytes(s.bytes),
                s.source.as_ref().map(|src| format!("from {}", src.drive_name)).unwrap_or_default(),
                s.message.as_ref().map(|m| format!("  {}", dim(m))).unwrap_or_default()
            );
        }
    }
}

pub fn store_info(s: &StoreInfo) {
    heading("Store");
    say!("  location   {}", s.root.display());
    if let Some(c) = &s.copy_of {
        let when = |t: chrono::DateTime<chrono::Utc>| local_time(t, "%Y-%m-%d %H:%M").to_string();
        let as_of = c.last_complete.map(|t| format!("as of {}", when(t)));
        say!(
            "  copy of    {} ({})",
            c.from.display(),
            as_of.unwrap_or_else(|| "no copy has finished whole yet".into())
        );
        if c.finished.is_none() {
            warn(format!("the latest copy, started {}, didn't finish", when(c.started)));
        }
        for p in &c.problems {
            warn(p);
        }
    }
    say!("  data       {} in {} files", bytes(s.stats.object_bytes), s.stats.objects);
    if s.embed_backup_bytes > 0 {
        say!("  embeds     {} kept for `art unembed`", bytes(s.embed_backup_bytes));
    }
    say!("  snapshots  {}", s.stats.snapshots);
    say!("  drives     {} known", s.stats.drives);
    for l in &s.libraries {
        say!(
            "  library    {} {}",
            style(&l.name).bold(),
            dim(l.head.map(|h| format!("@ {}", h.short())).unwrap_or_else(|| "(empty)".into()))
        );
    }
}

pub fn settings_schema() {
    let mut cat = None;
    let mut defs: Vec<&settings::SettingDef> = settings::SCHEMA.iter().collect();
    defs.sort_by_key(|d| d.category as u8);
    for d in defs {
        if cat != Some(d.category) {
            cat = Some(d.category);
            heading(&format!("{:?}", d.category));
        }
        let choices: Vec<String> = d
            .choices
            .iter()
            .map(|c| if c.id == d.default { style(c.id).underlined().to_string() } else { c.id.to_string() })
            .collect();
        say!("  {:<28} {}", d.key, choices.join(" | "));
    }
    crate::output::hint("Defaults underlined. Set with `locallibrary settings set KEY=VALUE`.");
}

pub fn profile(p: &SettingsProfile) {
    if p.base.is_empty() && p.drives.is_empty() {
        say!("The profile is empty: drives keep whatever settings they have.");
        crate::output::hint(
            "Start from rekordbox's settings with `locallibrary settings import`, or set values with `locallibrary settings set`.",
        );
        return;
    }
    heading("All drives");
    for (k, v) in &p.base {
        say!("  {k:<28} {v}");
    }
    for (id, o) in &p.drives {
        heading(&format!("{} {}", if o.name.is_empty() { "drive" } else { &o.name }, dim(id)));
        for (k, v) in &o.values {
            say!("  {k:<28} {v}");
        }
    }
}

pub fn drive_settings(v: &DriveSettingsView) {
    heading(&v.drive);
    let drift: IndexMap<&str, &settings::Drift> = v.drift.iter().map(|d| (d.key.as_str(), d)).collect();
    for def in settings::SCHEMA {
        let current = v.current.values.get(def.key).map(|v| v.to_string()).unwrap_or_else(|| "-".into());
        match drift.get(def.key) {
            Some(d) => say!(
                "  {:<28} {} → {}{}",
                def.key,
                style(current).red(),
                style(&d.expected).green(),
                if d.overridden { dim(" (drive override)").to_string() } else { String::new() }
            ),
            None => say!("  {:<28} {}", def.key, current),
        }
    }
    for f in &v.current.missing_files {
        say!("  {}", dim(format!("{} missing", f.file_name())));
    }
    for p in &v.current.problems {
        warn(p);
    }
    if !v.current.undecoded.is_empty() {
        let bytes: Vec<String> = v
            .current
            .undecoded
            .iter()
            .map(|b| format!("{}[{}]={:#04x}", b.file.file_name(), b.offset, b.value))
            .collect();
        say!(
            "  {}",
            dim(format!(
                "{} setting byte(s) from a newer rekordbox aren't known yet (kept as they are): {}",
                bytes.len(),
                bytes.join(" ")
            ))
        );
    }
    if v.drift.is_empty() {
        ok("matches the profile");
    } else {
        crate::output::hint(format!("{} setting(s) differ; `locallibrary settings apply` fixes them", v.drift.len()));
    }
}

pub fn settings_apply(reports: &IndexMap<String, ApplyReport>) {
    for (drive, r) in reports {
        if r.changes.is_empty() && r.files_written.is_empty() {
            ok(format!("{drive}: already matches"));
            continue;
        }
        let (verb, created) = if r.dry_run { ("would update", "would create") } else { ("updated", "created") };
        let updates: Vec<_> = r.changes.iter().filter(|c| c.from.is_some()).collect();
        ok(format!("{drive}: {verb} {} setting(s)", updates.len()));
        for c in updates {
            say!("    {:<28} {} → {}", c.key, c.from.as_ref().unwrap(), c.to);
        }
        if !r.files_created.is_empty() {
            let names: Vec<&str> = r.files_created.iter().map(|f| f.file_name()).collect();
            say!("    {created} {} from the profile", names.join(", "));
        }
    }
}

/// What importing a settings file does with each of its values that isn't as it is here: `+`
/// added, `~` replaced, `=` kept as it is here.
pub fn settings_file_changes(changes: &[settings::transfer::Change]) {
    use settings::transfer::Action;
    let shown = |v: &serde_json::Value| v.as_str().map_or_else(|| v.to_string(), str::to_string);
    for c in changes {
        let path = c.path.join(".");
        let current = c.current.as_ref().map_or_else(|| "(not set)".into(), shown);
        match c.action {
            Action::Add if c.current.is_some() => say!("  {} {path}: adds {}", style("+").green(), shown(&c.imported)),
            Action::Add => say!("  {} {path} = {}", style("+").green(), shown(&c.imported)),
            Action::Replace => say!("  {} {path}: {current} → {}", style("~").yellow(), shown(&c.imported)),
            Action::Keep => {
                say!("  {} {path}: {current} {}", dim("="), dim(format!("(the file has {})", shown(&c.imported))))
            }
        }
    }
}

fn verdict_mark(c: &Compatibility) -> console::StyledObject<String> {
    match c.verdict {
        Verdict::AllPlayers => style(format!("✓ {}", c.summary)).green(),
        Verdict::SomePlayers | Verdict::Unknown => style(format!("~ {}", c.summary)).yellow(),
        Verdict::NoPlayers => style(format!("✗ {}", c.summary)).red(),
    }
}

fn compatibility(prefix: &str, c: &Compatibility) {
    say!("{prefix}{}", verdict_mark(c));
    let indent = " ".repeat(prefix.chars().count() + 2);
    if !c.unsupported.is_empty() {
        say!("{indent}{}", dim(format!("not: {}", c.unsupported.join(", "))));
    }
    for n in &c.notes {
        say!("{indent}{}", dim(n));
    }
}

fn volume_line(v: &Volume) -> String {
    let fs = v.filesystem_name.clone().unwrap_or_else(|| "no file system".into());
    let cluster = v.cluster_size.map(|c| format!(", {} KB clusters", c / 1024)).unwrap_or_default();
    let mount = v.mount.as_ref().map(|m| m.display().to_string()).unwrap_or_else(|| "not mounted".into());
    format!("{:<14} {:<22} {fs}{cluster}, {}", v.label.clone().unwrap_or_else(|| v.id.clone()), mount, bytes(v.size))
}

fn scheme_name(s: Option<Scheme>) -> &'static str {
    match s {
        Some(Scheme::Mbr) => "MBR",
        Some(Scheme::Gpt) => "GPT",
        Some(Scheme::Apm) => "Apple partition map",
        Some(Scheme::Other) => "other partition table",
        None => "no partition table",
    }
}

pub fn disks(list: &[DiskView]) {
    if list.is_empty() {
        say!("No external disks found.");
        return;
    }
    for d in list {
        let bus = d.disk.bus.clone().map(|b| format!("{b}, ")).unwrap_or_default();
        say!("{}  {}", style(d.disk.describe()).bold(), dim(format!("{bus}{}", scheme_name(d.disk.scheme))));
        for v in &d.disk.volumes {
            say!("  {}", volume_line(v));
        }
        compatibility("  ", &d.compatibility);
        if let Some(p) = &d.problem {
            say!("  {}", dim(format!("can't be formatted: {p}")));
        }
    }
}

pub fn format_plan(p: &FormatPlan) {
    heading(&format!("Format {}", p.disk.describe()));
    if p.erases.is_empty() {
        say!("  erases   {}", dim("nothing (no volumes)"));
    }
    for e in &p.erases {
        say!("  erases   {}", volume_line(&e.volume));
        if let Some(name) = &e.drive_name {
            let role = e.role.map(|r| format!(", {} drive", r.name())).unwrap_or_default();
            let retired = if e.retired { ", retired" } else { "" };
            say!("           locallibrary drive {name:?}{}", style(format!("{role}{retired}")).yellow());
        }
        if let Some(n) = e.tracks {
            let backed = match e.in_store {
                Some(true) => style("in the store").green().to_string(),
                Some(false) => style("not in the store").yellow().to_string(),
                None => dim("store not checked").to_string(),
            };
            say!("           rekordbox library, {n} tracks ({backed})");
        }
    }
    say!(
        "  creates  {} {:?}, {} KB clusters, {}, one {} partition",
        p.filesystem.name(),
        p.label,
        p.cluster_size / 1024,
        bytes(p.partition_size),
        scheme_name(Some(p.scheme))
    );
    compatibility("  players  ", &p.compatibility);
    if let Some(n) = &p.keeps_name {
        say!("  keeps    its locallibrary name {n:?}");
    }
    for s in &p.steps {
        say!("  runs     {}", dim(s));
    }
    for w in &p.warnings {
        warn(w);
    }
}

pub fn format_report(r: &FormatReport) {
    let cluster = r.cluster_size.map(|c| format!(", {} KB clusters", c / 1024)).unwrap_or_default();
    ok(format!("formatted {} as {} {:?}{cluster}", r.disk, r.filesystem.name(), r.label));
    match &r.mount {
        Some(m) => {
            say!("  mounted at {}", m.display());
            let name = r.drive_name.clone().unwrap_or_else(|| m.display().to_string());
            crate::output::hint(format!(
                "Next: `locallibrary provision {name:?} --like <your library>` makes it a copy of your library, \
                     with the settings profile (or export to it from rekordbox)"
            ));
            crate::output::hint("Players show the Device Name set in rekordbox, not the volume label.");
        }
        None => crate::output::hint("Unplug and reconnect the drive to mount it."),
    }
}

#[cfg(test)]
mod tests;
