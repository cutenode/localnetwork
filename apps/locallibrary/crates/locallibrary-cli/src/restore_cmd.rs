//! `locallibrary undo` and `restore`: put a drive's backup back; `restore DRIVE --playlist P`:
//! bring back one playlist or folder from a snapshot or a backup.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use locallibrary::api::{self, PlaylistRestorePreview, RestoreSource};
use locallibrary::progress::Ctx;

use crate::edit_cmd::{render_plan, render_report};
use crate::output::{self, Outcome, Output, dim, heading, warn};

pub fn playlist(
    req: api::PlaylistRestoreRequest,
    store: &Option<PathBuf>,
    dry_run: bool,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    // Only an existing store is consulted.
    let store = crate::diff_cmd::existing_store(store);
    let preview = api::playlist_restore_plan(store.as_ref(), &req, ctx)?;
    if !out.json {
        render(&preview);
    }
    if preview.noop {
        let message = format!("{} on {} is already as it was", preview.path, req.drive);
        return Ok(out.nothing(&preview, message));
    }
    if !out.json {
        render_plan(&preview.plan.plan);
        output::clones(&preview);
    }
    if let Some(o) = out.decide(dry_run, &format!("Restore {}?", preview.path)) {
        return Ok(out.not_applied(&preview, o));
    }
    let report = preview.apply(store.as_ref(), ctx)?;
    Ok(out.applied(&preview, &report, Outcome::Applied, || render_report(&report, &req.drive)))
}

fn render(p: &PlaylistRestorePreview) {
    let what = if p.folder { format!("folder {} ({} playlists)", p.path, p.playlists) } else { p.path.clone() };
    let from = match &p.source {
        RestoreSource::Snapshot { snapshot, library } => format!(
            "snapshot {} of {} ({})",
            snapshot.short_id,
            library.as_deref().unwrap_or("an unknown library"),
            snapshot.created.format("%Y-%m-%d %H:%M")
        ),
        RestoreSource::Backup { backup } => {
            format!("backup {} ({}, {})", backup.id, backup.reason, backup.created.format("%Y-%m-%d %H:%M"))
        }
    };
    heading(&format!("Restoring {what} from {from}"));
    if !p.missing.is_empty() {
        warn(format!("{} entr(ies) left out: their tracks aren't on the drive any more", p.missing.len()));
        for m in p.missing.iter().take(10) {
            say!("    {}", dim(format!("{} #{}: {}", m.playlist, m.position, m.track)));
        }
        if p.missing.len() > 10 {
            say!("    {}", dim(format!("and {} more", p.missing.len() - 10)));
        }
    }
    if !p.untouched.is_empty() {
        say!(
            "{}",
            dim(format!("Left as they are (not in the {}): {}", source_kind(&p.source), p.untouched.join(", ")))
        );
    }
}

fn source_kind(s: &RestoreSource) -> &'static str {
    match s {
        RestoreSource::Snapshot { .. } => "snapshot",
        RestoreSource::Backup { .. } => "backup",
    }
}

/// Show what putting a backup back changes, ask, and put it back.
pub fn undo(req: api::UndoRequest, dry_run: bool, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let plan = api::undo_plan(&req)?;
    if !out.json {
        render_undo(&plan);
        output::clones(&plan);
    }
    if plan.noop {
        return Ok(out.not_applied(&plan, Outcome::NothingToDo));
    }
    if let Some(o) = out.decide(dry_run, &format!("Put backup {} back on {}?", plan.backup.id, plan.drive)) {
        return Ok(out.not_applied(&plan, o));
    }
    let b = plan.apply(None, ctx)?;
    Ok(out.applied(&plan, &b, Outcome::Applied, || output::ok(format!("put back backup {} ({})", b.id, b.reason))))
}

fn render_undo(p: &api::UndoPlan) {
    heading(&format!(
        "{}: backup {} ({}, {})",
        p.drive,
        p.backup.id,
        p.backup.reason,
        p.backup.created.format("%Y-%m-%d %H:%M")
    ));
    if p.files.is_empty() {
        say!("  {}", dim("no file differs from the backup"));
    }
    for f in p.files.iter().take(20) {
        say!("  {:<9} {} {}", f.change, f.path, dim(output::bytes(f.bytes)));
    }
    if p.files.len() > 20 {
        say!("  {}", dim(format!("and {} more", p.files.len() - 20)));
    }
    if !p.diff.is_empty() {
        crate::diff_cmd::render(&p.diff);
    }
    if !p.journal.is_empty() {
        say!("Recorded edits taken off the journal (`edits replay` won't make them again):");
        for e in &p.journal {
            for c in &e.changes {
                say!("  - {c} {}", dim(e.at.format("%Y-%m-%d %H:%M")));
            }
        }
    }
    if !p.history_lists.is_empty() {
        warn(format!("history lists players recorded since are dropped: {}", p.history_lists.join(", ")));
    }
    for m in p.missing_audio.iter().take(5) {
        warn(format!("no audio on the drive for {m}"));
    }
}

/// What `undo` can take back, per drive.
pub fn undo_list(l: &api::UndoList) {
    if l.drives.is_empty() && l.embeds.is_empty() {
        say!("Nothing to undo on the mounted drives.");
    }
    for d in &l.drives {
        heading(&format!("{} ({})", d.drive, d.root.display()));
        if d.backups.is_empty() {
            say!("  {}", dim("no backups"));
        }
        for b in &d.backups {
            let next = if b.next { " <- undo" } else { "" };
            let edits = if b.journal > 0 { format!(", {} recorded edit(s)", b.journal) } else { String::new() };
            say!(
                "  {}  {}  {}{}",
                b.backup.id,
                b.backup.created.format("%Y-%m-%d %H:%M"),
                b.backup.reason,
                dim(format!(" ({}{edits}){next}", crate::drive_cmd::backup_files(&b.backup)))
            );
        }
    }
    if !l.embeds.is_empty() {
        heading("Art embedded in files on this computer (`locallibrary art unembed`)");
        for e in &l.embeds {
            say!("  {}  {} file(s)  {}", e.id, e.files.len(), dim(e.image.display()));
        }
    }
}
