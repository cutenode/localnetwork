<!--
CONTRIBUTING.md has the checks CI runs. Running them first saves a round trip:

cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
-->

## What and why

<!-- What this changes and the problem it solves. "Fixes #123" closes the issue when it merges. -->

## How it was tested

<!-- The tests that cover it, and anything you ran by hand: which commands, on what drives, players or rekordbox version. -->

## Checklist

- [ ] Tests cover the change
- [ ] `CHANGELOG.md` has a line under Unreleased, if people using locallibrary will notice it
- [ ] The README, `docs/` and `--help` still say what the code does (`docs/rpc/` too, for a sidecar change: `cargo run -p locallibrary-cli -- rpc --emit-schema docs/rpc`)
