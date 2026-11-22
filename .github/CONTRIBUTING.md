# Contributing

This covers the whole repository. Each tool has more in its own docs:

- locallibrary: [apps/locallibrary/CONTRIBUTING.md](../apps/locallibrary/CONTRIBUTING.md)
- Local Network, the website: [apps/web/README.md](../apps/web/README.md)
- localtoolbox: [apps/localtoolbox/README.md](../apps/localtoolbox/README.md)

Adding an app or a locallibrary plugin? Follow [NEW_PROJECT.md](NEW_PROJECT.md).

Found a security problem? See [SECURITY.md](SECURITY.md) instead of opening an issue.

Found a bug? "Report a bug" in the website's footer, localtoolbox's settings, or `locallibrary bug` fills in your version and setup for you.

## Setup

Use the Node version in `.node-version` (24) and the pnpm in `package.json`'s `packageManager` (12.6.0). [fnm](https://github.com/Schniz/fnm) picks up `.node-version` (`fnm install && fnm use`). From the repository root:

```bash
pnpm install    # every TypeScript app's dependencies, plus the lefthook git hooks
```

The git hooks format and lint what you commit. Before a push they type-check and test each TypeScript app whose files the push changes (shared files like the lockfile and `.node-version` count for every app), and run clippy for Windows when Rust or Cargo files changed and the `x86_64-pc-windows-gnu` target and MinGW are installed. atstore's tests run in CI only.

A push that changes the website or the extension waits minutes for the pre-push hook, and git opens its SSH connection to GitHub before the hook starts, so GitHub can drop the connection as idle. The push then fails with exit 141 or "Connection closed" though every check passed. If you push over SSH, keep the connection alive in `~/.ssh/config`:

```text
Host github.com
  ServerAliveInterval 30
  ServerAliveCountMax 10
```

A push that fails after the hook passed lost its connection: push again, and don't skip the hook.

`rust-toolchain.toml` pins the Rust toolchain (1.96.1). rustup installs it the first time you run cargo in the checkout. A Rust that doesn't come from rustup (Homebrew's, say) ignores the file, and its clippy may disagree with CI's.

The rest depends on what you touch. CI pins each version below, and an older one may report different findings. The cargo tools install quickest with [cargo-binstall](https://github.com/cargo-bins/cargo-binstall) (`brew install cargo-binstall`); `cargo install --locked` builds them instead.

| Tool | For | Install |
| --- | --- | --- |
| Docker or [OrbStack](https://orbstack.dev) | the website's local network and its integration, e2e, Lighthouse and image tests; `pnpm policy` | running in the background |
| Playwright's Chromium | the website's and atstore's browser tests | `pnpm -C apps/web exec playwright install chromium` |
| StyLua 2.6.0 and selene 0.32.0 | HappyView's Lua scripts in `apps/web/lua/` | `brew install stylua selene` |
| typos 1.51.1 | spelling, with the repository's words in `_typos.toml` | `brew install typos-cli` |
| taplo 0.10.0 | every TOML file's format (`taplo.toml`) | `brew install taplo` |
| ShellCheck 0.11.0 | every shell script | `brew install shellcheck` |
| lychee 0.24.2 | the links in the Markdown | `brew install lychee` |
| Go 1.27 or newer | goat, which `pnpm lexicons:goat` runs with `go run` | `brew install go` |
| actionlint 1.7.12 and zizmor 1.31.1 | the workflows | `brew install actionlint zizmor` |
| cargo-nextest 0.9.148, cargo-shear 1.14.0 and dist 0.33.0 | `pnpm verify:rust` | `cargo binstall cargo-nextest@0.9.148 cargo-shear@1.14.0 cargo-dist@0.33.0` |
| cargo-llvm-cov 0.9.1, cargo-deny 0.20.2 and cargo-about 0.9.3 | coverage, the dependency checks and the third-party notices | `cargo binstall cargo-llvm-cov@0.9.1 cargo-deny@0.20.2 cargo-about@0.9.3` |
| The Windows target and MinGW | clippy for the Windows code | `rustup target add x86_64-pc-windows-gnu`, then `brew install mingw-w64` (or `gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64` on Debian and Ubuntu) |
| ALSA and SQLCipher | locallibrary's player and its SQLCipher tests (ALSA is Linux only) | `brew install sqlcipher`, or `libasound2-dev sqlcipher` on Debian and Ubuntu |
| valgrind | `tools/budgets/cachegrind.test.ts`, which skips without it (Linux only) | `valgrind` on Debian and Ubuntu |
| Rust 1.91 | the oldest Rust locallibrary supports (`rust-version` in `Cargo.toml`) | `rustup toolchain install 1.91 --profile minimal` |
| cargo-mutants 27.1.0 | Rust mutation testing | `cargo binstall cargo-mutants@27.1.0` |
| nightly Rust and cargo-fuzz 0.13.2 | fuzzing ([Fuzzing](../apps/locallibrary/CONTRIBUTING.md#fuzzing)) | `rustup toolchain install nightly --component rust-src && cargo binstall cargo-fuzz@0.13.2` |

## Running the apps

```bash
pnpm dev                          # the website on a local network with demo accounts (http://127.0.0.1:5173), and the extension's watch build
pnpm -C apps/web dev:network      # the website's local network alone (HappyView and its database, in Docker)
pnpm -C apps/web dev:app          # the website alone, against a network you started
cargo run --bin locallibrary -- --help
```

To try the extension, load `apps/localtoolbox/dist/` (which `pnpm dev` keeps built) as an unpacked extension: open `chrome://extensions`, turn on Developer mode and pick "Load unpacked" ([localtoolbox's README](../apps/localtoolbox/README.md) has more).

More on each part: [the website](../apps/web/README.md), [locallibrary](../apps/locallibrary/README.md), [atstore](../packages/atstore/README.md), [the flags](../flags/README.md), [the lexicons](../lexicons/README.md), [the benchmarks and budgets](../benchmarks/README.md), and each loclib crate's README in `crates/`.

## Checks

Run the checks for what you touched before you open a pull request. All of them run from the repository root, and CI runs each one only when a pull request changes its files ([What CI runs](#what-ci-runs)).

```bash
pnpm verify          # the TypeScript apps, the root's own TypeScript (pnpm *:tools), knip and the lexicons' format
pnpm verify:rust     # the Rust workspace, as locallibrary-ci.yml's lint and test jobs check it
pnpm verify:repo     # the repository's own conventions (see NEW_PROJECT.md); needs Docker running
pnpm verify:changed  # only the ones the branch's changes call for, committed or not, as ci.yml's filters pick CI's jobs
pnpm verify:rust --package <crate>  # one crate's fmt, clippy and tests, with cargo shear and pnpm policy
```

Each runs its steps in `tools/verify/steps.json` one after another, goes on past a failure, and ends with a line for each step: passed, failed or skipped, how long it took, and the command that reruns it alone.

`pnpm verify` runs each app's format, lint, type check, unit tests and build, the `*:tools` format, lint and type checks, knip, and the lexicons' format. `pnpm verify:rust` runs cargo fmt, [cargo-shear](https://github.com/Boshen/cargo-shear), clippy (with no default features too, and on `fuzz/`), the headless sidecar's commands test, the doctests, `cargo doc-strict`, [dist](https://axodotdev.github.io/cargo-dist/)'s plan with the check that only locallibrary is released, [cargo-nextest](https://nexte.st), and clippy for Windows when its target and MinGW are installed. `pnpm verify:repo` runs `pnpm policy`, ast-grep's rule tests and scan, `pnpm test:tools`, Bombadil's install and type check, ShellCheck, typos, taplo, goat's lint and breaking checks of the lexicons, and lychee's offline link check. Both check for the tools in [Setup](#setup) first and stop before the first step, with the install line, when one is missing or Docker isn't running. They warn when typos, taplo or ShellCheck isn't the version `repo-checks.yml` pins, and say what goes unchecked without the Windows target and MinGW (clippy for Windows, which they then skip) or the `sqlcipher` CLI. `pnpm policy` fails when a step of `locallibrary-ci.yml`'s lint job or `repo-checks.yml` is in no verify script's steps and not on the list of CI-only steps in `tools/policy/apps.rego`.

CI also runs these, which the verify scripts leave out. The website's jobs, after `pnpm -C apps/web exec playwright install chromium`:

```bash
(cd apps/web && stylua --check lua)
(cd apps/web/lua && selene .)
pnpm -C apps/web build-storybook
pnpm -C apps/web test:coverage                  # with Docker: the integration tests too, under the coverage thresholds
pnpm -C apps/web test:e2e --project chromium    # with Docker
pnpm -C apps/web test:lighthouse                # with Docker: the pull request's pages
docker run --rm -i hadolint/hadolint:v2.15.1 hadolint - < apps/web/Dockerfile
pnpm -C apps/web test:docker                    # with Docker, against the image you built
```

`test:docker` runs against an image tagged `localnetwork`; [the website's README](../apps/web/README.md) has the build command.

The Rust workspace's (see [apps/locallibrary/CONTRIBUTING.md](../apps/locallibrary/CONTRIBUTING.md) for the builds without default features, the fuzz targets and the property tests):

```bash
cargo coverage                # every test under coverage, held to the thresholds
cargo hegel-coverage          # the Hegel properties alone, held to the same thresholds
cargo +1.91 check --workspace --locked
cargo deny --locked check advisories bans licenses sources
cargo notices                 # the third-party notices, which fail on a licence about.toml doesn't accept
```

CI also sets `CI`, which makes Hegel try the same inputs on every run, `NEXTEST_PROFILE=ci`, and `LOCALLIBRARY_REQUIRE_SQLCIPHER=1`, which fails the SQLCipher tests that otherwise skip without it. Set all three to match it.

And the rest:

```bash
pnpm --filter localnetwork fixtures:mix     # rewrites the website's mix report fixture with locallibrary; commit what changes
pnpm audit --prod
pnpm dedupe --check
pnpm exec commitlint --from origin/main --to HEAD --verbose
actionlint
zizmor --offline .github
```

`tools/policy/contributing.rego`, part of `pnpm policy`, fails when a pull request's CI runs a step that neither a command here nor a verify script named here covers, unless it lists the step with why (setup, reports, the budgets).

The `*:tools` scripts cover what no app's own scripts reach. `format:tools` formats `tools/`, `.railway/`, the flags, budgets and tokens, and Bombadil's specs; `lint:tools` lints `tools/`, `.railway/` and Bombadil's specs; `check:tools` type-checks `tools/`, `.railway/railway.ts` and the generated RPC types in `apps/locallibrary/docs/rpc/`. Bombadil type-checks its specs with its own `typecheck`. The root's `pnpm format:check`, `pnpm lint` and `pnpm check` run them along with every app's. `pnpm test:tools` holds each file its tests load, but the oxlint plugin's, to full coverage. No test loads the command-line entry points (`cli.ts`, `release-cli.ts`), so they aren't measured.

[typos](https://github.com/crate-ci/typos) checks spelling, with the repository's own words in `_typos.toml`. [lychee](https://lychee.cli.rs) checks relative links and their anchors on each pull request, and the rest weekly. A link that can't work yet, or a site that turns lychee away, goes in `.lycheeignore` with why.

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

## What CI runs

CI (`ci.yml`) runs a tool's checks only when its files (or the shared ones) change. It ends with a "CI passed" check, which is skipped, and so counts as passing, unless a check failed. `pr-issue.yml` reports "PR issue passed" ([Pull requests](#pull-requests)). Those two are the checks `main` requires before a merge (`.github/rulesets/protect-main.json`), along with code scanning and a code owner's approval. On a draft pull request nothing runs, and "CI passed" fails, until you mark it ready for review. If no run starts then, use "Re-run all jobs" (re-running only the failed jobs keeps the draft's result).

Pull requests run on Linux, with a clippy check of the Windows code cross-compiled there, and locallibrary's Windows checks when its code changed. Its macOS checks run in the merge queue: once a pull request is approved and passing, add it to the queue, which runs them on it together with the pull requests ahead of it and merges it when they pass. A merge queue group also measures locallibrary's budgets and builds and measures its release, and runs the website's verify and Lighthouse jobs, the extension's and atstore's checks, the workflow lint, the policies, Bombadil's type check, the spelling and links, and CodeQL. The rest stays on pull requests: the Linux Rust jobs (tests, coverage, fuzzing, docs, cargo-deny), the website's integration, e2e and Docker tests, the lexicons, the commit messages, the diagrams, and dependency review. To run macOS on a pull request first, add the `all-os` label and re-run CI. A push to `main` runs only what pull requests use from it: the coverage and sizes they're compared with, the caches, and CodeQL.

The slow suites run weekly in the `*-scheduled.yml` workflows, each skipping the week when nothing it tests changed since its last run that passed, and you can start them on any branch from the Actions tab. A weekly run that fails opens an issue labelled `scheduled-failure`, and the next weekly run that passes closes it.

On a pull request, each tool's workflow keeps one comment up to date with its coverage: the Rust workspace's, with the Hegel property tests' alone beside it, and the website's and the extension's against `main`, with each changed file's. The website's and the extension's comments also show how the change moves what they ship against `main`: the website's client JavaScript and CSS, and the extension's zip. A pull request from a fork can't be commented on, so it finds the same in the run's summary. The website's Lighthouse scores are in the run's summary too, when the change touches what its pages are built from.

Pull requests that change a lockfile, a manifest or a workflow also get a dependency review (`dependency-review.yml`, which `ci.yml` calls, so it holds up the merge like any other check). It fails when a runtime dependency the pull request adds or updates has a known advisory, or when a dependency has a licence outside `deny.toml`'s list and the workflow's few additions (OFL-1.1 for fonts, MITNFA and BlueOak-1.0.0), unless the workflow allows that package by name. Then it comments on the pull request with what it found. GitHub's dependency graph can't read pnpm 12's two-document `pnpm-lock.yaml`, so the workflow submits the lockfile's packages for the pull request first, and `ci.yml` submits them for each push to `main`, the base the review compares with. It ignores the advisories `pnpm audit` does (`auditConfig.ignoreGhsas` in `pnpm-workspace.yaml`). A pull request from a fork can't submit, so its review only covers the npm packages the `package.json` files name.

CodeQL (`codeql.yml`, which `ci.yml` calls) scans the workflows, the TypeScript and the Rust on every pull request into `main`, merge queue group and push to `main` (a language whose code didn't change uploads `main`'s analysis of the commit it was diffed against again, in place of a new one), and `main` won't take a pull request that adds a high or critical security alert, or an error. The Rust tests are left out (`.github/codeql/codeql-config.yml`), so a new `#[cfg(test)]` module needs one of the names listed there, which `ast-grep scan` checks.

## Gates

Besides each check passing, a pull request has to stay within these:

- Coverage: each project has thresholds for its total coverage: the Rust workspace's in the `cargo coverage` alias (`.cargo/config.toml`), the apps' in their Vitest configs, and the root's in `vitest.config.ts`. When coverage goes up, raise them to just under the new numbers in the same pull request. Each project also holds the lines a pull request adds or changes to the share `tools/coverage/patch.json` sets (all of them), by [diff-cover](https://github.com/Bachmann1234/diff_cover) over its lcov report (`.github/actions/patch-coverage`), with the changed files' coverage in the run's summary. Code compiled only for another platform isn't in a Linux report, so it doesn't count. A file no test can reach goes in that file's `exclude`, with the reason in the pull request that adds it. The Rust workspace's per-crate floors (`tools/coverage/floors.json`) only warn, since main's merges move them.
- Budgets: locallibrary's performance budgets and the website's sizes (the JavaScript every page loads, the largest page's own, and the CSS) are in [benchmarks/README.md](../benchmarks/README.md). Each holds a pull request to how much it grows the number over the commit it merges into, and a merge queue group to how much its pull requests grow it together, so main's growth never turns a pull request red and no pull request edits a committed number. To grow one on purpose, add a commit with a `Budget-Raise: <area>/<metric> +<amount>: <why>` trailer; the failure gives the exact line.
- Flags: a release flag in `flags/` has a `removeBy` date, and the website's tests fail once it passes ([flags/README.md](../flags/README.md)). Remove the flag, or move the date in a pull request that says why.
- Changelog: a locallibrary change people will notice adds its changelog line in `apps/locallibrary/changes/`, in a file named after the branch without `bnb/` ([changes/README.md](../apps/locallibrary/changes/README.md)). No check sees it, so the template asks.
- Labels: `pnpm policy` fails a file that no rule in [labeler.yml](labeler.yml) labels, and a code file with no area label. A new folder needs a rule there.

## Pull requests

- Link the issue it's for, as in [Issues and planning](#issues-and-planning). The PR issue check fails otherwise; pull requests from bots skip it.
- Keep each pull request to one concern, and fill in the template.
- For UI changes, add before and after screenshots at 390 and 1280 px wide, in light and dark.
- Break each new test once, to check it fails when the code it covers is wrong.
- Keep the docs true in the same pull request: the READMEs, the Mermaid diagrams, the glossaries ([GLOSSARY.md](../GLOSSARY.md) and the apps' own) and the decisions in [docs/adr/](../docs/adr/).
- Write each commit's subject as a [Conventional Commit](https://www.conventionalcommits.org), `type(scope): summary`. CI checks every commit a pull request adds with commitlint (`commitlint.config.js`), and the `commit-msg` hook checks yours as you commit.
- A pull request merges once "CI passed" and "PR issue passed" pass and a code owner ([CODEOWNERS](CODEOWNERS)) has approved its last push. Every push dismisses the approvals, so push the fixes from a review together. Pull requests merge with a merge commit; squash and rebase merges are off.
- Bring changes in by merging, never by rebasing or force-pushing, which throw away the reviews and CI results the pull request has.

Labels are added automatically from the files a pull request changes ([labeler.yml](labeler.yml)): one for each tool (`web`, `locallibrary`, `localtoolbox`, `lexicons`, `atstore`), one for each area inside a tool (`web: epk`, `locallibrary: player`), and `ci`, `tooling`, `security`, `performance`, `accessibility` and `dependencies`. `tests` and `documentation` go on a pull request that changes only tests or only docs. [CODEOWNERS](CODEOWNERS) names one owner for the whole repository, who reviews every pull request.

## Issues and planning

Open issues here on GitHub, with the issue forms. Each one is synced both ways with the Localnetwork team (`LOC`) in [Linear](https://linear.app/localnetwork/team/LOC), where they're triaged and planned: title, description, status, assignee, labels, priority, sub-issues and comments, so an issue can be worked on from either side. Comments left in Linear outside the synced thread stay in Linear.

- Status: a new issue starts in Triage, then Backlog or Todo once someone has looked at it, In Progress, In Review and Done (or Canceled). Closing it on GitHub makes it Done, and reopening it brings it back.
- Pull requests: `Fixes #123` or `Fixes LOC-123` in the description closes the issue when the pull request merges, and `Part of LOC-123` links it without closing it. The PR issue check needs one of them, with its word: a bare `LOC-123` or `#123` doesn't count, and `#123` has to be an issue, not a pull request. It takes [Linear's magic words](https://linear.app/docs/github#magic-words) for `LOC-123` (`Closes`, `Resolves`, `Refs` and the rest), and the [keywords GitHub reads](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/linking-a-pull-request-to-an-issue) for `#123`. A branch named from Linear's "Copy git branch name" (`bnb/loc-123-…`) links it as soon as it's pushed, and passes the check too. GitHub reads its keywords only on a pull request into `main`, so a stacked pull request needs `Fixes LOC-123`, `Part of LOC-123` or that branch until it targets `main`. A linked pull request moves its issue to In Progress when it's opened, In Review when a review is requested, and Done when it merges.
- Labels: the ones in [labels.yml](labels.yml), a tool's (`locallibrary`) and an area's (`locallibrary: player`), and one for what kind of issue it is: `bug`, `enhancement`, `scheduled-failure`, or work across the tools (`tests`, `documentation`, `ci`, `tooling`, `security`, `performance`, `accessibility`, `design`). `needs decision` and `blocked` say why an issue is waiting. To move an issue to another tool or area, change its label.
- Priority and estimate are set while triaging. A release is a Linear project, like `locallibrary v0.1.0`, with a target date. Big work gets sub-issues, and work that has to wait gets a "blocked by" relation.
