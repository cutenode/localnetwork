//! `locallibrary profile`: what a DJ's sets, collection and settings say about how they play,
//! and the part of it they choose to show on a public profile.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use console::style;
use locallibrary::api::{self, ProfileRequest, ProfileView};
use locallibrary::local_time;
use locallibrary::model::normalize;
use locallibrary::profile::{
    self, Basis, DEFAULT_MIN_SET_TRACKS, DEFAULT_TOP, PlayedTrack, Precision, PrefsUpdate, ProfilePrefs, ProfileReport,
    RankedSection, Section,
};
use locallibrary::progress::Ctx;

use crate::open_store;
use crate::output::{Output, dim, fit, heading, ok, plural, thousands, warn};

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ProfileArgs {
    #[command(subcommand)]
    command: Option<ProfileCommand>,
    #[command(flatten)]
    source: SourceArgs,
    #[command(flatten)]
    view: ViewArgs,
}

#[derive(Args, Clone)]
pub struct SourceArgs {
    /// Drives to read [default: the mounted rekordbox drives the store knows, or all of them]
    drives: Vec<String>,
    /// Leave out the store (your sets, the history lists it archived, and its libraries)
    #[arg(long)]
    no_store: bool,
    /// Only this store library (repeatable) [default: the named drives' libraries, or all]
    #[arg(long = "library", short = 'l', value_name = "LIBRARY")]
    libraries: Vec<String>,
    /// Only sets played from this date: 2026, 2026-09 or 2026-09-30
    #[arg(long, value_name = "DATE")]
    since: Option<String>,
    /// Only sets played up to this date
    #[arg(long, value_name = "DATE")]
    until: Option<String>,
    /// Without declared sets: sessions with fewer tracks don't count
    #[arg(long, value_name = "N", default_value_t = DEFAULT_MIN_SET_TRACKS)]
    min_tracks: usize,
    /// Entries in ranked lists
    #[arg(long, short = 'n', value_name = "N", default_value_t = DEFAULT_TOP)]
    top: usize,
    /// Skip analysis files: no phrase mood or cue statistics, but much faster
    #[arg(long)]
    no_analysis: bool,
}

impl SourceArgs {
    fn request(&self) -> ProfileRequest {
        ProfileRequest {
            drives: self.drives.clone(),
            no_store: self.no_store,
            libraries: self.libraries.clone(),
            since: self.since.clone(),
            until: self.until.clone(),
            min_set_tracks: Some(self.min_tracks),
            top: Some(self.top),
            no_analysis: self.no_analysis,
        }
    }
}

#[derive(Args, Clone)]
struct ViewArgs {
    /// Only these sections (repeat, or separate with commas)
    #[arg(long = "section", short = 's', value_name = "SECTION", value_delimiter = ',', value_parser = parse_section)]
    sections: Vec<Section>,
    /// List every set (or, without declared sets, every session)
    #[arg(long)]
    sets: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum PrecisionArg {
    /// Counts as round numbers, shares to 5%, BPM to whole numbers
    Rounded,
    Exact,
}

impl From<PrecisionArg> for Precision {
    fn from(p: PrecisionArg) -> Self {
        match p {
            PrecisionArg::Rounded => Precision::Rounded,
            PrecisionArg::Exact => Precision::Exact,
        }
    }
}

#[derive(Subcommand)]
enum ProfileCommand {
    /// The full report (what `locallibrary profile` shows)
    Show {
        #[command(flatten)]
        source: SourceArgs,
        #[command(flatten)]
        view: ViewArgs,
    },
    /// What a public profile would show, as JSON
    #[command(alias = "doc")]
    Document {
        #[command(flatten)]
        source: SourceArgs,
        /// Write it to a file
        #[arg(long, short, value_name = "FILE")]
        output: Option<PathBuf>,
        /// With -o: replace the file if it's there already
        #[arg(long, requires = "output")]
        replace: bool,
    },
    /// Publish the document to your Atmosphere account, where anyone can read it (alpha)
    #[cfg(feature = "account")]
    Publish {
        #[command(flatten)]
        source: SourceArgs,
        /// Show what would be published without publishing
        #[arg(long)]
        dry_run: bool,
        #[command(flatten)]
        browser: crate::browser::BrowserArgs,
    },
    /// Every section: whether the document includes it, and what it reveals
    Sections,
    /// Put sections in the document (e.g. `include signature artists`)
    Include {
        /// Sections (`profile sections` lists them), repeated or separated with commas
        #[arg(required = true, value_delimiter = ',', value_parser = parse_section)]
        sections: Vec<Section>,
    },
    /// Leave sections out of the document
    Exclude {
        /// Sections (`profile sections` lists them), repeated or separated with commas
        #[arg(required = true, value_delimiter = ',', value_parser = parse_section)]
        sections: Vec<Section>,
    },
    /// Go back to the default sections
    DefaultSections,
    /// Name a track first among your signature tracks
    Pin {
        /// Part of "Artist - Title", matched against the tracks you've played
        query: String,
        /// Pin the text as given, without looking it up
        #[arg(long)]
        exact: bool,
        #[command(flatten)]
        source: SourceArgs,
    },
    /// Stop pinning a track
    Unpin { query: String },
    /// Never name a track (an unreleased promo, an edit) or, with --artist, an artist. Hidden
    /// tracks still count in the numbers.
    Hide {
        /// Part of "Artist - Title" (matched against the tracks you've played), or an artist
        query: String,
        /// Hide an artist wherever they're credited
        #[arg(long)]
        artist: bool,
        /// Hide the text as given, without looking it up
        #[arg(long)]
        exact: bool,
        #[command(flatten)]
        source: SourceArgs,
    },
    /// Name a hidden track (or, with --artist, artist) again
    Unhide {
        /// The track or artist as hidden
        query: String,
        /// It's an artist
        #[arg(long)]
        artist: bool,
    },
    /// Spell a genre your way: `alias dnb "Drum & Bass"` (without a genre: forget the alias)
    Alias {
        /// The genre text as tagged
        from: String,
        /// The genre to show
        to: Option<String>,
    },
    /// Your own one-line description (without text: back to the generated one)
    Headline { text: Option<String> },
    /// Rounded (the default) or exact numbers in the document
    Precision { precision: PrecisionArg },
    /// Your choices: sections, pinned and hidden tracks, genre aliases
    Prefs,
    /// Forget every choice
    Reset,
}

fn parse_section(s: &str) -> Result<Section, String> {
    Section::parse(s).ok_or_else(|| {
        let names: Vec<&str> = Section::ALL.iter().map(|s| s.name()).collect();
        format!("no section {s:?} (sections: {})", names.join(", "))
    })
}

pub fn run(args: ProfileArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let command = args.command.unwrap_or(ProfileCommand::Show { source: args.source, view: args.view });
    match command {
        ProfileCommand::Show { source, view } => {
            let store = crate::diff_cmd::existing_store(store_path)?;
            let v = api::profile(store.as_ref(), &source.request(), ctx)?;
            out.emit_paged(&v, |v| render(v, &view));
        }
        ProfileCommand::Document { source, output, replace } => {
            let store = crate::diff_cmd::existing_store(store_path)?;
            let doc = api::profile_document(store.as_ref(), &source.request(), ctx)?;
            match output {
                Some(path) => {
                    let mut text = serde_json::to_string_pretty(&doc)?;
                    text.push('\n');
                    locallibrary::fsutil::write_output(&path, text.as_bytes(), replace)?;
                    out.done(&serde_json::json!({ "output": path, "document": doc }), |_| {
                        ok(format!("wrote {}", path.display()));
                    });
                }
                None => out.emit(&doc, |d| println!("{}", serde_json::to_string_pretty(d).expect("serialisable"))),
            }
        }
        #[cfg(feature = "account")]
        ProfileCommand::Publish { source, dry_run, browser } => {
            return crate::publish_cmd::profile_publish(store_path, &source.request(), dry_run, browser, out, ctx);
        }
        ProfileCommand::Sections => {
            let on = crate::diff_cmd::read_or_empty(store_path, ProfilePrefs::load)?.published();
            let rows: Vec<_> = Section::ALL
                .iter()
                .map(|s| serde_json::json!({ "section": s, "published": on.contains(s), "reveals": s.reveals() }))
                .collect();
            out.emit(&rows, |_| {
                for s in Section::ALL {
                    let mark = if on.contains(&s) { style("●").green() } else { style("○").dim() };
                    say!("  {mark} {:<11} {}", s.name(), dim(s.reveals()));
                }
                say!();
                say!("{}", dim("● in the document · change with `profile include` / `profile exclude`"));
            });
        }
        ProfileCommand::Include { sections } => {
            update(store_path, PrefsUpdate { include: sections, ..Default::default() }, out, "sections updated")?
        }
        ProfileCommand::Exclude { sections } => {
            update(store_path, PrefsUpdate { exclude: sections, ..Default::default() }, out, "sections updated")?
        }
        ProfileCommand::DefaultSections => update(
            store_path,
            PrefsUpdate { default_sections: true, ..Default::default() },
            out,
            "back to the default sections",
        )?,
        ProfileCommand::Pin { query, exact, source } => {
            let name = if exact { query } else { find_played(store_path, &source, &query, ctx)? };
            let msg = format!("pinned {name}");
            update(store_path, PrefsUpdate { pin: vec![name], ..Default::default() }, out, &msg)?;
        }
        ProfileCommand::Unpin { query } => {
            let store = open_store(store_path)?;
            let name = find_entry(&ProfilePrefs::load(&store)?.pinned, &query, "pinned")?;
            let msg = format!("unpinned {name}");
            update(store_path, PrefsUpdate { unpin: vec![name], ..Default::default() }, out, &msg)?;
        }
        ProfileCommand::Hide { query, artist: true, .. } => {
            let msg = format!("{query} won't be named");
            update(store_path, PrefsUpdate { hide_artist: vec![query], ..Default::default() }, out, &msg)?;
        }
        ProfileCommand::Hide { query, exact, source, .. } => {
            let name = if exact { query } else { find_played(store_path, &source, &query, ctx)? };
            let msg = format!("{name} won't be named (it still counts in the numbers)");
            update(store_path, PrefsUpdate { hide: vec![name], ..Default::default() }, out, &msg)?;
        }
        ProfileCommand::Unhide { query, artist } => {
            let prefs = ProfilePrefs::load(&open_store(store_path)?)?;
            let u = if artist {
                PrefsUpdate {
                    unhide_artist: vec![find_entry(&prefs.hidden_artists, &query, "hidden")?],
                    ..Default::default()
                }
            } else {
                PrefsUpdate { unhide: vec![find_entry(&prefs.hidden, &query, "hidden")?], ..Default::default() }
            };
            update(store_path, u, out, "updated")?;
        }
        ProfileCommand::Alias { from, to: Some(to) } => {
            let msg = format!("{from:?} shows as {to:?}");
            update(store_path, PrefsUpdate { alias: [(from, to)].into(), ..Default::default() }, out, &msg)?;
        }
        ProfileCommand::Alias { from, to: None } => {
            let msg = format!("forgot the alias for {from:?}");
            update(store_path, PrefsUpdate { unalias: vec![from], ..Default::default() }, out, &msg)?;
        }
        ProfileCommand::Headline { text } => {
            let msg = if text.as_deref().is_some_and(|t| !t.trim().is_empty()) {
                "headline set"
            } else {
                "back to the generated headline"
            };
            update(
                store_path,
                PrefsUpdate { headline: Some(text.unwrap_or_default()), ..Default::default() },
                out,
                msg,
            )?;
        }
        ProfileCommand::Precision { precision } => update(
            store_path,
            PrefsUpdate { precision: Some(precision.into()), ..Default::default() },
            out,
            "precision set",
        )?,
        ProfileCommand::Prefs => {
            let prefs = crate::diff_cmd::read_or_empty(store_path, ProfilePrefs::load)?;
            out.emit(&prefs, render_prefs);
        }
        ProfileCommand::Reset => {
            if let Some(o) =
                out.decide(false, "Forget every profile choice (sections, pins, hidden tracks, aliases, headline)?")
            {
                return Ok(out.not_applied(&serde_json::Value::Null, o));
            }
            update(store_path, PrefsUpdate { reset: true, ..Default::default() }, out, "profile choices reset")?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn update(store_path: &Option<PathBuf>, u: PrefsUpdate, out: Output, done: &str) -> Result<()> {
    let store = open_store(store_path)?;
    let sections_changed = u.reset || u.default_sections || !u.include.is_empty() || !u.exclude.is_empty();
    let prefs = api::profile_prefs_update(&store, &u)?;
    out.done(&prefs, |p| {
        ok(done);
        if sections_changed {
            let names: Vec<&str> = p.published().into_iter().map(Section::name).collect();
            say!("  {}", dim(format!("the document includes: {}", names.join(", "))));
        }
    });
    Ok(())
}

/// The "Artist - Title" of the one played track `query` names.
fn find_played(store_path: &Option<PathBuf>, source: &SourceArgs, query: &str, ctx: &Ctx<'_>) -> Result<String> {
    let store = crate::diff_cmd::existing_store(store_path)?;
    // Names are all that's needed: skip the analysis files.
    let req = ProfileRequest { no_analysis: true, ..source.request() };
    let prefs = store.as_ref().map(ProfilePrefs::load).transpose()?.unwrap_or_default();
    let report = api::profile_report(store.as_ref(), &req, &prefs, ctx)?;
    pick(&report.signature.played, query)
}

fn pick(played: &[PlayedTrack], query: &str) -> Result<String> {
    let q = normalize(query);
    if let Some(p) = played.iter().find(|p| normalize(&p.name) == q) {
        return Ok(p.name.clone());
    }
    let matches: Vec<&PlayedTrack> = played.iter().filter(|p| normalize(&p.name).contains(&q)).collect();
    match matches.as_slice() {
        [] => bail!("no track you've played matches {query:?} (--exact takes the text as given)"),
        [one] => Ok(one.name.clone()),
        many => {
            let list: Vec<String> = many.iter().take(8).map(|p| format!("  {}", p.name)).collect();
            let more = if many.len() > 8 { format!("\n  ... and {} more", many.len() - 8) } else { String::new() };
            bail!("{query:?} matches {} tracks; be more specific:\n{}{more}", many.len(), list.join("\n"))
        }
    }
}

/// The one entry of a preference list that `query` names.
fn find_entry(list: &[String], query: &str, what: &str) -> Result<String> {
    let q = normalize(query);
    if let Some(e) = list.iter().find(|e| normalize(e) == q) {
        return Ok(e.clone());
    }
    let matches: Vec<&String> = list.iter().filter(|e| normalize(e).contains(&q)).collect();
    match matches.as_slice() {
        [] => bail!("nothing {what} matches {query:?}"),
        [one] => Ok((*one).clone()),
        many => bail!("{query:?} matches {} {what} entries; be more specific", many.len()),
    }
}

// ---------------------------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------------------------

const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A horizontal bar `frac` of `width` cells long, in eighths.
fn bar(frac: f64, width: usize) -> String {
    let eighths = (frac.clamp(0.0, 1.0) * width as f64 * 8.0).round() as usize;
    let mut s = "█".repeat(eighths / 8);
    if !eighths.is_multiple_of(8) {
        s.push(['▏', '▎', '▍', '▌', '▋', '▊', '▉'][eighths % 8 - 1]);
    }
    s
}

/// One block per value, scaled between the smallest and largest.
fn spark(values: &[Option<f64>]) -> String {
    let known: Vec<f64> = values.iter().flatten().copied().collect();
    let (Some(lo), Some(hi)) = (known.iter().copied().reduce(f64::min), known.iter().copied().reduce(f64::max)) else {
        return String::new();
    };
    values
        .iter()
        .map(|v| match v {
            None => ' ',
            Some(x) if hi > lo => BLOCKS[(((x - lo) / (hi - lo)) * 7.0).round() as usize],
            Some(_) => BLOCKS[3],
        })
        .collect()
}

fn pct(x: f64) -> String {
    format!("{:.0}%", x * 100.0)
}

fn opt_pct(x: Option<f64>) -> String {
    x.map_or_else(|| "–".to_string(), pct)
}

struct Page<'a> {
    report: &'a ProfileReport,
    prefs: &'a ProfilePrefs,
    published: &'a [Section],
    only: &'a [Section],
}

impl Page<'_> {
    /// Prints the section's heading; false when the section isn't asked for.
    fn section(&self, s: Section, note: &str) -> bool {
        if !self.only.is_empty() && !self.only.contains(&s) {
            return false;
        }
        let published = self.published.contains(&s) && self.report.basis.publishes(s);
        let mark = if published { style("●").green() } else { style("○").dim() };
        let state = if published { style("in the document".to_string()).green() } else { dim("only on this computer") };
        say!();
        say!(
            "{mark} {}  {state}{}",
            style(s.title_in(self.report.basis)).bold(),
            if note.is_empty() { String::new() } else { format!("  {}", dim(note)) }
        );
        true
    }
}

fn render(v: &ProfileView, view: &ViewArgs) {
    let r = &v.report;
    let c = Page { report: r, prefs: &v.prefs, published: &v.published, only: &view.sections };
    let sources: Vec<&str> = r.sources.iter().map(|s| s.name.as_str()).collect();
    heading(&format!("DJ profile from {}", sources.join(", ")));
    let basis = match r.basis {
        Basis::Plays => format!("{} in {}", plural(r.sets.plays, "play"), plural(r.sets.count, "set")),
        Basis::Sessions => {
            format!("no sets declared: {} in {}", plural(r.sets.plays, "play"), plural(r.sets.count, "session"))
        }
        Basis::Collection => "no sets found: describing the collection".to_string(),
    };
    say!("  {}", dim(format!("{basis} · built {}", local_time(r.generated, "%Y-%m-%d %H:%M"))));

    if c.section(Section::Summary, "") {
        let headline = v.prefs.headline.clone().unwrap_or_else(|| r.summary.headline.clone());
        say!("  {}", style(headline).cyan().bold());
        if v.prefs.headline.is_some() {
            say!("  {}", dim(format!("(your headline; generated: {})", r.summary.headline)));
        }
        for t in &r.summary.tags {
            say!("  {} {}  {}", style("✓").green(), t.label, dim(&t.why));
        }
    }
    render_sets(&c, view.sets);
    render_tempo(&c);
    render_arc(&c);
    render_mixing(&c);
    render_keys(&c);
    render_energy(&c);
    render_ranked(&c, Section::Genres, &r.genres, "with a genre");
    render_ranked(&c, Section::Labels, &r.labels, "with a label");
    render_ranked(&c, Section::Artists, &r.artists, "with an artist");
    render_era(&c);
    render_signature(&c);
    render_prep(&c);
    render_collection(&c);
    render_style(&c);

    for w in &r.warnings {
        warn(w);
    }
    say!();
    crate::output::hint(
        "`locallibrary profile document` shows exactly what a public profile would show · `profile sections` lists what each reveals",
    );
}

fn render_sets(c: &Page<'_>, list: bool) {
    let r = c.report;
    let s = &r.sets;
    if !c.section(Section::Sets, "") {
        return;
    }
    if s.count == 0 {
        let none = if r.options.since.is_some() || r.options.until.is_some() {
            "no sets or sessions in these dates"
        } else {
            "no sets declared, and no sessions with enough tracks (`--min-tracks` sets the bar)"
        };
        say!("  {}", dim(none));
        return;
    }
    let unit = r.basis.unit();
    say!(
        "  {} · {} in a typical {unit} (longest {}) · {} of {}",
        plural(s.count, unit),
        plural(s.median_tracks, "track"),
        s.longest,
        plural(s.plays, "play"),
        plural(s.distinct_tracks, "track")
    );
    let mut notes = Vec::new();
    if let (Some(a), Some(b)) = (s.oldest, s.newest) {
        let days = if a == b { format!("on {a}") } else { format!("{a} to {b}") };
        notes.push(match r.basis {
            Basis::Sessions => format!("archived sessions first seen {days}"),
            Basis::Plays | Basis::Collection => format!("played {days}"),
        });
    }
    if s.not_archived > 0 {
        notes.push(format!("{} only on a drive so far (`pull` archives them)", s.not_archived));
    }
    if s.too_short > 0 {
        notes.push(format!("{} left out", plural(s.too_short, "shorter session")));
    }
    if !notes.is_empty() {
        say!("  {}", dim(notes.join(" · ")));
    }
    if r.basis == Basis::Sessions {
        crate::output::hint_at("  ", "`locallibrary sets new --from ID` declares one of these as a set");
    }
    if list {
        for set in &s.list {
            let when = set.date.map_or_else(|| "on a drive".to_string(), |d| d.to_string());
            let bpm = match (set.opening_bpm, set.closing_bpm) {
                (Some(a), Some(b)) => format!("{a:.0}→{b:.0} BPM"),
                _ => String::new(),
            };
            // A set without a title is named after its venue.
            let place = Some(set.place.as_str()).filter(|p| *p != set.name).unwrap_or_default();
            say!(
                "    {:<12} {} {:>3} {:<6}  {:<13} {}  {}",
                when,
                fit(&set.name, 22),
                set.tracks,
                if set.tracks == 1 { "track" } else { "tracks" },
                bpm,
                set.harmonic_share.map(|h| format!("{} in key", pct(h))).unwrap_or_default(),
                dim(format!("{} {place}", set.id.get(..8).unwrap_or(&set.id)).trim_end())
            );
        }
    } else {
        say!("  {}", dim("--sets lists them"));
    }
}

fn render_tempo(c: &Page<'_>) {
    let t = &c.report.tempo;
    if !c.section(Section::Tempo, "") {
        return;
    }
    let (Some(core), Some(median)) = (t.core, t.median) else {
        say!("  {}", dim("no BPMs (tracks need rekordbox's analysis)"));
        return;
    };
    let (lo, hi) = (core.low, core.high);
    say!(
        "  {} in the middle 80% · median {median:.1} · half between {:.1} and {:.1}",
        style(profile::bpm_range(lo, hi)).bold(),
        t.p25.unwrap_or(median),
        t.p75.unwrap_or(median)
    );
    let max = t.histogram.iter().map(|b| b.count).max().unwrap_or(1).max(1);
    for (start, n) in t.histogram.iter().map(|b| (&b.bpm, &b.count)) {
        let label = format!("{:>5.0}–{:<3.0}", start, start + t.bin_width);
        let in_core = start + t.bin_width > lo && *start <= hi;
        let b = bar(*n as f64 / max as f64, 36);
        let b = if in_core { style(b).cyan() } else { style(b).dim() };
        say!("  {label} {b} {}", dim(n));
    }
}

fn render_arc(c: &Page<'_>) {
    let a = &c.report.arc;
    if !c.section(Section::Arc, "") {
        return;
    }
    let unit = c.report.basis.unit();
    if a.sets == 0 {
        say!("  {}", dim(format!("needs {unit}s of 6 or more tracks with BPMs")));
        return;
    }
    say!(
        "  opening {:.1} → closing {:.1} BPM, averaged over {}",
        a.opening_bpm.unwrap_or(0.0),
        a.closing_bpm.unwrap_or(0.0),
        plural(a.sets, unit)
    );
    say!("  builds {} · steady {} · winds down {}", pct(a.builds), pct(a.steady), pct(a.winds_down));
    let known: Vec<f64> = a.curve.iter().flatten().copied().collect();
    if let (Some(lo), Some(hi)) = (known.iter().copied().reduce(f64::min), known.iter().copied().reduce(f64::max)) {
        say!(
            "  start {} end  {}",
            style(spark(&a.curve)).cyan(),
            dim(format!("BPM against each {unit}'s median: {lo:+.1} to {hi:+.1}"))
        );
    }
}

fn render_mixing(c: &Page<'_>) {
    let m = &c.report.mixing;
    if !c.section(Section::Mixing, "") {
        return;
    }
    if m.transitions == 0 {
        say!("  {}", dim(format!("needs {}s", c.report.basis.unit())));
        return;
    }
    if m.keyed > 0 {
        say!(
            "  {} of {} transitions in key: same key {} · ±1 {} · relative {} · energy boost {} · clash {}",
            style(opt_pct(m.harmonic_share)).bold(),
            thousands(m.keyed),
            m.same_key,
            m.adjacent,
            m.relative,
            m.energy_boost,
            m.clash
        );
    }
    if m.timed > 0 {
        say!(
            "  tempo steps: median {:.1}% · {} within 3% · {} within 6%{}",
            m.median_step_pct.unwrap_or(0.0),
            opt_pct(m.within_3pct),
            opt_pct(m.within_6pct),
            if m.half_double > 0 { format!(" · {} at half or double time", m.half_double) } else { String::new() }
        );
    }
    if m.genred > 0 {
        say!("  {} of transitions change genre", opt_pct(m.genre_change_share));
    }
}

fn render_keys(c: &Page<'_>) {
    let k = &c.report.keys;
    if !c.section(Section::Keys, "Camelot wheel") {
        return;
    }
    if k.tracks == 0 {
        say!("  {}", dim("no keys (tracks need rekordbox's analysis)"));
        return;
    }
    let max = k.wheel.iter().map(|w| w.count).max().unwrap_or(1).max(1);
    let header: String = (1..=12).map(|n| format!("{n:>4}")).collect();
    say!("    {}", dim(header));
    for (row, letter) in [(0usize, 'A'), (1, 'B')] {
        let cells: String = (0..12)
            .map(|n| {
                let count = k.wheel[n * 2 + row].count;
                let cell = format!("{:>4}", if count == 0 { "·".to_string() } else { count.to_string() });
                if count * 3 >= max * 2 {
                    style(cell).cyan().bold().to_string()
                } else if count > 0 {
                    style(cell).cyan().to_string()
                } else {
                    dim(cell).to_string()
                }
            })
            .collect();
        say!("  {} {cells}", dim(letter));
    }
    let top: Vec<String> = k.top.iter().take(5).map(|s| format!("{} {}", s.name, pct(s.share))).collect();
    say!("  most played: {} · minor keys {}", top.join(" · "), opt_pct(k.minor_share));
}

fn render_energy(c: &Page<'_>) {
    if !c.section(Section::Energy, "rekordbox's phrase-analysis mood") {
        return;
    }
    let Some(e) = &c.report.energy else {
        say!("  {}", dim("no phrase analysis read (needs analysis files; not with --no-analysis)"));
        return;
    };
    for (name, share) in [("high", e.high), ("mid", e.mid), ("low", e.low)] {
        say!("  {name:<5} {} {}", style(bar(share, 30)).magenta(), pct(share));
    }
    if e.curve.iter().flatten().count() > 1 {
        say!(
            "  start {} end  {}",
            style(spark(&e.curve)).magenta(),
            dim(format!("mood across a {}", c.report.basis.unit()))
        );
    }
}

fn render_ranked(c: &Page<'_>, s: Section, r: &RankedSection, with: &str) {
    if !c.section(s, "") {
        return;
    }
    if r.top.is_empty() {
        say!("  {}", dim("none tagged"));
        return;
    }
    let max = r.top.iter().map(|x| x.share).fold(0.0, f64::max).max(f64::EPSILON);
    for x in &r.top {
        let hidden = s == Section::Artists && c.prefs.is_hidden_artist(&x.name);
        let name = fit(&x.name, 26);
        let name = if hidden { dim(&name).to_string() } else { name };
        say!(
            "  {name} {} {:>4}{}",
            style(bar(x.share / max, 24)).cyan(),
            pct(x.share),
            if hidden { dim("  hidden").to_string() } else { String::new() }
        );
    }
    let noun = match c.report.basis {
        Basis::Plays | Basis::Sessions => "plays",
        Basis::Collection => "tracks",
    };
    say!("  {}", dim(format!("shares of {} {noun} {with} · {} different", thousands(r.known), thousands(r.distinct))));
}

fn render_era(c: &Page<'_>) {
    let e = &c.report.era;
    if !c.section(Section::Era, "release years") {
        return;
    }
    if e.tracks == 0 {
        say!("  {}", dim("no release years tagged"));
        return;
    }
    say!(
        "  median {} ({}–{}) · {} released the year played or the year before",
        e.median_year.unwrap_or(0),
        e.oldest.unwrap_or(0),
        e.newest.unwrap_or(0),
        opt_pct(e.recent_share)
    );
    let max = e.decades.iter().map(|d| d.count).max().unwrap_or(1).max(1);
    for (decade, n) in e.decades.iter().map(|d| (&d.decade, &d.count)) {
        say!("  {decade}s {} {}", style(bar(*n as f64 / max as f64, 30)).yellow(), dim(n));
    }
}

fn render_signature(c: &Page<'_>) {
    let s = &c.report.signature;
    let p = c.prefs;
    if !c.section(Section::Signature, "") {
        return;
    }
    let unit = c.report.basis.unit();
    let of_sets = plural(c.report.sets.count, unit);
    for pin in &p.pinned {
        let found = s.played.iter().find(|t| normalize(&t.name) == normalize(pin));
        let when = found.map_or_else(|| format!("not in these {unit}s"), |t| format!("{}/{of_sets}", t.sets));
        say!("  {} {}  {}", style("★").yellow(), pin, dim(when));
    }
    if s.tracks.is_empty() && p.pinned.is_empty() {
        say!("  {}", dim(format!("no track is in two or more {unit}s yet (`profile pin` names one)")));
    }
    for t in &s.tracks {
        if p.is_pinned(&t.name) {
            continue;
        }
        let hidden = !p.may_name(&t.name, &t.artist);
        let line = format!("{:>3}/{of_sets}  {}", t.sets, t.name);
        if hidden {
            say!("    {}  {}", dim(line), dim("hidden"));
        } else {
            say!("    {line}");
        }
    }
    if !s.transitions.is_empty() {
        say!("  {}", dim("moves played again and again"));
        for t in &s.transitions {
            say!("    {:>3}×  {} → {}", t.sets, t.from, t.to);
        }
    }
    for (label, list) in [("openers", &s.openers), ("closers", &s.closers)] {
        if !list.is_empty() {
            let names: Vec<String> = list
                .iter()
                .take(3)
                .map(|t| format!("{} ({}×)", t.name, if label == "openers" { t.opened } else { t.closed }))
                .collect();
            say!("  {} {}", dim(format!("{label}:")), names.join(" · "));
        }
    }
    if !p.hidden.is_empty() || !p.hidden_artists.is_empty() {
        say!("  {}", dim(format!("never named: {} track(s), {} artist(s)", p.hidden.len(), p.hidden_artists.len())));
    }
}

fn render_prep(c: &Page<'_>) {
    if !c.section(Section::Prep, "cues in the collection") {
        return;
    }
    let Some(p) = &c.report.prep else {
        say!("  {}", dim("no cues read (needs analysis files; not with --no-analysis)"));
        return;
    };
    say!(
        "  hot cues on {}{} · memory cues on {} · loops on {}  {}",
        style(pct(p.hot_cue_share)).bold(),
        p.hot_cues_per_track.map(|n| format!(" ({n:.1} each)")).unwrap_or_default(),
        pct(p.memory_cue_share),
        pct(p.loop_share),
        dim(format!("of {} tracks", thousands(p.tracks)))
    );
}

fn render_collection(c: &Page<'_>) {
    let k = &c.report.collection;
    if !c.section(Section::Collection, "") {
        return;
    }
    say!(
        "  {} tracks · {} playlists in {} folders · {} rated · {} colour-tagged{}",
        thousands(k.tracks),
        thousands(k.playlists),
        thousands(k.folders),
        opt_pct(k.rated_share),
        opt_pct(k.coloured_share),
        k.played_share.map(|p| format!(" · {} played in a {}", pct(p), c.report.basis.unit())).unwrap_or_default()
    );
    if k.duplicate_files > 0 {
        crate::output::hint_at(
            "  ",
            format!(
                "{} more files are copies of these and count once (`locallibrary doctor` lists them)",
                thousands(k.duplicate_files)
            ),
        );
    }
}

fn render_style(c: &Page<'_>) {
    if !c.section(Section::Style, "My Settings") {
        return;
    }
    let Some(s) = &c.report.style else {
        crate::output::hint_at("  ", "no My Settings on these drives (`locallibrary settings import` sets a profile)");
        return;
    };
    let values: Vec<String> =
        s.values.iter().map(|v| format!("{} {}", v.label, style(&v.value_label).bold())).collect();
    say!("  {}", values.join(" · "));
    say!("  {}", dim(format!("from {}", s.source)));
}

fn render_prefs(p: &ProfilePrefs) {
    let on = p.published();
    heading("Profile choices");
    let names: Vec<&str> = on.iter().map(|s| s.name()).collect();
    say!(
        "  sections: {}{}",
        names.join(", "),
        if p.sections.is_none() { dim(" (the defaults)").to_string() } else { String::new() }
    );
    say!(
        "  numbers: {}",
        match p.precision {
            Precision::Rounded => "rounded",
            Precision::Exact => "exact",
        }
    );
    if let Some(h) = &p.headline {
        say!("  headline: {h}");
    }
    for (label, list) in [("pinned", &p.pinned), ("hidden tracks", &p.hidden), ("hidden artists", &p.hidden_artists)] {
        if !list.is_empty() {
            say!("  {label}:");
            for x in list {
                say!("    {x}");
            }
        }
    }
    if !p.genre_aliases.is_empty() {
        say!("  genre aliases:");
        for (from, to) in &p.genre_aliases {
            say!("    {from} → {to}");
        }
    }
}

#[cfg(test)]
mod tests;
