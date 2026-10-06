//! Publishing to the Atmosphere account: `settings publish`, `settings import --from-handle` /
//! `--from-account`, `profile publish` and `history publish`.
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
use locallibrary::api::{self, PlannedPublish, PublishPlan};
use locallibrary::progress::Ctx;
use locallibrary::publish;
use locallibrary::settings;
use serde::Serialize;
use serde_json::{Value, json};

use crate::account_cmd::BrowserArgs;
use crate::output::{Outcome, Output, dim, heading, ok, warn};

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

/// Nothing published (or imported): `--json` prints the plan, not applied, and the outcome.
fn stop(out: Output, plan: &impl Serialize, outcome: Outcome) -> ExitCode {
    out.not_applied(plan, outcome)
}

/// Nothing to publish: what's published is the same.
fn unchanged(out: Output, p: &PlannedPublish, what: &str) -> ExitCode {
    if !out.json {
        ok(format!("{what} already published, unchanged"));
    }
    stop(out, p, Outcome::NothingToDo)
}

/// The plan, then (once confirmed, and signed in again if the session may not publish) exactly
/// that record published, to the account the confirmation names.
fn publish_flow(
    Run { root, dry_run, browser, out, ctx }: Run<'_>,
    what: &str,
    plan: impl Fn() -> locallibrary::Result<PlannedPublish>,
    show: impl Fn(&Value, Option<&Value>),
) -> Result<ExitCode> {
    let mut p = plan()?;
    if !out.json {
        render_plan(&p, what, &show);
    }
    if dry_run {
        return Ok(stop(out, &p, Outcome::DryRun));
    }
    if out.json && !out.yes {
        return Ok(stop(out, &p, Outcome::NotConfirmed));
    }
    let Some(account) = p.account.clone() else {
        bail!("not signed in to an Atmosphere account: `locallibrary account login <handle> --publish` signs in");
    };
    // Already published as it is: no permission needed to leave it.
    if p.noop {
        return Ok(unchanged(out, &p, what));
    }
    if p.missing_scope.is_some() {
        if out.json || !std::io::stdin().is_terminal() {
            bail!(
                "{}'s session doesn't allow publishing yet: sign in again with `locallibrary account login --publish`",
                account.name()
            );
        }
        eprintln!(
            "Publishing needs your permission for locallibrary to write its own records (your My Settings, \
             profile and sets) to your account, and nothing else. That means signing in again."
        );
        if let Some(o) = out.decide(false, "Sign in again to allow publishing?") {
            return Ok(stop(out, &p, o));
        }
        let signed_in = crate::account_cmd::sign_in_to_publish(root, &account, browser, out, ctx)?;
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
        return Ok(stop(out, &p, o));
    }
    let published = p.apply(None, ctx)?;
    let uri = published.uri.clone();
    Ok(out.applied(&p, &published, Outcome::Applied, || {
        ok(format!("published {what}"));
        say!("  {}", dim(uri));
    }))
}

/// A record's time stamp as a day on this computer's calendar.
fn date(v: &Value, field: &str) -> Option<String> {
    let t = chrono::DateTime::parse_from_rfc3339(v[field].as_str()?).ok()?;
    Some(t.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
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
    publish_flow(
        Run { root: &root, dry_run, browser, out, ctx },
        "your My Settings",
        || api::settings_publish_plan(&root),
        render_settings,
    )
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

fn names(v: &Value) -> String {
    v.as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")
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
    line("tags", "Traits", Some(names(&r["tags"])));
    let s = &r["sets"];
    line(
        "sets",
        "Sets",
        s.is_object().then(|| {
            let since = s["since"].as_i64().map(|y| format!(" since {y}")).unwrap_or_default();
            format!("{}{since}, {} tracks in a typical set", count(s), s["medianTracks"])
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
            format!("{}{}", names(&k["top"]), percent(&k["minor"]).map(|p| format!("; {p} minor")).unwrap_or_default())
        }),
    );
    let e = &r["energy"];
    line(
        "energy",
        "Energy",
        e.is_object().then(|| format!("high {}%, mid {}%, low {}%", e["high"], e["mid"], e["low"])),
    );
    let genres: Vec<String> = r["genres"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|g| format!("{} {}%", g["name"].as_str().unwrap_or_default(), g["share"]))
        .collect();
    line("genres", "Genres", Some(genres.join(", ")));
    line("labels", "Labels", Some(names(&r["labels"])));
    line("artists", "Artists", Some(names(&r["artists"])));
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
    publish_flow(
        Run { root: &root, dry_run, browser, out, ctx },
        "your profile",
        || api::profile_publish_plan(&root, req, ctx),
        render_profile,
    )
}

// ---------------------------------------------------------------------------------------------
// Sets
// ---------------------------------------------------------------------------------------------

fn set_track(t: &Value) -> String {
    if t["unidentified"] == true {
        return "ID - ID".to_string();
    }
    let title = t["title"].as_str().unwrap_or_default();
    let mut s = match t["artist"].as_str() {
        Some(a) => format!("{a} - {title}"),
        None => title.to_string(),
    };
    let bpm = t["bpmCentis"].as_i64().map(|c| format!("{:.2} BPM", c as f64 / 100.0));
    let detail: Vec<String> = [bpm, t["key"].as_str().map(String::from)].into_iter().flatten().collect();
    if !detail.is_empty() {
        s.push_str(&format!("  {}", dim(detail.join(" · "))));
    }
    s
}

fn render_set(record: &Value, current: Option<&Value>) {
    let field = |f: &str| record[f].as_str().unwrap_or_default().to_string();
    let changed = |f: &str| current.is_some_and(|c| c[f] != record[f]);
    for (f, title) in [("title", "Title"), ("date", "Date"), ("venue", "Venue")] {
        if record[f].is_string() || changed(f) {
            let note = if changed(f) { dim(" (changed)").to_string() } else { String::new() };
            say!("  {title:<7} {}{note}", if record[f].is_string() { field(f) } else { "(none)".into() });
        }
    }
    let tracks = record["tracks"].as_array().cloned().unwrap_or_default();
    let note = if current.is_some_and(|c| c["tracks"] != record["tracks"]) { " (changed)" } else { "" };
    say!("  {}{}", style(format!("{} tracks", tracks.len())).bold(), dim(note));
    for (i, t) in tracks.iter().enumerate() {
        say!("  {:>3}. {}", i + 1, set_track(t));
    }
}

/// `locallibrary plays publish DRIVE LIST`.
pub fn history_publish(
    store: &Option<PathBuf>,
    req: &api::HistoryPublishRequest,
    dry_run: bool,
    browser: BrowserArgs,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let root = root(store);
    publish_flow(
        Run { root: &root, dry_run, browser, out, ctx },
        "this set",
        || api::history_publish_plan(&root, req, ctx),
        render_set,
    )
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
        return Ok(stop(out, &plan, Outcome::NothingToDo));
    }
    if let Some(o) = out.decide(dry_run, "Import these settings?") {
        return Ok(stop(out, &plan, o));
    }
    let profile = plan.apply(None, ctx)?;
    let (count, from) = (plan.published.values.len(), plan.from.name());
    Ok(out.applied(&plan, &profile, Outcome::Applied, || {
        ok(format!("imported {count} settings from {from}"));
        crate::output::hint("Run `locallibrary settings apply` to write the profile to your drives.");
    }))
}
