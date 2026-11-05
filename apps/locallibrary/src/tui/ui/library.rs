//! The Library view: the drive list, the playlists, the tracks, a track's details and the crate.

use locallibrary::fields::FieldValue;
use locallibrary::query::{FieldName, SortField, SortKey};
use locallibrary::{DeviceInfo, Track};
use loclib_plugin::{FieldDisplay, FieldFormat};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Cell, HighlightSpacing, List, ListItem, ListState, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState,
    Table, TableState, Wrap,
};

use crate::output::{bytes, clock};
use crate::tui::app::{App, Focus, View};
use crate::tui::browse::{NodeId, sort_label, stars};
use crate::tui::keys::Camelot;
use crate::tui::theme::{self, bold, dim, faint, fg, text};

use super::*;

/// The sidebar (drives and the tree), the track list, and the details and crate panels when
/// they're open: beside the list on wide terminals, in place of the sidebar on narrow ones
/// (the sidebar comes back while it has focus).
pub(super) fn draw_library(f: &mut Frame, app: &mut App, area: Rect) {
    let side_on = app.show_details || app.show_crate;
    let in_sidebar = matches!(app.focus, Focus::Drives | Focus::Playlists) || app.picking.is_some();
    let sidebar_w = (area.width / 4).clamp(24, 38);
    let (sidebar, tracks, side) = if side_on && area.width >= 120 {
        let [a, b, c] = Layout::horizontal([
            Constraint::Length((area.width / 5).clamp(24, 34)),
            Constraint::Min(0),
            Constraint::Length((area.width / 4).clamp(34, 50)),
        ])
        .areas(area);
        (Some(a), b, Some(c))
    } else if side_on && !in_sidebar {
        let side_w = (area.width * 2 / 5).clamp(30, 44).min(area.width / 2);
        let [b, c] = Layout::horizontal([Constraint::Min(0), Constraint::Length(side_w)]).areas(area);
        (None, b, Some(c))
    } else {
        let [a, b] = Layout::horizontal([Constraint::Length(sidebar_w), Constraint::Min(0)]).areas(area);
        (Some(a), b, None)
    };
    if let Some(side) = sidebar {
        let drives_h = (app.drives.len() as u16 + 2).clamp(3, (side.height / 3).max(3));
        let [drives, playlists] = Layout::vertical([Constraint::Length(drives_h), Constraint::Min(0)]).areas(side);
        app.areas.drives = drives;
        app.areas.playlists = playlists;
        draw_drive_list(f, app, drives, app.focus == Focus::Drives && app.view == View::Library);
        draw_playlists(f, app, playlists);
    }
    app.areas.tracks = tracks;
    draw_tracks(f, app, tracks);
    if let Some(side) = side {
        match (app.show_details, app.show_crate) {
            (true, true) => {
                let [d, t] = Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(side);
                draw_track_details(f, app, d);
                draw_crate(f, app, t);
            }
            (true, false) => draw_track_details(f, app, side),
            _ => draw_crate(f, app, side),
        }
    }
}

fn draw_drive_list(f: &mut Frame, app: &mut App, area: Rect, focused: bool) {
    let loaded_root = app.loaded.as_ref().map(|l| l.root().to_path_buf());
    let inner_w = area.width.saturating_sub(3);
    let items: Vec<ListItem> = (app.drives.iter())
        .map(|d| ListItem::new(drive_line(d, loaded_root.as_deref() == Some(d.root.as_path()), "", inner_w)))
        .collect();
    let list = drive_list(items, focused);
    app.drive_state.fit(area.height.saturating_sub(2) as usize, app.drives.len());
    f.render_stateful_widget(list, area, &mut app.drive_state);
}

/// A drive in a list: a dot (pink: open, green: has an export, hollow: blank), its name and its
/// track count, after `indent`.
pub(super) fn drive_line(d: &DeviceInfo, open: bool, indent: &str, width: u16) -> Line<'static> {
    let dot = if !d.has_export {
        Span::styled("○ ", faint())
    } else if open {
        Span::styled("● ", bold(theme::current().accent))
    } else {
        Span::styled("● ", fg(theme::current().good))
    };
    let name_style = if open { bold(theme::current().text) } else { text() };
    let left = vec![Span::raw(indent.to_string()), dot, Span::styled(drive_name(d), name_style)];
    let right = match &d.summary {
        Some(s) => vec![Span::styled(compact(s.tracks), dim())],
        None => vec![Span::styled("blank", faint())],
    };
    spread(left, right, width)
}

/// The drive lists' frame, and what they say with no drive.
pub(super) fn drive_list(items: Vec<ListItem<'static>>, focused: bool) -> List<'static> {
    let list = if items.is_empty() {
        List::new([ListItem::new(Line::styled("no rekordbox drives", dim()))])
    } else {
        List::new(items)
    };
    list.block(block(title("Drives", focused), focused))
        .highlight_style(if focused { theme::selected() } else { Style::new() })
        .highlight_symbol(Span::styled("▍", fg(theme::current().accent)))
}

/// Keep `sel` in a window of `page` rows starting at `offset`; returns the new offset.
pub(in crate::tui) fn scroll_window(offset: usize, sel: Option<usize>, page: usize, len: usize) -> usize {
    let mut offset = offset;
    if let Some(s) = sel {
        if s < offset {
            offset = s;
        } else if s >= offset + page {
            offset = s + 1 - page.max(1);
        }
    }
    offset.min(len.saturating_sub(page))
}

/// A list or table kept to [`scroll_window`] before it's drawn: drawn smaller, then larger, it
/// comes back up once its rows fit again (ratatui's own scrolling only ever follows the
/// selection, so rows stayed hidden above empty space).
pub(in crate::tui) trait Scrolled {
    /// Scroll for a window of `page` rows of `len`.
    fn fit(&mut self, page: usize, len: usize);
}

impl Scrolled for ListState {
    fn fit(&mut self, page: usize, len: usize) {
        *self.offset_mut() = scroll_window(self.offset(), self.selected(), page, len);
    }
}

impl Scrolled for TableState {
    fn fit(&mut self, page: usize, len: usize) {
        *self.offset_mut() = scroll_window(self.offset(), self.selected(), page, len);
    }
}

fn draw_playlists(f: &mut Frame, app: &mut App, area: Rect) {
    let picking = app.picking.is_some();
    let focused = app.focus == Focus::Playlists;
    let inner_w = area.width.saturating_sub(3);
    let heading = match &app.loaded {
        _ if picking => Line::from(vec![
            Span::raw(" "),
            Span::styled("Pick a playlist", bold(theme::current().warning)),
            Span::raw(" "),
        ]),
        Some(l) if l.stale => Line::from(vec![
            Span::raw(" "),
            Span::styled("Playlists", bold(theme::current().dim)),
            Span::styled(" unplugged ", bold(theme::current().critical)),
        ]),
        _ => title("Playlists", focused),
    };
    let mut b = block(heading, focused || picking);
    if picking {
        b = b.border_style(fg(theme::current().warning));
    }
    let Some(l) = &app.loaded else {
        f.render_widget(Paragraph::new(Line::styled("no drive open", dim())).block(b), area);
        return;
    };
    // Only the rows on screen: an open category can hold thousands of values.
    let page = area.height.saturating_sub(2) as usize;
    let len = l.tree.len();
    let sel = app.playlist_state.selected();
    let offset = scroll_window(app.playlist_state.offset(), sel, page, len);
    *app.playlist_state.offset_mut() = offset;
    let items: Vec<ListItem> = l.tree[offset..(offset + page).min(len)]
        .iter()
        .map(|n| {
            let indent = "  ".repeat(n.depth);
            let virtual_folder = matches!(n.id, NodeId::Browse | NodeId::Category(_));
            let (icon, icon_style) = match (&n.id, n.folder, n.expanded) {
                (NodeId::All, _, _) => ("♫ ", fg(theme::current().accent)),
                (_, true, true) => {
                    ("▾ ", fg(if virtual_folder { theme::current().secondary } else { theme::current().info }))
                }
                (_, true, false) => {
                    ("▸ ", fg(if virtual_folder { theme::current().secondary } else { theme::current().info }))
                }
                (NodeId::Group(..), false, _) => ("· ", faint()),
                _ => ("• ", faint()),
            };
            let current = !n.folder && n.id == l.node;
            let marked = match &n.id {
                NodeId::Playlist(id) => !app.list_marks.is_empty() && app.list_marks.contains(&l.playlist_path(*id)),
                _ => false,
            };
            let name_style = if current {
                bold(theme::current().accent)
            } else if virtual_folder {
                bold(theme::current().secondary)
            } else if n.folder {
                bold(theme::current().text)
            } else {
                text()
            };
            let (icon, icon_style) = if marked { ("✓ ", bold(theme::current().accent)) } else { (icon, icon_style) };
            let left =
                vec![Span::raw(indent), Span::styled(icon, icon_style), Span::styled(n.name.clone(), name_style)];
            let right = vec![Span::styled(compact(n.count), faint())];
            ListItem::new(spread(left, right, inner_w))
        })
        .collect();
    let list = List::new(items)
        .block(b)
        .highlight_style(if focused { theme::selected() } else { Style::new() })
        .highlight_symbol(Span::styled("▍", fg(theme::current().accent)));
    let mut window = ListState::default().with_selected(sel.map(|s| s.saturating_sub(offset)));
    f.render_stateful_widget(list, area, &mut window);
}

pub(super) fn rating(r: u8) -> String {
    stars(r)
}

/// A column of the track list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Col {
    Title,
    Artist,
    Album,
    Genre,
    /// My Tags (not a sort field).
    Tags,
    Rating,
    Added,
    Plays,
    Bpm,
    Key,
    Time,
    /// A plugin's field (`sound.bpm`).
    Field(FieldName),
}

impl Col {
    pub(super) fn field(self) -> Option<SortField> {
        Some(match self {
            Col::Title => SortField::Title,
            Col::Artist => SortField::Artist,
            Col::Album => SortField::Album,
            Col::Genre => SortField::Genre,
            Col::Tags => return None,
            Col::Rating => SortField::Rating,
            Col::Added => SortField::Added,
            Col::Plays => SortField::Plays,
            Col::Bpm => SortField::Bpm,
            Col::Key => SortField::Key,
            Col::Time => SortField::Time,
            Col::Field(name) => SortField::Field(name),
        })
    }

    pub(super) fn name(self) -> &'static str {
        self.field().map_or("My Tags", sort_label)
    }

    pub(super) fn of(f: SortField) -> Option<Col> {
        [
            Col::Title,
            Col::Artist,
            Col::Album,
            Col::Genre,
            Col::Rating,
            Col::Added,
            Col::Plays,
            Col::Bpm,
            Col::Key,
            Col::Time,
        ]
        .into_iter()
        .find(|c| c.field() == Some(f))
        .or(match f {
            SortField::Field(name) => Some(Col::Field(name)),
            _ => None,
        })
    }

    pub(super) fn width(self) -> Constraint {
        match self {
            Col::Title => Constraint::Fill(3),
            Col::Artist | Col::Album => Constraint::Fill(2),
            Col::Genre => Constraint::Fill(1),
            Col::Tags => Constraint::Fill(2),
            Col::Rating => Constraint::Length(7),
            Col::Added => Constraint::Length(10),
            Col::Plays | Col::Bpm => Constraint::Length(6),
            Col::Key => Constraint::Length(4),
            Col::Time => Constraint::Length(6),
            // Room for its name (or as wide as its analyzer says), and the sort's arrow.
            Col::Field(name) => {
                let named = || unicode_width::UnicodeWidthStr::width(name.as_str()).max(5) as u16;
                Constraint::Length(field_hint(name).and_then(|h| h.width).unwrap_or_else(named) + 1)
            }
        }
    }
}

/// How the field `name` (`sound.bpm`) is shown, as its analyzer says: read from its manifest,
/// never asked of its plugin.
fn field_hint(name: FieldName) -> Option<&'static FieldDisplay> {
    let (plugin, field) = name.as_str().split_once('.')?;
    crate::plugins::host().ok()?.display(plugin, field)
}

/// A plugin's field's value as its column shows it: as its analyzer says (`hint`), else numbers to
/// two places at most and a list joined; one worked out with nothing to show as a faint dash (a
/// track not analysed has nothing there at all). Numbers are on the right, a meter on the left.
pub(super) fn field_cell(v: &FieldValue, hint: Option<&FieldDisplay>) -> Line<'static> {
    if let Some(h) = hint.filter(|_| *v != FieldValue::Null) {
        let shown = h.show(v);
        return match (h.format, v) {
            (Some(FieldFormat::Meter), _) => Line::styled(shown, fg(theme::current().accent)),
            (_, FieldValue::Number(_) | FieldValue::Integer(_)) => {
                Line::styled(shown, fg(theme::current().warning)).right_aligned()
            }
            (_, FieldValue::Boolean(_)) => Line::styled(shown, dim()),
            _ => Line::styled(shown, fg(theme::current().info)),
        };
    }
    match v {
        FieldValue::Null | FieldValue::Structured(_) => Line::styled("-", faint()).right_aligned(),
        FieldValue::Number(n) => {
            let s = format!("{n:.2}");
            Line::styled(s.trim_end_matches('0').trim_end_matches('.').to_string(), fg(theme::current().warning))
                .right_aligned()
        }
        FieldValue::Integer(n) => Line::styled(n.to_string(), fg(theme::current().warning)).right_aligned(),
        FieldValue::Boolean(b) => Line::styled(if *b { "yes" } else { "no" }, dim()),
        FieldValue::String(s) => Line::styled(s.clone(), fg(theme::current().info)),
        FieldValue::Strings(all) => Line::styled(all.join(", "), fg(theme::current().info)),
    }
}

/// The columns that fit (ratings only when asked for), plus the sort column when it wouldn't
/// show otherwise.
pub(super) fn columns(width: u16, sort: Option<SortKey>, ratings: bool, tags: bool) -> Vec<Col> {
    let mut v = vec![Col::Title, Col::Artist];
    if width >= 110 {
        v.push(Col::Album);
    }
    if width >= 140 {
        v.push(Col::Genre);
    }
    if tags {
        v.push(Col::Tags);
    }
    if ratings {
        v.push(Col::Rating);
    }
    if let Some(c) =
        sort.and_then(|k| Col::of(k.field)).filter(|c| !v.contains(c) && !matches!(c, Col::Bpm | Col::Key | Col::Time))
    {
        v.push(c);
    }
    v.extend([Col::Bpm, Col::Key, Col::Time]);
    v
}

/// `cols` with the columns of the plugins' `fields` asked for, in order, before tempo, key and
/// time (one already there, as the sort column, staying where it is).
pub(super) fn with_fields(mut cols: Vec<Col>, fields: &[FieldName]) -> Vec<Col> {
    let at = cols.len().saturating_sub(3);
    let more: Vec<Col> = fields.iter().map(|&f| Col::Field(f)).filter(|c| !cols.contains(c)).collect();
    cols.splice(at..at, more);
    cols
}

/// The key the track list's keys are matched against, and whose file it is: the playing track's,
/// or with nothing playing (or no key to go on), the selected track's.
fn mix_from(app: &App) -> Option<(Camelot, String)> {
    if let Some(n) = &app.now
        && let Some(k) = n.key
    {
        return Some((k, n.track.file_path.clone()));
    }
    let l = app.loaded.as_ref()?;
    let t = &l.library.tracks[*app.visible.get(app.track_state.selected()?)?];
    Some((Camelot::parse(&t.key)?, t.file_path.clone()))
}

/// Whether `t` mixes with `from`'s key on the Camelot wheel (same key, ±1 or relative), other
/// than `from`'s own track.
fn mixes_with(from: &Option<(Camelot, String)>, t: &Track) -> bool {
    from.as_ref().is_some_and(|(key, path)| {
        *path != t.file_path && Camelot::parse(&t.key).is_some_and(|k| key.relation(k).is_some())
    })
}

fn draw_tracks(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Tracks;
    let (heading, count) = match &app.loaded {
        Some(l) => (l.playlist_name(), app.visible.len()),
        None => ("Tracks".to_string(), 0),
    };
    let left_out = app.loaded.as_ref().map_or(0, |l| l.left_out());
    let mut left = vec![
        Span::raw(" "),
        Span::styled(heading, if focused { bold(theme::current().info) } else { bold(theme::current().dim) }),
        Span::styled(format!(" {count} "), faint()),
    ];
    if left_out > 0 {
        left.push(Span::styled(format!("· {left_out} not on this drive "), dim()));
    }
    if !app.marks.is_empty() {
        left.push(Span::styled(format!("✓{} ", app.marks.len()), bold(theme::current().accent)));
    }
    if app.loaded.as_ref().is_some_and(|l| l.stale) {
        left.push(Span::styled("unplugged ", bold(theme::current().critical)));
    }
    let mut b = block(Line::from(left), focused);
    let mix = mix_from(app);
    // The key that green keys mix with, the search and the sort, on the right of the title.
    let mut right = Vec::new();
    if let Some((k, _)) = &mix
        && !app.visible.is_empty()
    {
        right.push(Span::styled(format!(" ⇄ {} ", k.code()), fg(theme::current().good)));
    }
    if !app.query.is_empty() {
        right.push(Span::styled(" / ", theme::key()));
        right.push(Span::styled(clip(&app.query, 28), text()));
        if app.search_all {
            right.push(Span::styled(" whole drive", fg(theme::current().info)));
        }
        right.push(Span::raw(" "));
    }
    if let Some(k) = app.sort {
        right.push(Span::styled(
            format!(" {} {} ", sort_label(k.field), if k.descending { "↓" } else { "↑" }),
            fg(theme::current().warning),
        ));
    }
    if !right.is_empty() {
        b = b.title_top(Line::from(right).right_aligned());
    }
    let Some(l) = &app.loaded else {
        let msg = if app.loading.is_some() {
            vec![Line::styled("Reading library…", dim())]
        } else {
            vec![
                Line::raw(""),
                Line::styled("No rekordbox drive open.", bold(theme::current().text)),
                Line::raw(""),
                Line::styled("Plug in a USB drive with a rekordbox export, or start with", dim()),
                Line::styled("locallibrary tui /path/to/drive", fg(theme::current().info)),
            ]
        };
        f.render_widget(Paragraph::new(msg).alignment(Alignment::Center).block(b), area);
        return;
    };

    let cols = columns(area.width, app.sort, app.show_ratings, app.show_tags_column);
    let cols = with_fields(cols, &app.field_columns);
    let values = l.values().cloned();
    let my_tags = match &l.my_tags {
        crate::tui::app::MyTags::Read(t) => Some(t.clone()),
        _ => None,
    };
    let playing_path = app.now.as_ref().filter(|n| n.root == l.root()).map(|n| n.track.file_path.as_str());
    let t = app.started.elapsed().as_secs_f32();
    let eq = ["▁", "▃", "▅", "▇", "▅", "▃"];
    let paused = !app.is_playing();

    // Three narrow columns first: playing or marked, colour, problems.
    let mut widths = vec![Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)];
    widths.extend(cols.iter().map(|c| c.width()));
    let header: Vec<Cell> = ["", "", ""]
        .into_iter()
        .map(Cell::from)
        .chain(cols.iter().map(|c| {
            let name = c.name();
            match app.sort.filter(|k| Some(k.field) == c.field()) {
                Some(k) => Cell::from(Span::styled(
                    format!("{name}{}", if k.descending { "↓" } else { "↑" }),
                    bold(theme::current().warning),
                )),
                None => Cell::from(Span::styled(name, bold(theme::current().dim))),
            }
        }))
        .collect();

    // Where the headers are, for sorting by clicking one (as the table lays them out: a column
    // for the highlight symbol, then the columns one space apart). Under 3 rows the pane has no
    // room for them: the row under its top border is its bottom border, or another pane's.
    let height = area.height.saturating_sub(2).min(1);
    let inner = Rect { x: area.x + 1, y: area.y + 1, width: area.width.saturating_sub(2), height };
    let cells = Rect { x: inner.x + 1, width: inner.width.saturating_sub(1), ..inner };
    let rects = Layout::horizontal(widths.clone()).flex(Flex::Start).spacing(1).split(cells);
    app.areas.columns = cols.iter().zip(rects.iter().skip(3)).filter_map(|(c, r)| Some((c.field()?, *r))).collect();

    // Build only the rows on screen: libraries run to tens of thousands of tracks, redrawn at 60 fps.
    let page = area.height.saturating_sub(3) as usize;
    let len = app.visible.len();
    let sel = app.track_state.selected();
    let offset = scroll_window(app.track_state.offset(), sel, page, len);
    *app.track_state.offset_mut() = offset;
    let window = &app.visible[offset..(offset + page).min(len)];
    let mix_bg = theme::behind([30, 66, 42]);
    // Without colours, keys that mix are marked and underlined instead of green.
    let (mix_mark, mix_style) = if theme::plain() {
        ("⇄", bold(theme::current().good).add_modifier(Modifier::UNDERLINED))
    } else {
        (" ", bold(theme::current().good))
    };

    let rows: Vec<Row> = window
        .iter()
        .map(|&i| {
            let tr: &Track = &l.library.tracks[i];
            let is_playing = playing_path == Some(tr.file_path.as_str());
            let marked = app.marks.contains(&tr.file_path);
            let indicator = if is_playing {
                let sym = match (paused, theme::reduced_motion()) {
                    (true, _) => "‖",
                    (false, true) => "▶",
                    (false, false) => eq[(t * 8.0) as usize % eq.len()],
                };
                Span::styled(sym, bold(theme::current().accent))
            } else if marked {
                Span::styled("✓", bold(theme::current().accent))
            } else {
                Span::raw(" ")
            };
            let color = tr.color.map_or(Span::raw(" "), theme::colour_dot);
            let problem = if l.problems.is_missing(i) {
                Span::styled("✖", bold(theme::current().critical))
            } else if l.problems.has_copies(i) {
                Span::styled("≈", fg(theme::current().warning))
            } else {
                Span::raw(" ")
            };
            let title_style = if is_playing { bold(theme::current().accent) } else { text() };
            let mut cells = vec![Cell::from(indicator), Cell::from(color), Cell::from(problem)];
            for c in &cols {
                cells.push(match c {
                    Col::Title => Cell::from(Span::styled(tr.title.clone(), title_style)),
                    Col::Artist => Cell::from(Span::styled(
                        tr.artist.clone(),
                        fg(theme::mix(theme::current().text, theme::current().dim, 0.4)),
                    )),
                    Col::Album => Cell::from(Span::styled(tr.album.clone(), dim())),
                    Col::Genre => Cell::from(Span::styled(tr.genre.clone(), dim())),
                    Col::Tags => {
                        let key = locallibrary::paths::file_key(&tr.file_path);
                        let tags = my_tags.as_ref().and_then(|t| t.get(&key));
                        let names: Vec<&str> =
                            tags.into_iter().flatten().map(|t| t.rsplit('/').next().unwrap_or(t)).collect();
                        Cell::from(Span::styled(names.join(", "), fg(theme::current().secondary)))
                    }
                    Col::Rating => Cell::from(Span::styled(rating(tr.rating), fg(theme::current().warning))),
                    Col::Added => Cell::from(Span::styled(tr.date_added.chars().take(10).collect::<String>(), dim())),
                    Col::Plays => Cell::from(Line::styled(tr.play_count.to_string(), dim()).right_aligned()),
                    Col::Bpm => {
                        let bpm = if tr.bpm > 0.0 { format!("{:>6.1}", tr.bpm) } else { String::new() };
                        Cell::from(Line::styled(bpm, fg(theme::current().warning)).right_aligned())
                    }
                    Col::Key if mixes_with(&mix, tr) => {
                        Cell::from(Span::styled(format!("{mix_mark}{}", tr.key), mix_style)).style(mix_bg)
                    }
                    Col::Key => Cell::from(Span::styled(format!(" {}", tr.key), fg(theme::current().info))),
                    Col::Time => Cell::from(Line::styled(clock(tr.duration_secs as f64), dim()).right_aligned()),
                    Col::Field(name) => {
                        let value = values.as_ref().and_then(|v| v.get(tr.id, name.as_str()));
                        Cell::from(value.map(|v| field_cell(v, field_hint(*name))).unwrap_or_default())
                    }
                });
            }
            let row = Row::new(cells);
            if marked { row.style(theme::behind([40, 26, 46])) } else { row }
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(Row::new(header))
        .block(b)
        .column_spacing(1)
        .highlight_spacing(HighlightSpacing::Always)
        .row_highlight_style(if focused { theme::selected() } else { theme::behind([32, 32, 46]) })
        .highlight_symbol(Span::styled("▍", fg(theme::current().accent)));
    let mut window_state = TableState::default().with_selected(sel.map(|s| s - offset));
    f.render_stateful_widget(table, area, &mut window_state);

    if app.visible.is_empty() && area.height > 4 {
        let why = if !app.query.is_empty() {
            "Nothing matches the search."
        } else if l.reading_set() {
            "Reading the sets…"
        } else if left_out > 0 {
            "None of its tracks are on this drive."
        } else {
            "Nothing in this list."
        };
        let r = Rect { y: area.y + 3, height: 1, ..area.inner(Margin { vertical: 0, horizontal: 2 }) };
        f.render_widget(Paragraph::new(Line::styled(why, dim())).alignment(Alignment::Center), r);
    }
    if app.visible.len() > page {
        let mut sb = ScrollbarState::new(app.visible.len()).position(app.track_state.selected().unwrap_or(0));
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(fg(theme::current().dim))
                .track_style(faint()),
            area.inner(Margin { vertical: 1, horizontal: 0 }),
            &mut sb,
        );
    }
}

/// The selected track's details: everything rekordbox knows about it, what's wrong with it, the
/// playlists it's in, plugins' panels, and the tracks that mix into it (playable and taggable
/// from here).
fn draw_track_details(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Details;
    app.areas.details = area;
    let b = block(title("Track", focused), focused);
    let inner = b.inner(area).inner(Margin { vertical: 0, horizontal: 1 });
    f.render_widget(b, area);
    let (Some(l), Some(i)) = (&app.loaded, app.selected_track()) else {
        f.render_widget(Paragraph::new(Line::styled("No track selected.", dim())), inner);
        return;
    };
    let t = &l.library.tracks[i];
    let w = inner.width as usize;
    let mut lines = vec![
        Line::styled(clip(if t.title.is_empty() { "Untitled" } else { &t.title }, w), bold(theme::current().text)),
        Line::styled(clip(&t.artist, w), bold(theme::current().accent)),
    ];
    let mut chips: Vec<Span> = Vec::new();
    if let Some(c) = t.color {
        chips.extend([theme::colour_dot(c), Span::raw(" ")]);
    }
    if t.bpm > 0.0 {
        chips.push(Span::styled(format!("{:.2} BPM  ", t.bpm), fg(theme::current().warning)));
    }
    match crate::tui::keys::Camelot::parse(&t.key) {
        Some(k) => chips.push(Span::styled(format!("{} {}  ", k.code(), k.note()), fg(theme::current().info))),
        None if !t.key.is_empty() => chips.push(Span::styled(format!("{}  ", t.key), fg(theme::current().info))),
        None => {}
    }
    chips.push(Span::styled(clock(t.duration_secs as f64), dim()));
    if t.rating > 0 {
        chips.push(Span::styled(format!("  {}", rating(t.rating)), fg(theme::current().warning)));
    }
    lines.push(Line::from(fit(chips, w)));
    // Problems early, so they show even in a short panel.
    if l.problems.is_missing(i) {
        lines.push(Line::styled(clip("✖ the audio file isn't on the drive", w), bold(theme::current().critical)));
    }
    let others: Vec<usize> = l.problems.others(i).collect();
    if let Some(&o) = others.first() {
        let other = &l.library.tracks[o];
        let more = if others.len() > 1 { format!(" (+{} more)", others.len() - 1) } else { String::new() };
        lines
            .push(Line::styled(clip(&format!("≈ also at {}{more}", other.file_path), w), fg(theme::current().warning)));
    }
    let mut about: Vec<String> =
        [&t.album, &t.remixer, &t.mix_name].into_iter().filter(|s| !s.is_empty()).cloned().collect();
    if t.year > 0 {
        about.push(t.year.to_string());
    }
    if !about.is_empty() {
        lines.push(Line::styled(clip(&about.join(" · "), w), dim()));
    }
    let kv = |k: &str, v: String| Line::from(vec![Span::styled(format!("{k:<8}"), faint()), Span::styled(v, text())]);
    for (k, v) in [("genre", &t.genre), ("label", &t.label)] {
        if !v.is_empty() {
            lines.push(kv(k, clip(v, w.saturating_sub(8))));
        }
    }
    let played = if t.play_count > 0 { format!("played {}×", t.play_count) } else { "never played".into() };
    let added = if t.date_added.is_empty() { String::new() } else { format!("added {} · ", t.date_added) };
    lines.push(Line::styled(clip(&format!("{added}{played}"), w), dim()));
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
    lines.push(Line::styled(clip(&format.join(" · "), w), faint()));
    if !t.comment.trim().is_empty() {
        lines.push(Line::styled(clip(&format!("“{}”", t.comment.trim()), w), dim().add_modifier(Modifier::ITALIC)));
    }

    let details = app.details.as_ref().filter(|_| app.details_current());
    let playlists = details.map_or(&[][..], |d| &d.playlists[..]);
    // The details first, then the playlists, then the matches, when there's room for them.
    let matches_h = if inner.height >= 12 { 4 } else { 0 };
    let info_h = (lines.len() as u16).min(inner.height.saturating_sub(matches_h));
    let room = inner.height - info_h - matches_h;
    let lists_h = if playlists.is_empty() || room < 2 { 0 } else { (playlists.len() as u16 + 1).min(room).min(4) };
    // Plugins' panels in what's left, before the matches: a panel's title and a row at least.
    let panels_h = crate::tui::plugins::panels::height(app, room - lists_h);
    let [info, lists_area, panels_area, mixes] = Layout::vertical([
        Constraint::Length(info_h),
        Constraint::Length(lists_h),
        Constraint::Length(panels_h),
        Constraint::Min(0),
    ])
    .areas(inner);
    f.render_widget(Paragraph::new(lines), info);
    if lists_h > 0 {
        draw_in_playlists(f, lists_area, playlists, lists_h as usize - 1);
    }
    let why = if app.details_lookup_failed() {
        "couldn't look for matches (! says why)"
    } else if details.is_none() {
        "finding matches…"
    } else if crate::tui::keys::Camelot::parse(&t.key).is_none() {
        "no key to match"
    } else {
        "nothing in a compatible key and tempo"
    };
    let matches = details.map(|d| &d.matches);
    let rows = draw_matches(f, mixes, matches, None, why, &mut app.details_state, focused);
    app.areas.matches = rows;
    if panels_h > 0 {
        crate::tui::plugins::panels::draw(f, app, panels_area);
    }
}

/// The crate panel.
fn draw_crate(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Crate;
    app.areas.crate_panel = area;
    let list = &app.crate_list;
    let mut heading = vec![
        Span::raw(" "),
        Span::styled("Crate", if focused { bold(theme::current().info) } else { bold(theme::current().dim) }),
        Span::styled(format!(" {} ", list.len()), faint()),
    ];
    if !list.is_empty() {
        heading.push(Span::styled(format!("{} ", clock(list.duration_secs() as f64)), faint()));
    }
    let b = block(Line::from(heading), focused);
    let inner = b.inner(area);
    if list.is_empty() {
        let p = Paragraph::new(vec![
            Line::styled("The crate is empty.", dim()),
            Line::from(vec![
                Span::styled("a", theme::key()),
                Span::styled(" adds the selected or marked tracks to it.", dim()),
            ]),
        ])
        .wrap(Wrap { trim: true })
        .block(b);
        f.render_widget(p, area);
        return;
    }
    app.areas.crate_rows = inner;
    let root = app.loaded.as_ref().map(|l| l.root().to_path_buf());
    let playing = app.now.as_ref().map(|n| (n.root.clone(), n.track.file_path.clone()));
    let w = inner.width.saturating_sub(1);
    let items: Vec<ListItem> = list
        .items
        .iter()
        .enumerate()
        .map(|(n, item)| {
            let t = &item.track;
            let is_playing = playing.as_ref().is_some_and(|(r, p)| *r == item.root && *p == t.file_path);
            let mut left = vec![
                Span::styled(format!("{:>2} ", n + 1), faint()),
                Span::styled(t.title.clone(), if is_playing { bold(theme::current().accent) } else { text() }),
                Span::styled(format!("  {}", t.artist), dim()),
            ];
            if root.as_ref() != Some(&item.root) {
                left.push(Span::styled(format!(" · {}", item.drive), faint()));
            }
            let mut right = Vec::new();
            if t.bpm > 0.0 {
                right.push(Span::styled(format!("{:.0} ", t.bpm), fg(theme::current().warning)));
            }
            right.push(Span::styled(format!("{:<3}", t.key), fg(theme::current().info)));
            let lw: usize = left.iter().map(Span::width).sum::<usize>();
            let rw: usize = right.iter().map(Span::width).sum::<usize>();
            let left = fit(left, (w as usize).saturating_sub(rw + 1).max(lw.min(8)));
            ListItem::new(spread(left, right, w))
        })
        .collect();
    let list = List::new(items)
        .block(b)
        .highlight_style(if focused { theme::selected() } else { Style::new() })
        .highlight_symbol(Span::styled("▍", fg(theme::current().accent)));
    app.crate_state.fit(inner.height as usize, app.crate_list.len());
    f.render_stateful_widget(list, area, &mut app.crate_state);
}
