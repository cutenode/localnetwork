# The app gets the plain name

The command line package is `locallibrary`, in `apps/locallibrary`, and the library it's built on is
`locallibrary-core`, in `crates/locallibrary`, with `[lib] name = "locallibrary"`. So the name
people install, type and see in installers, archives and the Homebrew formula (and on crates.io,
once it's published) is the app's, and every `use locallibrary::...` stays as it was. The folder and
the package name don't match on purpose: a crate's import name is what code depends on, and a
package name is what people see.

Every release is tagged `<package>-v<version>`: `locallibrary-v0.1.0` for the app, and
`loclib-anlz-v0.1.0` for a `loclib-*` crate when one is published. locallibrary-release.yml runs
only on tags that start with `locallibrary` (`tag-namespace` in `dist-workspace.toml`), so a
`loclib-*` tag never starts it. `locallibrary-core` isn't published, so it's never tagged: a
`locallibrary-core-v0.1.0` tag would match that namespace too, and `dist plan` reads it as a release
of the app.

"core" is a crate name only. Docs and anything a user reads say locallibrary, or name the crate
(`locallibrary-core`) where it's the crate that's meant, never "the core".

## Considered options

- Keep `locallibrary-cli` and `locallibrary`: a `locallibrary-v` tag picks the library, which has
  nothing to release, and the installers are called `locallibrary-cli-installer.sh` for good once
  v0.1.0 ships.
- Rename the library's folder to `crates/locallibrary-core` as well: moves every path the tests,
  budgets, coverage floors and diagrams name, for a name nobody outside the repository sees.
- Rename the library's import name to `locallibrary_core`: every `use` in the workspace and every
  plugin changes, with no gain for anyone running the app.
- One tag per repository (`v0.1.0`): the repository releases several apps, and dist needs the
  package in the tag to know which one.
