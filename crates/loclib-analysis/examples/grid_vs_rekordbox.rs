//! Scores the beat grids `analyze` works out against the ones rekordbox made for the same tracks.
//!
//! ```sh
//! cargo run --release -p loclib-analysis --example grid_vs_rekordbox -- PAIRS.tsv > rows.tsv
//! ```
//!
//! Each line of `PAIRS.tsv` is an `ANLZ0000.DAT` and its audio file, tab-separated (rekordbox's
//! own analysis files, under its `share/PIONEER/USBANLZ`, don't name the audio: its `master.db`
//! does, as `locallibrary collection tracks --json` shows with `analyze_path`). A row for each
//! track goes to stdout and the summary to stderr. `THREADS` sets how many run at once.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use loclib_analysis::{Want, analyze};
use loclib_anlz::beat_grid;
use loclib_progress::Ctx;

/// How one track's grid compares.
struct Row {
    file: String,
    rb_bpm: f64,
    bpm: f64,
    /// Our tempo over rekordbox's is within 2% of 1.
    same_tempo: bool,
    /// Where rekordbox's beats sit against ours, in ms, the median; `None` unless the tempo is
    /// the same.
    offset_ms: Option<f64>,
    /// Whether rekordbox's beat 1 is ours, when the beats agree.
    same_bar: Option<bool>,
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn score(dat: &Path, audio: &Path) -> Option<Row> {
    let rb = beat_grid(&std::fs::read(dat).ok()?).ok()?;
    if rb.len() < 32 {
        return None;
    }
    let want = Want { estimate: false, loudness: false, waveform: false, grid: true };
    let analysis = analyze(audio, want, &Ctx::silent()).ok()?;
    let grid = analysis.grid?;
    let rb_bpm = rb[0].bpm;
    let same_tempo = (grid.bpm / rb_bpm - 1.0).abs() < 0.02;
    let period = grid.period_secs();
    let (offset_ms, same_bar) = if same_tempo {
        let offs: Vec<f64> = rb
            .iter()
            .map(|b| {
                let x = (b.time_ms as f64 / 1000.0 - grid.first_beat_secs).rem_euclid(period);
                if x > period / 2.0 { x - period } else { x }
            })
            .collect();
        let off = median(offs);
        let bar = (off.abs() < period / 4.0).then(|| {
            rb.iter().find(|b| b.beat_number == 1).is_some_and(|b| {
                let i = ((b.time_ms as f64 / 1000.0 - grid.first_beat_secs) / period).round() as i64;
                i.rem_euclid(4) == 0
            })
        });
        (Some(off * 1000.0), bar)
    } else {
        (None, None)
    };
    Some(Row {
        file: audio.file_name()?.to_string_lossy().into_owned(),
        rb_bpm,
        bpm: grid.bpm,
        same_tempo,
        offset_ms,
        same_bar,
    })
}

fn main() {
    let pairs = std::fs::read_to_string(std::env::args().nth(1).expect("a PAIRS.tsv")).expect("readable pairs");
    let tracks: Vec<(PathBuf, PathBuf)> =
        pairs.lines().filter_map(|l| l.split_once('\t')).map(|(d, a)| (d.into(), a.into())).collect();
    let threads = std::env::var("THREADS")
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()));
    let (next, rows) = (AtomicUsize::new(0), Mutex::new(Vec::new()));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                while let Some((dat, audio)) = tracks.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if let Some(r) = score(dat, audio) {
                        rows.lock().unwrap().push(r);
                    }
                }
            });
        }
    });
    let rows = rows.into_inner().unwrap();
    println!("file\trb_bpm\tbpm\toffset_ms\tsame_bar");
    for r in &rows {
        let off = r.offset_ms.map_or(String::new(), |o| format!("{o:.1}"));
        let bar = r.same_bar.map_or(String::new(), |b| b.to_string());
        println!("{}\t{:.2}\t{:.2}\t{off}\t{bar}", r.file, r.rb_bpm, r.bpm);
    }
    let pct = |n: usize, of: usize| 100.0 * n as f64 / of.max(1) as f64;
    let same: Vec<&Row> = rows.iter().filter(|r| r.same_tempo).collect();
    let exact = same.iter().filter(|r| (r.bpm - r.rb_bpm).abs() < 0.015).count();
    let offs: Vec<f64> = same.iter().filter_map(|r| r.offset_ms).collect();
    let within = |ms: f64| offs.iter().filter(|o| o.abs() <= ms).count();
    let half = same.iter().filter(|r| r.offset_ms.is_some_and(|o| o.abs() > 15_000.0 / r.bpm)).count();
    let bars: Vec<bool> = same.iter().filter_map(|r| r.same_bar).collect();
    eprintln!("{} tracks with a grid of {} paired", rows.len(), tracks.len());
    eprintln!("tempo in rekordbox's octave: {:.1}%", pct(same.len(), rows.len()));
    eprintln!("  and within 0.01 BPM: {:.1}%", pct(exact, same.len()));
    eprintln!(
        "beats within 5 ms: {:.1}%, 10 ms: {:.1}%, 20 ms: {:.1}%",
        pct(within(5.0), offs.len()),
        pct(within(10.0), offs.len()),
        pct(within(20.0), offs.len())
    );
    eprintln!("  half a beat off: {:.1}%", pct(half, offs.len()));
    eprintln!("bar 1 where rekordbox's is: {:.1}%", pct(bars.iter().filter(|b| **b).count(), bars.len()));
}
