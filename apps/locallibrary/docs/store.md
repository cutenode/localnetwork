# The store

`pull` saves a library in the store as a snapshot, from the drives that hold it: every file, kept
once however many snapshots and drives share it. Snapshots belong to a library; a drive pulls into
its own library unless `--library` names another.

```bash
locallibrary pull                                   # the drives of the library plugged in
locallibrary pull REKORDBOX --label "before NYE"    # name a snapshot you may want back
locallibrary library log                            # every library's snapshots, labels shown
locallibrary library push BACKUP --snapshot "before NYE"
locallibrary library ls                             # libraries, their latest snapshot and drives
locallibrary library rename "My library" Home
locallibrary store label 3f9a2c "Friday warmup"
locallibrary store unlabel "Friday warmup"
```

A label works wherever a snapshot id does (`library push --snapshot`, `drive restore --snapshot`,
`diff snapshot:...`) and is unique in the store, ignoring case. `pull --library NAME` only pulls
into a library that exists: a name the store doesn't know is asked about in a terminal and refused
otherwise (also with `--yes`), so a typo doesn't start a new library; `--new-library` starts one.

## Saving drives as they're plugged in

rekordbox deletes the history lists players recorded on a drive when it imports them, so a pulled
snapshot is the only copy that lasts. `watch` pulls drives for you as they're plugged in:

```bash
locallibrary watch                    # say which drives hold changes the store lacks
locallibrary watch --pull             # and pull them
locallibrary watch --pull --sync-group  # then sync the library's other drives from it
locallibrary watch --pull --once      # look at what's plugged in now, then stop
```

It runs in the foreground (Ctrl-C stops it) and logs a line for each drive plugged in or
unplugged and for what it did (`--json`: one JSON object per line). A drive is looked at when it's
plugged in, when it's there as `watch` starts, and when its databases change (rekordbox
exporting to it), once they have gone unchanged for 5 seconds (`--settle SECS`). Drives ready
at the same time are saved as `prep` saves them: the newest export first, then drives back from a
set beside it. While another operation holds the store or a drive (the TUI, the RPC sidecar, a
pull), it says so once and waits; `--once` gives up after a minute with an error.

- Only drives of a library the store has are pulled: a drive that was never pulled, or whose
  library isn't in the store, is left alone and never starts a library (pull it once yourself).
  Archive and retired drives are left alone too.
- A drive is pulled when it holds a newer state of its library than the newest snapshot (new history
  lists, an export). One that came back from a set after the main drive's newer export was pulled,
  with only player changes since (history lists, cues), is saved beside the newest snapshot, which
  stays the newest (`library push` still writes the newer export; `plays lists --all` lists its
  history lists, and `library log` shows it marked `beside`). One with other changes since is left
  alone: merge or sync it yourself.
- With `--sync-group`, after a drive is pulled (or found saved) as its library's newest, the
  library's other drives plugged in (`drive groups`) are synced from it when it's the newest of
  them with confidence, as `sync` with no drives does. When another drive is newer, or which is
  newer isn't certain, nothing is synced and the log says why. A drive saved beside the newest
  snapshot doesn't sync its group.

To have it run whenever you log in, starting now, install it (`setup` offers to):

```bash
locallibrary watch --install                   # watch --pull --sync-group at log in
locallibrary watch --install --no-sync-group   # only pull
locallibrary watch --uninstall                 # stop it
```

On macOS that's a launchd agent, `~/Library/LaunchAgents/local.locallibrary.watch.plist`, logging
to `~/Library/Logs/locallibrary-watch.log`; on Linux a systemd user service,
`~/.config/systemd/user/locallibrary-watch.service` (`journalctl --user -u locallibrary-watch`
shows the log). It runs the `locallibrary` you installed it with, with `--store` when you gave one,
so install it again after moving the binary. Installing again replaces it, options and all.
`status` reminds you of it when a drive holds changes the store lacks.

macOS may ask to let `locallibrary` read removable volumes; if drives are never looked at, check
System Settings > Privacy & Security > Files & Folders. On Windows, a Task Scheduler task that runs
`locallibrary watch --pull --sync-group` at log on does the same.

Stopping it in the middle of a pull leaves no snapshot half made: a snapshot is recorded once all
its files are in the store.

## Pruning old snapshots

Every pull adds a snapshot, so the store only grows. `store prune` removes the old ones and then
frees the files nothing else uses (as `store gc` does; `gc` never removes a snapshot of a library,
saved beside its latest or not):

```bash
locallibrary store prune --keep-last 10 --keep-daily 7 --keep-weekly 8 --dry-run
locallibrary store prune --keep-last 20 --library Home
```

`--keep-last N` keeps each library's newest N snapshots; `--keep-daily N` and `--keep-weekly N`
keep the newest snapshot of each of the last N days or weeks that have one (in local time;
weeks start on Monday). Some
snapshots are always kept:

- each library's latest, and labelled ones (take a label off with `store unlabel` to let it go);
- the state each drive was last seen holding (retired drives aside): `library push` relies on it to
  know a drive's changes are saved, `drive ls --known` counts how far behind a drive is from it, and
  a drive back from a set is told apart from a new export by it. When such a drive was registered
  at its unsaved set state, the snapshot it last shared with its partner isn't kept for it, and
  once pruned, `sync` refuses the two as diverged (safe: pull or merge it);
- the newest snapshot holding each history list, so `plays lists --all` and the DJ profile keep
  every set the store has seen; a snapshot whose databases can't be read is kept too.

Which of two drives is newer comes from the lineage each drive keeps, so pruning doesn't change it.
The plan is shown first, and exactly that is removed (a pull or label in between makes it plan
again); history still runs through what was removed (`library log` skips it), and a removed
snapshot's id says when it was pruned.

## Off-site copies

```bash
locallibrary store copy /Volumes/NAS/locallibrary        # the first copy takes a while; later ones copy what's new
locallibrary store copy ~/Dropbox/locallibrary --verify  # also re-read what the copy has
locallibrary store copy /Volumes/NAS/locallibrary --mirror   # and remove what the store no longer has
locallibrary store copies                                # where copies were made, when, and whether they're current
locallibrary --store /Volumes/NAS/locallibrary library log   # a copy reads like the store
```

A copy has the store's layout, so `--store DIR` (or `LOCALLIBRARY_STORE`) works with it as it
does with the store: after losing this computer, `library push` a snapshot from it onto a new drive.
Stored files are named by their content, so only ones the copy lacks are copied, each checked
against its name as it's copied. Snapshots are written once all their files are there, and the
list of each library's latest snapshot last, so a copy that's stopped (or loses its network
share) still holds the store as it was at the last copy that finished; run `store copy` again to
carry on. The copy's folder is made, but not the folders above it, so an unmounted share isn't
mistaken for an empty folder.

A copy keeps snapshots the store has since pruned unless `--mirror`. It takes the store's libraries,
labels, sets, drive registry, settings profile, rule-based playlists, remembered player models and
profile preferences (and which snapshots were pruned), but not your Atmosphere sign-in. A snapshot
whose files are damaged in the store is left out and reported, and `store copy` exits with status 5
(`store verify` says more). A copy that was written to since the last copy (pulled into, labelled or
given player models with `--store`) isn't copied over unless `--force`.
