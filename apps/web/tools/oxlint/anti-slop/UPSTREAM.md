# Vendored anti-slop

Source: [dmmulroy/anti-slop](https://github.com/dmmulroy/anti-slop), commit `e6676e8d0bf17c678cb45b9dacb2bd6ca8dea53a` (the latest commit touching `skills/install-anti-slop` when this was copied; upstream `main` was `c44ef22ca116d0ba62a3ff663a0bd13a3f3fa40b`).

Copied from `skills/install-anti-slop/assets/anti-slop/` (git tree `a7831feb0c097943ac813ddcb1367e26eef52da5`) by the skill's `scripts/install.mjs`, installed on 2026-10-06 from skill tree `89044d21c75a367eac1ddbaf208e650b1a7d5820`. Every file here except this one matches that tree byte for byte.

## Installed paths

- `index.ts`: the generic `anti-slop` plugin, registered in `.oxlintrc.json` `jsPlugins` with all 18 rules (plus native `oxc/no-accumulating-spread`) at `error`.
- `effect/`: the opt-in `anti-slop-effect` plugin. Copied with the snapshot but not registered, because this app doesn't depend on `effect`.
- `vendor/eslint-stylistic/`: the padding-line rule behind `require-readable-spacing`, with its own `LICENSE` and `UPSTREAM.md`. Keep both.

## Local configuration

- `@oxlint/plugins` is pinned to the exact installed `oxlint` version (1.86.0). Upgrade them together.
- `.oxlintrc.json` ignores this directory and agent tooling directories; `.oxfmtrc.json` ignores this directory so formatting doesn't drift from upstream.
- `tsconfig.json` includes `tools`, so `pnpm check` typechecks the plugin against the pinned `@oxlint/plugins` types.

## Deviations

None. Update through the skill's update procedure (`references/update.md`), which merges upstream changes against this snapshot.
