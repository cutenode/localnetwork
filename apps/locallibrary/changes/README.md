# Unreleased changes

A pull request that adds a line to the changelog adds it here, in a file named after its branch
without `bnb/` (`bnb/player-fixes` writes `player-fixes.md`), instead of under Unreleased in
`../CHANGELOG.md`. Every branch appending to the same list in one file made each merge conflict
with the rest.

Write it as the changelog would, under the Keep a Changelog heading it goes in:

```markdown
### Fixed

- `Q` in the player turns quantize off in a terminal that reports keys by their codes.
```

A change that rewords an entry already in `../CHANGELOG.md` edits it there.

At a release, each file's lines move under the same heading in Unreleased and the files go,
before Unreleased is renamed ([Releasing](../CONTRIBUTING.md#releasing)).
