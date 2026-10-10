# loclib-progress

Progress events and cooperative cancellation for long-running DJ library work, such as copying a
library to a drive, hashing files or fingerprinting tracks. It was split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), whose CLI draws these events as progress
bars and whose JSON-RPC sidecar forwards them to a desktop app.

An operation takes a `Ctx`, reports phases, items, bytes and warnings through it, and calls
`Ctx::check` between steps. `check` fails with `Cancelled` once anything holding a clone of the
context's `CancelToken` cancels it. `Steps` and `Ctx::for_target` mark events with the step and
the drive they belong to (a `Scope`), and `WarningLimiter` passes on the first few warnings of
each kind and sums up the rest. Events serialise as tagged JSON objects, such as
`{"event":"phase_start","phase":"copy","items":2,"bytes":0}`.

```rust
use loclib_progress::{CancelToken, Cancelled, Ctx, Event, Phase};

fn copy_all(files: &[&str], ctx: &Ctx<'_>) -> Result<(), Cancelled> {
    ctx.phase_start(Phase::Copy, files.len() as u64, 0);
    for file in files {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Copy, path: file.to_string(), bytes: 0 });
    }
    ctx.phase_end(Phase::Copy);
    Ok(())
}

let print = |event: &Event| println!("{event:?}");
let ctx = Ctx::new(&print, CancelToken::new());
assert_eq!(copy_all(&["Contents/a.mp3", "Contents/b.mp3"], &ctx), Ok(()));

ctx.cancel.cancel();
assert_eq!(copy_all(&["Contents/c.mp3"], &ctx), Err(Cancelled));
```

## Features

- `schemars`: JSON Schema (`schemars::JsonSchema`) for the event types.

## What it doesn't do

- Draw anything. Progress bars, logs and UIs are up to the front end.
- Stop work by force. Cancelling sets a flag that the work checks between steps, so nothing is
  interrupted mid-write, and there are no timeouts.
- Run anything. There are no threads and no async runtime.
- Take phases of your own. `Phase` is a fixed list of the phases locallibrary has (`scan`,
  `hash`, `copy`, `write`, `delete`, `verify`, `format`, `fingerprint`, `index`, `listen`,
  `match`).

## Licence

MIT.
