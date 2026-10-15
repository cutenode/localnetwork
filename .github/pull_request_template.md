<!--
apps/locallibrary/CONTRIBUTING.md has the checks CI runs for locallibrary. Running the ones for what you touched first saves a round trip.

locallibrary (Rust, from the repo root):
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

Local Network and localtoolbox (from the repo root, or the app's folder for just that one):
pnpm verify
-->

## What and why

<!-- What this changes and the problem it solves. "Fixes #123" closes the issue when it merges. -->

## How it was tested

<!-- The tests that cover it, and anything you ran by hand: which commands, pages, drives, players or rekordbox version. -->

## Checklist

- [ ] Tests cover the change
- [ ] For locallibrary: `apps/locallibrary/CHANGELOG.md` has a line under Unreleased, if people using it will notice
- [ ] The READMEs, `docs/` and `--help` still say what the code does (`apps/locallibrary/docs/rpc/` too, for a sidecar change)
