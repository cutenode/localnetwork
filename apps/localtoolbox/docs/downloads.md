# Downloads and gates

Two tools work together here. The **Manager** keeps a list of tracks and shows where each one can
be downloaded. The **third-party downloader** walks the download gates behind those links and
saves the files. The **session queue** in the header holds what you've picked out in between.

## The Manager's list

Nothing goes on the list by itself. Open a SoundCloud playlist, Likes page or track page, scroll a
long list to the end, then open the Manager. The bar at the top names the page and how many of its
tracks are new:

- **add 12** puts them on the list with where each one can be had, from what the page has loaded.
  Tracks further down than you've scrolled say **not loaded yet**.
- **refresh 42** shows up instead once the whole page is on the list, and reads them again.
- **queue** adds the tracks in the session queue, from any tab.

The list stays until you take tracks off it, across tabs and browser restarts.

## Where each track downloads

**by site** groups the list by where its links go: **soundcloud.com** first, then sites like
hypeddit.com, droploud.com and bandcamp.com, biggest first. **list order** shows each track once,
in the order you added it.

- A row in an outside site's section has a download button that opens the link in a background
  tab and puts the track in the session queue. Turn off **background tabs** to open links in front.
- In **soundcloud.com**, the button opens the track's page. SoundCloud's download is the Download
  file item in the track's More menu, and using it marks the track. A track whose download limit
  is used up says "no downloads left".
- **no download links** lists tracks with neither. Each has search badges for the artist on
  Beatport, Bandcamp, ZIPDJ and MyMP3Pool. Pick which ones show in Settings, under search badges.
- **not loaded yet** is for tracks the page hadn't loaded when you added it. Scroll to them or open
  the track, then add the page again.

Each row also has a button to queue the track without opening anything and a check button to mark
it downloaded by hand. Click a row's cover art to copy the artwork at full size; ⌘-click opens it
in a tab. The chevron opens the row into its title, artist, genre and link, each copied with a
click.

## Working through a lot at once

The search box keeps the rows matching every word you type, by title, artist, site or the page
they came from. **hide downloaded** drops the tracks already marked, so what's left is what you
still need.

**select several tracks**, the tick-box button in the header, puts a tick box on every row.
Shift-click to tick a run of rows, or use **tick this page** for just the rows on the tab in front.
Then **queue**, **mark** or **remove** them in one go. Removing takes tracks off the list, not
their marks or their place in the queue.

## The session queue

The queue button in the header shows how many tracks you've picked out and opens them as a sheet.
Opening a link from the Manager, the queue button on SoundCloud's cover art and a failed gate you
open by hand all put tracks here.

Once the files are saved, **mark N tracks as downloaded** marks the batch, and **take N marked
out** clears what's done. **send the queue somewhere** hands it to playlist to text or the
third-party downloader; a tool already open under the sheet picks it up the next time it opens.
The queue lasts until you close the browser; to keep a batch longer, add
it to the Manager's list or [save it to a file](saving.md).

## Download gates

A free download on SoundCloud often isn't on SoundCloud. The Buy or Free Download link goes to a
gate that wants a follow, a repost or your email before it hands over the file. The third-party
downloader takes each link as far as it goes:

| Site            | Ends in a file?                                          |
| --------------- | -------------------------------------------------------- |
| Hypeddit        | Yes                                                      |
| Droploud        | Yes                                                      |
| Pump Your Sound | Yes                                                      |
| ToneDen         | Sometimes                                                |
| FanLink         | Sometimes                                                |
| Laylo           | No: it signs you up, and the artist emails you the track |
| Feature.fm      | Rarely: it's usually a page of shop and streaming links  |

Open it from the menu. With **this page** it takes every gate link on the SoundCloud page in front,
or the one link on a gate's own tab. With **session queue** it takes the links of the queued
tracks, from any tab. You can also paste links, or a whole message with links in it, into **add
links by hand** at the bottom.

**download all** walks every row, one at a time, or click **get** on one row. Rows say how they're
going. Files land in a folder per site (`Hypeddit`, `Droploud` and so on) in your downloads folder,
and saving one marks its track downloaded unless you turn off **mark the SoundCloud track
downloaded** in Settings. **hide downloaded** and **hide queued** narrow the list to what you
haven't got yet.

## When a gate needs you

Some gates want something only you can give:

- **An email address or name.** Hypeddit and Laylo can ask for one. The row offers **add email**
  or **add name**, which opens Settings where you type it once.
- **Spotify.** A Hypeddit gate with a Spotify step follows the artist from your Spotify account.
  The first one needs you to press Connect on the gate once; after that they go through on their
  own.
- **ToneDen and FanLink** often need one visit by hand per gate. The row says which account the
  gate wants.
- **A CAPTCHA**, or a step the downloader can't do, needs the gate opened in a tab.

A failed row leads with **open**, which takes you to the gate and puts the track in the session
queue, so you can mark it once the file's in. **get** stays there to try again.

The Download button localtoolbox puts on SoundCloud's pages walks Hypeddit, Droploud and Pump Your
Sound gates the same way. See [Marks on SoundCloud pages](marks.md#download-buttons).
