<!-- The checks CI runs, and how to run them first: https://github.com/cutenode/localnetwork/blob/main/.github/CONTRIBUTING.md#checks -->

## What and why

<!-- What this changes and the problem it solves. "Fixes #123" closes the issue when it merges. -->

## How it was tested

<!-- The tests that cover it, and anything you ran by hand: which commands, pages, drives, players or rekordbox version. -->

## Screenshots

<!-- For UI changes: before and after, at phone and wide widths, in light and dark if it matters. Delete this section otherwise. -->

## New app or plugin

<!-- Only for a pull request that adds an app in apps/ or a locallibrary plugin in plugins/. Delete this section otherwise. -->

- [ ] I went through [Adding an app or plugin](https://github.com/cutenode/localnetwork/blob/main/.github/NEW_PROJECT.md) and did every step for this kind of project, and Repo checks passes
- [ ] The steps no check sees are done too: `CODEOWNERS` if someone else reviews it, a changelog line, and READMEs that say what it does
- [ ] Apps: its "… passed" check is in `.github/rulesets/protect-main.json`, and an admin runs `tools/ruleset.sh apply` once this merges
- [ ] Apps that run as a service: it has a service in `.railway/railway.ts`, applied with `railway config apply`, that deploys from `main` once checks pass

## Checklist

- [ ] Tests cover the change
- [ ] For the website: Storybook stories cover any new UI states
- [ ] For locallibrary: `apps/locallibrary/CHANGELOG.md` has a line under Unreleased, if people using it will notice
- [ ] The READMEs, `docs/` and `--help` still say what the code does (`apps/locallibrary/docs/rpc/` too, for a sidecar change)
