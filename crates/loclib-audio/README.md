# loclib-audio

Decodes the audio a DJ deals with, tracks and recordings alike, without trusting what a file's
headers say. It was split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which reads
files from rekordbox USB drives and recordings of sets, where one damaged or crafted file shouldn't
take the program down with it.

Everything a drive or a recorder produces (MP3, AAC and ALAC in `.m4a`, FLAC, WAV, AIFF) decodes
through [symphonia](https://github.com/pdeljanov/Symphonia).

- `decode_native` hands over a file's samples at its own rate and channel count, in packets, so a
  long 24/96 file never sits in memory.
- `decode` and `decode_until` average the channels to mono and resample to the rate you ask for as
  they stream, so an hour-long recording never sits in memory at full rate.
- `Resampler` is the windowed-sinc resampler behind them, for other sources (an editor's project,
  a live input) to feed themselves.

```rust,no_run
use loclib_audio::decode;
use loclib_progress::Ctx;

# fn main() -> loclib_audio::Result<()> {
let mut loudest = 0.0f32;
let format = decode("set.flac".as_ref(), 11_025, &Ctx::silent(), &mut |samples| {
    loudest = samples.iter().fold(loudest, |m, s| m.max(s.abs()));
})?;
println!("{} Hz, {} channels, {:.0} s, peak {loudest:.2}", format.sample_rate, format.channels, format.duration_secs);
# Ok(())
# }
```

## Files it doesn't trust

symphonia trusts what a file's headers say: a WAV saying 0 Hz panics it. `decode` and
`decode_native` catch that, so one bad file is that file's error (`CRASHED`) rather than a crash;
a program that wants the panic's message kept off stderr installs its own panic hook around the
call. An MP4 claiming huge sample counts would make it abort the process instead, so `open` checks
its sample tables against the file's size first (`vet_container`). A header saying more than
`MAX_RATE` (768 kHz) is refused, since resampling from it would take memory in proportion to the
rate.

Decoding checks the `Ctx` for cancellation. A `decode_native` sink stops it by returning an error
or `ControlFlow::Break`, a `decode_until` sink by breaking.

## What it doesn't do

- Encode or write audio. loclib-transcode writes AIFF, WAV and FLAC copies.
- Read tags or artwork.
- Play audio, or decode in real time from a device.

## Licence

MIT.
