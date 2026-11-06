//! The Drives view, with each library's clones, and the Snapshots view.

use locallibrary::DeviceInfo;
use locallibrary::api::{DriveStatus, FleetLibrary};
use locallibrary::compare::Relation;
use locallibrary::fleet::{Role, Standing};
use locallibrary::local_time;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Cell, ListItem, Paragraph, Row, Table, Wrap};
use uuid::Uuid;

use crate::tui::icons;

use crate::output::{bytes, duration, plural};
use crate::tui::app::App;
use crate::tui::drives::DriveRow;
use crate::tui::theme::{self, bold, dim, faint, fg, text};

use super::*;

/// A section's heading in the Drives view's right pane.
fn pane_heading(s: &str) -> Line<'static> {
    Line::styled(s.to_uppercase(), bold(theme::current().info))
}

/// A labelled value in the Drives view's right pane.
fn pane_field(k: &str, v: String) -> Line<'static> {
    Line::from(vec![Span::styled(format!("  {k:<12}"), dim()), Span::styled(v, text())])
}

/// Where a drive stands against the store: a mark and a few words, coloured, and (with `hints`)
/// what to do about it.
pub(super) fn standing(s: &DriveStatus, hints: bool) -> Vec<Span<'static>> {
    let save = Some("e, then save a snapshot");
    let (icon, msg, hint, c) = match (s.relation, &s.error) {
        (_, Some(e)) => (icons::CROSS.get(), format!("error: {e}"), None, theme::current().critical),
        (None, _) if s.library.is_none() => {
            (icons::RING.get(), "not in the store".to_string(), save, theme::current().dim)
        }
        (None, _) => (icons::RING.get(), "library has no snapshots".to_string(), None, theme::current().dim),
        (Some(Relation::Identical), _) => (icons::CHECK.get(), "up to date".to_string(), None, theme::current().good),
        (Some(Relation::RightNewer), _) => match s.back_from_set {
            Some(0) => (icons::AHEAD.get(), "back from a set".to_string(), save, theme::current().warning),
            Some(n) => (
                icons::AHEAD.get(),
                format!("back from a set: {}", plural(n, "new session")),
                save,
                theme::current().warning,
            ),
            None => (icons::AHEAD.get(), "has changes not in the store".to_string(), save, theme::current().warning),
        },
        (Some(Relation::LeftNewer), _) => (
            icons::BEHIND.get(),
            "behind the store".to_string(),
            Some("e, then sync, or locallibrary library push"),
            theme::current().warning,
        ),
        (Some(Relation::Diverged), _) => {
            (icons::FORK.get(), "diverged from the store".to_string(), None, theme::current().critical)
        }
        (Some(other), _) => ("?", format!("{other:?}").to_lowercase(), None, theme::current().dim),
    };
    let msg = match hint.filter(|_| hints) {
        Some(h) => format!("{msg} ({h})"),
        None => msg,
    };
    vec![Span::styled(format!("{icon} "), bold(c)), Span::styled(msg, fg(c))]
}

pub(super) fn draw_drives(f: &mut Frame, app: &mut App, area: Rect) {
    let [list, detail] =
        Layout::horizontal([Constraint::Length(36.min(area.width / 2)), Constraint::Min(0)]).areas(area);
    app.areas.drive_list = list;
    draw_drive_rows(f, app, list);
    match app.drive_header {
        Some(DriveRow::Library(id)) => return draw_library_clones(f, app, id, detail),
        Some(_) => {
            let p = Paragraph::new(vec![
                Line::raw(""),
                Line::styled("These drives aren't in the store yet.", bold(theme::current().text)),
                Line::styled("Select one, then e to save a snapshot: that starts its library.", dim()),
            ])
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(block(title("No library", false), false));
            return f.render_widget(p, detail);
        }
        None => {}
    }
    let Some(d) = app.drive_state.selected().and_then(|i| app.drives.get(i)) else {
        let p = Paragraph::new(vec![
            Line::raw(""),
            Line::styled("No rekordbox drives mounted.", bold(theme::current().text)),
            Line::styled("Drives appear here as soon as they're plugged in.", dim()),
        ])
        .alignment(Alignment::Center)
        .block(block(title("Drive", false), false));
        f.render_widget(p, detail);
        return;
    };
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::styled(drive_name(d), bold(theme::current().accent)));
    let id =
        d.id.map_or("not adopted (locallibrary drive adopt)".to_string(), |id| format!("id {}", &id.to_string()[..8]));
    lines.push(Line::styled(format!("{} · {id}", d.root.display()), faint()));
    lines.push(Line::raw(""));

    if let Some(s) = &d.summary {
        lines.push(pane_heading("Library"));
        lines.push(pane_field("tracks", format!("{}  ·  {} artists", s.tracks, s.artists)));
        lines.push(pane_field("playlists", format!("{} in {} folders", s.playlists, s.folders)));
        lines.push(pane_field("duration", duration(s.total_duration_secs)));
        lines.push(pane_field("audio", bytes(s.audio_bytes)));
        if s.tracks_without_artwork > 0 {
            lines
                .push(pane_field("no artwork", format!("{} tracks (locallibrary art auto)", s.tracks_without_artwork)));
        }
        if let Some(at) = d.exported_at {
            let rev = d.pdb.as_ref().map(|p| format!("  ·  export.pdb rev {}", p.sequence)).unwrap_or_default();
            lines.push(pane_field("exported", format!("{}{rev}", local_time(at, "%Y-%m-%d %H:%M"))));
        }
        if d.has_library_plus {
            lines.push(pane_field("players", "OneLibrary (exportLibrary.db) present".into()));
        }
        lines.push(Line::raw(""));
    } else {
        lines.push(Line::styled("No rekordbox export on this drive.", dim()));
        lines.push(Line::raw(""));
    }

    if let (Some(free), Some(total)) = (d.free_space, d.total_space)
        && total > 0
    {
        lines.push(pane_heading("Space"));
        let used = 1.0 - free as f64 / total as f64;
        let w = detail.width.saturating_sub(6).min(48) as usize;
        let filled = (used * w as f64).round() as usize;
        let mut spans = vec![Span::raw("  ")];
        for i in 0..w {
            let c = theme::gradient(
                &[theme::current().info, theme::current().secondary, theme::current().accent],
                i as f32 / w as f32,
            );
            spans.push(Span::styled(
                if i < filled { "█" } else { "░" },
                fg(if i < filled { c } else { theme::current().faint }),
            ));
        }
        lines.push(Line::from(spans));
        lines.push(Line::styled(
            format!("  {:.0}% used · {} free of {}", used * 100.0, bytes(free), bytes(total)),
            dim(),
        ));
        lines.push(Line::raw(""));
    }

    let fleet = app.fleet_drive(&d.root);
    lines.push(pane_heading("Store"));
    match (&app.status, fleet) {
        (None, _) | (_, None) if app.status_loading => lines.push(Line::styled("  checking…", dim())),
        (Some(Err(e)), _) => lines.push(Line::styled(format!("  {e}"), dim())),
        (_, Some(fd)) => {
            let s = &fd.status;
            lines.push(Line::from([vec![Span::raw("  ")], standing(s, true)].concat()));
            if let Some(lib) = &s.library {
                let group = app
                    .status
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .map_or(0, |list| list.iter().filter(|o| o.status.library.as_ref() == Some(lib)).count());
                let plugged = if group > 1 { format!("  ·  {group} of its drives plugged in") } else { String::new() };
                lines.push(pane_field("library", format!("{lib}{plugged}")));
            }
            if let Some(h) = &s.head {
                lines
                    .push(pane_field("latest", format!("{}  {}", h.short_id, local_time(h.created, "%Y-%m-%d %H:%M"))));
            }
            if s.settings_drift > 0 {
                lines.push(Line::styled(
                    format!("  {} setting(s) differ from the profile (locallibrary settings apply)", s.settings_drift),
                    fg(theme::current().warning),
                ));
            }
        }
        _ => lines.push(Line::styled("  press R to check", dim())),
    }
    lines.push(Line::raw(""));

    // What the registry knows: its role, its logbook, copies of it.
    if let Some(fd) = fleet {
        let mut warn = Vec::new();
        if fd.read_only {
            warn.push("write-protected: players can't save history or cues to it".to_string());
        }
        if fd.low_space {
            warn.push("nearly full: players need room for history and cues".to_string());
        }
        for other in &fd.same_identity_as {
            warn.push(format!(
                "a copy of it (or it of) {}: locallibrary drive reidentify gives one its own",
                other.display()
            ));
        }
        let role = fd.role.map(crate::tui::drives::role_name);
        if role.is_some() || fd.log.is_some() || !warn.is_empty() {
            lines.push(pane_heading("Logbook"));
            if let Some(r) = role {
                lines.push(pane_field("role", r));
            }
            if let Some(log) = &fd.log {
                if log.writes > 0 {
                    lines.push(pane_field("written", format!("{} times, {}", log.writes, bytes(log.bytes_written))));
                }
                if log.clean_ejects > 0 {
                    lines.push(pane_field("ejected", format!("{} times cleanly", log.clean_ejects)));
                }
                for (what, c) in [("verified", &log.last_scrub), ("fs check", &log.last_fs_check)] {
                    if let Some(c) = c {
                        let mark = if c.ok { icons::CHECK.get() } else { icons::CROSS.get() };
                        lines.push(pane_field(what, format!("{mark} {}  {}", local_time(c.at, "%Y-%m-%d"), c.summary)));
                    }
                }
                if let Some(p) = log.problems.last() {
                    lines.push(Line::styled(
                        format!(
                            "  {} {} {}: {}",
                            icons::CROSS.get(),
                            local_time(p.at, "%Y-%m-%d"),
                            p.kind.replace('_', " "),
                            p.detail
                        ),
                        fg(theme::current().critical),
                    ));
                }
            }
            for w in warn {
                lines.push(Line::styled(format!("  {} {w}", icons::WARNING.get()), fg(theme::current().warning)));
            }
            lines.push(Line::raw(""));
        }
    }

    if !d.settings_files.is_empty() {
        lines.push(pane_heading("My Settings"));
        lines.push(Line::styled(format!("  {}", d.settings_files.join(" · ")), text()));
        lines.push(Line::raw(""));
    }
    if d.lineage_len > 0 {
        lines.push(pane_heading("States"));
        let last = d
            .last_known
            .as_ref()
            .map(|e| format!(" · last: {:?} {}", e.op, local_time(e.at, "%Y-%m-%d %H:%M")).to_lowercase())
            .unwrap_or_default();
        let states = if d.lineage_len == 1 { "state" } else { "states" };
        lines.push(Line::styled(format!("  {} {states} recorded{last}", d.lineage_len), text()));
        lines.push(Line::raw(""));
    }
    if let Some(p) = &d.pending {
        lines.push(Line::styled(
            format!(
                "{} interrupted: {} ({}), re-run it to finish",
                icons::WARNING.get(),
                p.description,
                local_time(p.started, "%Y-%m-%d %H:%M")
            ),
            bold(theme::current().warning),
        ));
        lines.push(Line::raw(""));
    }
    lines.push(pane_heading("Actions"));
    // `u` undoes on the drive changed last this run, which may be another.
    let undo = app.undo_drive().map(|r| format!("undo the last change to {}", app.drive_label(&r))).unwrap_or_default();
    for (k, what) in [
        ("e", "save a snapshot · check · sync · pack · bring back a playlist · replay edits · eject"),
        ("u", undo.as_str()),
        ("enter", "browse it in the library"),
    ] {
        lines.push(Line::from(vec![Span::styled(format!("  {k:<6}"), theme::key()), Span::styled(what, text())]));
    }

    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block(title("Drive", false), false).padding(ratatui::widgets::Padding::horizontal(1)));
    f.render_widget(p, detail);
}

/// The Drives view's list: each library (how many of its clones are plugged in, how they stand),
/// its drives under it, then the drives of no library. Each row takes two lines.
fn draw_drive_rows(f: &mut Frame, app: &mut App, area: Rect) {
    let rows = app.drive_rows();
    let loaded_root = app.loaded.as_ref().map(|l| l.root().to_path_buf());
    let inner_w = area.width.saturating_sub(3);
    // Drives are indented under the libraries, when there are any.
    let indent = if rows.iter().any(|r| matches!(r, DriveRow::Library(_))) { "  " } else { "" };
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| {
            let lines = match *row {
                DriveRow::Library(id) => {
                    let name = app.library_names.get(&id).cloned().unwrap_or_default();
                    let plugged = app.drives.iter().filter(|d| app.library_id(d) == Some(id)).count();
                    let fleet = app.fleet_libraries.iter().find(|l| l.id == id);
                    let count = match fleet {
                        Some(l) => format!("{plugged} of {}", plugged + l.offline.len()),
                        None => plugged.to_string(),
                    };
                    let left = vec![Span::styled(name, bold(theme::current().text))];
                    let about = match fleet {
                        Some(l) => library_state(l),
                        None if app.status_loading => vec![Span::styled("checking…", faint())],
                        None => Vec::new(),
                    };
                    vec![
                        spread(left, vec![Span::styled(count, dim())], inner_w),
                        Line::from([vec![Span::raw("  ")], about].concat()),
                    ]
                }
                DriveRow::Unlinked => {
                    vec![
                        Line::styled("No library", bold(theme::current().dim)),
                        Line::styled("  not in the store yet", faint()),
                    ]
                }
                DriveRow::Drive(i) => {
                    let d = &app.drives[i];
                    let open = loaded_root.as_deref() == Some(d.root.as_path());
                    let root = Line::styled(format!("{indent}  {}", d.root.display()), faint());
                    vec![drive_line(d, open, indent, inner_w), root]
                }
            };
            ListItem::new(Text::from(lines))
        })
        .collect();
    app.drive_rows.select(app.drive_row(&rows));
    // Each row takes two lines.
    app.drive_rows.fit(area.height.saturating_sub(2) as usize / 2, rows.len());
    f.render_stateful_widget(drive_list(items, true), area, &mut app.drive_rows);
}

/// How a library's clones plugged in stand together: whether they hold the same, and whether
/// the store has what they hold.
fn library_state(l: &FleetLibrary) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut say = |s: &'static str, c| {
        if !out.is_empty() {
            out.push(Span::styled(" · ", faint()));
        }
        out.push(Span::styled(s, fg(c)));
    };
    match l.identical {
        Some(true) => say("identical", theme::current().good),
        Some(false) => say("not identical", theme::current().warning),
        None => {}
    }
    match (&l.head, l.unsaved) {
        (None, _) => say("no snapshots yet", theme::current().dim),
        (Some(_), true) => say("unsaved", theme::current().warning),
        (Some(_), false) => say("saved", theme::current().dim),
    }
    out
}

/// The Drives view's right pane for a library: its clones (those not plugged in too) and where
/// each stands, the store's latest snapshot of it, and what can be done to all of them.
fn draw_library_clones(f: &mut Frame, app: &App, id: Uuid, area: Rect) {
    let name = app.library_names.get(&id).cloned().unwrap_or_default();
    let fleet = app.fleet_libraries.iter().find(|l| l.id == id);
    let here: Vec<&DeviceInfo> = app.drives.iter().filter(|d| app.library_id(d) == Some(id)).collect();
    let away = fleet.map_or(&[][..], |l| l.offline.as_slice());
    let mut lines = vec![
        Line::styled(name, bold(theme::current().accent)),
        Line::styled(format!("{} of {} plugged in", here.len(), plural(here.len() + away.len(), "clone")), faint()),
        Line::raw(""),
        pane_heading("Clones"),
    ];
    let name_w = here
        .iter()
        .map(|d| drive_name(d).chars().count())
        .chain(away.iter().map(|k| k.name.chars().count()))
        .max()
        .unwrap_or(0);
    let clone = |dot: Span<'static>, name: String, role: Option<Role>, state: Vec<Span<'static>>| {
        let role = format!("{:<9}", role.map_or("", Role::name));
        Line::from(
            [
                vec![
                    Span::raw("  "),
                    dot,
                    Span::styled(format!("{name:<name_w$}  "), text()),
                    Span::styled(role, dim()),
                ],
                state,
            ]
            .concat(),
        )
    };
    for d in &here {
        let fd = app.fleet_drive(&d.root);
        let state = match fd {
            Some(fd) => standing(&fd.status, false),
            None if app.status_loading => vec![Span::styled("checking…", dim())],
            None => Vec::new(),
        };
        lines.push(clone(
            Span::styled(icons::DOT.spaced(), fg(theme::current().good)),
            drive_name(d),
            fd.and_then(|f| f.role),
            state,
        ));
    }
    for k in away {
        let behind = match &k.standing {
            Standing::Behind { snapshots, .. } => format!(" · {} behind", plural(*snapshots, "snapshot")),
            Standing::UpToDate => " · up to date".into(),
            Standing::Unsaved => " · had changes not in the store".into(),
            Standing::Unknown => String::new(),
        };
        let state = vec![Span::styled(format!("not plugged in{behind}"), dim())];
        lines.push(clone(Span::styled(icons::RING.spaced(), faint()), k.name.clone(), k.role, state));
    }
    lines.push(Line::raw(""));
    lines.push(pane_heading("Store"));
    match (&app.status, fleet) {
        (_, Some(l)) => {
            if let Some(h) = &l.head {
                lines
                    .push(pane_field("latest", format!("{}  {}", h.short_id, local_time(h.created, "%Y-%m-%d %H:%M"))));
            }
            lines.push(Line::from(
                [vec![Span::styled(format!("  {:<12}", "clones"), dim())], library_state(l)].concat(),
            ));
        }
        (Some(Err(e)), _) => lines.push(Line::styled(format!("  {e}"), dim())),
        _ if app.status_loading => lines.push(Line::styled("  checking…", dim())),
        _ => lines.push(Line::styled("  press R to check", dim())),
    }
    lines.push(Line::raw(""));
    lines.push(pane_heading("Actions"));
    for (k, what) in [("enter", "browse its newest clone in the library"), ("e", "sync, check or eject its clones")] {
        lines.push(Line::from(vec![Span::styled(format!("  {k:<6}"), theme::key()), Span::styled(what, text())]));
    }
    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block(title("Library", false), false).padding(ratatui::widgets::Padding::horizontal(1)));
    f.render_widget(p, area);
}

pub(super) fn draw_history(f: &mut Frame, app: &mut App, area: Rect) {
    app.areas.history = area;
    let b = block(title("Snapshots", true), true);
    let libs = match &app.history {
        None => {
            f.render_widget(Paragraph::new(Line::styled("loading…", dim())).block(b), area);
            return;
        }
        Some(Err(e)) => {
            let p = Paragraph::new(vec![Line::raw(""), Line::styled(e.clone(), dim())])
                .alignment(Alignment::Center)
                .block(b);
            f.render_widget(p, area);
            return;
        }
        Some(Ok(libs)) => libs,
    };
    let mut rows = Vec::new();
    for (lib, snaps) in libs {
        for (i, s) in snaps.iter().enumerate() {
            let head = i == 0 && lib.head.as_ref() == Some(&s.id);
            let source = s.source.as_ref().map(|x| x.drive_name.clone()).unwrap_or_default();
            rows.push(Row::new(vec![
                Cell::from(Span::styled(
                    if i == 0 { lib.name.clone() } else { String::new() },
                    bold(theme::current().accent),
                )),
                Cell::from(Span::styled(
                    if head { icons::DOT.get() } else { "│" },
                    fg(if head { theme::current().good } else { theme::current().faint }),
                )),
                Cell::from(Span::styled(s.short_id.clone(), fg(theme::current().warning))),
                Cell::from(Span::styled(local_time(s.created, "%Y-%m-%d %H:%M").to_string(), text())),
                Cell::from(Line::styled(s.summary.tracks.to_string(), text()).right_aligned()),
                Cell::from(Line::styled(bytes(s.bytes), dim()).right_aligned()),
                Cell::from(Span::styled(source, fg(theme::current().info))),
                Cell::from(Span::styled(s.message.clone().unwrap_or_default(), dim())),
            ]));
        }
    }
    if rows.is_empty() {
        let p = Paragraph::new(vec![
            Line::raw(""),
            Line::styled("The store has no snapshots yet.", bold(theme::current().text)),
            Line::styled("locallibrary pull <drive> -m \"after Friday's export\"", fg(theme::current().info)),
        ])
        .alignment(Alignment::Center)
        .block(b);
        f.render_widget(p, area);
        return;
    }
    // Under the border and the header row.
    app.history_state.fit(area.height.saturating_sub(3) as usize, rows.len());
    let header = ["Library", "", "Snapshot", "Created", "Tracks", "Size", "From", "Message"];
    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(1),
            Constraint::Length(9),
            Constraint::Length(16),
            Constraint::Length(7),
            Constraint::Length(9),
            Constraint::Length(16),
            Constraint::Fill(1),
        ],
    )
    .header(Row::new(header.into_iter().map(|h| Cell::from(Span::styled(h, bold(theme::current().dim))))))
    .block(b)
    .column_spacing(2)
    .row_highlight_style(theme::selected())
    .highlight_symbol(Span::styled(icons::SELECTED.get(), fg(theme::current().accent)));
    f.render_stateful_widget(table, area, &mut app.history_state);
}
