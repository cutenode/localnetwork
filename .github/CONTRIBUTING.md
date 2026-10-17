# Contributing

This covers the whole repository. Each tool has more in its own docs:

- locallibrary: [apps/locallibrary/CONTRIBUTING.md](../apps/locallibrary/CONTRIBUTING.md)
- Local Network, the website: [apps/web/README.md](../apps/web/README.md)
- localtoolbox: [apps/localtoolbox/README.md](../apps/localtoolbox/README.md)

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

CI runs a tool's checks only when its files (or the shared ones) change. Each workflow ends with a "… passed" check that reports either way, and those are the checks `main` requires before a merge. The slow suites run weekly in the `*-scheduled.yml` workflows, and you can start them on any branch from the Actions tab.

## Pull requests

- Keep each pull request to one concern.
- Fill in the template.
- For UI changes, add before and after screenshots at phone and wide widths.
- Pull requests merge with a merge commit. Squash and rebase merges are off.

Labels are added automatically from the files a pull request changes ([labeler.yml](labeler.yml)): one for each tool (`web`, `locallibrary`, `localtoolbox`, `lexicons`), one for each area inside a tool (`web: epk`, `locallibrary: player`), and `tests`, `documentation`, `dependencies`, `github_actions` and `tooling`. [CODEOWNERS](CODEOWNERS) requests a review from whoever owns the tools it changes.
