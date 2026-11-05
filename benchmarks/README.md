# Performance budgets

`budgets/` holds a limit for each number we hold locallibrary to, one file per area, so branches
don't collide on a shared list. Every number is one a run gives exactly: bytes, allocations,
instructions under cachegrind, package counts. Wall-clock time on a shared runner isn't, so it's
never gated on; `cargo bench` and CodSpeed still track time and instructions weekly
(`.github/workflows/locallibrary-bench.yml`).

| area | what it holds | measured by | when |
| --- | --- | --- | --- |
| `alloc` | allocations and bytes of reading, filtering, comparing and planning | `crates/locallibrary/tests/counters.rs`, in the `budgets` job of `locallibrary-ci.yml` | every pull request that runs the Linux jobs |
| `deps` | packages linked into the default CLI on Linux | `cargo tree`, in the same job | the same |
| `size-x86_64-unknown-linux-gnu` | release binary and archive | `.github/actions/release-budgets` | weekly, by hand, and pull requests that change a manifest or the lockfile |
| `cli` | instructions of everyday commands on the 3,886-track fixture | the same | the same |
| `size-<other target>` | release binary and archive | `locallibrary-scheduled.yml`'s `release-build` | weekly and by hand |

## The ratchet

A measurement over its limit plus the file's `tolerance_percent` fails. So does one under its limit
by more than that: an improvement has to be locked in, so it can't quietly erode. To lock it in,
take the numbers from CI, since limits are the runner's: a byte count includes the length of its
paths, and a binary's size is its target's.

```bash
gh run download <run> -n budgets -D target/budgets
node tools/budgets/cli.ts ratchet target/budgets pull-request
```

`ratchet` only lowers limits, skips what's unchanged and can run any number of times. The
`release-budgets` artifact goes through it the same way, with the job `release`. A limit of `null`
means the right machine hasn't measured it yet: anything passes, and `ratchet` sets it.

Loosening a budget (raising a limit, dropping a metric, widening a tolerance) needs a line in the
pull request's body, which the `budgets` job checks with `tools/budgets/cli.ts raises`:

```text
budget-raise: size-x86_64-unknown-linux-gnu/locallibrary: the Opus decoder adds 400 KiB
```

Rerun the job after editing the body.

## Adding a metric

Add it to its area's file with `"limit": null` and a sentence on what it measures, write it to
`<area>.json` where its job measures into, then ratchet. A new area is a new file whose `job` says
which job measures it; nothing else lists areas. `budget.schema.json` says what a file may hold.
