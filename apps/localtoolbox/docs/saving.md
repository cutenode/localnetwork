# Saving and restoring everything

localtoolbox keeps everything in the extension's own storage, in this browser profile. It isn't
synced, and removing the extension deletes it. To keep it, or move it to another browser, save it
to a file.

## What's in the file

One JSON file holds most of it:

- your downloaded marks,
- the Manager's list,
- the artists list, without their profiles (those last until you close the browser),
- the session queue,
- and three options: background tabs, the Manager's sort order and hide downloaded.

**your data**, in Settings, shows how many marks, listed tracks, artists and queued tracks you
have.

## Export

Open Settings and press **export** under **your data**. It downloads `manager-<date>.json`.

The session queue ends when the browser closes, so exporting is the one way to keep a batch you
haven't worked through.

## Import

Press **import…**, then **choose a file** or paste the file's text in the box. Pasting is there
because Chrome can close the popup when a file picker opens.

localtoolbox reads the file as soon as it's in the box, and says what's in it ("a backup from
2026-09-08", with counts) or what's wrong with it. Nothing is written until you press the button.

## Adding, or replacing

An import adds to what you have, and removes nothing:

- marks are added, and a track marked in both keeps the earlier date,
- tracks and artists are added to their lists, and ones already there keep their place,
- queued tracks are added to the queue, once each,
- an option is set only if you haven't set it here.

To restore an exact copy instead, turn on **replace what's here instead of adding to it**. The
button changes to **replace everything**: your marks, lists, queue and those three options become
the file's, and anything not in the file is gone. SoundCloud tabs already open show the new marks
straight away.

A file saved by an older version may also hold download details for each track. It still imports,
and those are left out.

If any part of the file isn't valid, nothing is imported.

## The format

The file is plain JSON, described by a [JSON Schema](manager-state.schema.json), so other tools
can read or write it.
