# Contributing

This covers the whole repository. Each tool has more in its own docs:

- locallibrary: [apps/locallibrary/CONTRIBUTING.md](../apps/locallibrary/CONTRIBUTING.md)
- Local Network, the website: [apps/web/README.md](../apps/web/README.md)
- localtoolbox: [apps/localtoolbox/README.md](../apps/localtoolbox/README.md)

Adding an app or a locallibrary plugin? Follow [NEW_PROJECT.md](NEW_PROJECT.md).

Found a security problem? See [SECURITY.md](SECURITY.md) instead of opening an issue.

Have a question about how something works? Ask in [Discussions](https://github.com/cutenode/localnetwork/discussions).

Found a bug? "Report a bug" in the website's footer, localtoolbox's settings, or `locallibrary bug` fills in your version and setup for you.

## Setup

Use the Node version in `.node-version` (24). From the repository root:

```bash
pnpm install    # every TypeScript app's dependencies, plus the lefthook git hooks
```

`rust-toolchain.toml` pins the Rust toolchain. rustup installs it the first time you run cargo in the checkout.

## Checks

Run the checks for what you touched before you open a pull request. All of them run from the repository root.

The TypeScript apps and the lexicons:

```bash
pnpm verify
```

The Rust workspace:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The repository's own conventions (see [NEW_PROJECT.md](NEW_PROJECT.md)), with Docker running:

```bash
tools/policy.sh
```

CI runs a tool's checks only when its files (or the shared ones) change. Each workflow ends with a "… passed" check that reports either way, and those are the checks `main` requires before a merge. On a draft pull request, the Rust workspace's and the website's checks wait until you mark it ready for review, so open a pull request as a draft while it isn't next to merge (a stacked one, say). The slow suites run weekly in the `*-scheduled.yml` workflows, and you can start them on any branch from the Actions tab. A weekly run that fails opens an issue labelled `scheduled-failure`, and the next weekly run that passes closes it.

On a pull request, each tool's workflow keeps one comment up to date with its coverage: the Rust workspace's, with the Hegel property tests' alone beside it, and the website's and the extension's against `main`, with each changed file's. The website's and the extension's comments also show how the change moves what they ship against `main`: the website's client JavaScript and CSS, and the extension's zip. A pull request from a fork can't be commented on, so it finds the same in the run's summary. The website's Lighthouse scores are in the run's summary too, when the change touches what its pages are built from.

Pull requests also get a dependency review (`dependency-review.yml`). It fails when a dependency the pull request adds or updates (dev tooling aside) has a known advisory or a licence `deny.toml` doesn't allow (fonts may also be OFL-1.1), and then comments on the pull request with what it found.

## Pull requests

- Keep each pull request to one concern.
- Fill in the template.
- For UI changes, add before and after screenshots at phone and wide widths.
- Pull requests merge with a merge commit. Squash and rebase merges are off.

Labels are added automatically from the files a pull request changes ([labeler.yml](labeler.yml)): one for each tool (`web`, `locallibrary`, `localtoolbox`, `lexicons`), one for each area inside a tool (`web: epk`, `locallibrary: player`), and `tests`, `documentation`, `dependencies`, `github_actions` and `tooling`. [CODEOWNERS](CODEOWNERS) requests a review from whoever owns the tools it changes.

## Issues and the project

Every open issue is on the organization's [Local Network project](https://github.com/orgs/cutenode/projects/9). The [Project workflow](workflows/project.yml) adds each new issue and fills in its Status and Product.

- Status: Inbox (new), Todo (triaged), In Progress, Done. Closing an issue, or merging the pull request that closes it, moves it to Done.
- Product: the tool, from the issue's labels (`locallibrary`, or any `locallibrary: <area>` label). An issue with no tool's label gets `repo`. To move an issue, change its label, not the field.
- Type: Bug, Feature or Task.
- Priority and Effort: set while triaging. Features can also get a Start date and Target date, which place them on the roadmap.
- A release is a milestone, like `locallibrary v0.1.0`. Big work gets sub-issues, and work that has to wait gets a "blocked by" link.

The views: Board (a column per Status, a row per Product), Inbox to triage, Backlog, Bugs, Releases (by milestone) and Roadmap (features by date).
