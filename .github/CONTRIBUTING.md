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

The repository's own conventions (see [NEW_PROJECT.md](NEW_PROJECT.md)), with Docker running for the policies:

```bash
tools/policy.sh
pnpm exec ast-grep test --skip-snapshot-tests && pnpm exec ast-grep scan
```

ast-grep's rules (`sgconfig.yml`, in `tools/ast-grep/rules/` with their cases in `tools/ast-grep/tests/`) include `no-direct-pds-read`, which fails a record read that skips HappyView, in TypeScript, Svelte or Rust. Its exceptions are in the rule. clippy (`clippy.toml`) bans jacquard's typed requests for the same reads.

CI runs a tool's checks only when its files (or the shared ones) change. Each workflow ends with a "… passed" check that reports either way, and those are the checks `main` requires before a merge. On a draft pull request, the Rust workspace's and the website's checks wait until you mark it ready for review, so open a pull request as a draft while it isn't next to merge (a stacked one, say). Their "… passed" checks fail on a draft that has something for them to check, since none of it ran. Marking it ready starts a new run, and if one doesn't show up, re-run the workflow. The slow suites run weekly in the `*-scheduled.yml` workflows, and you can start them on any branch from the Actions tab. A weekly run that fails opens an issue labelled `scheduled-failure`, and the next weekly run that passes closes it.

On a pull request, each tool's workflow keeps one comment up to date with its coverage: the Rust workspace's, with the Hegel property tests' alone beside it, and the website's and the extension's against `main`, with each changed file's. The website's and the extension's comments also show how the change moves what they ship against `main`: the website's client JavaScript and CSS, and the extension's zip. A pull request from a fork can't be commented on, so it finds the same in the run's summary. The website's Lighthouse scores are in the run's summary too, when the change touches what its pages are built from.

Pull requests also get a dependency review (`dependency-review.yml`). It fails when a dependency the pull request adds or updates (dev tooling aside) has a known advisory or a licence `deny.toml` doesn't allow (fonts may also be OFL-1.1), and then comments on the pull request with what it found.

CodeQL scans the workflows, the TypeScript and the Rust on every pull request (`codeql.yml`), and `main` won't take a pull request that adds a high or critical security alert, or an error. The Rust tests are left out (`.github/codeql/codeql-config.yml`), so a new `#[cfg(test)]` module needs one of the names listed there, which `ast-grep scan` checks.

## Pull requests

- Keep each pull request to one concern.
- Fill in the template.
- For UI changes, add before and after screenshots at phone and wide widths.
- Pull requests merge with a merge commit. Squash and rebase merges are off.

Labels are added automatically from the files a pull request changes ([labeler.yml](labeler.yml)): one for each tool (`web`, `locallibrary`, `localtoolbox`, `lexicons`), one for each area inside a tool (`web: epk`, `locallibrary: player`), and `tests`, `documentation`, `dependencies`, `github_actions` and `tooling`. [CODEOWNERS](CODEOWNERS) requests a review from whoever owns the tools it changes.

## Issues and planning

Open issues here on GitHub, with the issue forms. Each one is synced both ways with the Localnetwork team (`LOC`) in [Linear](https://linear.app/localnetwork/team/LOC), where they're triaged and planned: title, description, status, assignee, labels, priority, sub-issues and comments, so an issue can be worked on from either side. Comments left in Linear outside the synced thread stay in Linear.

- Status: a new issue starts in Triage, then Backlog or Todo once someone has looked at it, In Progress, In Review and Done (or Canceled). Closing it on GitHub makes it Done, and reopening it brings it back.
- Pull requests: `Fixes #123` or `Fixes LOC-123` in the description closes the issue when the pull request merges, and `Part of LOC-123` links it without closing it. A branch named from Linear's "Copy git branch name" (`bnb/loc-123-…`) links it as soon as it's pushed. A linked pull request moves its issue to In Progress when it's opened, In Review when a review is requested, and Done when it merges.
- Labels: the ones in [labels.yml](labels.yml), a tool's (`locallibrary`) and an area's (`locallibrary: player`), and `bug`, `enhancement` and `scheduled-failure` for what kind of issue it is. To move an issue to another tool or area, change its label.
- Priority and estimate are set while triaging. A release is a Linear project, like `locallibrary v0.1.0`, with a target date. Big work gets sub-issues, and work that has to wait gets a "blocked by" relation.
- Agents: Claude sessions here follow [.claude/skills/linear](../.claude/skills/linear/SKILL.md), and [linear-triage](../.claude/skills/linear-triage/SKILL.md) for the Triage queue. `agent-ready` marks an issue scoped well enough for an agent to take on alone. To start Claude Code on an issue from Linear, add it under Settings > Code & reviews (the desktop app) with this prompt template: "Make a worktree from origin/main on this issue's branch name. Load the linear skill. Reproduce the problem with a failing test, fix the cause, and leave the changes uncommitted for review."
