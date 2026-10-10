# loclib-waveform

[ratatui](https://ratatui.rs) widgets for music software: waveforms, cue markers, phrases, a
spectrum, a scope and cover art, drawn in block and braille characters. They were split out of
[locallibrary](https://github.com/cutenode/locallibrary)'s terminal player, which draws
rekordbox's analysis with them.

| Widget | What it draws |
| --- | --- |
| `Overview` | the whole track: cue markers above a two-layer waveform, the played part dimmed, and the playhead (a plain progress rail without a waveform) |
| `Detail` | a scrolling, mirrored waveform centred on the playhead, with beat ticks, cue lines, loop washes and cue names |
| `CueLane` | the stretch of track around the playhead as one rail: hot cues as lettered chips, memory cues as diamonds, loops as bars, names underneath |
| `PhraseStrip` | phrases across the whole track, each a block of its colour with its name |
| `Spectrum` | bars with peak caps, coloured by height, optionally mirrored with the bass in the middle |
| `Scope` | an oscilloscope trace in braille, auto-gained to fill the height |
| `Pixels` | an image in half blocks, two pixels a cell |

`vinyl` makes the pixels of a spinning record for tracks without artwork, and `Art` decodes
cover art and resamples it to a cell size. `cue_label` and `cue_letter` give a cue's on-screen
name and hot cue letter, and `bar_layout` how many spectrum bands fit a width.

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
what the terminal draws (`paint`, and `image` for pictures). `Palette::default()` is for a dark
terminal in 24-bit colour; give it your own `paint` for a light background or a 256-colour
terminal, and `plain: true` for NO_COLOR, where coloured cells show in reverse video and
pictures are left out. `color::{mix, scale, gradient}` is the colour arithmetic the widgets shade
with.

## Features

- `image`: `Art`, decoding cover art (JPEG, PNG, WebP, BMP, GIF) and resampling it to cells.

## What it doesn't do

It draws what it's given. Reading analysis files, decoding audio, the FFT behind a spectrum,
letting peak caps fall and laying the widgets out are up to you.

## Licence

MIT.
