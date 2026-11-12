### Fixed

- The coverart plugin waits as long as a busy MusicBrainz asks (`Retry-After`) before asking
  again, and leaves the album out when that's over 30 seconds. It asked again at once.
- Publishing to SoundCloud or Mixcloud waits out a `Retry-After` given as a date too.
- A tab or line break in a set's title (published to plyr.fm, the Internet Archive or Mixcloud)
  or a track's name (in the tracklist plyr.fm and the Internet Archive get) is sent as a space,
  as SoundCloud and YouTube already had it.
