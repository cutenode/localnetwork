# Performance budgets

`budgets/` lists each number we hold locallibrary and the website's client bundle to, one file per area, so branches don't collide
on a shared list. Every number is one a run gives exactly: bytes, allocations, instructions under
cachegrind, the names of the packages the CLI links. Wall-clock time on a shared runner isn't, so
it's never gated on; `cargo bench` and CodSpeed still track time and instructions weekly
(`.github/workflows/locallibrary-bench.yml`).

The files hold no limits. A pull request is held to how much it grows each number over the commit
it merges into, so main's own growth never turns it red and it never edits these files.

| area | what it holds | measured by | when |
| --- | --- | --- | --- |
| `alloc` | allocations and bytes of reading, filtering, comparing and planning | `crates/locallibrary/tests/counters.rs`, in the `budgets` job of `locallibrary-ci.yml` | every pull request that runs the Linux jobs, and merge queue groups |
| `export` | what writing a library to a blank drive asks of the filesystem: files and folders made, renames, flushes, writes and their size | `crates/locallibrary/tests/export_counters.rs`, in the same job, under `strace` | the same |
| `tui` | allocations and bytes of a frame of the player, for the Library view, the Deck layout, the Drives view and the Prepare view | `apps/locallibrary/src/tui/frames/tests.rs`, in the same job | the same |
| `deps` | the names of the packages linked into the default CLI on Linux, whatever their version | `cargo tree`, in `.github/actions/release-budgets` | weekly, by hand, merge queue groups, and pull requests that change a manifest, the lockfile or the Rust that ships |
| `size-x86_64-unknown-linux-gnu` | release binary and archive | `.github/actions/release-budgets` | the same |
| `cli` | instructions of everyday commands on the 3,886-track fixture, less a run of `/bin/true` on the same runner: the loader and libc starting, which counts differently on different runners' CPUs. Each runs with only the environment it needs (`tools/budgets/cachegrind.sh`), since the environment's size moves the count by thousands, and with plugins' checks off, which `check` and `doctor` would otherwise hand the whole library to | the same | the same |
| `tui-instructions` | instructions of a frame of the player's draw, for the same views, counted inside the frame test's `frame` alone, so not setting the player up or the audio before each frame. The draw still reads the clock for its animations, so these move by up to about 1,600 between runs of the same binary | the same, running the `tui` test | the same |
| `tui-debug-instructions` | instructions of a frame of the player for the same views in the debug profile, which catch a regression and aren't what a user's frame costs. Counted as `tui-instructions` is | `tools/budgets/frame-instructions.sh`, on the `unit` test binary of the archive the `budgets` job runs | every pull request that runs the Linux jobs, and merge queue groups |
| `web` | the website's JavaScript every page loads, the largest page's own on top of it, and all its CSS, uncompressed | `apps/web/scripts/bundle-sizes.ts`, after `pnpm test:build` in the `verify` job of `web-ci.yml` | pull requests and merge queue groups that change the website, and pushes to main |
| `size-<other target>` | release binary and archive, recorded in the run's summary with no base to compare with | `locallibrary-scheduled.yml`'s `release-build` | weekly and by hand |

## Growth, not limits

Each metric has an `allowance` in its own unit: how much a pull request may grow it without saying
so, sized to the measurement's run-to-run noise. A pull request's run compares what it measures
with its base, the commit its merge commit sits on (`HEAD^1`), and fails a metric that grew by more
than its allowance. Getting smaller always passes. For a list of names, the growth is the names that
are new.

The base's numbers come from the CI run that measured that commit, or the nearest commit before it
on main that changed none of what the job measures (`tools/budgets/baseline.ts`, which reads
`ci.yml`'s paths filter for the job and lists the job's artifacts once). Merge queue groups measure
what the `budgets` job does and run `release-budgets`, and a group's head becomes main, so a commit
has its numbers when it lands. The artifacts keep the repository's default retention. When no run
measured the base, the job measures it too and says so in a notice.

A pull request's merge commit has to sit on a base with its branch's current `tools/budgets/`
(`tools/budgets/cli.ts current`), or the job fails before measuring: GitHub's merge commit can be
on an old base, which would check with old tooling. Close and reopen the pull request once
`gh api repos/<owner>/<repo>/pulls/<number> --jq .merge_commit_sha` prints a merge commit on the
current base (the message gives the command), and update the branch only if that doesn't happen.

A merge queue group runs `release-budgets` on its pull requests together, against the group's base,
and allows each pull request in it its allowance, so two that each passed alone can't land over
together.

Growing a metric past its allowance on purpose takes a `Budget-Raise` trailer on one of the pull
request's commits, saying by how much in the metric's unit and why. A commit of its own does it:

```bash
git commit --allow-empty -m 'chore(budgets): make room for the Opus decoder' \
  --trailer 'Budget-Raise: size-x86_64-unknown-linux-gnu/locallibrary +409600: the Opus decoder'
```

The failure says the exact trailer it needs. Raises add up, on top of the allowance. In a commit,
review covers a raise, and the merge queue reads the trailers of every pull request in a group. A
package name counts as 1, so a new crate takes `Budget-Raise: deps/locallibrary +1: <why>`.

Widening an allowance or dropping a metric takes a trailer naming it too (`+0` will do), which the
`budgets` job checks with `tools/budgets/cli.ts raises`.

Until 2026-11-30, a `budget-raise: <metric>: <why>` line in the pull request's body still allows any
growth, for pull requests opened before trailers. Rerun the job after editing the body.

To run the check yourself, download a run's measurements and its base's:

```bash
gh run download <run> -n budgets -D target/budgets
gh run download <base-run> -n budgets -D target/budgets-base
node tools/budgets/cli.ts check pull-request target/budgets-base target/budgets
```

## Creep and noise

Each pull request is held to its own growth, so many small ones can still add up. The weekly
`budget-creep` job in `repo-scheduled.yml` lays out what the merge queue's and main's runs measured
over the week (`tools/budgets/points.sh`) and sums each metric's growth with `tools/budgets/cli.ts
creep`: how much `Budget-Raise` trailers declared, how much came in under allowances, and the pull
requests that grew it most. It fails, which opens a `scheduled-failure` issue, when a metric's
undeclared growth passes five allowances.

An allowance is only as good as the noise under it. The weekly `noise` job in
`locallibrary-bench.yml` measures one commit five times over (`tools/budgets/measure-archive.sh`
and `tools/budgets/measure-release.sh`) and fails a metric whose runs spread over half its
allowance, with `tools/budgets/cli.ts noise`. Widen that allowance, with a `Budget-Raise: <metric>
+0: <why>` trailer.

## Adding a metric

Add it to its area's file with an `allowance` and a sentence on what it measures, and write it to
`<area>.json` where its job measures into. Until a base has it, any number passes. A new area is a
new file whose `job` says which job measures it; nothing else lists areas. `budget.schema.json`
says what a file may hold.
