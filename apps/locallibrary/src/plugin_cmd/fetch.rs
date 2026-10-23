//! `art fetch` and `track fetch`: what the plugins' providers find for a drive's tracks, cover art
//! or tags, each track's candidate picked for the user when one alone scores the
//! `providers.auto_pick` setting or more, the others asked about (or given with `--choose`), and
//! the edits planned, shown and asked about as any are.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use locallibrary::plugin::{PlanProvided, Provide, plan_provided};
use locallibrary::progress::Ctx;
use loclib_plugin::ProviderKind;

use super::choose;
use crate::jobs_cmd::usage;
use crate::output::{self, Output, heading};
use crate::plugins::{self, CHOOSE};

/// What `art fetch` and `track fetch` take.
#[derive(Args)]
pub struct FetchArgs {
    /// A library's name, or a drive's mount path, name or id prefix [default: the only mounted
    /// rekordbox drive, or the one library plugged in]
    drive: Option<String>,
    /// Only this plugin's providers, or this provider (PLUGIN or PLUGIN.PROVIDER)
    #[arg(long, value_name = "PROVIDER")]
    from: Option<String>,
    /// Also for tracks that have it, to replace it [default: only for tracks without]
    #[arg(long)]
    replace: bool,
    #[command(flatten)]
    select: crate::SelectArgs,
    /// Choose for a track (by id) the candidate numbered N, or none with null: asked in a terminal
    /// otherwise
    #[arg(long, value_name = "TRACK=N")]
    choose: Vec<String>,
    /// Show the plan without changing anything
    #[arg(long)]
    dry_run: bool,
}

/// Finds `kind` for the tracks `f` selects [default: every one], and plans what's chosen.
pub fn fetch(
    kind: ProviderKind,
    f: FetchArgs,
    store_path: &Option<PathBuf>,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let plugins = plugins::host()?;
    let drive = crate::jobs_cmd::drive_or_default(f.drive, store_path, "fetch")?;
    let store = crate::diff_cmd::existing_store(store_path)?;
    let req = Provide { kind, drive: drive.clone(), tracks: f.select.selector(), from: f.from, replace: f.replace };
    let provided = plugins.provide(&req, store.as_ref(), ctx)?;
    let mut given = choose::given(&f.choose)?;
    // Before asking anything.
    if let Some(track) = given.keys().find(|t| !provided.offers.iter().any(|o| o.track.to_string() == **t)) {
        return Err(usage(format!("--{CHOOSE} {track}=...: nothing was found for track #{track} to choose from")));
    }
    if !out.json {
        provided.warnings.iter().for_each(output::warn);
    }
    let (mut picks, mut unanswered) = (Vec::new(), Vec::new());
    for o in &provided.offers {
        let c = o.choice();
        let pick = match given.shift_remove(&c.param) {
            Some(v) if c.items.iter().any(|i| i.value == v) => o.chosen(&v),
            Some(v) => return Err(usage(format!("{v} isn't an answer: {}", choose::unanswered(&[&c])))),
            None if o.picked.is_some() => o.pick(),
            None if choose::interactive(out) => o.chosen(&choose::ask(&c, out)?.concat()[0]),
            None => {
                unanswered.push(c);
                continue;
            }
        };
        picks.extend(pick);
    }
    if !unanswered.is_empty() {
        return Err(usage(choose::unanswered(&unanswered.iter().collect::<Vec<_>>())));
    }
    let found = provided.offers.len();
    let (images, replace) = (provided.images, f.replace);
    let plan = plan_provided(&PlanProvided { kind, drive: drive.clone(), picks, images, replace }, ctx)?;
    if !out.json {
        say!("{} found for {found} of {} track(s)", kind.found(), provided.asked);
        heading(&plan.summary[0]);
    }
    crate::edit_cmd::confirm_and_apply(&drive, plan, f.dry_run, out, ctx)
}
