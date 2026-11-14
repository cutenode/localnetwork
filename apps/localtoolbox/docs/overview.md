# Overview

localtoolbox is a free Chrome extension for DJs who dig on SoundCloud. It isn't in the Chrome Web
Store yet: [Installing localtoolbox](install.md) shows how to build it and load it into Chrome.

Click its icon in the toolbar and pick a tool:

- **Manager** keeps a list of tracks and shows where each one can be downloaded: SoundCloud's own
  Download file, or the link the uploader set, grouped by site (Hypeddit, Droploud, Bandcamp and
  so on).
- **third-party downloader** gets the files behind download gates like Hypeddit and Droploud,
  opening the ones you do on the site's own page with the steps outlined.
- **SoundCloud playlist to text** turns a playlist, Likes page or track page into plain text,
  CSV, TSV or JSON.
- **artists** keeps a list of the artists behind your tracks, with their socials and contact
  addresses.

Every view's header has two more buttons. The **session queue** holds the tracks you've picked
out while working through a page, until you mark the batch as downloaded or hand it to another
tool. **Settings** has the choices for the whole extension, and saving everything to a file.

On SoundCloud's own pages, localtoolbox marks the tracks you've downloaded, flags the ones with no
download, and puts a queue button on each track's cover art. See
[Marks on SoundCloud pages](marks.md).

## What it reads and sends

There's no account to sign in to, and nothing goes to Local Network. Everything localtoolbox keeps
stays in your browser, in the extension's own storage.

It reads SoundCloud pages as they're drawn. It asks SoundCloud's public API about a page's tracks
when you add the page to the Manager's list or open the third-party downloader on it, and about
artists when you add them to the artists list. The downloader talks to the gate sites it walks,
and gives a gate the email address and name you typed in Settings when the gate asks for one.

## Guides

- [Installing localtoolbox](install.md)
- [Marks on SoundCloud pages](marks.md)
- [Downloads and gates](downloads.md)
- [Playlist to text](playlist-to-text.md)
- [Artists](artists.md)
- [Saving and restoring everything](saving.md)

Developers: the [README](../README.md) describes how each part works, and how to build, test and
change it.
