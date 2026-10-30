//! Publishing to the Atmosphere account: `settings publish`, `settings import --from-handle` /
//! `--from-account`, `profile publish` and `sets publish`.
//!
//! Each shows exactly what will be published (or imported), asks first, and then publishes (or
//! imports) exactly that, with the exit statuses every other write has. A session signed in
//! without permission to publish signs in again, asking for it.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};
use console::style;
use locallibrary::Store;
use locallibrary::api::{self, PlannedPublish, PlannedUpload, PublishPlan};
use locallibrary::atmosphere::AccountInfo;
use locallibrary::local_time;
use locallibrary::progress::Ctx;
use locallibrary::publish;
use locallibrary::publish::recording::Encoding;
use locallibrary::settings;
use serde::Serialize;
use serde_json::{Value, json};

use crate::browser::BrowserArgs;
use crate::output::{Outcome, Output, dim, envelope, heading, ok, plural, print_json, warn};

fn root(store: &Option<PathBuf>) -> PathBuf {
    store.clone().unwrap_or_else(Store::default_root)
}

/// How a publish command runs.
#[derive(Clone, Copy)]
struct Run<'a> {
    root: &'a Path,
    dry_run: bool,
    /// How signing in again goes, when the session may not publish yet.
    browser: BrowserArgs,
    out: Output,
    ctx: &'a Ctx<'a>,
}

/// What publishing a record came to, its human output already printed: the outcome, and the
/// `--json` envelope saying so, which another command's can hold.
pub struct Done {
    pub outcome: Outcome,
    pub envelope: Value,
}

impl Done {
    /// `--json` prints the envelope. Returns the exit status.
    pub fn print(self, out: Output) -> ExitCode {
        if out.json {
            print_json(self.envelope);
        }
        self.outcome.exit()
    }
}

/// Nothing published (or imported): the plan, not applied, and the outcome.
fn stop(plan: &impl Serialize, outcome: Outcome) -> Done {
    Done { outcome, envelope: envelope(plan, &Value::Null, false, outcome) }
}

/// Nothing to publish: what's published is the same.
fn unchanged(out: Output, p: &PlannedPublish, what: &str) -> Done {
    if !out.json {
        ok(format!("{what} already published, unchanged"));
    }
    stop(p, Outcome::NothingToDo)
}

/// The plan, then (once confirmed, and signed in again if the session may not publish) exactly
/// that record published, to the account the confirmation names.
fn publish_flow(
    Run { root, dry_run, browser, out, ctx }: Run<'_>,
    what: &str,
    plan: impl Fn() -> locallibrary::Result<PlannedPublish>,
    show: impl Fn(&Value, Option<&Value>),
) -> Result<Done> {
    let mut p = plan()?;
    if !out.json {
        render_plan(&p, what, &show);
    }
    if dry_run {
        return Ok(stop(&p, Outcome::DryRun));
    }
    if out.json && !out.yes {
        return Ok(stop(&p, Outcome::NotConfirmed));
    }
    let Some(account) = p.account.clone() else {
        bail!("not signed in to an Atmosphere account: `locallibrary account login <handle> --publish` signs in");
    };
    // Already published as it is: no permission needed to leave it.
    if p.noop {
        return Ok(unchanged(out, &p, what));
    }
    // In its last days, signing in again is offered first.
    if account.due_soon() {
        warn(format!("{}: {}", account.name(), crate::account_cmd::again(&account)));
        if !out.yes && std::io::stdin().is_terminal() && out.confirm("Sign in again now?") {
            crate::account_cmd::sign_in_again(root, &account, &[], "publishing", browser, out, ctx)?;
            p = plan()?;
        }
    }
    if p.missing_scope.is_some() {
        let signed_in = match sign_in_to_publish(Run { root, dry_run, browser, out, ctx }, &account)? {
            Ok(signed_in) => signed_in,
            Err(o) => return Ok(stop(&p, o)),
        };
        // Planned again for whoever the browser approved, which needn't be the account before.
        p = plan()?;
        if !out.json {
            render_plan(&p, what, &show);
        }
        if p.missing_scope.is_some() {
            bail!("{} didn't allow publishing (it wasn't granted when you signed in)", signed_in.name());
        }
        if p.noop {
            return Ok(unchanged(out, &p, what));
        }
    }
    let account = p.account.as_ref().map_or_else(|| account.name(), |a| a.name());
    if let Some(o) = out.decide(false, &format!("Publish {what} as {account}? Anyone can read it")) {
        return Ok(stop(&p, o));
    }
    let published = p.apply(None, ctx)?;
    if !out.json {
        ok(format!("published {what}"));
        say!("  {}", dim(&published.uri));
    }
    Ok(Done { outcome: Outcome::Applied, envelope: envelope(&p, &published, true, Outcome::Applied) })
}

/// Sign `account` in again allowing publishing, once the DJ says so: the account the browser
/// approved, or the outcome when they don't.
fn sign_in_to_publish(
    Run { root, browser, out, ctx, .. }: Run<'_>,
    account: &AccountInfo,
) -> Result<std::result::Result<AccountInfo, Outcome>> {
    if out.json || !std::io::stdin().is_terminal() {
        bail!(
            "{}'s session doesn't allow publishing yet: sign in again with `locallibrary account login --publish`",
            account.name()
        );
    }
    eprintln!(
        "Publishing needs your permission for locallibrary to write your My Settings, profile, sets and \
         their recordings to your account, and to upload recordings, and nothing else. That means signing in again."
    );
    if let Some(o) = out.decide(false, "Sign in again to allow publishing?") {
        return Ok(Err(o));
    }
    let publishing = locallibrary::atmosphere::publish_scopes();
    Ok(Ok(crate::account_cmd::sign_in_again(root, account, &publishing, "publishing", browser, out, ctx)?))
}

/// A record's time stamp as a day on this computer's calendar.
fn date(v: &Value, field: &str) -> Option<String> {
    let t = chrono::DateTime::parse_from_rfc3339(v[field].as_str()?).ok()?;
    Some(local_time(t, "%Y-%m-%d").to_string())
}

fn render_plan(p: &PublishPlan, what: &str, show: &impl Fn(&Value, Option<&Value>)) {
    match &p.account {
        Some(a) => heading(&format!("Publish {what} as {}", a.name())),
        None => {
            let mut chars = what.chars();
            let what: String = chars.next().into_iter().flat_map(char::to_uppercase).chain(chars).collect();
            heading(&format!("{what}, as it would be published"))
        }
    }
    if let Some(uri) = &p.uri {
        let state = match &p.current {
            None => "not published yet".to_string(),
            Some(_) if !p.changed => "already published, unchanged".to_string(),
            Some(c) => match ["updatedAt", "generatedAt", "createdAt"].iter().find_map(|f| date(&c.value, f)) {
                Some(d) => format!("replaces what you published {d}"),
                None => "replaces what's published there".to_string(),
            },
        };
        say!("  {}", dim(format!("{uri} · {state}")));
    } else {
        crate::output::hint_at("  ", "not signed in: `locallibrary account login <handle> --publish` signs in");
    }
    show(&p.record, p.current.as_ref().map(|c| &c.value));
    for note in &p.notes {
        say!("  {}", dim(note));
    }
    if p.missing_scope.is_some() {
        warn("this session can't publish yet: it was signed in without permission to (publishing signs in again)");
    }
    say!("{}", dim("Anyone can read what you publish, and copies stay on the network even after it's deleted."));
}

// ---------------------------------------------------------------------------------------------
// My Settings
// ---------------------------------------------------------------------------------------------

/// A setting's label and a value's, as rekordbox shows them.
fn describe(key: &str, value: &str) -> (String, String) {
    match settings::find(key) {
        Some(def) => {
            (def.label.to_string(), def.choice(value).map_or_else(|| value.to_string(), |c| c.label.to_string()))
        }
        None => (key.to_string(), value.to_string()),
    }
}

fn group_title(group: &str) -> &'static str {
    match group {
        "player" => "Player",
        "display" => "Display",
        _ => "Mixer",
    }
}

fn render_settings(record: &Value, current: Option<&Value>) {
    for group in ["player", "display", "mixer"] {
        let fields = record[group].as_object();
        let gone: Vec<(&String, &Value)> = current
            .and_then(|c| c[group].as_object())
            .into_iter()
            .flatten()
            .filter(|(f, _)| fields.is_none_or(|m| !m.contains_key(*f)))
            .collect();
        if fields.is_none() && gone.is_empty() {
            continue;
        }
        say!("  {}", style(group_title(group)).bold());
        for (field, value) in fields.into_iter().flatten() {
            let key = publish::snake(field);
            let (label, shown) = describe(&key, value.as_str().unwrap_or_default());
            let note = match current.map(|c| &c[group][field]) {
                Some(Value::Null) => " (new)".to_string(),
                Some(was) if was != value => format!(" (was {})", describe(&key, was.as_str().unwrap_or_default()).1),
                _ => String::new(),
            };
            say!("    {label:<34} {shown}{}", dim(note));
        }
        for (field, was) in gone {
            let (label, shown) = describe(&publish::snake(field), was.as_str().unwrap_or_default());
            say!("    {label:<34} {}", dim(format!("no longer published (was {shown})")));
        }
    }
}

/// `locallibrary settings publish`.
pub fn settings_publish(
    store: &Option<PathBuf>,
    dry_run: bool,
    browser: BrowserArgs,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let root = root(store);
    let done = publish_flow(
        Run { root: &root, dry_run, browser, out, ctx },
        "your My Settings",
        || api::settings_publish_plan(&root),
        render_settings,
    )?;
    Ok(done.print(out))
}

// ---------------------------------------------------------------------------------------------
// The profile
// ---------------------------------------------------------------------------------------------

fn count(v: &Value) -> String {
    match (v["count"].as_i64(), v["atLeast"].as_i64()) {
        (Some(n), _) => n.to_string(),
        (_, Some(n)) => format!("{n}+"),
        _ => "?".into(),
    }
}

fn percent(v: &Value) -> Option<String> {
    v.as_i64().map(|p| format!("{p}%"))
}

/// The `field` of each object in a list, joined.
fn names(v: &Value, field: &str) -> String {
    v.as_array().into_iter().flatten().filter_map(|x| x[field].as_str()).collect::<Vec<_>>().join(", ")
}

/// Each name in a list of shares, with its percent.
fn shares(v: &Value) -> String {
    let shares: Vec<String> = v
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| format!("{} {}%", s["name"].as_str().unwrap_or_default(), s["share"]))
        .collect();
    shares.join(", ")
}

/// One line per section in a profile record: (field, title, what it says).
fn profile_lines(r: &Value) -> Vec<(&'static str, &'static str, String)> {
    let mut lines = Vec::new();
    let mut line = |field: &'static str, title: &'static str, text: Option<String>| {
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            lines.push((field, title, text));
        }
    };
    line("headline", "Headline", r["headline"].as_str().map(String::from));
    line("tags", "Traits", Some(names(&r["tags"], "name")));
    let s = &r["sets"];
    line(
        "sets",
        "Sets",
        s.is_object().then(|| {
            let since = s["since"].as_i64().map(|y| format!(" since {y}")).unwrap_or_default();
            format!("{}{since}, {} tracks in a typical set", count(&s["total"]), s["medianTracks"])
        }),
    );
    let t = &r["tempo"];
    line("tempo", "Tempo", t.is_object().then(|| format!("{}-{} BPM, median {}", t["low"], t["high"], t["median"])));
    let a = &r["arc"];
    line(
        "arc",
        "Set shape",
        a.is_object().then(|| {
            format!(
                "{} (builds {}%, steady {}%, winds down {}%)",
                a["shape"].as_str().unwrap_or_default(),
                a["builds"],
                a["steady"],
                a["windsDown"]
            )
        }),
    );
    let m = &r["mixing"];
    let mixing: Vec<String> =
        [("harmonic", "in key"), ("smoothTempo", "smooth tempo changes"), ("genreChanges", "change genre")]
            .iter()
            .filter_map(|(f, what)| percent(&m[f]).map(|p| format!("{p} {what}")))
            .collect();
    line("mixing", "Mixing", Some(mixing.join(", ")));
    let k = &r["keys"];
    line(
        "keys",
        "Keys",
        k.is_object().then(|| {
            let minor = percent(&k["minor"]).map(|p| format!("; {p} minor")).unwrap_or_default();
            format!("{}{minor}", names(&k["top"], "key"))
        }),
    );
    let e = &r["energy"];
    line(
        "energy",
        "Energy",
        e.is_object().then(|| format!("high {}%, mid {}%, low {}%", e["high"], e["mid"], e["low"])),
    );
    line("genres", "Genres", Some(shares(&r["genres"])));
    line("labels", "Labels", Some(shares(&r["labels"])));
    line("artists", "Artists", Some(shares(&r["artists"])));
    let era = &r["era"];
    let era_text = [
        era["medianYear"].as_i64().map(|y| format!("half released by {y}")),
        percent(&era["recent"]).map(|p| format!("{p} new releases")),
    ];
    line("era", "Era", Some(era_text.into_iter().flatten().collect::<Vec<_>>().join(", ")));
    let signature: Vec<String> = r["signature"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| match t["artist"].as_str() {
            Some(a) => format!("{a} - {}", t["title"].as_str().unwrap_or_default()),
            None => t["title"].as_str().unwrap_or_default().to_string(),
        })
        .collect();
    line("signature", "Signature tracks", Some(signature.join("; ")));
    let p = &r["prep"];
    line(
        "prep",
        "Prep",
        p.is_object()
            .then(|| format!("hot cues {}%, memory cues {}%, loops {}%", p["hotCues"], p["memoryCues"], p["loops"])),
    );
    let c = &r["collection"];
    line(
        "collection",
        "Collection",
        c.is_object().then(|| format!("{} tracks, {} playlists", count(&c["tracks"]), count(&c["playlists"]))),
    );
    let style: Vec<String> = r["style"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(f, v)| {
            let (label, shown) = describe(&publish::snake(f), v.as_str().unwrap_or_default());
            format!("{label} {shown}")
        })
        .collect();
    line("style", "Player style", Some(style.join(", ")));
    lines
}

fn render_profile(record: &Value, current: Option<&Value>) {
    let lines = profile_lines(record);
    for (field, title, text) in &lines {
        let note = match current.map(|c| &c[*field]) {
            Some(Value::Null) => " (new)",
            Some(was) if *was != record[*field] => " (changed)",
            _ => "",
        };
        say!("  {title:<17} {text}{}", dim(note));
    }
    if let Some(current) = current {
        for (field, title, _) in profile_lines(current) {
            if !lines.iter().any(|(f, _, _)| *f == field) {
                say!("  {title:<17} {}", dim("no longer published"));
            }
        }
    }
    crate::output::hint_at(
        "  ",
        format!(
            "{} numbers · `locallibrary profile sections` shows what each section reveals",
            record["precision"].as_str().unwrap_or("rounded")
        ),
    );
}

/// `locallibrary profile publish`.
pub fn profile_publish(
    store: &Option<PathBuf>,
    req: &api::ProfileRequest,
    dry_run: bool,
    browser: BrowserArgs,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let root = root(store);
    let done = publish_flow(
        Run { root: &root, dry_run, browser, out, ctx },
        "your profile",
        || api::profile_publish_plan(&root, req, ctx),
        render_profile,
    )?;
    Ok(done.print(out))
}

// ---------------------------------------------------------------------------------------------
// Sets
// ---------------------------------------------------------------------------------------------

fn set_track(t: &Value) -> String {
    let start = t["startSeconds"].as_u64().map(|s| format!("{}  ", crate::output::clock(s as f64))).unwrap_or_default();
    if t["unidentified"] == true {
        return format!("{start}ID - ID");
    }
    let title = t["title"].as_str().unwrap_or_default();
    let mut s = match t["artist"].as_str() {
        Some(a) => format!("{start}{a} - {title}"),
        None => format!("{start}{title}"),
    };
    let bpm = t["bpmCentis"].as_i64().map(|c| format!("{:.2} BPM", c as f64 / 100.0));
    let detail: Vec<String> = [bpm, t["key"].as_str().map(String::from)].into_iter().flatten().collect();
    if !detail.is_empty() {
        s.push_str(&format!("  {}", dim(detail.join(" · "))));
    }
    s
}

/// A set record's fields, as lines: (field, title, what it says).
fn set_lines(r: &Value) -> Vec<(&'static str, &'static str, String)> {
    let text = |f: &str| r[f].as_str().map(String::from);
    let place = &r["location"];
    let place = [&place["locality"], &place["country"]].into_iter().filter_map(Value::as_str).collect::<Vec<_>>();
    let with: Vec<String> = r["playedWith"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            let name = p["name"].as_str().unwrap_or_default();
            p["did"].as_str().map_or_else(|| name.to_string(), |did| format!("{name} ({did})"))
        })
        .collect();
    let links: Vec<String> = r["links"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| {
            let uri = l["uri"].as_str()?;
            Some(l["name"].as_str().map_or_else(|| uri.to_string(), |name| format!("{name} ({uri})")))
        })
        .collect();
    [
        ("title", "Title", text("title")),
        ("date", "Date", text("date")),
        ("kind", "Kind", text("kind")),
        ("venue", "Venue", text("venue")),
        ("location", "Where", Some(place.join(", "))),
        ("playedWith", "With", Some(with.join(", "))),
        ("event", "Event", text("event")),
        ("links", "Listen", Some(links.join(", "))),
        ("recording", "Audio", r["recording"]["uri"].as_str().map(String::from)),
        ("sources", "From", Some(names(&r["sources"], "kind"))),
    ]
    .into_iter()
    .filter_map(|(field, title, line)| Some((field, title, line.filter(|l| !l.is_empty())?)))
    .collect()
}

fn render_set(record: &Value, current: Option<&Value>) {
    let lines = set_lines(record);
    let changed = |f: &str| current.is_some_and(|c| c[f] != record[f]);
    for (field, title, text) in &lines {
        let note = if changed(field) { " (changed)" } else { "" };
        say!("  {title:<7} {text}{}", dim(note));
    }
    for (field, title, _) in current.map(set_lines).unwrap_or_default() {
        if !lines.iter().any(|(f, _, _)| *f == field) {
            say!("  {title:<7} {}", dim("no longer published"));
        }
    }
    let tracks = record["tracks"].as_array().cloned().unwrap_or_default();
    let note = if changed("tracks") { " (changed)" } else { "" };
    say!("  {}{}", style(crate::output::plural(tracks.len(), "track")).bold(), dim(note));
    for (i, t) in tracks.iter().enumerate() {
        say!("  {:>3}. {}", i + 1, set_track(t));
    }
}

/// `locallibrary sets publish SET`, and the record `sets publish SET --to PLUGIN:DESTINATION`
/// offers once the plugin has published the set. With `lookup`, MusicBrainz IDs are looked up
/// first for the tracks it names (kept in the set, whether it's published or not), when it can be
/// published from here: not when it's only shown, no one is asked to confirm it (without `--yes`,
/// or a terminal), or no account is signed in (which the plan says first). What the lookup did is
/// among the plan's notes.
pub fn set_publish(
    store: &Option<PathBuf>,
    req: &api::SetPublishRequest,
    dry_run: bool,
    lookup: bool,
    browser: BrowserArgs,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<Done> {
    let root = root(store);
    let confirmable = out.yes || (!out.json && std::io::stdin().is_terminal());
    let notes = match lookup && !dry_run && confirmable && locallibrary::atmosphere::status(&root)?.is_some() {
        true => lookup_notes(&crate::plugins::roles::set_lookup(&root, req, ctx)?),
        false => Vec::new(),
    };
    publish_flow(
        Run { root: &root, dry_run, browser, out, ctx },
        "this set",
        || {
            let mut p = api::set_publish_plan(&root, req)?;
            p.plan.notes.extend(notes.iter().cloned());
            Ok(p)
        },
        render_set,
    )
}

/// What looking up a set's MusicBrainz IDs did, as notes of its plan.
fn lookup_notes(looked: &api::SetLookup) -> Vec<String> {
    let found = (looked.found > 0).then(|| format!("found MusicBrainz IDs for {}", plural(looked.found, "track")));
    let unmounted = (looked.unmounted > 0)
        .then(|| format!("plug in the drive to look up MusicBrainz IDs for {}", plural(looked.unmounted, "track")));
    found.into_iter().chain(unmounted).collect()
}

fn render_upload(p: &PlannedUpload) {
    match &p.account {
        Some(a) => heading(&format!("Upload this set's recording as {}", a.name())),
        None => heading("This set's recording, as it would be uploaded"),
    }
    if let Some(uri) = &p.uri {
        let state = match p.current.as_ref().and_then(|c| date(&c.value, "createdAt")) {
            None if p.current.is_none() => "not uploaded yet".to_string(),
            None => "replaces the recording there".to_string(),
            Some(d) => format!("replaces the recording you uploaded {d}"),
        };
        say!("  {}", dim(format!("{uri} · {state}")));
    } else {
        crate::output::hint_at("  ", "not signed in: `locallibrary account login <handle> --publish` signs in");
    }
    let a = &p.audio;
    say!(
        "  {:<9} {} {}",
        "Recording",
        a.path.display(),
        dim(format!(
            "· {} · {} · {}",
            a.format,
            crate::output::bytes(a.bytes),
            crate::output::clock(a.duration_secs as f64)
        ))
    );
    let how = match p.encoding {
        Encoding::AsIs => "as it is".to_string(),
        Encoding::Aac { kbps, bytes } => {
            format!("as AAC at {kbps} kbit/s in an M4A file, about {} (ffmpeg encodes it)", crate::output::bytes(bytes))
        }
    };
    say!("  {:<9} {how}", "Goes up");
    if !p.chapters.is_empty() {
        say!("  {:<9} {}", "Chapters", crate::output::plural(p.chapters.len(), "track"));
    }
    for note in &p.notes {
        say!("  {}", dim(note));
    }
    if p.missing_scope.is_some() {
        warn("this session can't upload yet: it was signed in without permission to (uploading signs in again)");
    }
    say!("  {}", dim("then this set's record, pointing at it"));
    say!("{}", dim("Anyone can read what you publish, and copies stay on the network even after it's deleted."));
}

/// `locallibrary sets publish SET --upload`: the set's recording uploaded to the account (encoded
/// first when it has to be) and published as a recording record, then the set's record pointing at
/// it, published as `sets publish SET` publishes it. One question covers both. A set record that
/// fails to publish says so (exit status 5), with the recording up.
#[allow(clippy::too_many_arguments)]
pub fn set_upload(
    store: &Option<PathBuf>,
    req: &api::SetPublishRequest,
    max_bytes: Option<u64>,
    dry_run: bool,
    lookup: bool,
    browser: BrowserArgs,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let root = root(store);
    let upload = api::SetUploadRequest { set: req.set.clone(), max_bytes, hide: req.hide.clone() };
    let mut p = api::set_upload_plan(&root, &upload)?;
    if !out.json {
        render_upload(&p);
    }
    if dry_run || (out.json && !out.yes) {
        return Ok(out.not_applied(&p, if dry_run { Outcome::DryRun } else { Outcome::NotConfirmed }));
    }
    let Some(account) = p.account.clone() else {
        bail!("not signed in to an Atmosphere account: `locallibrary account login <handle> --publish` signs in");
    };
    if p.missing_scope.is_some() {
        let signed_in = match sign_in_to_publish(Run { root: &root, dry_run, browser, out, ctx }, &account)? {
            Ok(signed_in) => signed_in,
            Err(o) => return Ok(out.not_applied(&p, o)),
        };
        p = api::set_upload_plan(&root, &upload)?;
        if !out.json {
            render_upload(&p);
        }
        if p.missing_scope.is_some() {
            bail!("{} didn't allow publishing (it wasn't granted when you signed in)", signed_in.name());
        }
    }
    let account = p.account.as_ref().map_or_else(|| account.name(), |a| a.name());
    let question = format!("Upload the recording and publish this set as {account}? Anyone can read them");
    if let Some(o) = out.decide(false, &question) {
        return Ok(out.not_applied(&p, o));
    }
    let uploaded = p.apply(None, ctx)?;
    if !out.json {
        ok("uploaded the recording");
        say!("  {}", dim(&uploaded.uri));
    }
    let mut done = envelope(&p, &uploaded, true, Outcome::Applied);
    // Already asked about: the set's record goes out without asking again.
    let outcome = match set_publish(store, req, false, lookup, browser, Output { yes: true, ..out }, ctx) {
        Ok(record) => {
            done["record"] = record.envelope;
            Outcome::Applied
        }
        Err(e) => {
            let e = format!("{e:#}");
            if !out.json {
                warn(format!("the set's record wasn't published: {e}"));
                crate::output::hint(format!("`locallibrary sets publish {}` publishes it later", p.set));
            }
            done["record"] = json!({ "error": { "message": e }, "outcome": "error" });
            Outcome::Partial
        }
    };
    if out.json {
        print_json(done);
    }
    Ok(outcome.exit())
}

/// `locallibrary settings import --from-handle HANDLE` / `--from-account`.
pub fn settings_import(
    store: &Option<PathBuf>,
    req: &api::ImportPublishedSettings,
    dry_run: bool,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let root = root(store);
    let plan = api::settings_import_published_plan(&root, req)?;
    if !out.json {
        let when = date(&json!({ "t": plan.published.updated_at }), "t");
        heading(&format!(
            "My Settings published by {}{}",
            plan.from.name(),
            when.map(|d| format!(" ({d})")).unwrap_or_default()
        ));
        say!("  {}", dim(&plan.uri));
        if plan.changes.is_empty() {
            say!("  {}", dim("the same as your settings profile"));
        }
        let shown = |key: &str, v: &Option<String>| match v {
            Some(v) => describe(key, v).1,
            None => "(not set)".to_string(),
        };
        for c in &plan.changes {
            let (label, _) = describe(&c.key, "");
            say!("    {label:<34} {} → {}", shown(&c.key, &c.from), shown(&c.key, &c.to));
        }
        for w in &plan.warnings {
            warn(w);
        }
        say!(
            "{}",
            dim(if plan.replace {
                "Importing replaces your settings profile's base; per-drive overrides stay."
            } else {
                "Importing adds these to your settings profile's base (--replace replaces it); per-drive overrides stay."
            })
        );
    }
    if plan.noop {
        return Ok(out.not_applied(&plan, Outcome::NothingToDo));
    }
    if let Some(o) = out.decide(dry_run, "Import these settings?") {
        return Ok(out.not_applied(&plan, o));
    }
    let profile = plan.apply(None, ctx)?;
    let (count, from) = (plan.published.values.len(), plan.from.name());
    Ok(out.applied(&plan, &profile, Outcome::Applied, || {
        ok(format!("imported {count} settings from {from}"));
        crate::output::hint("Run `locallibrary settings apply` to write the profile to your drives.");
    }))
}

#[cfg(test)]
mod tests;
