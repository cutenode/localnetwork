# Changelog

All notable changes to localtoolbox are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [1.0.0] - 2026-11-23

The first version in the Chrome Web Store. Builds before it were numbered 2.0.0 to 2.11.0 and
were only ever loaded unpacked from source, before there was a store listing. The numbering starts
again here, so 1.0.0 comes after 2.11.0.

It has four tools for SoundCloud: Playlist to text, the Manager, the Downloader and Artists, plus
the session queue, the Downloaded and nodl marks on SoundCloud's pages, and localtoolbox's own
Download button in place of SoundCloud's.

### Changed

- The first-run notice names everything that leaves the browser through the extension: the email
  and name a Hypeddit gate asks for, the Spotify step and the background tab it opens, the email
  typed into Droploud and Laylo forms, and the Download buttons on SoundCloud doing the same.
  Anyone who read the old notice sees the new one once.
- After a fresh install, SoundCloud's own download buttons stay until the notice has been read.
  Before, localtoolbox's replaced them from the moment it was installed.
- The README's Privacy section, the extension's privacy policy, says what the code does: what it
  reads, what it keeps and for how long, what leaves the browser and where to, and how to delete
  it.
