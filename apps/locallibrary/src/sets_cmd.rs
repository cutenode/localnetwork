//! `locallibrary sets ...`: the sets the DJ played, declared from the sessions players recorded
//! and from recordings, then edited, written out, turned into playlists and published. `set` is
//! another spelling of `sets` (it was the night-before job, `prep` now).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::builder::PossibleValuesParser;
use clap::{Args, Subcommand, ValueEnum};
use console::style;
use locallibrary::api::{self, SetFields, SetFormat};
use locallibrary::progress::Ctx;
use locallibrary::sets::query::Appearance;
use locallibrary::sets::{EntryOrigin, Session, Set, SetEntry, describe};
use loclib_plugin::{ExportFrom, ExportSource};

use crate::jobs_cmd::usage;
use crate::output::{Outcome, Output, clock, dim, duration, fit, heading, ok, plural, warn};

/// `sets`: with no subcommand, the list.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct SetsArgs {
    #[command(subcommand)]
    command: Option<SetsCommand>,
    #[command(flatten)]
    list: ListArgs,
}

#[derive(Subcommand)]
enum SetsCommand {
    /// Your sets, newest first
    #[command(visible_alias = "ls")]
    List(ListArgs),
    /// A set's details and tracklist
    Show {
        /// The set: `last`, its id (or the start of it), title or day [default: the latest at
        /// --venue]
        #[arg(value_name = "SET", required_unless_present = "venue")]
        set: Option<String>,
        /// The latest set at the venue whose name has this in it
        #[arg(long)]
        venue: Option<String>,
    },
    /// Declare a set: from sessions players recorded, a recording of it, or neither (then add
    /// its tracks with `sets edit --insert`)
    #[command(after_long_help = NEW_HELP)]
    New(NewArgs),
    /// Change a set's details and tracklist
    #[command(after_long_help = EDIT_HELP)]
    Edit(EditArgs),
    /// Delete a set (the sessions and recording it came from stay), and its record and its
    /// recording's from your Atmosphere account when they're published there
    Delete {
        #[arg(value_name = "SET")]
        set: String,
    },
    /// The sets the sessions players recorded suggest, with the evidence
    #[command(after_long_help = SUGGEST_HELP)]
    Suggest {
        #[arg(long, help = crate::DEFAULT_DRIVE_HELP, conflicts_with_all = ["dismiss", "undismiss"])]
        drive: Option<String>,
        /// Say these sessions weren't a set, so they're no longer suggested (ids, or list names)
        #[arg(long, value_name = "SESSION", num_args = 1.., conflicts_with = "undismiss")]
        dismiss: Vec<String>,
        /// Suggest these sessions again
        #[arg(long, value_name = "SESSION", num_args = 1..)]
        undismiss: Vec<String>,
    },
    /// Write a set's tracklist out: for Mixcloud, a CUE sheet, YouTube chapters, 1001Tracklists,
    /// podcast chapters, CSV, an M3U8 of its tracks on a drive, or in a format a plugin adds
    #[command(after_long_help = EXPORT_HELP)]
    Export {
        #[arg(value_name = "SET")]
        set: String,
        /// Or a format a plugin adds for sets [default: from -o's extension (.cue, .json, .csv,
        /// .m3u8, .txt), else text]
        #[arg(long, short, value_parser = formats())]
        format: Option<String>,
        /// Write it to this file [default: print it]
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// For m3u8: the drive the tracks are on [default: the only mounted rekordbox drive]; for a
        /// plugin's format, one to give it
        #[arg(long)]
        drive: Option<String>,
        /// Replace the file if it's there already
        #[arg(long, requires = "output")]
        replace: bool,
        /// A plugin's format: list what would be written, and write nothing
        #[arg(long)]
        dry_run: bool,
        /// A plugin's format: an option for it, like key=value (repeatable; `locallibrary plugin
        /// info` shows them)
        #[arg(long = "option", value_name = "KEY=VALUE")]
        options: Vec<String>,
    },
    /// When and where you played a track
    Played {
        /// Part of "Artist - Title", or the track's path on the drive (/Contents/...)
        query: String,
    },
    /// A playlist in a library with a set's tracks, in set order
    Playlist {
        #[arg(value_name = "SET")]
        set: String,
        #[arg(help = crate::LIBRARY_HELP)]
        drive: String,
        /// The playlist [default: Sets/<date> <title or venue>]
        #[arg(long, value_name = "PATH")]
        name: Option<String>,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Publish a set: with its recording at a plugin's set destination (--to), or on your
    /// Atmosphere account, its recording too with --upload, where anyone can read it (alpha)
    Publish {
        #[arg(value_name = "SET")]
        set: String,
        /// Publish it with its recording at a plugin's set destination (`plugin info PLUGIN` lists
        /// them): asked first, and its link kept (signed in, its record is offered next)
        #[arg(long, value_name = "PLUGIN:DESTINATION")]
        #[cfg_attr(not(feature = "account"), arg(required = true))]
        to: Option<String>,
        /// Publish these tracks without naming them, by position: 3,7 or 1-2,30 (tracks the set
        /// marks unnamed and tracks your profile hides are never named)
        #[arg(long, value_name = "POSITIONS", value_parser = positions)]
        hide: Vec<Positions>,
        /// Show what would be published without publishing (or looking anything up)
        #[arg(long)]
        dry_run: bool,
        /// Upload its recording to your Atmosphere account first, and point its record at it
        /// (encoded as AAC by ffmpeg when browsers can't play it or it's too big)
        #[cfg(feature = "account")]
        #[arg(long, conflicts_with = "to")]
        upload: bool,
        /// The most your account's data server takes, in MiB, when that's less than 300 (the
        /// recording is encoded to fit)
        #[cfg(feature = "account")]
        #[arg(long, value_name = "MIB", requires = "upload", value_parser = clap::value_parser!(u64).range(1..))]
        max_mib: Option<u64>,
        /// Don't look up MusicBrainz IDs for the tracks that have none
        #[cfg(feature = "account")]
        #[arg(long)]
        no_lookup: bool,
        #[cfg(feature = "account")]
        #[command(flatten)]
        browser: crate::browser::BrowserArgs,
    },
}

const NEW_HELP: &str = "Examples:
  locallibrary sets suggest                                  which sessions look like a set
  locallibrary sets new --from 1a2b3c4d --from 5e6f7a8b --venue \"Warehouse\" --date 2026-10-03
  locallibrary sets new --recording friday.wav --title Friday    the tracks a recording plays, timed

A recording's tracklist beats the sessions': it has start times, catches tracks played for less than
the minute a player needs to log one, and leaves out tracks only heard in headphones. Without
--date, the day is the recording's, or the one the sessions' windows agree on.";

const EDIT_HELP: &str = "Positions start at 1 and name the tracklist as it was before the edit, so one
edit's changes don't shift each other. A track moved up lands just before the track that was at
TO, one moved down just after it; an inserted track lands just before the track that was at AT
(one past the last appends). An empty value clears a detail (--city \"\").

Examples:
  locallibrary sets edit last --drop 1-2,30 --move 5:2
  locallibrary sets edit last --insert 4:\"Floorplan - Never Grow Old\"
  locallibrary sets edit last --unnamed 3,7 --link https://www.mixcloud.com/dj/friday/";

const SUGGEST_HELP: &str = "Players start a new history list every time a drive goes in, and log any
track that plays for about a minute, prep and practice too. locallibrary calls those sessions and
counts none as a set until you say so. A session's date is a window: players have no clock, so
it's when locallibrary last saw the drive without it and first saw it with it (`watch --pull`
keeps that tight). Sessions go together as one night only when their windows span 36 hours or
less; otherwise each is suggested on its own.";

const EXPORT_HELP: &str = "Examples:
  locallibrary sets export last --format mixcloud           paste into Mixcloud's tracklist
  locallibrary sets export last --format cue -o friday.cue  needs the recording attached
  locallibrary sets export last -o friday.json              chapters for podcast apps
  locallibrary sets export last --format m3u8 -o friday.m3u8 --drive Home

Every format but csv and m3u8 is for publishing: a track marked unnamed or hidden by your profile
(`profile hide`) is `ID - ID` there.";

/// `sets list`.
#[derive(Args)]
struct ListArgs {
    /// Only sets at a venue whose name has this in it
    #[arg(long)]
    venue: Option<String>,
    /// Only sets of this kind
    #[arg(long)]
    kind: Option<String>,
    /// Only sets played from this day: 2026, 2026-09 or 2026-09-30
    #[arg(long, value_name = "DATE")]
    since: Option<String>,
    /// Only sets played up to this day
    #[arg(long, value_name = "DATE")]
    until: Option<String>,
}

/// A set's details, as `sets new` and `sets edit` take them.
#[derive(Args)]
struct FieldArgs {
    /// What it's called: Friday at the Warehouse
    #[arg(long)]
    title: Option<String>,
    /// The day it was played: 2026-10-03
    #[arg(long, value_name = "DAY")]
    date: Option<String>,
    /// club, festival, radio, livestream, mix, or your own word
    #[arg(long)]
    kind: Option<String>,
    /// Where it was played: a club, a festival, a station
    #[arg(long)]
    venue: Option<String>,
    /// The venue's city
    #[arg(long)]
    city: Option<String>,
    /// Two letters: GB
    #[arg(long, value_name = "CC")]
    country: Option<String>,
    /// The event it was part of: the at:// URI of its record
    #[arg(long, value_name = "AT-URI")]
    event: Option<String>,
    /// Who you played with: a name, or did:... for their Atmosphere account (repeatable;
    /// replaces the list, "" clears it)
    #[arg(long = "with", value_name = "NAME")]
    with: Vec<String>,
    /// Where people can listen to it: https://...
    #[arg(long, value_name = "URL")]
    link: Option<String>,
    /// Your own notes, never published
    #[arg(long)]
    notes: Option<String>,
}

impl FieldArgs {
    fn fields(self) -> SetFields {
        SetFields {
            title: self.title,
            date: self.date,
            kind: self.kind,
            venue: self.venue,
            city: self.city,
            country: self.country,
            event: self.event,
            with: (!self.with.is_empty()).then_some(self.with),
            link: self.link,
            notes: self.notes,
        }
    }
}

/// Where a set's tracklist comes from, besides sessions.
#[derive(Args)]
struct SourceArgs {
    /// A recording of the set (WAV, AIFF, MP3, M4A, FLAC, or an Audacity project): its tracks are
    /// identified against --drive's library
    #[arg(long, value_name = "FILE", conflicts_with = "report")]
    recording: Option<PathBuf>,
    /// Or a recording's report kept in the store (`--recording` keeps one)
    #[arg(long, value_name = "ID")]
    report: Option<String>,
    /// The drive whose library a recording is matched against and inserted tracks are looked up
    /// in [default: the only mounted rekordbox drive]
    #[arg(long)]
    drive: Option<String>,
}

#[derive(Args)]
struct NewArgs {
    /// A session to build it from, in play order (repeatable): its id, or its list's name
    #[arg(long = "from", value_name = "SESSION")]
    sessions: Vec<String>,
    #[command(flatten)]
    source: SourceArgs,
    #[command(flatten)]
    fields: FieldArgs,
}

#[derive(Args)]
struct EditArgs {
    #[arg(value_name = "SET")]
    set: String,
    #[command(flatten)]
    fields: FieldArgs,
    /// Take tracks out, by position: 3, 1-2 or 1-2,30
    #[arg(long, value_name = "POSITIONS", value_parser = positions)]
    drop: Vec<Positions>,
    /// Move a track: FROM:TO, like 5:2 (repeatable)
    #[arg(long = "move", value_name = "FROM:TO", value_parser = a_move)]
    moves: Vec<api::Move>,
    /// Put a track in: AT:TRACK, like 4:"Artist - Title" (a search on --drive, else as typed;
    /// repeatable)
    #[arg(long, value_name = "AT:TRACK", value_parser = an_insert)]
    insert: Vec<api::Insert>,
    /// Add a session's tracks at the end (repeatable)
    #[arg(long = "add-from", value_name = "SESSION")]
    add_sessions: Vec<String>,
    #[command(flatten)]
    source: SourceArgs,
    /// Publish these tracks without naming them: 3,7
    #[arg(long, value_name = "POSITIONS", value_parser = positions)]
    unnamed: Vec<Positions>,
    /// Name these tracks again
    #[arg(long, value_name = "POSITIONS", value_parser = positions)]
    named: Vec<Positions>,
}

/// Tracklist positions: `3`, `1-2`, `1-2,30`.
#[derive(Clone)]
struct Positions(Vec<usize>);

fn positions(s: &str) -> std::result::Result<Positions, String> {
    let bad = || format!("{s:?} isn't a list of positions: give them like 3, 1-2 or 1-2,30");
    let mut out = Vec::new();
    for part in s.split(',').map(str::trim) {
        let n = |p: &str| p.trim().parse::<usize>().map_err(|_| bad());
        match part.split_once('-') {
            Some((a, b)) if n(a)? <= n(b)? => out.extend(n(a)?..=n(b)?),
            Some(_) => return Err(format!("{part:?} runs backwards: give it like 1-2")),
            None => out.push(n(part)?),
        }
    }
    Ok(Positions(out))
}

fn all(p: Vec<Positions>) -> Vec<usize> {
    p.into_iter().flat_map(|p| p.0).collect()
}

fn a_move(s: &str) -> std::result::Result<api::Move, String> {
    let parsed = s.split_once(':').and_then(|(a, b)| Some((a.trim().parse().ok()?, b.trim().parse().ok()?)));
    let (from, to) = parsed.ok_or_else(|| format!("{s:?} isn't a move: give FROM:TO, like 5:2"))?;
    Ok(api::Move { from, to })
}

fn an_insert(s: &str) -> std::result::Result<api::Insert, String> {
    let parsed = s.split_once(':').and_then(|(a, t)| Some((a.trim().parse().ok()?, t.to_string())));
    let (at, track) =
        parsed.ok_or_else(|| format!("{s:?} isn't an insert: give AT:TRACK, like 4:\"Artist - Title\""))?;
    Ok(api::Insert { at, track })
}

/// `export --format`'s values: locallibrary's formats, and those plugins add for sets.
fn formats() -> PossibleValuesParser {
    let own = FormatArg::value_variants().iter().filter_map(ValueEnum::to_possible_value);
    PossibleValuesParser::new(own.chain(crate::export_cmd::plugin_formats(ExportFrom::Set)).collect::<Vec<_>>())
}

/// `export --format`.
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum FormatArg {
    /// 01. Artist - Title
    Text,
    /// Mixcloud's tracklist, with its rules checked
    Mixcloud,
    /// A CUE sheet for the recording
    Cue,
    /// Chapters for a YouTube description
    Youtube,
    /// For 1001Tracklists
    #[value(name = "1001")]
    Tracklists,
    /// Podcasting 2.0 JSON chapters, for podcast apps
    Chapters,
    /// Every detail, for a spreadsheet
    Csv,
    /// A playlist of the tracks on a drive
    M3u8,
}

impl From<FormatArg> for SetFormat {
    fn from(f: FormatArg) -> Self {
        match f {
            FormatArg::Text => SetFormat::Text,
            FormatArg::Mixcloud => SetFormat::Mixcloud,
            FormatArg::Cue => SetFormat::Cue,
            FormatArg::Youtube => SetFormat::Youtube,
            FormatArg::Tracklists => SetFormat::Tracklists,
            FormatArg::Chapters => SetFormat::Chapters,
            FormatArg::Csv => SetFormat::Csv,
            FormatArg::M3u8 => SetFormat::M3u8,
        }
    }
}

/// The format a file's extension names, where only one does.
fn format_of(path: &Path) -> Option<SetFormat> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "cue" => Some(SetFormat::Cue),
        "json" => Some(SetFormat::Chapters),
        "csv" => Some(SetFormat::Csv),
        "m3u8" => Some(SetFormat::M3u8),
        "txt" => Some(SetFormat::Text),
        _ => None,
    }
}

pub fn run(args: SetsArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let store = || crate::open_store(store_path);
    match args.command.unwrap_or(SetsCommand::List(args.list)) {
        SetsCommand::List(a) => {
            let req = api::SetsQuery { venue: a.venue, kind: a.kind, since: a.since, until: a.until };
            let sets = crate::diff_cmd::read_or_empty(store_path, |s| api::sets_list(s, &req))?;
            out.emit_paged(&sets, |l| list(l));
        }
        SetsCommand::Show { set, venue } => {
            let s = api::set_get(&crate::diff_cmd::store_to_read(store_path)?, &api::SetRef { set, venue })?;
            out.emit_paged(&s, show);
        }
        SetsCommand::New(a) => {
            let req = api::NewSet {
                fields: a.fields.fields(),
                sessions: a.sessions,
                recording: a.source.recording,
                report: a.source.report,
                drive: a.source.drive,
            };
            let change = api::set_new(&store()?, &req, ctx)?;
            return Ok(out.done(&change, |c| changed("declared", c)));
        }
        SetsCommand::Edit(a) => {
            let req = api::EditSet {
                set: a.set,
                fields: a.fields.fields(),
                drop: all(a.drop),
                moves: a.moves,
                insert: a.insert,
                add_sessions: a.add_sessions,
                recording: a.source.recording,
                report: a.source.report,
                drive: a.source.drive,
                unnamed: all(a.unnamed),
                named: all(a.named),
            };
            let change = api::set_edit(&store()?, &req, ctx)?;
            return Ok(out.done(&change, |c| changed("changed", c)));
        }
        SetsCommand::Delete { set } => {
            let store = store()?;
            let s = api::set_get(&store, &api::SetRef { set: Some(set), venue: None })?;
            let what = format!("delete the set {} (its sessions and recording stay)", describe(&s));
            #[cfg(feature = "account")]
            let what = match api::set_published_uris(store.root(), &s.id)?.as_slice() {
                [] => what,
                [uri] => format!("{what}, and its record on your Atmosphere account ({uri})"),
                uris => format!("{what}, and its records on your Atmosphere account ({})", uris.join(", ")),
            };
            let id = api::SetRef { set: Some(s.id.clone()), venue: None };
            return out.one_step(
                &what,
                "Delete it?",
                || Ok(api::set_delete(&store, &id)?),
                |s| ok(format!("deleted {}", describe(s))),
            );
        }
        SetsCommand::Suggest { dismiss, undismiss, .. } if !dismiss.is_empty() || !undismiss.is_empty() => {
            let undo = dismiss.is_empty();
            let req = api::DismissSessions { sessions: if undo { undismiss } else { dismiss }, undo };
            let ids = api::sessions_dismiss(&store()?, &req, ctx)?;
            return Ok(out.done(&ids, |ids| {
                let short: Vec<&str> = ids.iter().map(|id| &id[..id.len().min(8)]).collect();
                match undo {
                    true => ok(format!("{} suggested again", short.join(", "))),
                    false => ok(format!("{} won't be suggested (`--undismiss` takes it back)", short.join(", "))),
                }
            }));
        }
        SetsCommand::Suggest { drive, .. } => {
            let store = crate::diff_cmd::existing_store(store_path)?;
            let s = api::sets_suggest(store.as_ref(), &api::SuggestSets { drive }, ctx)?;
            out.emit_paged(&s, suggestions);
        }
        SetsCommand::Export { set, format: Some(format), output, drive, replace, dry_run, options }
            if FormatArg::from_str(&format, true).is_err() =>
        {
            let output = output.ok_or_else(|| usage("a plugin's format writes a file: say where with -o"))?;
            let source = ExportSource { drive: drive.unwrap_or_default(), set: Some(set), ..ExportSource::default() };
            let written = crate::export_cmd::Written { output: &output, options: &options, dry_run, replace };
            let store = crate::diff_cmd::existing_store(store_path)?;
            return crate::export_cmd::plugin_export(&format, source, written, store.as_ref(), out, ctx);
        }
        SetsCommand::Export { dry_run: true, .. } => bail!("--dry-run is for plugins' formats"),
        SetsCommand::Export { options, .. } if !options.is_empty() => bail!("--option is for plugins' formats"),
        SetsCommand::Export { set, format, output, drive, replace, .. } => {
            let format = format.map(|f| FormatArg::from_str(&f, true).expect("one of locallibrary's").into());
            let format = format.or_else(|| output.as_deref().and_then(format_of));
            let req = api::SetExport { set, format: format.unwrap_or(SetFormat::Text), drive, output, replace };
            let r = api::set_export(&crate::diff_cmd::store_to_read(store_path)?, &req)?;
            let Some(path) = &r.path else {
                out.emit(&r, |r| {
                    print!("{}", r.text.as_deref().unwrap_or_default());
                    for w in &r.warnings {
                        crate::output::diag(w);
                    }
                });
                return Ok(ExitCode::SUCCESS);
            };
            return Ok(out.done(&r, |r| {
                ok(format!("wrote {} ({})", path.display(), plural(r.tracks, "track")));
                for w in &r.warnings {
                    warn(w);
                }
            }));
        }
        SetsCommand::Played { query } => {
            let req = api::TrackPlaysQuery { track: query.clone() };
            let found = crate::diff_cmd::read_or_empty(store_path, |s| api::track_plays(s, &req))?;
            out.emit_paged(&found, |a| played(&query, a));
        }
        SetsCommand::Playlist { set, drive, name, dry_run } => {
            let req = api::SetPlaylist { set, drive: drive.clone(), name };
            let plan = api::set_playlist_plan(&store()?, &req, ctx)?;
            return crate::edit_cmd::confirm_and_apply(&drive, plan, dry_run, out, ctx);
        }
        SetsCommand::Publish {
            set,
            to,
            dry_run,
            hide,
            #[cfg(feature = "account")]
            upload,
            #[cfg(feature = "account")]
            max_mib,
            #[cfg(feature = "account")]
            no_lookup,
            #[cfg(feature = "account")]
            browser,
        } => {
            if let Some(spec) = to {
                let (store, hide) = (store()?, all(hide));
                #[cfg(feature = "account")]
                let then = then(&store, store_path, &hide, !no_lookup, browser, out, ctx)?;
                #[cfg(not(feature = "account"))]
                let then = None;
                return publish_to(&store, &spec, &set, &hide, dry_run, then, out, ctx);
            }
            #[cfg(feature = "account")]
            {
                let req = api::SetPublishRequest { set, hide: all(hide) };
                if upload {
                    let max_bytes = max_mib.map(|m| m.saturating_mul(1024 * 1024));
                    return crate::publish_cmd::set_upload(
                        store_path, &req, max_bytes, dry_run, !no_lookup, browser, out, ctx,
                    );
                }
                return Ok(crate::publish_cmd::set_publish(store_path, &req, dry_run, !no_lookup, browser, out, ctx)?
                    .print(out));
            }
            #[cfg(not(feature = "account"))]
            unreachable!("clap requires --to without the Atmosphere account");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Publishing the set's record once a plugin has published the set (`sets publish SET --to`),
/// offered when signed in: the account it goes to, and publishing it as `sets publish SET` does,
/// into the command's JSON envelope as `record`.
struct Then<'a> {
    account: String,
    publish: PublishRecord<'a>,
}

/// Publishes the record of the set by this id, into this JSON envelope.
type PublishRecord<'a> = Box<dyn FnOnce(&str, &mut serde_json::Value) -> Outcome + 'a>;

/// [`Then`], when someone's signed in. The set is published at the plugin's destination either
/// way: declining leaves its record for later, and a record that fails to publish says so (exit
/// status 5) without undoing that.
#[cfg(feature = "account")]
fn then<'a>(
    store: &locallibrary::Store,
    store_path: &'a Option<PathBuf>,
    hide: &'a [usize],
    lookup: bool,
    browser: crate::browser::BrowserArgs,
    out: Output,
    ctx: &'a Ctx<'_>,
) -> Result<Option<Then<'a>>> {
    let Some(account) = locallibrary::atmosphere::status(store.root())? else { return Ok(None) };
    let publish = move |set: &str, done: &mut serde_json::Value| {
        let later = format!("`locallibrary sets publish {set}` publishes its record later");
        let req = api::SetPublishRequest { set: set.to_string(), hide: hide.to_vec() };
        match crate::publish_cmd::set_publish(store_path, &req, false, lookup, browser, out, ctx) {
            Ok(record) => {
                if !out.json && record.outcome == Outcome::Declined {
                    crate::output::hint(later);
                }
                done["record"] = record.envelope;
                Outcome::Applied
            }
            Err(e) => {
                let e = format!("{e:#}");
                if !out.json {
                    warn(format!("its record wasn't published: {e}"));
                    crate::output::hint(later);
                }
                done["record"] = serde_json::json!({ "error": { "message": e }, "outcome": "error" });
                Outcome::Partial
            }
        }
    };
    Ok(Some(Then { account: account.name(), publish: Box::new(publish) }))
}

/// `sets publish SET --to PLUGIN:DESTINATION`: the plugin's dry run shown, and publishing always
/// asked about, since it reaches outside this computer; then, signed in, the set's record with
/// what the plugin answered (see [`Then`]).
#[allow(clippy::too_many_arguments)]
fn publish_to(
    store: &locallibrary::Store,
    spec: &str,
    set: &str,
    hide: &[usize],
    dry_run: bool,
    then: Option<Then<'_>>,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let plan = crate::plugins::host()?.publish_plan(spec, set, hide, store, ctx)?;
    if !out.json {
        crate::output::summary(&plan.summary, &plan.warnings);
        if let Some(then) = &then {
            crate::output::hint(format!("then publishing its record as {} is offered", then.account));
        }
    }
    if let Some(o) = out.decide(dry_run, "Publish it?") {
        return Ok(out.not_applied(&plan, o));
    }
    let r = plan.apply(Some(store), ctx)?;
    if !out.json {
        match (&r.url, &r.name) {
            (Some(url), Some(name)) => ok(format!("published at {name}: {url}")),
            (Some(url), None) => ok(format!("published: {url}")),
            _ => ok("published"),
        }
        for w in &r.warnings {
            warn(w);
        }
    }
    let mut done = crate::output::envelope(&plan, &r, true, Outcome::Applied);
    let outcome = then.map_or(Outcome::Applied, |then| (then.publish)(&r.set, &mut done));
    if out.json {
        crate::output::print_json(done);
    }
    Ok(outcome.exit())
}

fn list(sets: &[Set]) {
    if sets.is_empty() {
        say!("no sets yet");
        crate::output::hint("`locallibrary sets suggest` finds them in the sessions players recorded");
        return;
    }
    for s in sets {
        let place = s.place();
        let what = match (s.title.as_deref(), place.is_empty()) {
            (Some(t), false) => format!("{t} · {place}"),
            (None, false) => place,
            (t, true) => t.unwrap_or("untitled").to_string(),
        };
        let kind = s.kind.as_deref().map(|k| format!(" · {k}")).unwrap_or_default();
        say!(
            "{}  {}  {what}{}",
            dim(s.short_id()),
            s.date,
            dim(format!("{kind} · {}", plural(s.tracks.len(), "track")))
        );
    }
}

/// A set's details and tracklist.
fn show(s: &Set) {
    heading(&s.heading());
    let kind = s.kind.as_deref().map(|k| format!(" · {k}")).unwrap_or_default();
    say!("  {}", dim(format!("{} · {}{kind}", s.id, s.date)));
    let place =
        [Some(s.place()), s.country.clone()].into_iter().flatten().filter(|p| !p.is_empty()).collect::<Vec<_>>();
    let with: Vec<String> = s
        .with
        .iter()
        .map(|p| match &p.did {
            Some(did) if *did != p.name => format!("{} ({did})", p.name),
            _ => p.name.clone(),
        })
        .collect();
    let sessions: Vec<&str> = s.sessions.iter().map(|id| &id[..id.len().min(8)]).collect();
    let sources = [
        match sessions.as_slice() {
            [] => None,
            [one] => Some(format!("session {one}")),
            many => Some(format!("sessions {}", many.join(", "))),
        },
        s.recording.as_ref().map(|r| format!("the recording {}", r.path.display())),
    ];
    let lines = [
        ("Where", Some(place.join(", "))),
        ("With", Some(with.join(", "))),
        ("Event", s.event.clone()),
        ("Listen", s.link.clone()),
        ("From", Some(sources.into_iter().flatten().collect::<Vec<_>>().join(" and "))),
        ("Notes", s.notes.clone()),
    ];
    for (label, text) in lines {
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            say!("  {label:<7} {text}");
        }
    }
    let length = duration(s.tracks.iter().map(|e| u64::from(e.duration_secs)).sum());
    say!("  {}{}", style(plural(s.tracks.len(), "track")).bold(), dim(format!(" · {length}")));
    let timed = s.tracks.iter().any(|e| e.start_seconds.is_some());
    for (i, e) in s.tracks.iter().enumerate() {
        let start = match (timed, e.start_seconds) {
            (false, _) => String::new(),
            (true, Some(at)) => format!("{:>8}  ", clock(at.into())),
            (true, None) => format!("{:>8}  ", ""),
        };
        say!("  {:>3}  {start}{}{}", i + 1, e.name(), dim(marks(e)));
    }
}

/// What sets an entry apart: unnamed, added by hand, heard in the recording.
fn marks(e: &SetEntry) -> String {
    let origin = match e.origin {
        EntryOrigin::Session => None,
        EntryOrigin::Added => Some("added"),
        EntryOrigin::Recording => Some("from the recording"),
    };
    let marks: Vec<&str> = [e.unnamed.then_some("unnamed"), origin].into_iter().flatten().collect();
    if marks.is_empty() { String::new() } else { format!("  {}", marks.join(" · ")) }
}

/// What `sets new` and `sets edit` came to.
fn changed(did: &str, c: &api::SetChange) {
    ok(format!("{did} {}", describe(&c.set)));
    show(&c.set);
    for n in &c.notes {
        say!("{} {n}", style("•").cyan());
    }
}

fn suggestions(s: &api::Suggestions) {
    if s.candidates.is_empty() {
        say!("no sets to suggest: every session long enough is in a set or was dismissed");
        crate::output::hint("`locallibrary plays lists --all` lists the sessions");
        return;
    }
    for (i, c) in s.candidates.iter().enumerate() {
        if i > 0 {
            say!();
        }
        let when = c.window.label();
        let mut when = when.chars();
        let when: String = when.next().into_iter().flat_map(char::to_uppercase).chain(when).collect();
        heading(&format!("{when} · score {} of 100", c.score));
        let sessions: Vec<&Session> =
            c.sessions.iter().filter_map(|id| s.sessions.iter().find(|x| x.id == *id)).collect();
        for x in &sessions {
            say!("  {}", session_line(x, true));
        }
        say!("  {}", dim(c.reasons.join(" · ")));
        let from: Vec<String> = sessions.iter().rev().map(|x| format!("--from {}", x.short_id())).collect();
        say!("  {}", style(format!("locallibrary sets new {}", from.join(" "))).cyan());
    }
    say!();
    crate::output::hint("Add --date, --venue and --title as you like; `--dismiss ID` stops suggesting a session.");
}

/// A session in a line: id, list, the drive it's on (`with_drive`; not under that drive's heading),
/// tracks, length, when.
pub(crate) fn session_line(s: &Session, with_drive: bool) -> String {
    let drive = match with_drive {
        true => format!("{} ", fit(s.drives.first().or(s.first_seen_on.as_ref()).unwrap_or(&s.source), 12)),
        false => String::new(),
    };
    let n = s.tracks.len();
    let length = duration(s.tracks.iter().map(|e| u64::from(e.duration_secs)).sum());
    format!(
        "{}  {:<14} {drive}{n:>4} {:<6} {length:>8}  {}",
        s.short_id(),
        fit(&s.path, 14),
        if n == 1 { "track" } else { "tracks" },
        s.window.label()
    )
}

fn played(query: &str, found: &[Appearance]) {
    let Some(latest) = found.first() else {
        say!("{query:?} isn't in any of your sets");
        return;
    };
    for a in found {
        let what = match (&a.title, &a.venue) {
            (Some(t), Some(v)) => format!("{t}, {v}"),
            (Some(n), None) | (None, Some(n)) => n.clone(),
            (None, None) => "untitled".into(),
        };
        let at = a.start_seconds.map(|s| format!(" at {}", clock(s.into()))).unwrap_or_default();
        say!("  {}  {:<30} {}", a.date, fit(&what, 30), dim(format!("track {}{at} · {}", a.position, a.track)));
    }
    let tracks = found.iter().map(|a| a.track.as_str()).collect::<rustc_hash::FxHashSet<_>>().len();
    let times = if found.len() == 1 { "once".to_string() } else { format!("{} times", found.len()) };
    let what = if tracks > 1 { format!("{tracks} tracks matching {query:?}") } else { latest.track.clone() };
    let place = latest.venue.as_deref().map(|v| format!(" at {v}")).unwrap_or_default();
    say!("{what}: played {times}, last on {}{place}", latest.date);
}

#[cfg(test)]
mod tests;
