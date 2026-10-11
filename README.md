# Local Network

Tools for DJs and the people who book them, built on the AT Protocol. Each tool stands on its own; they share an Atmosphere account, a lexicon tree (`computer.localnetwork.*`), and file formats.

| Tool                              | What it is                                                                                                             | Built with                |
| --------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ------------------------- |
| [Local Network](apps/web)         | The website at localnetwork.computer: promoters find local DJs, DJs keep a profile, bookings run in private spaces     | SvelteKit, atproto spaces |
| [locallibrary](apps/locallibrary) | A command line tool and terminal player that keeps rekordbox USB drives in sync, backed up and edited                  | Rust                      |
| [localtoolbox](apps/localtoolbox) | A Chrome extension for digging on SoundCloud: track lists, download links, artists' socials, and a queue to batch them | Svelte, Manifest V3       |

## Layout

```
apps/web/             Local Network, the website (its README covers setup and deployment)
apps/locallibrary/    locallibrary's command line tool, player and sidecar, with its docs
apps/localtoolbox/    localtoolbox, the Chrome extension (its README is also its spec, checked by its tests)
lexicons/             Every tool's lexicons (computer.localnetwork.*), plus copies of others' they use
crates/               Rust libraries: locallibrary's core and the loclib-* crates other DJ tools can use
vendor/               Vendored Rust crates
tools/oxlint/         The vendored anti-slop Oxlint plugin, for every TypeScript app
```

The repo root is both the pnpm workspace and the Cargo workspace, so commands run from here.

## Working on it

```bash
pnpm install                 # every TypeScript app's dependencies and the git hooks
pnpm dev                     # the website (see apps/web/README.md for its .env) and the extension's watch build
pnpm verify                  # each app's format, lint, type check, tests and build, and the lexicons' format

cargo build                  # locallibrary
cargo test --workspace --locked
```

The TypeScript apps share one lint config (`.oxlintrc.json`, which each app's own extends) and one format config (`.oxfmtrc.json`). CI runs each tool's checks only when its files (or the shared ones) change.

## Adding a tool

A new tool goes in `apps/<tool>`, with shared code in `crates/` (Rust) or `packages/` (TypeScript). Records only it writes live under its own lexicon group, `computer.localnetwork.<tool>.*`; records other tools read too go in a shared group named for what they are, like `computer.localnetwork.dj.*` for DJ sets and profiles. Binaries and crates follow the same names as the rest: `local<thing>` for a tool, `loclib-*` for a Rust library.
