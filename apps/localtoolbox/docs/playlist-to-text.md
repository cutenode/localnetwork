# Playlist to text

**SoundCloud playlist to text** turns a list of tracks into text you can paste into a message, a
spreadsheet or a tracklist. It only reads the page; it asks SoundCloud nothing.

## Use it

1. Open a SoundCloud playlist, a Likes page (yours, in either view, or anyone's) or a track's page.
2. Scroll a long list to the end. SoundCloud only draws more rows as you scroll, and localtoolbox
   reads what's drawn.
3. Click the localtoolbox icon and pick **SoundCloud playlist to text**.

**this page** reads the tab in front. **session queue** reads the tracks you've queued instead,
from any tab.

**copy** puts the text on the clipboard. **download** saves it as a file named after the playlist.

## Options

- **format**: `artist - title`, `title - artist`, `title only`, a `numbered list`, CSV, TSV or
  JSON.
- **pull the artist out of "Artist - Title"**: many uploads come from channels that put the real
  artist in the title. With this on, a title like
  `Justin Timberlake ft. Timbaland - Sexyback (EDIT)` is split, and the artist in it replaces the
  channel's name.
- **drop duplicates** drops repeats of the same artist and title.

## Limits

- On a playlist page, the artist is the uploader's SoundCloud name. Likes pages show SoundCloud's
  artist name, which can differ.
- Liked playlists on a Likes page are skipped, and counted.
- The **Filter** box on your Likes page narrows what SoundCloud draws, so only the filtered tracks
  come out.
