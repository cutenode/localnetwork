# Vendored anti-slop

Source: [dmmulroy/anti-slop](https://github.com/dmmulroy/anti-slop), commit `e6676e8d0bf17c678cb45b9dacb2bd6ca8dea53a` (the latest commit touching `skills/install-anti-slop` when this was copied; upstream `main` was `c44ef22ca116d0ba62a3ff663a0bd13a3f3fa40b`).

Copied from `skills/install-anti-slop/assets/anti-slop/` (git tree `a7831feb0c097943ac813ddcb1367e26eef52da5`) by the skill's `scripts/install.mjs`, installed on 2026-10-06 from skill tree `89044d21c75a367eac1ddbaf208e650b1a7d5820`. Every rule and shared file here still matches that tree byte for byte; `index.ts` differs only by the removed `require-readable-spacing` registration (see Deviations).

## Installed paths

- `index.ts`: the generic `anti-slop` plugin, registered in the repo root's `.oxlintrc.json` `jsPlugins` (each TypeScript app's config extends it) with all 17 rules (plus native `oxc/no-accumulating-spread`) at `error`.
- `rules/*.test.ts`: upstream's RuleTester suites from `src/rules/` at the same commit, run by the root's `pnpm test:tools` (vitest, with `vitest.config.ts`'s coverage thresholds).
- `rule-tester.ts`: local. Hands Oxlint's RuleTester vitest's `describe` and `it`, which upstream's suites get as globals from their own runner.

## Local configuration

- Moved from `apps/web/tools/oxlint/anti-slop/` to the repo root when a second TypeScript app (apps/localtoolbox) joined.
- `@oxlint/plugins` is pinned, in the root `package.json`, to the exact `oxlint` version every app installs (1.86.0). Upgrade them together.
- The root `.oxlintrc.json` ignores this directory and agent tooling directories; the root `.oxfmtrc.json` ignores this directory so formatting doesn't drift from upstream.
- `apps/web/tsconfig.json` includes `../../tools`, so the web app's `pnpm check` typechecks the plugin and its tests against the pinned `@oxlint/plugins` types.

## Deviations

- `require-readable-spacing` and `vendor/eslint-stylistic/` (upstream's copy of ESLint Stylistic's `padding-line-between-statements`) are removed. The root `.oxlintrc.json` loads the published `@stylistic/eslint-plugin` as a JS plugin and enables `@stylistic/padding-line-between-statements` with the wrapper's options, unchanged. Upstream's `require-readable-spacing.test.ts` cases at this commit pass against that rule and those options.
- `effect/` (the opt-in `anti-slop-effect` plugin) is removed: it was never registered, and no package here depends on `effect`.
- Each test file imports `tester` from `../rule-tester.ts` in place of its own `RuleTester` import and `new RuleTester(...)` line, which had the same config in every file.
- `rules/no-widen-then-assert.test.ts` has a second, local block of cases after upstream's, which covered about half the rule.

## Pending upstream changes

- [`d2a9aea`](https://github.com/dmmulroy/anti-slop/commit/d2a9aea3b896ffcb291a85f2c1301aa2543ad8c8) keeps consecutive single-line bindings grouped at module scope in `require-readable-spacing`. Not adopted: it changes the policy. With the published rule, the closest match is a last `"any"` entry between `singleline-const`/`let`/`var`/`using` statements in `.oxlintrc.json`; upstream also starts a new group at a binding with its own leading comment, so check that case before adopting it.

Update through the skill's update procedure (`references/update.md`), which merges upstream changes against this snapshot. Every other upstream change through `13909c9` is already here.
