# locallibrary diagrams: Library data

Reading a library, My Settings, sets, mixes and the DJ profile. The rest are listed in the [README](../../README.md#diagrams).

- [Reading a library](#reading-a-library)
- [My Settings](#my-settings)
- [Importing published settings](#importing-published-settings)
- [rekordbox's collection](#rekordboxs-collection)
- [Exporting a collection](#exporting-a-collection)
- [Sets from histories and recordings](#sets-from-histories-and-recordings)
- [Play statistics](#play-statistics)
- [Identifying a mix](#identifying-a-mix)
- [The DJ profile](#the-dj-profile)
- [Writing a library out](#writing-a-library-out)

## Reading a library

How the bytes on a drive become the library every command works with. A `combined` read adds what only OneLibrary holds, as exports do.

```mermaid
flowchart TD
    pdb["export.pdb"] --> reader["rekordbox-pdb"]
    one["exportLibrary.db and its WAL"] --> decrypt["Decrypt, replay the WAL's<br/>committed transactions"]
    reader --> library["Library model:<br/>tracks, playlists, history lists"]
    decrypt -->|"no export.pdb, or combined"| library
    decrypt --> drift["Compared with export.pdb<br/>for drift"]
    ext["exportExt.pdb"] --> tags["My Tags"]
    decrypt --> tags
    anlz["Analysis files"] --> analysis["Beat grid, cues, waveforms, phrases"]
    fields["The store's analyzer fields"] --> filter["Filters and sorts:<br/>bpm:120-128 sound.bpm:>120"]
    library --> filter
    tags --> filter
    library --> cache["Sidecar: parsed libraries of the last 4 drives,<br/>read again when a database changes"]
```

Source: [`crates/locallibrary/src/device.rs`](../../../../crates/locallibrary/src/device.rs), [`crates/locallibrary/src/pdb.rs`](../../../../crates/locallibrary/src/pdb.rs), [`crates/locallibrary/src/onelibrary/mod.rs`](../../../../crates/locallibrary/src/onelibrary/mod.rs), [`crates/locallibrary/src/anlz.rs`](../../../../crates/locallibrary/src/anlz.rs), [`crates/locallibrary/src/query.rs`](../../../../crates/locallibrary/src/query.rs), [`crates/locallibrary/src/api/cache.rs`](../../../../crates/locallibrary/src/api/cache.rs)

## My Settings

Where the settings profile comes from and where it goes.

```mermaid
flowchart TD
    drive["A drive's *SETTING*.DAT"] -->|"settings import DRIVE"| profile[("settings.json<br/>base values, per-drive overrides")]
    rekordbox["rekordbox's own settings"] -->|"settings import"| profile
    file["A file from settings export"] -->|"settings import FILE"| profile
    published["Published My Settings"] -->|"--from-handle, --from-account"| profile
    cli["settings set, unset"] --> profile
    profile -->|"settings apply, refresh, provision"| patched["Patched into each drive's files,<br/>backed up first"]
    profile -->|"settings publish: base values"| record["The DJ account"]
    profile -->|"settings export"| out["One file, for another computer"]
```

Source: [`crates/locallibrary/src/settings.rs`](../../../../crates/locallibrary/src/settings.rs), [`crates/locallibrary/src/settings/transfer.rs`](../../../../crates/locallibrary/src/settings/transfer.rs), [`crates/locallibrary/src/api.rs`](../../../../crates/locallibrary/src/api.rs), [`crates/locallibrary/src/api/publish.rs`](../../../../crates/locallibrary/src/api/publish.rs), [`crates/loclib-mysettings/src/lib.rs`](../../../../crates/loclib-mysettings/src/lib.rs)

## Importing published settings

`settings import --from-handle` and `--from-account`: someone's published My Settings into your profile's base.

```mermaid
flowchart TD
    ask["A handle or DID, or the signed-in account"] --> own{"Your own?"}
    own -->|yes| index["Ask the directory to read you again,<br/>the first time on this computer"]
    own -->|no| read["Read the record from the directory"]
    index --> read
    read --> plan["Plan: add to the base, or replace it with --replace.<br/>Values this version doesn't know are left out, with a warning"]
    plan --> apply["Confirmed: only the base changes, overrides stay.<br/>stale_plan if the profile changed since"]
```

Source: [`crates/locallibrary/src/api/publish.rs`](../../../../crates/locallibrary/src/api/publish.rs), [`crates/locallibrary/src/publish.rs`](../../../../crates/locallibrary/src/publish.rs), [`crates/locallibrary/src/atmosphere/directory.rs`](../../../../crates/locallibrary/src/atmosphere/directory.rs), [`src/main.rs`](../../src/main.rs)

## rekordbox's collection

How `collection` reads `master.db` without touching it, and `collection export` makes a library from it.

```mermaid
flowchart TD
    master[("master.db and its WAL")] --> copy["Copied to a temporary folder,<br/>decrypted, WAL replayed"]
    copy --> read["Opened read only"]
    read --> reads["collection info, tracks, playlists"]
    read --> diff["collection diff:<br/>what a drive lacks"]
    read --> export["collection export"]
    export --> write["Copy what differs, build both databases,<br/>swap them in"]
    write --> target["A drive, or a folder laid out like one"]
    target -->|"pull"| store[("Store")]
    store -->|"library push"| drive["Any drive"]
```

Source: [`crates/locallibrary/src/masterdb.rs`](../../../../crates/locallibrary/src/masterdb.rs), [`crates/locallibrary/src/collection_export.rs`](../../../../crates/locallibrary/src/collection_export.rs), [`crates/locallibrary/src/library_write.rs`](../../../../crates/locallibrary/src/library_write.rs), [`src/collection_cmd.rs`](../../src/collection_cmd.rs)

## Exporting a collection

`collection export`, as a writer of whole libraries. `--from PLUGIN:SOURCE` fills the same writer from a plugin's library source.

```mermaid
flowchart TD
    source["master.db, or a plugin's source<br/>with the folders you --grant"] --> plan["Plan the files that differ. A library it holds<br/>from elsewhere is replaced only when asked"]
    plan --> saved["Save what only the target has in the store"]
    saved --> backup["Back up what it replaces, not audio,<br/>if it holds a library. Mark it as being written"]
    backup --> copy["Copy what differs: library_write, as pack does"]
    copy --> build["Build both databases and exportExt.pdb,<br/>checked, then swapped in"]
    build --> delete["Delete what's no longer used, record the<br/>new state, write collection.json, forget the journal"]
```

Source: [`crates/locallibrary/src/collection_export.rs`](../../../../crates/locallibrary/src/collection_export.rs), [`crates/locallibrary/src/library_write.rs`](../../../../crates/locallibrary/src/library_write.rs), [`crates/locallibrary/src/plugin/sources.rs`](../../../../crates/locallibrary/src/plugin/sources.rs), [`crates/locallibrary/src/api/collection.rs`](../../../../crates/locallibrary/src/api/collection.rs), [`src/collection_cmd.rs`](../../src/collection_cmd.rs)

## Sets from histories and recordings

How what players logged becomes a set the DJ declares, and where a set goes.

```mermaid
flowchart TD
    lists["History lists on drives"] --> sessions["Histories"]
    archive["History lists in snapshots"] --> sessions
    imported["Beat Link Trigger logs,<br/>plugins' importers"] --> sessions
    sessions -->|"--from. sets suggest groups one night's"| new["sets new"]
    recording["A recording, identified by mix"] -->|"its tracklist wins"| new
    new --> book[("sets.json")]
    edit["sets edit"] --> book
    book --> export["sets export: text, Mixcloud, CUE,<br/>YouTube, 1001Tracklists, Podcasting 2.0,<br/>CSV, M3U8, a plugin's format"]
    book --> playlist["sets playlist: a playlist on a drive"]
    book --> played["sets played: when and where<br/>you played a track"]
    book --> publish["sets publish"]
    book --> profile["DJ profile"]
```

Source: [`crates/locallibrary/src/sets/mod.rs`](../../../../crates/locallibrary/src/sets/mod.rs), [`crates/locallibrary/src/sets/build.rs`](../../../../crates/locallibrary/src/sets/build.rs), [`crates/locallibrary/src/sets/suggest.rs`](../../../../crates/locallibrary/src/sets/suggest.rs), [`crates/locallibrary/src/sets/import.rs`](../../../../crates/locallibrary/src/sets/import.rs), [`crates/locallibrary/src/sets/export.rs`](../../../../crates/locallibrary/src/sets/export.rs), [`crates/locallibrary/src/plays.rs`](../../../../crates/locallibrary/src/plays.rs), [`src/sets_cmd.rs`](../../src/sets_cmd.rs)

## Play statistics

`plays stats`: what a library's play counts and history lists add up to.

```mermaid
flowchart TD
    source["A library (its newest drive, else its latest<br/>snapshot), a drive, or snapshot:ID"] --> stats["Tracks, history lists, played,<br/>never played, most played"]
    source -->|"--since a snapshot or a date"| since["New history lists since,<br/>and the tracks played since"]
```

Source: [`crates/locallibrary/src/plays.rs`](../../../../crates/locallibrary/src/plays.rs), [`src/plays_cmd.rs`](../../src/plays_cmd.rs)

## Identifying a mix

How `mix` and the player's Mixes view find which tracks a recording plays.

```mermaid
flowchart TD
    tracks["The drive's tracks up to 20 minutes"] --> prints["Fingerprints, each track decoded once,<br/>cached in the store"]
    prints --> index["A hash index for the drive,<br/>kept while under the limit"]
    mix["A recording, or an Audacity project"] --> listen["Decoded and fingerprinted"]
    listen --> match["Matcher: where each track sits,<br/>at what speed"]
    index --> match
    match --> report["Mix report: plays, transitions,<br/>tempo, keys, a 3-band waveform"]
    report --> html["--html, --open: the interactive page"]
    report --> data["--export: JSON for Local Network,<br/>or a plugin's format"]
    report -.->|"--format, --open, sets new --recording,<br/>a save in the player or sidecar"| kept["Kept in the store's reports/"]
    report --> set["sets new, from the recording"]
```

Source: [`crates/locallibrary/src/mix/mod.rs`](../../../../crates/locallibrary/src/mix/mod.rs), [`crates/locallibrary/src/mix/index.rs`](../../../../crates/locallibrary/src/mix/index.rs), [`crates/locallibrary/src/mix/report.rs`](../../../../crates/locallibrary/src/mix/report.rs), [`crates/locallibrary/src/mix/export.rs`](../../../../crates/locallibrary/src/mix/export.rs), [`src/mix_cmd.rs`](../../src/mix_cmd.rs)

## The DJ profile

What the report is built from, and what's allowed out of it.

```mermaid
flowchart TD
    sets["Declared sets"] --> report["Profile report: stays on this computer"]
    sessions["Histories, until sets are declared"] --> report
    collection["The collection, with neither"] --> report
    analysis["Analysis and tags, from mounted drives<br/>and the store's snapshots"] --> report
    settings["The drives' My Settings"] --> report
    report --> document["Profile document: rounded unless<br/>precision is exact, no paths, list<br/>names or exact dates"]
    prefs[("profile.json<br/>include, pin, hide, alias, headline, precision")] --> document
    document -->|"profile document -o"| file["A file"]
    document -->|"profile publish"| account["computer.localnetwork.dj.profile"]
```

Source: [`crates/locallibrary/src/profile/mod.rs`](../../../../crates/locallibrary/src/profile/mod.rs), [`crates/locallibrary/src/profile/document.rs`](../../../../crates/locallibrary/src/profile/document.rs), [`src/profile_cmd.rs`](../../src/profile_cmd.rs)

## Writing a library out

The commands that write files for other software.

```mermaid
flowchart LR
    export["export"] --> own["M3U8, CSV, text"]
    export --> format["A plugin's format"]
    xml["xml"] --> rekordbox["interop plugin:<br/>rekordbox XML"]
    mix["mix --export --format"] --> format
    sets["sets export"] --> format
    format --> traktor["interop plugin: Traktor, Serato, Engine DJ,<br/>and rekordbox for a drive's library"]
    plays["plays export"] --> own
    sets --> setformats["Text, Mixcloud, CUE, YouTube, 1001Tracklists,<br/>Podcasting 2.0, CSV, M3U8"]
```

Source: [`src/export_cmd.rs`](../../src/export_cmd.rs), [`crates/locallibrary/src/export.rs`](../../../../crates/locallibrary/src/export.rs), [`crates/locallibrary/src/plugin/exports.rs`](../../../../crates/locallibrary/src/plugin/exports.rs), [`plugins/interop/locallibrary-plugin.toml`](../../../../plugins/interop/locallibrary-plugin.toml), [`crates/locallibrary/src/sets/export.rs`](../../../../crates/locallibrary/src/sets/export.rs)
