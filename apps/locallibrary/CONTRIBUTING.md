# Contributing

locallibrary is the CLI in this folder plus the library crates in the repository's `crates/` and
`vendor/`, one Cargo workspace whose root is the repository root. Paths below that start with
`crates/`, `vendor/` or `.github/` are from there; the rest are from this folder.

```bash
cargo test --workspace --locked     # unit + integration tests against real rekordbox export fixtures
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
```

CI expects all four to pass, and the builds without the default features to build and pass
their tests too (see [Install](docs/install.md#install) for the features; CI tests them weekly):

```bash
cargo test -p locallibrary --no-default-features --locked
cargo test -p locallibrary-cli --no-default-features --locked
cargo test -p locallibrary-cli --no-default-features --features account --locked
cargo clippy -p locallibrary-cli --no-default-features --features tui,account --all-targets --locked -- -D warnings
```

Code that needs a feature is behind `#[cfg(feature = "...")]`; tests that need one say so the
same way (`#![cfg(feature = "account")]` for a whole file). The RPC contract in `docs/rpc/`
describes the default build, so the test that checks it only runs there.

The crates in `crates/` besides `locallibrary` are for other tools too (see
[Crates for other DJ tools](README.md#crates-for-other-dj-tools)), so they never depend on
locallibrary: it depends on them, re-exports them where their code used to live, and converts
their errors into its own in `crates/locallibrary/src/error.rs`, keeping the messages and RPC
error codes. Their tests sit beside their code (`cargo test -p loclib-tracklist`), and tests that
need locallibrary's fixtures stay in locallibrary. Each one's README is its crate docs, so the
README's examples are doctests: run `cargo test --doc -p <crate>` after changing one, since CI
skips changes to Markdown alone.

**Toolchain:** `rust-toolchain.toml` pins the Rust that CI and
release builds use (1.96.1, with clippy and rustfmt), and rustup installs it the first time
cargo runs in the checkout. A Rust that doesn't come from rustup (Homebrew's, say) ignores the
file, and its clippy may disagree with CI's. The oldest Rust locallibrary supports (its MSRV) is
`rust-version` in Cargo.toml, 1.91; check it with `cargo +1.91 check --workspace --locked`.
Change either on purpose: bump the pin in a commit of its own, fixing what the newer clippy
finds, and raise `rust-version` together with the msrv job in `locallibrary-ci.yml`.

**CI** (`.github/workflows/locallibrary-ci.yml`) runs the quick checks on pull requests and
pushes to `main` that change the workspace (not when only Markdown changed):

- format, clippy and the tests on macOS, Linux and Windows. Tests run under
  [cargo-nextest](https://nexte.st) (`cargo nextest run --workspace` locally), doctests with
  `cargo test --doc`. On Linux they run under coverage (see below). On macOS and Linux the tests
  have the `sqlcipher` CLI, and must find it (see the fixtures below);
- clippy on the CLI without its default features, on Linux without the ALSA headers;
- the MSRV check, and the docs build with warnings denied;
- [cargo-deny](https://embarkstudios.github.io/cargo-deny/) with `deny.toml`: RustSec advisories
  (each ignored one says why), banned crates (OpenSSL: TLS goes through rustls) and crate
  sources, and that every shipped dependency's licence is one `deny.toml` allows.

`.github/workflows/locallibrary-scheduled.yml` runs the slow suites every Monday, and on any
branch from the Actions tab (locallibrary scheduled, Run workflow):

- the builds without the default features (on Linux, without the ALSA headers): the tests of the
  library and the CLI with none and of the CLI with only `account`, and clippy over every
  combination of features with [cargo-hack](https://github.com/taiki-e/cargo-hack);
- the Bombadil explorations below;
- cargo-deny's advisory check, for advisories published since the last push.

Actions are pinned to commits, and Dependabot proposes updates to them and to `Cargo.lock` once a
month.

The library's integration tests are one test binary, `crates/locallibrary/tests/it/` (a module
for each area: `edit`, `sync`, `props_pdb`..., with an area's submodules in a folder of its name,
like `edit/refusals.rs`, and the helpers they share in `tests/common/`), so the library is linked
once and every test runs at once. Tests that need the mounted volumes, or the store drive
lookups consult, name them for their own thread (`locallibrary::testing::volumes_on_this_thread`
and `store_root_on_this_thread`), not with `$LOCALLIBRARY_VOLUMES` or `api::set_store_root`,
which hold for the process. The CLI's are one binary too, `tests/it/`,
running the `locallibrary` cargo built for them with a store of their own and
`LOCALLIBRARY_VOLUMES` set, so they never see the machine's drives. What they share is in
`support.rs`: the demo drives, and `Env`, a store and folder of drives for one test that runs
commands in them.

`LOCALLIBRARY_VOLUMES` (a path list, like `PATH`) is a test switch, set together with
`LOCALLIBRARY_STORE`: drives are looked for only in the folders it names (set empty, nowhere),
never among the machine's mount points or where the store last saw them outside those folders,
and `drive disks`, `drive format`, `drive fsck` and `drive rename --label` refuse to run. Every
command warns when it's set. Tests sharing a process name their folders for their own thread
(`locallibrary::testing::volumes_on_this_thread`).

```bash
cargo test -p locallibrary --test it                        # the library's integration tests
cargo test -p locallibrary --test it sync::                 # one area's
cargo test -p locallibrary-cli --test it rpc::              # the sidecar's, in the CLI's
```

The fixtures in `crates/locallibrary/tests/fixtures/` are real rekordbox files from
rekordcrate (MPL-2.0), including a 3,886-track export database (see the README there); the
tests also use the vendored rekordbox-pdb's exports (MIT, `vendor/rekordbox-pdb/tests/data/`).
The tests cross-check what locallibrary writes against rekordcrate's independent parser. The
OneLibrary fixtures (`onelibrary/`, rekordbox's schema with synthetic tracks) are made by the
`sqlcipher` CLI (`make.sh`), so reading is checked against an independent SQLCipher. With
`sqlcipher` installed, the tests that merge, clean, convert or edit a OneLibrary database also
have it check the result (`tests/support/sqlcipher.rs`); without it they skip that check, unless
`LOCALLIBRARY_REQUIRE_SQLCIPHER=1` is set, as in CI, which makes them fail.

Publishing is tested against a pretend data server on 127.0.0.1
(`crates/locallibrary/tests/support/mock_pds.rs`, shared with the CLI's tests): it keeps records,
checks each write's DPoP proof and hands out a DPoP nonce, and refuses on cue. The tests write the
session as files, or to an in-memory keychain, so none of them signs in to a real account,
touches the computer's keychain or goes on the network.

## Coverage

```bash
cargo coverage              # the tests under coverage, with a summary per file
cargo coverage --open       # and an HTML report
tools/hegel-coverage.sh     # the property tests alone, against the same thresholds (from the repository root)
```

`cargo coverage` is an alias in `.cargo/config.toml` for
[cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) over nextest. It fails when line,
function or region coverage drops below the thresholds there, set just under what CI measures on
Linux; raise them when coverage goes up. It measures only the code that ships: unit tests live in
a module's own `tests.rs` (`foo.rs` declares `#[cfg(test)] mod tests;`, and the tests are in
`foo/tests.rs`), which the alias leaves out with the TUI's other test-only files (`props.rs`,
`screens.rs`, `testing.rs`) and `test_util.rs`. Keep new unit tests in those files, not inline. It
needs `cargo-llvm-cov`, `cargo-nextest` and rustup's `llvm-tools-preview` component, found
through rustup's `cargo`. Homebrew's Rust has no `llvm-profdata`, so with it installed put rustup's
proxies ahead of it on `PATH` (`$(brew --prefix rustup)/bin` for Homebrew's rustup).

`tools/hegel-coverage.sh` runs `cargo coverage` over only the `#[hegel::test]` tests (picked by
function name, so not the fixed examples beside them in the `props_*` modules), and so fails below
the same thresholds: the properties alone have to reach what the whole suite does. It also needs
`rg`. Code it reports as missed is code no property reaches: a place where a new or wider
generator would test more.

## Property-based tests

The `props_*` modules in `crates/locallibrary/tests/it/` (and a few unit tests in the CLI) use
[Hegel](https://github.com/hegeldev/hegel-rust): each test states something that must hold for
every input, Hegel generates hundreds of inputs and, when one fails, shrinks it to the smallest
example. They run with `cargo test` like any other test. The bigger ones are model tests: random
sequences of edits to `export.pdb`, `exportExt.pdb`, OneLibrary cue rows, analysis files, whole
drives (plan, apply, undo) and the store, checked after every step against an in-memory model and,
where it can, rekordcrate's independent parser. `props_sync` does the same for sync, push, pack
and merge, with the DJ's own files around the library and runs cancelled, or cut off as by a pulled
plug, at any progress checkpoint. The CLI's own, in `apps/locallibrary/tests/it/props_cli.rs` and
`props_rpc.rs`, run the built binary: every command with random arguments and flags against the
output contract, commands without `--yes` against the files they mustn't touch, edits against a
model, and the sidecar against the contract in `docs/rpc/schema.json`. `props_cli_human.rs` and
`props_cli_changes.rs` check that each command's text says what its `--json` says, on drawn drives
and stores; `props_cli_asked.rs` answers `setup`'s questions in a terminal and stops `watch` and
`store serve` with Ctrl-C.

```bash
cargo test -p locallibrary --test it props_pdb::            # one module
HEGEL_SEED=7 cargo test ...                                 # repeat a run
```

To search harder, raise `test_cases` in the test's `#[hegel::test(...)]` for the run. Most tests
set one, and it wins over `HEGEL_TEST_CASES`, which only reaches the tests that don't.

A failure prints the shrunk example and a `#[hegel::reproduce_failure("…")]` line to replay it;
locally, failing examples are also kept in `.hegel/` in the crate's folder (ignored by git), under
the test's path (`it::props_pdb::...`), and tried first next time.
On CI runs are derandomised.

## Exploring the TUI and the mix report

[Bombadil](https://github.com/antithesishq/bombadil) drives the TUI in a terminal and the mix
report in headless Chrome with random keys, clicks and resizes, checking properties over time
(`bombadil/tui.ts`, `bombadil/report.ts`): the header and footer are always drawn, digits switch
views, help opens and closes, a typed line keeps its cursor in view, edits finish, the report's
counts agree and its player stays consistent. macOS and Linux only, with Node 22:

```bash
cd bombadil && npm ci       # from this folder
./tui.sh                    # 2 minutes; TIME_LIMIT=30m ./tui.sh for longer
./report.sh                 # needs Chrome: npx -y playwright@1.63.0 install --only-shell chromium
```

`tui.sh` runs the TUI on a throwaway copy of a fixture drive with `LOCALLIBRARY_VOLUMES` pointing
at the folder that holds only that copy, an empty store, `open` stubbed out and a home and
working folder of its own, so files it writes stay in the run's folder; it lets the run write
edits to that copy and checks afterwards that the drive still reads. `report.sh` writes
synthetic audio for a fixture drive's tracks, records a "set" from four of them
(`make-mix.mjs`), has `locallibrary mix` identify it and explores the report it writes. Both
print where the trace went; add Bombadil options after the script name (`./tui.sh --seed 7`).

## Releasing

[dist](https://axodotdev.github.io/cargo-dist/) builds releases (`dist-workspace.toml` at the
repository root, `.github/workflows/locallibrary-release.yml`) when a version tag is pushed:

1. In `CHANGELOG.md`, rename "Unreleased" to the version and date (`## [0.2.0] - 2026-11-02`)
   and start a new "Unreleased" above it. The GitHub Release takes its notes from that section.
2. Bump `version` in `[workspace.package]` in the root `Cargo.toml`, run `cargo check` so
   `Cargo.lock` follows, and commit.
3. Tag the commit with the CLI package's name and push the tag:
   `git tag locallibrary-cli-v0.2.0 && git push origin locallibrary-cli-v0.2.0`. Only tags that
   start with `locallibrary` release it (`tag-namespace` in `dist-workspace.toml`), so the
   repository's other apps can tag their own. A version with a suffix
   (`locallibrary-cli-v0.2.0-beta.1`) is a prerelease, which leaves the Homebrew formula alone.

The workflow builds an archive for each of macOS (Apple silicon, Intel), Linux (x86_64 glibc) and
Windows (x86_64), plus `locallibrary-cli-installer.sh`, `locallibrary-cli-installer.ps1` and a
Homebrew formula, creates the GitHub Release with all of them and pushes the formula to the tap.
On pull requests that change a `Cargo.toml`, `Cargo.lock`, `dist-workspace.toml`,
`rust-toolchain.toml` or the workflow itself, it only runs `dist plan`, which checks the
configuration. `locallibrary-release.yml` is dist 0.33.0's generated workflow with hand-written
additions listed at its top, so `dist-workspace.toml` sets `allow-dirty = ["ci"]`. After changing
the dist configuration or upgrading dist, regenerate the file (`dist generate --mode=ci`, with
`allow-dirty` removed for the run) and carry the additions over.

Still to set up before the first release (the workflow runs without them, and says what it
skipped):

- **Third-party notices.** dist puts `LICENSE` in the archives and the Homebrew formula, but the
  binary also holds code under other licences (the dependencies, and the vendored rekordbox-pdb
  with its `template.pdb`), whose notices have to ship with it: generate a notices file at
  release (with `cargo about`, say) and include it.
- **The Homebrew tap.** Create the `cutenode/homebrew-tap` repository, and save a fine-grained
  token that can write to its contents as the `HOMEBREW_TAP_TOKEN` secret. Until then the
  formula is only attached to the release.
- **macOS signing and notarization**, with an Apple Developer Program membership. Without these
  secrets the macOS binaries ship unsigned, and macOS refuses one downloaded with a browser
  until its quarantine flag is removed (the installers and Homebrew don't set it). Signing
  (dist runs `codesign` with the hardened runtime before archiving) needs:
  - `APPLE_CERTIFICATE`: a Developer ID Application certificate and its private key, exported
    from Keychain Access as a `.p12` and base64-encoded (`base64 -i cert.p12 | pbcopy`);
  - `APPLE_CERTIFICATE_PASSWORD`: the password the `.p12` was exported with;
  - `APPLE_SIGNING_IDENTITY`: the certificate's name, `Developer ID Application: Name (TEAMID)`.

  Notarization (of signed binaries) needs an App Store Connect API key with the Developer role:
  - `APPLE_API_KEY`: the contents of its `AuthKey_<key id>.p8` file;
  - `APPLE_API_KEY_ID`: the key's ID;
  - `APPLE_API_ISSUER`: the issuer ID shown above the keys list.

  Setting only some of a group fails the release instead of shipping it half done. None of this
  has run yet: check the first signed release's log for Apple's verdict.
- Windows binaries aren't signed (SmartScreen warns about them); dist can sign them through
  Azure Artifact Signing if that's ever wanted.
