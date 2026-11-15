# Marks on SoundCloud pages

localtoolbox draws on SoundCloud's own pages, so you can see what you have while you browse:
playlist rows, Likes rows and tiles, a track page's artwork and the player bar. It adds to the
page, and the two Settings switches below change SoundCloud's download buttons and your likes view.
Nothing is sent to SoundCloud for it.

## Downloaded

A track you've downloaded gets a **Downloaded** pill on its cover art. Where the art is small, on
playlist rows and in the player bar, the pill is a check mark.

A track counts as downloaded when:

- you click SoundCloud's **Download file** or a purchase link on a SoundCloud page,
- the third-party downloader saves its file,
- or you mark it by hand, in the Manager, the downloader or the session queue.

Opening a link from the Manager doesn't mark the track. The file isn't saved yet, so the track goes
in the session queue instead, and you mark the batch from there once the files are in. See
[Downloads and gates](downloads.md#the-session-queue).

**Unmark** in the Manager's header takes every mark off. It asks once more before it does.

## No download

A track that's been looked up and has no download link at all gets a dark **nodl** pill, so you
can skip it at a glance. Lookups are kept for thirty days, then asked again.

## The queue button

Hover a track's cover art, or tab to it, and a **+ queue** button comes up in the top right
corner. It puts the track in the session queue without leaving the page. On a track already in the
queue it stays up and reads **queued**; click it again to take the track out.

The button appears only after you've opened the popup once in this browser session, because
that's when Chrome lets the page see the queue. A tab already open gets it the next time the queue
changes, or when you reload it.

## Download buttons

With **replace SoundCloud's download buttons** on in Settings (it's on to begin with), SoundCloud's
Download file item and the uploader's Buy or Free Download link are swapped for localtoolbox's
own **Download** button. Hover it to see where the file comes from, such as `hypeddit.com` or
`soundcloud.com`. Clicking it follows the link, or presses SoundCloud's own Download file for
you, and puts the track in the session queue.

A link to Hypeddit is walked there and then, the way the third-party downloader does it: the
file is saved and the track marked downloaded. If the gate needs you (a CAPTCHA, an email address
Settings doesn't have yet, a Spotify step to connect once), it opens in a tab and the track is
queued. A link to Droploud, Pump Your Sound or Laylo opens in a tab with the steps outlined and
your email filled in, and the track is queued; once you've done the gate, the file is saved and
the track marked. Hold ⌘, Ctrl, Shift or Alt to follow the link instead.

Turn the setting off and SoundCloud's own buttons come back, in every open tab.

## Your likes as a list

`soundcloud.com/you/likes` opens as a grid of tiles unless you've switched it to a list. Tiles show
less: no genre, no like or repost buttons, and no purchase links to replace. Turn on **open your
likes as a list** in Settings and localtoolbox presses SoundCloud's own List switch when that page
shows the grid. It's off to begin with, because SoundCloud remembers the switch after you leave.

## Limits

- On a track page you're signed in to SoundCloud on, the Download button works, but the Manager
  can't add the track yet.
- SoundCloud only draws more rows as you scroll, so scroll a long playlist to the end before you
  add it to anything.
- SoundCloud can change its pages at any time. If marks stop showing up, use **report a bug** at
  the bottom of Settings.
