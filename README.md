# Local Network

Tools for DJs and the people who book them, built on the AT Protocol. Each tool stands on its own; they share an Atmosphere account, a lexicon tree (`computer.localnetwork.*`), and file formats.

| Tool                                  | What it is                                                                                                     | Built with                    |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------------- | ----------------------------- |
| [Local Network](apps/web)             | The website at localnetwork.computer: promoters find local DJs, DJs keep a profile, bookings run in private spaces | SvelteKit, atproto spaces     |
| [locallibrary](apps/locallibrary)     | A command line tool and terminal player that keeps rekordbox USB drives in sync, backed up and edited           | Rust                          |

## Layout

```
apps/web/             Local Network, the website (its README covers setup and deployment)
apps/locallibrary/    locallibrary's command line tool, player and sidecar, with its docs
crates/               Rust libraries: locallibrary's core and the loclib-* crates other DJ tools can use
vendor/               Vendored Rust crates
```

The repo root is both the pnpm workspace and the Cargo workspace, so commands run from here.

## Working on it

```bash
pnpm install                 # the web app's dependencies and the git hooks
pnpm dev                     # the website, see apps/web/README.md for its .env
pnpm verify                  # format, lint, type check, tests and build for the website

cargo build                  # locallibrary
cargo test --workspace --locked
```

CI runs each tool's checks only when its files (or the shared ones) change.

## Adding a tool

A new tool goes in `apps/<tool>`, with shared code in `crates/` (Rust) or `packages/` (TypeScript). Its records live under its own lexicon group, `computer.localnetwork.<tool>.*`, and binaries and crates follow the same names as the rest: `local<thing>` for a tool, `loclib-*` for a Rust library.
