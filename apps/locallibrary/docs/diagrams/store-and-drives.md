# locallibrary diagrams: the store and drives

What's in the store and on a drive, pull, push, copies and checks. The rest are listed in the [README](../../README.md#diagrams).

- [Store records](#store-records)
- [Inside the store folder](#inside-the-store-folder)
- [Inside a drive](#inside-a-drive)
- [The .locallibrary folder](#the-locallibrary-folder)
- [Saving a drive with pull](#saving-a-drive-with-pull)
- [Writing a snapshot with library push](#writing-a-snapshot-with-library-push)
- [Copies of the store](#copies-of-the-store)
- [Serving a store to another computer](#serving-a-store-to-another-computer)
- [What a fetch does to each library](#what-a-fetch-does-to-each-library)
- [Pruning and collecting](#pruning-and-collecting)
- [Watching drives](#watching-drives)
- [A drive's life](#a-drives-life)
- [Checking drives](#checking-drives)
- [Repairing a drive](#repairing-a-drive)
- [Clearing a drive](#clearing-a-drive)
- [What's using a drive](#whats-using-a-drive)
- [Recordings and firmware](#recordings-and-firmware)

## Store records

What the store's records say about each other.

```mermaid
erDiagram
    LIBRARY ||--o{ SNAPSHOT : "holds"
    LIBRARY |o--o| SNAPSHOT : "head"
    SNAPSHOT }o--o{ SNAPSHOT : "parents"
    SNAPSHOT }o--o{ PRUNED_SNAPSHOT : "parents gone from snapshots/"
    SNAPSHOT ||--|{ FILE_ENTRY : "lists"
    FILE_ENTRY }o--|| OBJECT : "hash names"
    LABEL }o--|| SNAPSHOT : "names"
    DRIVE_RECORD }o--o| LIBRARY : "library_id"
    SNAPSHOT {
        hash id
        hash fingerprint
        json lineage
    }
    FILE_ENTRY {
        string path
        int size
        int mtime
    }
    DRIVE_RECORD {
        uuid id
        string role
        path last_mount
    }
```

Source: [`crates/locallibrary/src/store.rs`](../../../../crates/locallibrary/src/store.rs), [`crates/locallibrary/src/tree.rs`](../../../../crates/locallibrary/src/tree.rs)

## Inside the store folder

What the store keeps, by what it's for. `~/Library/Application Support/locallibrary` on a Mac, or `--store` and `$LOCALLIBRARY_STORE`.

```mermaid
flowchart TD
    hub["Store"]
    hub --> history["Library history<br/>store.json, objects/, snapshots/, libraries.json, labels.json, pruned.json"]
    hub --> yours["What you set<br/>settings.json, players.json, smart.json, sets.json, profile.json, mix.json"]
    hub --> registry["The registry<br/>drives.json"]
    hub --> worked["Worked out, kept to save time<br/>cache/, reports/, profile-document.json"]
    hub --> player["The player<br/>tui.json"]
    hub --> art["art embed's copies<br/>embed-backups/"]
    hub --> plugins["Plugins<br/>plugins/ID/work/, scratch/, settings.json, plugins/settings.json, suggested.json, seen.json"]
    hub --> accounts["Accounts<br/>atmosphere.json, accounts.json, and without a keychain atmosphere-session.json, secrets/"]
    hub --> copies["Its copies<br/>copies.json, and copy.json in a copy"]
    hub --> locks["Locks<br/>lock, drives.lock, accounts.lock, atmosphere.lock, accounts/, plugins/suggested.lock, each scratch folder's .lock"]
    history ~~~ player
    yours ~~~ art
    worked ~~~ plugins
    player ~~~ accounts
    art ~~~ copies
    plugins ~~~ locks
```

Source: [`crates/locallibrary/src/store.rs`](../../../../crates/locallibrary/src/store.rs), [`crates/locallibrary/src/plugin/settings.rs`](../../../../crates/locallibrary/src/plugin/settings.rs), [`crates/locallibrary/src/plugin/accounts/kept.rs`](../../../../crates/locallibrary/src/plugin/accounts/kept.rs), [`crates/locallibrary/src/atmosphere/store.rs`](../../../../crates/locallibrary/src/atmosphere/store.rs), [`crates/locallibrary/src/audio_cache.rs`](../../../../crates/locallibrary/src/audio_cache.rs), [`crates/locallibrary/src/offsite.rs`](../../../../crates/locallibrary/src/offsite.rs)

## Inside a drive

Each kind of file on a drive, and what locallibrary does with it.

```mermaid
flowchart LR
    db["PIONEER/rekordbox/<br/>export.pdb, exportExt.pdb,<br/>exportLibrary.db and its WAL"] --> library["Library: synced,<br/>saved in snapshots"]
    anlz["PIONEER/USBANLZ/"] --> library
    art["PIONEER/Artwork/"] --> library
    audio["Contents/, and audio the<br/>database names elsewhere"] --> library
    other["Anything else under PIONEER/"] --> library
    settings["PIONEER/*SETTING*.DAT"] --> profile["Only the settings profile's<br/>commands write them, never a sync"]
    identity["PIONEER/DeviceLibBackup/"] --> stays["Stays with the drive: never copied,<br/>deleted or fingerprinted"]
    extracted["PIONEER/extracted/"] --> stays
    players["LOG, CDJ, MPJ, TRASHBOX, RBFLTR.DAT,<br/>playlists3*.sync"] --> stays
    shm["exportLibrary.db-shm"] --> rebuilt["Never copied, deleted with<br/>the database it belongs to"]
    outside["Anything else outside<br/>PIONEER/ and Contents/"] --> left["Left alone, but for<br/>the commands made for it"]
```

Source: [`crates/locallibrary/src/paths.rs`](../../../../crates/locallibrary/src/paths.rs), [`crates/locallibrary/src/sync.rs`](../../../../crates/locallibrary/src/sync.rs), [`crates/locallibrary/src/settings.rs`](../../../../crates/locallibrary/src/settings.rs)

## The .locallibrary folder

What locallibrary keeps on each drive for itself.

```mermaid
flowchart LR
    state[(".locallibrary/")] --> id["state.json<br/>identity, library, lineage, the write marker"]
    state --> index["index.json<br/>hash cache by size and modification time"]
    state --> lock["lock<br/>one writer per drive"]
    state --> edits["edits.json<br/>the journal of edits"]
    state --> staged["artwork/, waveforms/<br/>files the journal's edits name"]
    state --> backups["backups/ID/<br/>backup.json, carried.json, files as they were"]
    state --> pack["pack.json<br/>what it was packed from"]
    state --> collection["collection.json<br/>what it was exported from"]
    state --> transcodes["transcodes.json<br/>the copies pack made"]
```

Source: [`crates/locallibrary/src/device.rs`](../../../../crates/locallibrary/src/device.rs), [`crates/locallibrary/src/backup.rs`](../../../../crates/locallibrary/src/backup.rs), [`crates/locallibrary/src/edit.rs`](../../../../crates/locallibrary/src/edit.rs), [`crates/locallibrary/src/pack.rs`](../../../../crates/locallibrary/src/pack.rs), [`crates/locallibrary/src/pack/transcode.rs`](../../../../crates/locallibrary/src/pack/transcode.rs), [`crates/locallibrary/src/collection_export.rs`](../../../../crates/locallibrary/src/collection_export.rs)

## Saving a drive with pull

How one named drive becomes a snapshot. `pull` with a library's name or no drive decides as `watch --pull` does, and warns where this refuses.

```mermaid
flowchart TD
    locks["Lock the store and the drive.<br/>A write cut off refuses, without --force"] --> scan["Adopt it, scan it, fingerprint it"]
    scan --> library["Find its library, or start one"]
    library --> head{"Against the<br/>library's head"}
    head -->|same| linked["Up to date: link the drive"]
    head -->|"older"| older["Refused: push to update it"]
    head -->|"diverged"| diverged["Refused: force it,<br/>or pull into another library"]
    head -->|"no head yet, descends,<br/>or --force"| import["Import each library file,<br/>reading only what the store lacks"]
    import --> save["Flush, save the snapshot, move the head"]
    save --> link["Link the drive: lineage and registry"]
```

Source: [`crates/locallibrary/src/sync.rs`](../../../../crates/locallibrary/src/sync.rs), [`crates/locallibrary/src/store.rs`](../../../../crates/locallibrary/src/store.rs), [`src/store_cmd.rs`](../../src/store_cmd.rs)

## Writing a snapshot with library push

How any snapshot gets back onto a drive.

```mermaid
flowchart TD
    which["The snapshot named,<br/>else the library's head"] --> scan["Scan the drive"]
    scan --> unsaved{"Its state in the store,<br/>or behind the snapshot?"}
    unsaved -->|"no, no --force"| refused["Refused: pull it first"]
    unsaved -->|"yes, or --force"| plan["Plan the files to copy and delete"]
    plan --> confirm["Confirm"]
    confirm --> copy["Copy from the store's objects,<br/>as a sync copies"]
    copy --> lineage["The drive takes on the snapshot's<br/>lineage and library"]
    lineage --> register["Update the registry"]
```

Source: [`crates/locallibrary/src/sync.rs`](../../../../crates/locallibrary/src/sync.rs), [`src/library_cmd.rs`](../../src/library_cmd.rs)

## Copies of the store

Where a store can be copied, and how a copy comes back.

```mermaid
flowchart TD
    store[("Store")] -->|"store copy DIR"| folder["A folder: NAS, disk, cloud folder"]
    store -->|"store copy --to PLUGIN:DESTINATION"| dest["A plugin's store destination"]
    store -->|"store serve, code printed"| peer["Another computer's store fetch"]
    folder -->|"--store DIR reads it"| reader["locallibrary, reading the copy"]
    dest -->|"store restore --from"| store
    dest -->|"drive verify --from --repair"| drive["A damaged drive"]
    order["Order of a copy: files, then snapshots<br/>with all their files, then libraries.json"] -.-> folder
    order -.-> dest
    leftout["Never copied: accounts, plugins' folders,<br/>mix.json, store-prefs.json, tui.json,<br/>embed-backups/, locks, caches, reports"] -.-> folder
    leftout -.-> dest
```

Source: [`crates/locallibrary/src/offsite.rs`](../../../../crates/locallibrary/src/offsite.rs), [`crates/locallibrary/src/plugin/destinations.rs`](../../../../crates/locallibrary/src/plugin/destinations.rs), [`crates/locallibrary/src/scrub.rs`](../../../../crates/locallibrary/src/scrub.rs), [`src/store_cmd.rs`](../../src/store_cmd.rs)

## Serving a store to another computer

How `store serve` and `store fetch` copy snapshots over iroh. With `--local` nothing goes through a relay.

```mermaid
sequenceDiagram
    participant S as store serve
    participant R as n0 relay, when needed
    participant F as store fetch
    S->>S: Print a code: iroh ticket and a token for this run
    F->>S: Connect, directly or through the relay
    F->>S: locallibrary/store/1, with the token
    S-->>F: Catalog: libraries, snapshots, labels
    loop Each snapshot and file this store lacks
        F->>S: Ask by hash, one stream each
        S-->>F: Its bytes, hashed on arrival
    end
    F->>F: Save manifests, libraries, labels
```

Source: [`crates/locallibrary/src/peers/mod.rs`](../../../../crates/locallibrary/src/peers/mod.rs), [`crates/locallibrary/src/peers/serve.rs`](../../../../crates/locallibrary/src/peers/serve.rs), [`crates/locallibrary/src/peers/fetch.rs`](../../../../crates/locallibrary/src/peers/fetch.rs), [`crates/locallibrary/src/catalog.rs`](../../../../crates/locallibrary/src/catalog.rs)

## What a fetch does to each library

`store fetch` and `store restore --from`: what each library comes to. Nothing this store has is replaced.

```mermaid
flowchart LR
    fetch["Each library the<br/>other store has"] -->|"not here"| added["added"]
    fetch -->|"theirs is newer"| updated["updated: the latest moves on"]
    fetch -->|"same, or this one newer"| same["up_to_date"]
    fetch -->|"both changed"| diverged["diverged: keeps its latest,<br/>theirs beside it"]
    fetch -->|"their latest can't be had"| unavailable["unavailable: left as it was"]
```

Source: [`crates/locallibrary/src/catalog.rs`](../../../../crates/locallibrary/src/catalog.rs), [`crates/locallibrary/src/offsite.rs`](../../../../crates/locallibrary/src/offsite.rs), [`crates/locallibrary/src/peers/fetch.rs`](../../../../crates/locallibrary/src/peers/fetch.rs)

## Pruning and collecting

What `store prune` keeps, and what `store gc` frees.

```mermaid
flowchart TD
    prune["store prune"] --> keep["Keeps each library's latest, labelled ones,<br/>what each drive in use last held, the newest with<br/>each history list, those it can't read, and what<br/>--keep-last, --keep-daily, --keep-weekly ask for"]
    keep --> gone["The rest go, their parents kept in pruned.json"]
    gone -->|"then, in the same run"| gc["store gc"]
    gc --> freed["Deletes snapshots of no library (not labelled,<br/>a head, or a kept one's parent), objects no snapshot<br/>uses, what cut-off writes left, unread caches,<br/>plugins' abandoned scratch folders"]
```

Source: [`crates/locallibrary/src/retention.rs`](../../../../crates/locallibrary/src/retention.rs), [`crates/locallibrary/src/store.rs`](../../../../crates/locallibrary/src/store.rs), [`crates/locallibrary/src/api/snapshots.rs`](../../../../crates/locallibrary/src/api/snapshots.rs)

## Watching drives

What `watch` does with each drive, and the same from the sidecar's `watch.start`.

```mermaid
flowchart TD
    poll["Poll volumes each second"] --> settle["Plugged in or changed:<br/>wait until it's still for 5 s"]
    settle --> ours{"In a library the store has,<br/>not archive or retired?"}
    settle -->|"in use: says so, looks again.<br/>--once gives up after 60 s"| settle
    ours -->|no| skip["Skipped, and why"]
    ours -->|yes| next{"Against the<br/>newest snapshot"}
    next -->|"older, another library,<br/>diverged otherwise"| skip
    next -->|"saved beside it already"| done["Up to date, no group sync"]
    next -->|identical| saved["Up to date"]
    next -->|newer| pull["--pull: pull it.<br/>Without: say so"]
    next -->|"only players changed it"| beside["--pull: pull beside the newest.<br/>Without: say so"]
    saved --> group["--sync-group: sync the library's other<br/>drives, if newest with confidence"]
    pull -->|pulled| group
```

Source: [`crates/locallibrary/src/api/autopull.rs`](../../../../crates/locallibrary/src/api/autopull.rs), [`crates/locallibrary/src/watch/mod.rs`](../../../../crates/locallibrary/src/watch/mod.rs), [`src/watch_cmd.rs`](../../src/watch_cmd.rs), [`src/autostart.rs`](../../src/autostart.rs)

## A drive's life

The states a drive goes through in the store's registry and on the drive. `drive format` keeps its identity and empties its lineage. `drive forget` takes it out of the registry, and the next write puts it back.

```mermaid
stateDiagram-v2
    state "No identity" as Unknown
    state "Has an identity" as Adopted
    state "In a library" as Member
    state "Lost its identity" as Lost
    state "Identity shared with a copy" as Twin
    [*] --> Unknown
    Unknown --> Adopted: drive adopt
    Unknown --> Member: pull, sync, push, setup, provision
    Adopted --> Member: pull, sync, push
    Adopted --> Retired: drive retire
    Member --> Retired: drive retire
    Retired --> Member: --undo
    Member --> Lost: erased elsewhere
    Lost --> Member: drive adopt --as, setup
    Lost --> Adopted: drive adopt
    Member --> Twin: copied by another tool
    Twin --> Member: drive reidentify
```

Source: [`crates/locallibrary/src/fleet.rs`](../../../../crates/locallibrary/src/fleet.rs), [`crates/locallibrary/src/device.rs`](../../../../crates/locallibrary/src/device.rs), [`crates/locallibrary/src/api/drives.rs`](../../../../crates/locallibrary/src/api/drives.rs)

## Checking drives

What each health command reads, and what it adds up to.

```mermaid
flowchart LR
    status["status"] --> lineage["Lineage, the registry,<br/>settings drift"]
    check["check"] --> db["Databases, files, history saved,<br/>what each player reads"]
    doctor["doctor"] --> files["Duplicates, missing and orphaned<br/>files, drift, large files, --audio"]
    doctor -->|"--analyze"| opinions["Opinions on tempo, key<br/>and level, never a problem"]
    verify["drive verify"] --> bytes["Every byte against a snapshot<br/>or the hash cache"]
    fsck["drive fsck"] --> os["The OS's file system check"]
    storeverify["store verify"] --> objects["Every stored file<br/>against its hash"]
    check --> plugins["Plugins' checks"]
    doctor --> plugins
    db --> four["Exit 4 on problems"]
    files --> four
    bytes --> four
    os --> four
    objects --> four
    plugins --> four
```

Source: [`crates/locallibrary/src/check.rs`](../../../../crates/locallibrary/src/check.rs), [`crates/locallibrary/src/doctor.rs`](../../../../crates/locallibrary/src/doctor.rs), [`crates/locallibrary/src/scrub.rs`](../../../../crates/locallibrary/src/scrub.rs), [`crates/locallibrary/src/fscheck.rs`](../../../../crates/locallibrary/src/fscheck.rs), [`crates/locallibrary/src/fleet.rs`](../../../../crates/locallibrary/src/fleet.rs), [`crates/locallibrary/src/store.rs`](../../../../crates/locallibrary/src/store.rs)

## Repairing a drive

What `drive verify` compares with, and what `--repair` writes back.

```mermaid
flowchart TD
    verify["drive verify reads every byte"] --> refs["Against a snapshot of this state, else<br/>the hash cache, else an older snapshot"]
    refs --> found["Damaged, unreadable, missing"]
    found -->|"--repair"| again["Each checked again: intact, or<br/>no good copy, is skipped"]
    again --> source["A copy from the store, --against a drive,<br/>or --from a destination, fetched first"]
    source --> write["The one write routine, no backup,<br/>each copy checked as it's staged"]
    write --> back["Read back against its hash"]
```

Source: [`crates/locallibrary/src/scrub.rs`](../../../../crates/locallibrary/src/scrub.rs), [`crates/locallibrary/src/api/drives.rs`](../../../../crates/locallibrary/src/api/drives.rs), [`src/drive_cmd.rs`](../../src/drive_cmd.rs)

## Clearing a drive

The two commands that remove what nothing needs.

```mermaid
flowchart TD
    fix["doctor --fix-orphans"] --> orphans["Audio, analysis and artwork<br/>no track in either database uses"]
    orphans --> commit["Confirmed, then the one write routine: backed up,<br/>undo takes it back, copied onto clones"]
    tidy["drive tidy"] --> clutter["._*, .DS_Store, Thumbs.db, .TemporaryItems,<br/>Spotlight, .fseventsd, cut-off temp files,<br/>the trash with --empty-trash"]
    clutter --> gone["Confirmed, then deleted: no backup.<br/>It reaches outside PIONEER/ and Contents/"]
```

Source: [`crates/locallibrary/src/doctor.rs`](../../../../crates/locallibrary/src/doctor.rs), [`crates/locallibrary/src/api/doctor.rs`](../../../../crates/locallibrary/src/api/doctor.rs), [`crates/locallibrary/src/tidy.rs`](../../../../crates/locallibrary/src/tidy.rs), [`crates/locallibrary/src/paths.rs`](../../../../crates/locallibrary/src/paths.rs)

## What's using a drive

What `drive space` reports.

```mermaid
flowchart TD
    space["drive space"] --> walk["Walks the whole volume, by kind: audio, analysis, artwork,<br/>databases, My Settings, backups, recordings, clutter, trash,<br/>audio copied by hand"]
    space --> forecast["With the store: the library's growth<br/>across its snapshots, and when it fills"]
    space --> fits["Whether it fits on the library's other drives,<br/>with 200 MB left for players"]
```

Source: [`crates/locallibrary/src/space.rs`](../../../../crates/locallibrary/src/space.rs), [`src/drive_cmd.rs`](../../src/drive_cmd.rs)

## Recordings and firmware

Files that live outside the library, in a drive's root.

```mermaid
flowchart TD
    recordings["drive recordings: XDJ-AZ and OPUS-QUAD<br/>MASTER REC files, listed"] -->|"--copy-to"| copied["Copied here, every byte checked,<br/>a tracklist beside it if asked"]
    copied -->|"--delete, confirmed"| deleted["Deleted from the drive once verified: no backup"]
    firmware["drive firmware FILE.UPD"] --> rules["Checks its name, the drive's partitions<br/>and file system, other update files"]
    rules -->|"a problem, no --force"| refused["Refused"]
    rules -->|"confirmed"| staged["Copied to the root: no backup"]
    staged -->|"--remove, after updating"| removed["Update files removed"]
```

Source: [`crates/locallibrary/src/recordings.rs`](../../../../crates/locallibrary/src/recordings.rs), [`crates/locallibrary/src/firmware.rs`](../../../../crates/locallibrary/src/firmware.rs), [`src/drive_cmd.rs`](../../src/drive_cmd.rs)
