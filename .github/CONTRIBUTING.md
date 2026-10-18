# Contributing

This covers the whole repository. Each tool has more in its own docs:

- locallibrary: [apps/locallibrary/CONTRIBUTING.md](../apps/locallibrary/CONTRIBUTING.md)
- Local Network, the website: [apps/web/README.md](../apps/web/README.md)
- localtoolbox: [apps/localtoolbox/README.md](../apps/localtoolbox/README.md)

Found a security problem? See [SECURITY.md](SECURITY.md) instead of opening an issue.

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

Spelling, across the whole repository, with [typos](https://github.com/crate-ci/typos):

```bash
typos
```

It isn't an npm package, so install it yourself: `brew install typos-cli` or `cargo install typos-cli --locked`. Once it's installed, the pre-commit hook checks the files you commit as well. When it flags a word that's right, like a column name from rekordbox's database or a misspelling a test makes on purpose, add it to `typos.toml` and say why there.

CI runs a tool's checks only when its files (or the shared ones) change, and typos on every pull request. Each workflow with a changes filter ends with a "… passed" check that reports either way, and those are the checks `main` requires before a merge. The slow suites run weekly in the `*-scheduled.yml` workflows, and you can start them on any branch from the Actions tab. A weekly run that fails opens an issue labelled `scheduled-failure`, and the next weekly run that passes closes it.

On a pull request, each tool's workflow keeps one comment up to date with its coverage: the Rust workspace's, and the website's and the extension's against `main`, with each changed file's. The website's and the extension's comments also show how the change moves what they ship against `main`: the website's client JavaScript and CSS, and the extension's zip. A pull request from a fork can't be commented on, so it finds the same in the run's summary. The website's Lighthouse scores are in the run's summary too, when the change touches what its pages are built from.

Pull requests also get a dependency review (`dependency-review.yml`). It fails when a dependency the pull request adds or updates (dev tooling aside) has a known advisory or a licence `deny.toml` doesn't allow (fonts may also be OFL-1.1), and then comments on the pull request with what it found.

## Pull requests

- Keep each pull request to one concern.
- Fill in the template.
- For UI changes, add before and after screenshots at phone and wide widths.
- Pull requests merge with a merge commit. Squash and rebase merges are off.

Labels are added automatically from the files a pull request changes ([labeler.yml](labeler.yml)): one for each tool (`web`, `locallibrary`, `localtoolbox`, `lexicons`), one for each area inside a tool (`web: epk`, `locallibrary: player`), and `tests`, `documentation`, `dependencies`, `github_actions` and `tooling`. [CODEOWNERS](CODEOWNERS) requests a review from whoever owns the tools it changes.
