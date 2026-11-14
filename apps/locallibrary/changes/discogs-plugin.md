### Added

- A discogs plugin finds front covers (`art fetch --from discogs.covers`) and an album's title,
  year, record label and style (`track fetch --from discogs.tags`) on Discogs, which lists many
  white labels and vinyl-only releases that MusicBrainz doesn't. Each album is searched by its name, artist and
  label, then by name and artist, and a track without an album by its title. It searches with
  your Discogs account: make a personal access token at discogs.com/settings/developers and sign
  in with `locallibrary account discogs sign-in --for discogs` (the new discogs-account plugin).
  Searches go a second apart, and wait when Discogs says the account is over its limit.
