# loclib-aup

Reads an Audacity project without Audacity: its tags, its labels, and its audio mixed down to
mono. It was split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which finds
the tracks a recorded DJ set plays, and takes the set as the Audacity project it was edited in as
readily as an exported file.

It reads Audacity 3's `.aup3` and Audacity 4's `.aup4`, which are the same container: a SQLite
database whose `project` table holds the document as Audacity's binary XML (a dictionary of names,
then a document of tokens referring to them) and whose `sampleblocks` table holds the audio. Each
wave track is one channel, made of clips placed on the timeline; a clip plays a trimmed window of
its sequence of blocks.

- `Project::open` reads the document, or the autosave of a project that was never saved. A
  document missing its closing tags (Audacity stopped mid-write) opens with what's there.
- `tags` and `tag` give the project's metadata (TITLE, ARTIST, GENRE...), and `labels` the labels
  of its label tracks.
- `render` mixes every audible track to mono and resamples it to the rate you ask for with
  [loclib-audio](https://github.com/cutenode/localnetwork/tree/main/crates/loclib-audio)'s
  `Resampler`, in chunks, so an hour-long set never sits in memory. Each clip plays its trimmed
  window where it sits on the timeline, at its track's gain; muted tracks are left out, and while
  any track is soloed only soloed ones are heard. 16-bit, 24-bit and 32-bit float samples all
  read.
- `duration_secs` is where the last audible clip ends.

```rust,no_run
use loclib_aup::Project;
use loclib_progress::Ctx;

# fn main() -> loclib_aup::Result<()> {
let project = Project::open("set.aup3".as_ref())?;
for label in project.labels() {
    println!("{:>7.1} s  {}", label.start, label.title);
}
let mut loudest = 0.0f32;
let format = project.render(11_025, &Ctx::silent(), &mut |samples| {
    loudest = samples.iter().fold(loudest, |m, s| m.max(s.abs()));
})?;
let title = project.tag("title").unwrap_or("untitled");
println!("{title}: {:.0} s at {} Hz, peak {loudest:.2}", format.duration_secs, format.sample_rate);
# Ok(())
# }
```

## Files it doesn't trust

A project is whatever file it's handed, so it opens it read-only and hardened with
[loclib-sqlcipher](https://github.com/cutenode/localnetwork/tree/main/crates/loclib-sqlcipher)'s
`untrusted::distrust`: SQL in the schema can only call functions without side effects, and a view,
which Audacity never makes, is refused, since one standing in for a table could loop for ever when
read. A damaged document is an error naming the file (`Error::Parse`), never a panic: elements
nested more than 64 deep and sample rates outside 1 Hz to 1 MHz are refused. Rendering checks the
`Ctx` for cancellation.

`Project` keeps the file's path, and `render` opens the file again to read the audio.

## Features

- `schemars`: JSON Schema (`schemars::JsonSchema`) for `Label`.
- `test-util`: `test_util`, which writes projects for tests in the crates built on this one. It
  isn't part of the stable API.

## What it doesn't do

- Pan, envelopes, effects, or any other processing Audacity applies on playback. Gain and
  mute/solo are all it honours.
- Keep stereo: a stereo track's two channels are mixed into the one mono signal with the rest.
- Resample a track recorded at another rate than the project's: its samples play at the
  project's rate.
- Stretch time the way Audacity does. A clip's speed (Audacity 3.4+) spreads its samples over the
  longer or shorter span, without keeping the pitch.
- Read Audacity 2's `.aup` (XML with a folder of `.au` files), or write projects.

## Credits

The file format is [Audacity](https://www.audacityteam.org)'s: the tables its project files hold,
and the binary XML its project serializer writes. This crate isn't part of Audacity or affiliated
with it.

## Licence

MIT.
