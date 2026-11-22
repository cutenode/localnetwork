# Contributing

This covers the whole repository. Each tool has more in its own docs:

- locallibrary: [apps/locallibrary/CONTRIBUTING.md](../apps/locallibrary/CONTRIBUTING.md)
- Local Network, the website: [apps/web/README.md](../apps/web/README.md)
- localtoolbox: [apps/localtoolbox/README.md](../apps/localtoolbox/README.md)

Adding an app or a locallibrary plugin? Follow [NEW_PROJECT.md](NEW_PROJECT.md).

Found a security problem? See [SECURITY.md](SECURITY.md) instead of opening an issue.

Found a bug? "Report a bug" in the website's footer, localtoolbox's settings, or `locallibrary bug` fills in your version and setup for you.

## Setup

Use the Node version in `.node-version` (24). From the repository root:

```bash
pnpm install    # every TypeScript app's dependencies, plus the lefthook git hooks
```

The git hooks format and lint what you commit. Before a push they type-check and test each TypeScript app whose files the push changes (shared files like the lockfile and `.node-version` count for every app), and run clippy for Windows when Rust or Cargo files changed and the `x86_64-pc-windows-gnu` target and MinGW are installed. atstore's tests run in CI only.

`rust-toolchain.toml` pins the Rust toolchain. rustup installs it the first time you run cargo in the checkout.

## Checks

Run the checks for what you touched before you open a pull request. All of them run from the repository root.

```bash
pnpm verify          # the TypeScript apps, the root's own TypeScript (pnpm *:tools) and the lexicons
pnpm verify:rust     # the Rust workspace, as locallibrary-ci.yml's lint and test jobs check it
pnpm verify:repo     # the repository's own conventions (see NEW_PROJECT.md); needs Docker running
pnpm verify:changed  # only the ones the branch's changes call for, committed or not, as ci.yml's filters pick CI's jobs
pnpm verify:rust --package <crate>  # one crate's fmt, clippy and tests, with cargo shear and pnpm policy
```

Each runs its steps in `tools/verify/steps.json` one after another, goes on past a failure, and ends with a line for each step: passed, failed or skipped, how long it took, and the command that reruns it alone.

`pnpm verify:rust` runs cargo fmt, [cargo-shear](https://github.com/Boshen/cargo-shear), clippy (with no default features too, and on `fuzz/`), the headless sidecar's commands test, the doctests, `cargo doc-strict`, [dist](https://axodotdev.github.io/cargo-dist/)'s plan with the check that only locallibrary is released, and [cargo-nextest](https://nexte.st) (`cargo binstall cargo-shear@1.14.0 cargo-nextest@0.9.148 cargo-dist@0.33.0`, the versions CI runs), and clippy for Windows when the `x86_64-pc-windows-gnu` target and MinGW are installed (`rustup target add x86_64-pc-windows-gnu`, `brew install mingw-w64`). `pnpm verify:repo` runs `pnpm policy` (with Docker running), ast-grep's rule tests and scan, `pnpm test:tools`, Bombadil's install and type check, ShellCheck, typos, taplo, goat's lint and breaking checks of the lexicons, and lychee's offline link check (`brew install shellcheck typos-cli taplo go lychee`). Both check for their tools first and stop before the first step, with the install line, when one is missing or Docker isn't running. They warn when typos, taplo or ShellCheck isn't the version `repo-checks.yml` pins, and say what goes unchecked without the Windows target and MinGW (clippy for Windows, which they then skip) or the `sqlcipher` CLI (`brew install sqlcipher`). `pnpm policy` fails when a step of `locallibrary-ci.yml`'s lint job or `repo-checks.yml` is in no verify script's steps and not on the list of CI-only steps in `tools/policy/apps.rego`.

`pnpm verify` leaves out these jobs of the website's CI, each run from `apps/web` (`pnpm exec playwright install chromium` first):

- Lua: `stylua --check lua && cd lua && selene .`
- Storybook build: `pnpm build-storybook`
- integration tests and the coverage thresholds: `pnpm test:coverage`
- end-to-end: `pnpm test:e2e --project chromium`
- Lighthouse: `pnpm test:lighthouse`
- Docker image: `pnpm test:docker`

The `*:tools` scripts format, lint and type-check what no app's own scripts reach: `tools/`, `.railway/`, Bombadil's specs, the flags, budgets and tokens, and the generated RPC types in `apps/locallibrary/docs/rpc/`. The root's `pnpm format:check`, `pnpm lint` and `pnpm check` run them along with every app's. [typos](https://github.com/crate-ci/typos) checks spelling, with the repository's own words in `_typos.toml`, and CI also checks every link in the Markdown with [lychee](https://lychee.cli.rs): relative ones and their anchors on each pull request, the rest weekly. A link that can't work yet goes in `.lycheeignore` with why.

ast-grep's rules (`sgconfig.yml`, in `tools/ast-grep/rules/` with their cases in `tools/ast-grep/tests/`) include `no-direct-pds-read`, which fails a record read that skips HappyView, in TypeScript, Svelte or Rust. Its exceptions are in the rule. clippy (`clippy.toml`) bans jacquard's typed requests for the same reads.

Each Mermaid diagram in an app's README and `docs/diagrams/` names the files it describes on the `Source:` line under it. When a pull request changes one of those files (or anything under a directory it names) and not the Markdown file the diagram is in, the `diagrams` job in Repo checks fails and lists the diagram. Update the diagram, or if it still holds, add a line of its own to the pull request's body for each one it lists, with the file and heading anchor it printed:

```text
diagrams-checked: apps/web/docs/diagrams/accounts.md#scope-fallback: only the log message changed
```

A line covers only the diagram it names, or the diagrams under that heading when it has more than one. A line with the file and no heading, as in `diagrams-checked: apps/web/docs/diagrams/accounts.md: <why>`, covers every diagram in that file. Rerun the job after editing the body. To check a branch first, with the body in a file:

```bash
node tools/readmes/cli.ts drift origin/main body.md
```

`cargo remove` also deletes the root `Cargo.toml`'s `[profile.dev.package.*]` entries, with their comments, for every crate no member depends on directly: its clean-up after a removal ([rust-lang/cargo#11194](https://github.com/rust-lang/cargo/pull/11194), since Cargo 1.67) only keeps the entries for direct dependencies. `pnpm policy` fails while one is missing and its crate is still in `Cargo.lock`, so put it back with `git checkout -p Cargo.toml`. A new entry goes in `dev_optimised` in `tools/policy/cargo.rego` too.

CI (`ci.yml`) runs a tool's checks only when its files (or the shared ones) change, and ends with a "CI passed" check, the one `main` requires before a merge. It's skipped, which counts as passing, unless a check failed. On a draft pull request, nothing runs until you mark it ready for review, so open a pull request as a draft while it isn't next to merge (a stacked one, say). "CI passed" fails on a draft, since none of its checks ran. Marking it ready starts a new run, and if one doesn't show up, use "Re-run all jobs" (re-running only the failed jobs keeps the draft's result). To re-run CI after `main` moves, merge `main` into the branch rather than closing and reopening the pull request.

Pull requests run on Linux, with a clippy check of the Windows code cross-compiled there, and locallibrary's Windows checks when its code changed. Its macOS checks run in the merge queue: once a pull request is approved and passing, add it to the queue, which runs them on it together with the pull requests ahead of it, along with the quick checks (formatting, lint, types, unit tests, the repository's policies), and merges it when they pass. To run them on a pull request first, add the `all-os` label and re-run CI. A push to `main` runs only what pull requests use from it: the coverage and sizes they're compared with, the caches, and CodeQL. The slow suites run weekly in the `*-scheduled.yml` workflows, each skipping the week when nothing it tests changed since its last run that passed, and you can start them on any branch from the Actions tab. A weekly run that fails opens an issue labelled `scheduled-failure`, and the next weekly run that passes closes it.

On a pull request, each tool's workflow keeps one comment up to date with its coverage: the Rust workspace's, with the Hegel property tests' alone beside it, and the website's and the extension's against `main`, with each changed file's. The website's and the extension's comments also show how the change moves what they ship against `main`: the website's client JavaScript and CSS, and the extension's zip. A pull request from a fork can't be commented on, so it finds the same in the run's summary. The website's Lighthouse scores are in the run's summary too, when the change touches what its pages are built from.

locallibrary's performance budgets ([benchmarks/README.md](../benchmarks/README.md)) hold a pull request to how much it grows each number over the commit it merges into, and a merge queue group to how much its pull requests grow them together, so main's growth never turns a pull request red and no pull request edits a committed number. To grow one on purpose, add a commit with a `Budget-Raise: <area>/<metric> +<amount>: <why>` trailer; the failure gives the exact line.

Each project also holds a pull request to the coverage of the lines it adds or changes: every one of them, the share `tools/coverage/patch.json` sets, by [diff-cover](https://github.com/Bachmann1234/diff_cover) over the project's lcov report (`.github/actions/patch-coverage`), with the changed files' coverage in the run's summary. Code compiled only for another platform isn't in a Linux report, so it doesn't count. A file no test can reach goes in that file's `exclude`, with the reason in the pull request that adds it. The projects' own thresholds still hold their totals. The Rust workspace's per-crate floors (`tools/coverage/floors.json`) only warn, since main's merges move them.

Pull requests that change a lockfile, a manifest or a workflow also get a dependency review (`dependency-review.yml`, which `ci.yml` calls, so it holds up the merge like any other check). It fails when a dependency the pull request adds or updates (dev tooling aside) has a known advisory or a licence `deny.toml` doesn't allow (fonts may also be OFL-1.1), and then comments on the pull request with what it found. GitHub's dependency graph can't read pnpm 12's two-document `pnpm-lock.yaml`, so the workflow submits the lockfile's packages for the pull request first, and `ci.yml` submits them for each push to `main`, the base the review compares with. It ignores the advisories `pnpm audit` does (`auditConfig.ignoreGhsas` in `pnpm-workspace.yaml`). A pull request from a fork can't submit, so its review only covers the npm packages the `package.json` files name.

CodeQL (`codeql.yml`, which `ci.yml` calls) scans the workflows, the TypeScript and the Rust on every pull request into `main`, merge queue group and push to `main` (a language whose code didn't change uploads `main`'s analysis of the commit it was diffed against again, in place of a new one), and `main` won't take a pull request that adds a high or critical security alert, or an error. The Rust tests are left out (`.github/codeql/codeql-config.yml`), so a new `#[cfg(test)]` module needs one of the names listed there, which `ast-grep scan` checks.

## Pull requests

- Keep each pull request to one concern.
- Fill in the template.
- Link the issue it's for, as in [Issues and planning](#issues-and-planning). The PR issue check fails otherwise; pull requests from bots skip it.
- For UI changes, add before and after screenshots at phone and wide widths.
- Pull requests merge with a merge commit. Squash and rebase merges are off.
- Write each commit's subject as a [Conventional Commit](https://www.conventionalcommits.org), `type(scope): summary`. CI checks every commit a pull request adds with commitlint (`commitlint.config.js`), and the `commit-msg` hook checks yours as you commit.

Labels are added automatically from the files a pull request changes ([labeler.yml](labeler.yml)): one for each tool (`web`, `locallibrary`, `localtoolbox`, `lexicons`, `atstore`), one for each area inside a tool (`web: epk`, `locallibrary: player`), and `ci`, `tooling`, `security`, `performance`, `accessibility` and `dependencies`. `tests` and `documentation` go on a pull request that changes only tests or only docs. [CODEOWNERS](CODEOWNERS) requests a review from whoever owns the tools it changes.

## Issues and planning

Open issues here on GitHub, with the issue forms. Each one is synced both ways with the Localnetwork team (`LOC`) in [Linear](https://linear.app/localnetwork/team/LOC), where they're triaged and planned: title, description, status, assignee, labels, priority, sub-issues and comments, so an issue can be worked on from either side. Comments left in Linear outside the synced thread stay in Linear.

- Status: a new issue starts in Triage, then Backlog or Todo once someone has looked at it, In Progress, In Review and Done (or Canceled). Closing it on GitHub makes it Done, and reopening it brings it back.
- Pull requests: `Fixes #123` or `Fixes LOC-123` in the description closes the issue when the pull request merges, and `Part of LOC-123` links it without closing it. The PR issue check needs one of them, with its word: a bare `LOC-123` or `#123` doesn't count, and `#123` has to be an issue, not a pull request. It takes [Linear's magic words](https://linear.app/docs/github#magic-words) for `LOC-123` (`Closes`, `Resolves`, `Refs` and the rest), and the [keywords GitHub reads](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/linking-a-pull-request-to-an-issue) for `#123`. A branch named from Linear's "Copy git branch name" (`bnb/loc-123-…`) links it as soon as it's pushed, and passes the check too. GitHub reads its keywords only on a pull request into `main`, so a stacked pull request needs `Fixes LOC-123`, `Part of LOC-123` or that branch until it targets `main`. A linked pull request moves its issue to In Progress when it's opened, In Review when a review is requested, and Done when it merges.
- Labels: the ones in [labels.yml](labels.yml), a tool's (`locallibrary`) and an area's (`locallibrary: player`), and one for what kind of issue it is: `bug`, `enhancement`, `scheduled-failure`, or work across the tools (`tests`, `documentation`, `ci`, `tooling`, `security`, `performance`, `accessibility`, `design`). `needs decision` and `blocked` say why an issue is waiting. To move an issue to another tool or area, change its label.
- Priority and estimate are set while triaging. A release is a Linear project, like `locallibrary v0.1.0`, with a target date. Big work gets sub-issues, and work that has to wait gets a "blocked by" relation.
