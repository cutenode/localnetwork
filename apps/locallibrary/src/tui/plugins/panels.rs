//! Plugins' panels in the player: under the selected track's details, what each plugin's panel
//! shows about it, asked for in the background once the selection has settled (holding an arrow
//! key asks for none) and drawn from what came back, never another track's. A panel under way
//! says so in the room its last answer took, so the details don't jump as answers come; one that
//! failed says so (the messages, `!`, say why), and so does one showing nothing. Nothing a plugin
//! does runs as a frame is drawn.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::prelude::*;
use locallibrary::progress::{Ctx, NoProgress};
use locallibrary::{CancelToken, Library, Store};
use loclib_plugin::{ChartStyle, Series, View};
use loclib_waveform::Art;

use loclib_waveform::Pixels;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph};

use super::super::app::{App, Level, Msg, View as Screen};
use super::super::drives::Tone;
use super::super::theme::{self, dim, faint, fg};
use super::super::ui::{clip, section_with, tone_style};
use super::{Names, view_lines};

/// How long the selection stays on a track before its panels are asked for.
const SETTLE: Duration = Duration::from_millis(250);

/// The most rows a panel's body takes.
pub const MOST_ROWS: u16 = 8;

/// The most points of a series drawn: more are thinned out, evenly.
const MOST_POINTS: usize = 256;

/// The selected track's panels.
#[derive(Default)]
pub struct Panels {
    /// The track selected (the library load's generation, its index) and since when.
    seen: Option<(u64, usize, Instant)>,
    /// The track `shown` is for.
    track: Option<(u64, usize)>,
    /// Each plugin's panel, in the order the plugins are.
    pub shown: Vec<Shown>,
    /// Stops the panels under way once another track's are asked for.
    cancel: CancelToken,
}

/// A panel, and what it shows now.
pub struct Shown {
    pub plugin: &'static str,
    pub panel: &'static str,
    pub title: &'static str,
    /// Its plugin's name.
    pub by: &'static str,
    pub state: State,
    /// How many rows its body took last: kept while the next answer comes.
    pub rows: u16,
}

pub enum State {
    Loading,
    /// It can't be asked here, and why (a folder opened by path, which a plugin can't name).
    Unavailable(&'static str),
    Failed,
    Ready(Box<Ready>),
}

/// A panel's view, as the player draws it: worked out in the background.
pub struct Ready {
    /// It as lines: what's drawn, but for a picture or a chart.
    pub lines: Vec<(Tone, String)>,
    pub art: Option<Art>,
    pub chart: Option<Plot>,
}

/// A chart, thinned out to be drawn.
pub struct Plot {
    pub bars: bool,
    pub series: Vec<(String, Vec<(f64, f64)>)>,
    /// The least and most x of every point, and y.
    pub x: [f64; 2],
    pub y: [f64; 2],
}

impl Ready {
    /// `view` as it's drawn, `names` naming the tracks of a `track` column.
    pub fn of(view: &View, names: Names<'_>) -> Ready {
        let lines = view_lines(view, names);
        let art = match view {
            View::Image { data: Some(url), .. } => {
                let bytes = url.split_once(";base64,").and_then(|(_, b)| BASE64_STANDARD.decode(b).ok());
                bytes.and_then(|b| Art::decode(&b))
            }
            _ => None,
        };
        let chart = match view {
            View::Chart { style, series, .. } if !view.is_empty() => Some(Plot::of(*style, series)),
            _ => None,
        };
        Ready { lines, art, chart }
    }

    /// How many rows its body takes: all there are for a picture or a chart, else a row a line.
    fn rows(&self) -> u16 {
        match self.art.is_some() || self.chart.is_some() {
            true => MOST_ROWS,
            false => (self.lines.len() as u16).clamp(1, MOST_ROWS),
        }
    }
}

impl Plot {
    fn of(style: ChartStyle, series: &[Series]) -> Plot {
        let thinned = |points: &[(f64, f64)]| -> Vec<(f64, f64)> {
            let step = points.len().div_ceil(MOST_POINTS).max(1);
            points.iter().step_by(step).copied().collect()
        };
        let series: Vec<(String, Vec<(f64, f64)>)> =
            series.iter().map(|s| (s.name.clone(), thinned(&s.points))).collect();
        let range = |of: fn(&(f64, f64)) -> f64| {
            let all = series.iter().flat_map(|(_, p)| p.iter().map(of));
            let (lo, hi) = all.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
            // A flat range still has room to draw in.
            if lo < hi { [lo, hi] } else { [lo - 1.0, hi + 1.0] }
        };
        let bars = style == ChartStyle::Bar;
        let [lo, hi] = range(|p| p.1);
        // Bars stand on zero.
        let y = if bars { [lo.min(0.0), hi.max(0.0)] } else { [lo, hi] };
        Plot { bars, x: range(|p| p.0), y, series }
    }
}

impl Panels {
    /// Whether it's waiting for the selection (`selected`, as [`Panels::track`] keeps it) to
    /// settle, or for an answer: the screen is drawn often until it's done.
    pub fn waiting(&self, selected: Option<(u64, usize)>) -> bool {
        let any = crate::plugins::host().is_ok_and(|p| p.manifests().any(|m| !m.contributes.panels.is_empty()));
        let settling = any && selected.is_some() && self.track != selected;
        settling || self.shown.iter().any(|s| matches!(s.state, State::Loading))
    }

    /// Whether what's shown is the track `selected`'s: otherwise every panel is drawn loading.
    pub fn of_track(&self, selected: (u64, usize)) -> bool {
        self.track == Some(selected)
    }
}

impl App {
    /// Ask for the selected track's panels once the selection has settled on it a moment: each
    /// panel in the background, those under way for another track stopped.
    pub(in super::super) fn want_panels(&mut self) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let all = plugins.panels();
        if all.is_empty() || !self.show_details || self.view != Screen::Library {
            return;
        }
        let (Some(l), Some(i)) = (&self.loaded, self.selected_track()) else { return };
        let (generation, root) = (l.generation, l.root().to_path_buf());
        if self.panels.track == Some((generation, i)) {
            return;
        }
        match self.panels.seen {
            Some((g, j, at)) if (g, j) == (generation, i) => {
                if at.elapsed() < SETTLE {
                    return;
                }
            }
            _ => {
                self.panels.seen = Some((generation, i, Instant::now()));
                return;
            }
        }
        let (library, track) = (l.library.clone(), l.library.tracks[i].id);
        let why = (self.pluggable(Some(&root)) == Some(false)).then_some("only on a plugged-in drive");
        self.panels.cancel.cancel();
        self.panels.cancel = CancelToken::new();
        let last = |plugin: &str, panel: &str| {
            let old = self.panels.shown.iter().find(|s| (s.plugin, s.panel) == (plugin, panel));
            old.map_or(1, |s| s.rows)
        };
        let shown = all.iter().map(|&(plugin, p)| Shown {
            plugin,
            panel: &p.name,
            title: &p.title,
            by: plugins.manifest(plugin).map_or("", |m| m.name.as_str()),
            state: why.map_or(State::Loading, State::Unavailable),
            rows: last(plugin, &p.name),
        });
        self.panels.shown = shown.collect();
        self.panels.track = Some((generation, i));
        if why.is_some() {
            return;
        }
        for (at, (plugin, p)) in all.into_iter().enumerate() {
            let (cancel, store_root, root, library) =
                (self.panels.cancel.clone(), self.store_root.clone(), root.clone(), library.clone());
            let job = move || {
                let answer = render(plugin, &p.name, &root, track, &store_root, &library, cancel);
                Msg::Panel((generation, i), at, answer.map(Box::new))
            };
            self.spawn_or(job, move |e| Msg::Panel((generation, i), at, Err(e)));
        }
    }

    /// Panel `at` of `track`'s answered: shown, if they're still the track's panels, in the room
    /// it takes; what went wrong in the messages.
    pub(in super::super) fn panel_answered(
        &mut self,
        track: (u64, usize),
        at: usize,
        answer: Result<Box<Ready>, String>,
    ) {
        if self.panels.track != Some(track) {
            return;
        }
        let Some(shown) = self.panels.shown.get_mut(at) else { return };
        match answer {
            Ok(ready) => {
                shown.rows = ready.rows();
                shown.state = State::Ready(ready);
            }
            Err(e) => {
                shown.state = State::Failed;
                let said = format!("{} panel: {e}", shown.title);
                self.remember(said, Level::Error);
            }
        }
    }
}

/// How many rows the selected track's panels take: each its heading and the rows its last
/// answer took (one before any), however its answer is coming along, so the details don't
/// jump. None without a panel.
pub fn height(app: &App) -> u16 {
    let shown = &app.panels.shown;
    match shown.is_empty() {
        true => crate::plugins::host().map_or(0, |p| p.panels().len() as u16 * 2),
        false => shown.iter().map(|s| 1 + s.rows).sum(),
    }
}

/// The selected track's panels in `area`, one after another, each under its title and plugin's
/// name: what it shows (a picture, a chart, else its lines, cut at the edge), or that it's
/// coming, failed or can't be shown here. Another track's are drawn as coming, in their room.
pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    let current = app.selected_key().is_some_and(|k| app.panels.of_track(k));
    // Before any track's were asked for: each coming, in a row.
    let mut coming: Vec<Shown> = Vec::new();
    if app.panels.shown.is_empty() {
        let Ok(plugins) = crate::plugins::host() else { return };
        coming = (plugins.panels().into_iter())
            .map(|(plugin, p)| Shown {
                plugin,
                panel: &p.name,
                title: &p.title,
                by: plugins.manifest(plugin).map_or("", |m| m.name.as_str()),
                state: State::Loading,
                rows: 1,
            })
            .collect();
    }
    let all = if coming.is_empty() { &mut app.panels.shown } else { &mut coming };
    let mut y = area.y;
    for shown in all.iter_mut() {
        // Its title and a row at least.
        if area.bottom() - y < 2 {
            break;
        }
        let r = Rect { y, height: (1 + shown.rows).min(area.bottom() - y), ..area };
        y += r.height;
        let heading =
            vec![Span::styled(format!(" {} ", shown.title), dim()), Span::styled(format!("{} ", shown.by), faint())];
        let body = section_with(f, r, Line::from(heading));
        let said =
            |f: &mut Frame, why: &str| f.render_widget(Paragraph::new(Line::styled(why.to_string(), dim())), body);
        match &mut shown.state {
            _ if !current => said(f, "loading…"),
            State::Loading => said(f, "loading…"),
            State::Unavailable(why) => said(f, why),
            State::Failed => said(f, "couldn't show it (! says why)"),
            State::Ready(ready) => draw_ready(f, ready, body),
        }
    }
}

/// A panel's answer in `area`: its picture (in colour; without colours, its lines say what it
/// shows), its chart, or its lines.
fn draw_ready(f: &mut Frame, ready: &mut Ready, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    if let Some(art) = ready.art.as_mut().filter(|_| !theme::plain()) {
        // Square: a cell is two pixels high.
        let cols = (area.height * 2).min(area.width);
        let px = art.pixels(cols, area.height);
        f.render_widget(Pixels { width: cols, pixels: px, palette: theme::palette() }, Rect { width: cols, ..area });
        return;
    }
    if let Some(plot) = &ready.chart {
        let t = theme::current();
        let colours = [t.accent, t.info, t.warning, t.secondary, t.good];
        let graph = if plot.bars { GraphType::Bar } else { GraphType::Line };
        let datasets = plot.series.iter().enumerate().map(|(i, (name, points))| {
            let data = Dataset::default().name(name.clone()).marker(Marker::Braille).graph_type(graph);
            data.style(fg(colours[i % colours.len()])).data(points)
        });
        let axis = |[lo, hi]: [f64; 2]| Axis::default().bounds([lo, hi]).style(faint());
        let chart = Chart::new(datasets.collect()).x_axis(axis(plot.x)).y_axis(axis(plot.y));
        // One series needs no legend.
        let chart = if plot.series.len() < 2 { chart.legend_position(None) } else { chart };
        f.render_widget(chart, area);
        return;
    }
    let w = area.width as usize;
    let rows = area.height as usize;
    let mut lines: Vec<Line> = ready
        .lines
        .iter()
        .take(rows)
        .map(|(tone, l)| {
            let (mark, style) = tone_style(*tone);
            Line::styled(clip(&format!("{mark}{l}"), w), style)
        })
        .collect();
    if ready.lines.len() > rows
        && let Some(last) = lines.last_mut()
    {
        *last = Line::styled(format!("+ {} more", ready.lines.len() - rows + 1), dim());
    }
    f.render_widget(Paragraph::new(lines), area);
}

/// The plugin's panel's view of `track` on the drive at `root`, as it's drawn (`library` names the
/// tracks of a `track` column): stopped once `cancel` is.
fn render(
    plugin: &str,
    panel: &str,
    root: &Path,
    track: u32,
    store_root: &PathBuf,
    library: &Arc<Library>,
    cancel: CancelToken,
) -> Result<Ready, String> {
    let plugins = crate::plugins::host().map_err(|e| e.to_string())?;
    let store = Store::open(store_root).ok();
    let ctx = Ctx::new(&NoProgress, cancel);
    let drive = root.display().to_string();
    let view = plugins.render_panel(plugin, panel, &drive, track, store.as_ref(), &ctx).map_err(|e| e.to_string())?;
    let names = |id| library.track(id).map(|t| t.display_name());
    Ok(Ready::of(&view, &names))
}

#[cfg(test)]
mod tests;
