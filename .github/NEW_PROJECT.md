# Adding an app or plugin

This covers what the repository runs on and every place a new project has to be registered. An app is a folder in `apps/` (`local<thing>` for a tool); a plugin is one of locallibrary's, a crate in `plugins/<id>/` or, for an account plugin, a manifest alone in `plugins/accounts/<id>/`. A pull request that adds either has a section in the [template](pull_request_template.md) that asks you to confirm you went through the checklist for it below.

Most of the steps are checked, so a missed one fails CI with a message that says what to add:

- `tools/policy.sh` (the Repo checks workflow) runs Rego policies in `tools/policy/` over the config files with [conftest](https://www.conftest.dev). They check each TypeScript app's scripts, `package.json` fields and git hooks; that every workflow on pull requests (but the benchmarks, the release plan and dependency review) has the `changes`, work and `passed` jobs and no `paths` in its trigger, and that one of them runs when each app, package, crate or plugin changes; that `.github/rulesets/protect-main.json` requires every `passed` check; that the labels, labeler and issue forms know every app and every file gets a label; that Rust packages share the workspace's settings and the dev profile keeps its entries for the crates in `Cargo.lock`; that lexicons are filed by NSID; and that an app with a Dockerfile has a Railway service in `.railway/railway.ts`, which has to type-check against the railway SDK. It needs Docker, and Node with the root's dev dependencies locally. The policies' own tests have to cover every line.
- The website's tests check that every folder in `apps/` has a tool page and is named in the root README, CONTRIBUTING, SECURITY, the website's README and the `web: tools` label (`apps/web/src/lib/tools/docs.test.ts`).
- locallibrary's build fails while a folder in `plugins/` isn't one of its dependencies, or a folder in `plugins/accounts/` has no manifest (`apps/locallibrary/build.rs`), and its tests check that every one ships and is in its docs (`apps/locallibrary/tests/it/plugins.rs`).
- clippy's `cargo_common_metadata` wants full metadata on every crate that isn't `publish = false`, and the release workflow's plan fails while it would release anything but locallibrary.
- `tools/lexicons.sh` lints the lexicons with [goat](https://github.com/bluesky-social/goat) and compares them with the published ones. It runs goat's pinned version with `go run`, so it needs Go 1.27 or newer.

What no check sees: `CODEOWNERS`, the changelog, what the READMEs say, and settings outside the repository (Railway's, DNS, secrets).

## The stack

### Languages and toolchains

| What         | Version and where it's set                                                                                                               |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------------------- |
| Node.js      | 24, from `.node-version`; [fnm](https://github.com/Schniz/fnm) picks it up. `engines.node` is `>=24.12` and pnpm is engine-strict        |
| pnpm         | `packageManager` in the root `package.json` (12.6.0); one lockfile for every TypeScript app                                              |
| TypeScript   | strict, in each app's `tsconfig.json`                                                                                                    |
| UI           | Svelte 5 with Vite and Tailwind; SvelteKit with `adapter-node` for the website, a Manifest V3 build for the extension                    |
| Rust         | 1.96.1 with clippy and rustfmt, from `rust-toolchain.toml`; the oldest supported (MSRV) is `rust-version` in the root `Cargo.toml`, 1.91 |
| Rust edition | 2024, inherited from `[workspace.package]`                                                                                               |

The repository root is both the pnpm workspace (`pnpm-workspace.yaml`: each folder in `apps/` and `packages/` with a `package.json`) and the Cargo workspace (`members` in `Cargo.toml`: every crate in `crates/` and `plugins/` but `plugins/accounts/`, plus the Rust apps and vendored crates it lists). `fuzz/` is a Cargo workspace of its own.

### Developing

```bash
pnpm install    # every TypeScript app's dependencies, and the lefthook git hooks
pnpm dev        # every app's dev script at once
pnpm verify     # every app's verify script, plus the lexicons' format
cargo build
cargo nextest run --workspace
```

`pnpm dev` runs the website on a local network: a PLC directory and a PDS from `@atproto/dev-env`, HappyView in Docker (so Docker or OrbStack has to be running), and demo accounts, all from `apps/web/scripts/dev-network.ts`, which writes `apps/web/.env.local`. The extension's `dev` is a watch build into `apps/localtoolbox/dist/`.

`lefthook.yml` formats and lints staged files before a commit and runs each app's type check and tests before a push. Each TypeScript app and the lexicons have jobs of their own; Rust has none.

Lint and format:

- TypeScript: [oxlint](https://oxc.rs) with type-aware rules and the vendored anti-slop plugin (`tools/oxlint/`, see its `UPSTREAM.md`), configured in the root `.oxlintrc.json`, which each app's own `.oxlintrc.json` extends. [oxfmt](https://oxc.rs) formats with the root `.oxfmtrc.json` (tabs, single quotes, 120 columns). `oxlint` and `@oxlint/plugins` are pinned to the same version: upgrade them together.
- Rust: `cargo fmt` with `rustfmt.toml`, and `cargo clippy -- -D warnings`. `clippy.toml` bans `BTreeMap` (use `indexmap::IndexMap`), and the workspace uses `rustc-hash` for hash maps. Each member has `[lints] workspace = true`, so the lints in the root `Cargo.toml` apply to it.
- `knip` finds unused files, exports and dependencies in each TypeScript app (`knip.json`).

Logging: the website logs with [pino](https://getpino.io) (`apps/web/src/lib/server/logger.ts`), pretty in dev and JSON lines in production, with secrets redacted. locallibrary has no logging crate: what it tells people goes through `output::diag`, and plugins' stderr is relayed as `id: line`.

### Testing

| Project      | Tests                                                                                                                         | Coverage and other bars                                                                                                           |
| ------------ | ----------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| website      | Vitest projects in `apps/web/vite.config.ts` (server, client in browser mode, integration, build, Storybook); Playwright e2e  | Vitest thresholds in `vite.config.ts`; Lighthouse budgets in `lighthouserc.cjs`; Storybook stories fail on a11y violations        |
| localtoolbox | Vitest with jsdom; its README is its spec, checked by `test/docs.test.ts`                                                     | per-file thresholds in `vitest.config.ts`; Stryker mutation score of at least 91 (`stryker.config.mjs`)                           |
| Rust         | cargo-nextest (`.config/nextest.toml`); unit tests in each module's `tests.rs`; Hegel property tests; fuzz targets in `fuzz/` | `cargo coverage` (`.cargo/config.toml`) fails under its thresholds; `tools/hegel-coverage.sh` holds the Hegel tests alone to them |

`LOCALLIBRARY_BLESS=1` regenerates the Rust golden files: locallibrary's RPC schema, loclib-equipment's JSON, and `crates/loclib-model/src/tokens.rs` from `packages/tokens/colors.json`. `TOKENS_BLESS=1` regenerates `packages/tokens/data-colors.css` from the same file.

### CI

Every workflow the ruleset requires has the same three parts. Copy `localtoolbox-ci.yml`, the smallest:

1. a `changes` job that runs [dorny/paths-filter](https://github.com/dorny/paths-filter) over the files the app depends on, including the shared ones (the lockfile, `.node-version`, the lint and format configs, the workflow itself). On a push it diffs against the last commit the workflow passed on (`.github/actions/last-passed`), so a run that a newer push cancelled, or one that failed, leaves its files to the next run;
2. the jobs that do the work, each with `needs: changes` and skipped when nothing matched;
3. a `passed` job named "`<tool>` CI passed", with `if: always()`, that fails if any job failed or was cancelled, or was skipped while the `changes` output it runs on was true (a job whose build didn't finish is skipped too).

Jobs that only report stay out of the `passed` job's needs, so they never block a merge: the coverage `report` jobs that comment on the pull request, and locallibrary-ci's `semver`. Each workflow's concurrency group cancels the run a new push supersedes, on `main` too. locallibrary-ci and web-ci, the two that take the most runners, also skip their work on draft pull requests until they're marked ready for review.

The "Protect Main" ruleset requires the `passed` jobs, which report either way, so a path-filtered workflow never blocks a pull request that doesn't touch it. The ruleset is `.github/rulesets/protect-main.json`: an admin applies it with `tools/ruleset.sh apply`, and the Repo checks workflow fails on `main` while GitHub's copy differs. It requires Web CI passed, locallibrary CI passed, localtoolbox CI passed, Mix report passed, npm advisories passed, Workflow lint passed, Repo checks passed and atstore CI passed, and code scanning, which is `codeql.yml`'s CodeQL analysis of the workflows, the TypeScript and the Rust, leaving out the Rust tests (`.github/codeql/codeql-config.yml`).

Changes reach `main` only through a pull request, merged as a merge commit. Each one needs an approval from a code owner (`.github/CODEOWNERS`) given after its last push, and a new push dismisses earlier approvals, so an agent's pull request waits for a person to approve it.

Workflow conventions, which `workflow-lint.yml` checks with [actionlint](https://github.com/rhysd/actionlint) and [zizmor](https://zizmor.sh):

- actions pinned to a commit SHA with the version in a comment;
- `permissions: contents: read` unless a job needs more, and `persist-credentials: false` on checkout;
- Node from `.node-version` with `actions/setup-node`, pnpm from `packageManager` with `pnpm/action-setup`, and `pnpm install --frozen-lockfile`;
- a concurrency group per ref that cancels superseded runs.

Slow suites (mutation testing, fuzzing, every browser, release builds) run weekly in `<tool>-scheduled.yml`, on a Monday cron plus `workflow_dispatch` so they can run on any branch. They have no `passed` job and no path filter.

Repository-wide checks: `npm-advisories.yml` runs `pnpm audit --prod` over the lockfile on pull requests and weekly, `cargo deny` (in `locallibrary-ci.yml`, with `deny.toml`) checks the Rust dependencies' advisories, licences and sources, OpenSSF Scorecard (`scorecard.yml`) runs osv-scanner over every lockfile and skips what `osv-scanner.toml` lists (a lockfile in another directory needs its own, as `fuzz/` has), and Dependabot updates the npm lockfile, Cargo (`/` and `/fuzz`) and Actions monthly. npm advisories and Dependabot cover a new app without changes; `cargo deny` runs when `locallibrary-ci.yml`'s paths change, which a new Rust app adds to.

Labels: `pr-labels.yml` labels pull requests from `labeler.yml`, and `labels.yml` (the workflow) syncs `.github/labels.yml` to GitHub. Each tool has a label, and its areas are `"<tool>: <area>"` labels in a lighter shade of its colour.

Issues are planned in Linear, in the `localnetwork` workspace's Localnetwork team (`LOC`). Linear's GitHub integration syncs this repository's issues both ways with that team and links pull requests to them, so nothing in the repository runs for it. Its settings live in Linear, so they're listed here:

- Settings > Integrations > GitHub: the Linear app installed on `cutenode` with access to `cutenode/localnetwork`, and two-way Issues Sync from `cutenode/localnetwork` to Localnetwork. The issues from before the sync came in through the GitHub importer (Settings > Import/Export), which keeps them synced the same way.
- Settings > Workspace > Integrations > GitHub > Branch format: `bnb/{issueIdentifier}-{issueTitle}`, so "Copy git branch name" gives a `bnb/loc-123-…` branch.
- The Localnetwork team: Triage on, and under Workflows & automations, a pull request moves its issues to In Progress when it's drafted or opened, In Review when a review is requested, and Done when it merges.

### Production and releases

The website (`apps/web`) is a Docker image, built from the repository root with `apps/web/Dockerfile`, that serves on port 3000 and keeps its SQLite database on a `/data` volume. It has to run as exactly one instance. It goes on Railway: the `localnetwork.computer` project has one service, `localnetwork`, which deploys from `main` once its checks pass. `.railway/railway.ts` holds the project: the service's build and deploy settings (the Dockerfile, the health check, one replica, and the paths whose changes redeploy it), the `/data` volume, the bucket the database's daily backups go to, and its variables, with the secrets left on Railway. `railway config plan` shows how Railway differs from it and `railway config apply` applies it (`.railway/README.md`). HappyView, the AppView it reads records through, runs in its own Railway project (`localnetwork.computer HappyView`, with staging and production environments). CI builds the image and smoke-tests it with `apps/web/scripts/docker-smoke.sh` but never pushes it. The rest of a deploy is in [Deploying](../apps/web/README.md#deploying): a root account on a PDS that runs the spaces alpha, `pnpm lexicons:publish` with the `_lexicon` TXT records in Cloudflare, and `locallibrary.localnetwork.computer` pointed at the same deployment.

locallibrary is released with [cargo-dist](https://axodotdev.github.io/cargo-dist/) (`dist-workspace.toml`, `locallibrary-release.yml`) when a `locallibrary-cli-vX.Y.Z` tag is pushed: archives for macOS, Linux and Windows, shell and PowerShell installers, and a Homebrew formula for `cutenode/homebrew-tap`. macOS signing waits on the `APPLE_*` secrets, and the formula on `HOMEBREW_TAP_TOKEN`. The steps are in [Releasing](../apps/locallibrary/CONTRIBUTING.md#releasing), and the release notes come from `apps/locallibrary/CHANGELOG.md` ([Keep a Changelog](https://keepachangelog.com/en/1.1.0/)). Plugins ship inside the `locallibrary` binary.

localtoolbox has no release workflow. `pnpm zip` builds `localtoolbox.zip`, people load `dist/` unpacked, and its version lives in both `package.json` and `src/manifest.json`, which a test keeps equal.

### Lexicons, tokens and the website's tool pages

- Lexicons live in `lexicons/` by NSID. A new tool's own records go under `computer.localnetwork.<tool>.*`, and records several tools read go in a group named for what they are (`computer.localnetwork.dj.*`). The website's records predate this and sit at the top level (`booking`, `epk`, `project`). A tool's OAuth permission set is a top-level document too, like `locallibraryPublishing.json`. A new shared group goes in the list in `tools/policy/lexicons.rego`. locallibrary's tests list every document in `locallibrary/` and `dj/` (`crates/locallibrary/src/atmosphere/lexicon/tests.rs`), so a new one there needs adding to them. `pnpm lexicons:format` formats the lexicons and `pnpm lexicons:publish` (in `apps/web`) publishes all of them.
- `packages/tokens/colors.json` holds the colours that mean the same thing wherever DJ data is drawn. Use it instead of new colours for the same things.
- `packages/tokens/theme.css` is the Local Network design system the website and localtoolbox share (tokens, type scale, shared classes). A new web UI imports it after `@import 'tailwindcss'`, along with the two `@fontsource-variable` fonts it names.
- Each tool has a page at `localnetwork.computer/tools/<tool>`: one file in `apps/web/src/lib/tools/`, listed in its `index.ts`. The page shows the tool's `README.md` and `docs/*.md`, which the website's Docker build picks up from `apps/<tool>/`.

## A new TypeScript app

1. Create `apps/<tool>/` with a `package.json`, which makes it part of the pnpm workspace. Add dependencies with `pnpm add --filter <name>`.
2. Give its `package.json` `"type": "module"`, `"private": true`, the root's `engines.node`, `oxlint` at the root's exact version, and these scripts, which the root's `pnpm -r` scripts and CI call: `dev`, `build`, `check`, `lint` (`oxlint --type-aware`), `format` and `format:check` (`oxfmt`), `knip`, `test`, `test:coverage`, and `verify`, which runs everything CI's verify job runs.
3. Add an `.oxlintrc.json` that extends `../../.oxlintrc.json`, a strict `tsconfig.json`, a `knip.json`, a `.gitignore` for its build output, and coverage thresholds just under its current numbers. If its build output isn't already in `.oxfmtrc.json`'s ignores, add it.
4. Add `format <tool>` and `lint <tool>` jobs under `pre-commit` and `typecheck <tool>` and `tests <tool>` jobs under `pre-push` in `lefthook.yml`, with `root: apps/<tool>/`.
5. Add `.github/workflows/<tool>-ci.yml` from `localtoolbox-ci.yml`, with its own paths and a `passed` job named "`<tool>` CI passed", and `<tool>-scheduled.yml` for anything too slow for every pull request.
6. Add "`<tool>` CI passed" to the required checks in `.github/rulesets/protect-main.json`. Once the pull request merges, an admin runs `tools/ruleset.sh apply`.
7. If it runs as a service, give it a `Dockerfile` that builds from the repository root, re-include its files in `.dockerignore` (which allows only what the website needs), and have CI build and smoke-test the image as `web-ci.yml`'s `docker` job does. Add its service to `.railway/railway.ts`, building its Dockerfile from `main` once its checks pass, and run `railway config apply`.

Then do the steps every app shares, below.

## A new Rust app

1. Create `apps/<tool>/` and add it to `members` in the root `Cargo.toml`. Inherit `edition`, `rust-version`, `license` and `repository` from the workspace, but give it a `version` of its own: the workspace's moves with locallibrary's releases. Add `[lints] workspace = true`. Take shared dependencies from `[workspace.dependencies]` (`cargo add --package <name>`).
2. Put unit tests in each module's own `tests.rs` (`#[cfg(test)] mod tests;`), so coverage leaves them out, and integration tests in one binary, `tests/it/`.
3. `locallibrary-ci.yml` already checks the whole workspace (fmt, clippy with `-D warnings`, the tests on macOS, Linux and Windows, `cargo coverage` and its thresholds, the MSRV build on 1.91, the docs and `cargo deny`), but only when its paths change. Add `apps/<tool>/**` to its `rust` filter. The new app's code counts toward the shared coverage thresholds and has to build on the MSRV. Anything only the app needs goes in a `<tool>-ci.yml` with a `passed` job, added to the ruleset file.
4. `locallibrary-scheduled.yml` runs cargo-mutants weekly over every workspace package but locallibrary's library and CLI, so the app is in it. If its tests are too slow to run once per mutant, leave it out there as those two are.
5. Set `publish = false` unless it's meant for crates.io: clippy's `cargo_common_metadata` wants full metadata on anything publishable, and dist (which releases on `locallibrary` tags) leaves unpublished packages out. When it gets a release of its own, set `[package.metadata.dist] dist = true`, as `apps/locallibrary/Cargo.toml` does.

Then do the steps every app shares, below.

## Steps every app shares

1. A `README.md` that says what it is, how to install it and how to work on it, plus any guides in `docs/*.md`. They become its pages on the website.
2. Its page: `apps/web/src/lib/tools/<tool>.ts`, listed in `apps/web/src/lib/tools/index.ts` (copy `localtoolbox.ts`), and its name in the Tools paragraph of `apps/web/README.md`.
3. Its lexicons, if it writes records, under `lexicons/computer/localnetwork/<tool>/`.
4. If it signs people in with their Atmosphere account, as locallibrary does: a permission-set lexicon (`<tool>Publishing.json`), `hosts` and `clientMetadata` in its tool entry, and DNS for its own host (see [Tools' pages and hosts](../apps/web/README.md#tools-pages-and-hosts)).
5. A label for it in `.github/labels.yml`, its paths under that label in `.github/labeler.yml`, and any areas as `"<tool>: <area>"` labels in both. The `web: tools` label's description names every tool.
6. `.github/ISSUE_TEMPLATE/<tool>-bug.yml` (labels `["bug", "<tool>"]`, `type: Bug`), and the tool in the description and "Which tool?" dropdown of `feature_request.yml`. Questions go to Discussions, which has no form.
7. A line in `.github/CODEOWNERS` if someone other than the default owner reviews it.
8. Its row in the root `README.md`'s table and layout; its docs and its label in `.github/CONTRIBUTING.md`; and in `.github/SECURITY.md`, its supported version and where to find the version it's on.

## A locallibrary plugin

[Plugins](../apps/locallibrary/docs/plugins.md) has what plugins do, and [loclib-plugin](../crates/loclib-plugin/README.md) has the protocol, the manifest's fields and the SDK. `crates/loclib-plugin/examples/echo/` is a complete plugin.

1. Create `plugins/<id>/` with a `Cargo.toml` named `locallibrary-plugin-<id>`, `publish = false`, the workspace's package fields, `[lints] workspace = true`, and `loclib-plugin.workspace = true`.
2. Write its manifest, `plugins/<id>/locallibrary-plugin.toml`: `id`, `name`, `version`, `protocol = "^0.2"`, `capabilities` (only what it needs) and what it `contributes`.
3. In `src/lib.rs`, export `pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");` and `pub fn serve(input, output)`, which calls `loclib_plugin::serve` with `MANIFEST` and its `Hooks`.
4. Test it in `src/tests.rs` over in-memory pipes with a fake host, as `plugins/sound/src/tests.rs` does, including a test that the manifest passes `validate(true)` and that `describe` returns it.
5. Make it a dependency of locallibrary: from the repository's root, `cargo add -p locallibrary-cli --path plugins/<id>`. `apps/locallibrary/build.rs` links in every folder in `plugins/` but `accounts/`, in alphabetical order (as `plugin list` shows them), and fails the build while one isn't a dependency.
6. List it in `apps/locallibrary/docs/plugins.md` and in the layout in `apps/locallibrary/docs/how-it-works.md`, and add a changelog line in `apps/locallibrary/changes/`.
7. If it fits an existing area label, add `plugins/<id>/**` to it in `.github/labeler.yml`. The `locallibrary` and `locallibrary: plugins` labels already cover `plugins/**`.

A new capability is a change to loclib-plugin, and also needs a case in `means()` in `apps/locallibrary/src/plugins.rs`, which says it in plain words. A plugin that changes what an analyzer computes bumps that analyzer's `version`, since stored values are keyed by it.

### An account plugin

An [account plugin](../apps/locallibrary/docs/plugins.md#account-plugins) is a manifest and nothing else: locallibrary signs in from it and never runs it.

1. Create `plugins/accounts/<id>/locallibrary-plugin.toml` with `id` (ending in `-account`), `name`, `version`, `protocol` and one `[[contributes.account_types]]` (the fields are in [loclib-plugin](../crates/loclib-plugin/README.md)). No crate, no `Cargo.toml`: `apps/locallibrary/build.rs` includes every folder there, and the workspace leaves them out.
2. Give each scope the requests it covers, taken from the code of the plugins that will use it, and test from those plugins that their requests are covered, as `plugins/youtube/src/tests.rs` does.
3. A token exchange is Local Network's (`apps/web/src/lib/server/token-exchange/services.json`), so its service has to be there.
4. List it in `apps/locallibrary/docs/plugins.md` and `apps/locallibrary/docs/how-it-works.md`, and add a changelog line in `apps/locallibrary/changes/`.

## Shared libraries

A Rust library other DJ tools can use is a `loclib-<thing>` crate in `crates/`: in `[workspace.dependencies]` with `version = "0.1.0"` and its path, and in the layout in `apps/locallibrary/docs/how-it-works.md`, with `[lints] workspace = true`, a `README.md` that is its crate docs (`#![doc = include_str!("../README.md")]`, so its examples are doctests), a `LICENSE`, and a line in [Crates for other DJ tools](../apps/locallibrary/README.md#crates-for-other-dj-tools). It never depends on `locallibrary`.

Shared TypeScript code goes in `packages/`. `packages/tokens` is plain JSON and CSS that apps import by relative path, so it isn't a workspace package; a package with code of its own has a `package.json`, which makes it part of the workspace like an app.
