# loclib-waveform

[ratatui](https://ratatui.rs) widgets for music software: waveforms, cue markers, phrases, a
spectrum, a scope and cover art, drawn in block and braille characters. They were split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary)'s terminal player, which draws
rekordbox's analysis with them.

| Widget | What it draws |
| --- | --- |
| `Overview` | the whole track: cue markers above a two-layer waveform, the played part dimmed, and the playhead (a plain progress rail without a waveform) |
| `WaveOverview` | `Overview` of any `Wave`, with a second row of cue markers where they collide |
| `Detail` | a scrolling, mirrored waveform centred on the playhead, with beat ticks, cue lines, loop washes and cue names |
| `ZoomRing` | what `Detail` draws from a `Zoom`, keeping columns between frames so one costs the same at any zoom or track length, with a ruler and bar numbers |
| `CueLane` | the stretch of track around the playhead as one rail: hot cues as lettered chips, memory cues as diamonds, loops as bars, names underneath |
| `PhraseStrip` | phrases across the whole track, each a block of its colour with its name |
| `Spectrum` | bars with peak caps, coloured by height, optionally mirrored with the bass in the middle |
| `Scope` | an oscilloscope trace in braille, auto-gained to fill the height |
| `Pixels` | an image in half blocks, two pixels a cell |

`vinyl` makes the pixels of a spinning record for tracks without artwork, and `Art` decodes
cover art, resamples it to a cell size and finds its two main colours. `cue_label` and `cue_letter` give a cue's on-screen
name and hot cue letter, and `bar_layout` how many spectrum bands fit a width.

A `Wave` is what a waveform's drawn from: a `Waveform`'s two layers, or a `Bands`' three (`band_gain`
turns a quiet master up to the height). A column's boundaries fall on eighths of a cell, a cell
showing the two colours either side of one. `playhead_cells` draws a playhead a cell wide that moves
in eighths, and `place_markers` lays out cue markers in rows so no two share a cell and no two hot
cue letters touch.

The track data is loclib-model's `analysis` types (`Waveform`, `Beat`, `Cue`), which locallibrary
reads from rekordbox's ANLZ files.

```rust
use loclib_model::analysis::{Cue, WaveColumn, WaveLayer, Waveform, WaveformStyle};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;
use loclib_waveform::{Overview, Palette};

let column = WaveColumn {
    back: WaveLayer { height: 0.8, color: [40, 120, 255] },
    front: WaveLayer { height: 0.4, color: [255, 255, 255] },
};
let wave = Waveform { style: WaveformStyle::Rgb, columns_per_sec: None, columns: vec![column; 400] };
let drop = Cue {
    hot_cue: Some(0),
    start_ms: 60_000,
    end_ms: None,
    comment: "Drop".into(),
    color: None,
    color_id: 0,
    loop_beats: None,
    active_loop: false,
};

let area = Rect::new(0, 0, 40, 4);
let mut buf = Buffer::empty(area);
Overview {
    wave: Some(&wave),
    progress: 0.25,
    cues: &[drop],
    duration_ms: 240_000.0,
    palette: Palette::default(),
}
.render(area, &mut buf);
assert_eq!(buf[(10, 0)].symbol(), "A"); // hot cue A, a quarter of the way in
```

Every widget takes a `Palette`: the colours it draws in, and functions that turn a colour into
what the terminal draws (`paint`, and `image` for pictures). `Palette::of(&theme)` has a
loclib-model `Theme`'s colours (its waveform gradients, cue and track colours, playhead), and
`Palette::default()` the default theme's, for a dark terminal in 24-bit colour. For a light
background or a terminal of fewer colours, give it your own `paint`, `marker` (a cue's or a
waveform's own colour as it reads on the background) and `background` (what colours fade towards),
and `plain: true` for NO_COLOR, where coloured cells show in reverse video and pictures are left
out. `flat: true` is for colours that can't be trusted to shade: what's played takes the dim
attribute, nothing's tinted, and hot cues' letters are in reverse video. `color::{mix, scale, gradient}` is the colour arithmetic the widgets shade with, and
`color::{luminance, contrast}` WCAG's.

## A spectrum and a scope from audio

With the `analyzer` feature, `analyzer::Analyzer` turns the samples being played into what
`Spectrum` and `Scope` draw. Fed the latest samples once a frame, it keeps each spectrum's bands
(log-spaced from 30 Hz to 16 kHz, rising fast and falling as a meter does, with peak caps that hold
a moment, then drop), an overall `level`, and a `scope` trace that starts on a rising zero crossing
so it stands still. One FFT a frame feeds every spectrum on screen, whatever its number of bands.
Given `None` (paused, stopped), it all falls back to silence, and `is_idle` says when there's
nothing left to move.

```rust
use loclib_waveform::analyzer::Analyzer;
use loclib_waveform::{Palette, Spectrum, bar_layout};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

let area = Rect::new(0, 0, 60, 8);
let (n, _, _) = bar_layout(area.width, false);
let mut analyzer = Analyzer::default();
// A frame's worth: the latest samples played (mono, a 440 Hz tone at 44.1 kHz) and the time since
// the last frame.
let latest: Vec<f32> = (0..4096).map(|i| (i as f32 * 0.0627).sin() / 2.0).collect();
analyzer.update(Some(&latest), 44_100, &[n], 1.0 / 60.0);

let bands = analyzer.bands(n).unwrap();
assert!(bands.levels.iter().any(|&level| level > 0.3));
let mut buf = Buffer::empty(area);
Spectrum { bands: &bands.levels, peaks: &bands.peaks, mirror: false, tint: None, palette: Palette::default() }
    .render(area, &mut buf);
```

## Features

- `image`: `Art`, decoding cover art (JPEG, PNG, WebP, BMP, GIF) and resampling it to cells.
- `analyzer`: `analyzer::Analyzer`, the analysis behind a spectrum and a scope (an FFT, with
  rustfft).

## What it doesn't do

It draws what it's given. Reading analysis files, decoding and playing audio and laying the widgets
out are up to you.

## Licence

MIT.
