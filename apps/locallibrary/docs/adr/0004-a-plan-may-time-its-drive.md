# A plan may time the drive it writes to

To say how long a write will take before it starts, locallibrary has to know what the drive costs,
and only a timed write on that drive tells. So the plan of a write big enough to matter times a
drive that has no result, or whose result has gone stale, before it's shown. This is part of what a
plan is now: making a plan writes nothing to a drive, except that one may time a drive and say so.
What it writes is scratch data in the drive's `.locallibrary/`, removed after, and the result in
`.locallibrary/speed.json`. The test never writes more than the write it's for. Three things keep
this from being a surprise: a dry run or a plan made for later (`--json` without `--yes`, RPC)
writes nothing and says what's known, the plan says it ran a test, and a plan that isn't applied
leaves a drive that had no `.locallibrary/` as it found it. That last one means saying no on a new
drive costs the next plan the same test again, which is the better trade: a drive that was never
written to stays that way.

The port's speed stays out of the sum. On the drives we've timed it was never the limit, and most of
an export went on making files and folders. It's only said beside the estimate when a rate comes
close to what the port can carry.

## Considered options

- Estimate from the port alone: wrong by a factor of three or more on every drive timed, since a
  stick on a 5 Gb/s link writes 150 MB/s and takes 12 ms a file.
- Time the drive after the user says yes: the point is to know before saying yes.
- Keep the result when a plan on a new drive is declined: the next plan saves 20 seconds, but
  saying no leaves a hidden folder on a drive locallibrary was told not to write to.
- Ask before timing: a second question ahead of the one that matters, on every new drive.
- Learn only from real writes: nothing to say on a drive's first export, when it matters most.
  Real writes do correct the rate afterwards, since they run past any cache region a short test
  stays inside.
- Reuse the capacity probe, which already writes test regions when formatting: 256 scattered 1 MiB
  writes say nothing about sustained speed, and it only runs on a disk being erased.
