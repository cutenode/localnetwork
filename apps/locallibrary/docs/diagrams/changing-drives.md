# locallibrary diagrams: Changing drives

Plans, the write routine, syncs, undo, edits and jobs. The rest are listed in the [README](../../README.md#diagrams).

- [Plan, confirm, apply](#plan-confirm-apply)
- [A plan's life](#a-plans-life)
- [The one write routine](#the-one-write-routine)
- [Copying a whole library](#copying-a-whole-library)
- [Which way a sync goes](#which-way-a-sync-goes)
- [Back from a set](#back-from-a-set)
- [Syncing a library's drives](#syncing-a-librarys-drives)
- [A change reaches the other clones](#a-change-reaches-the-other-clones)
- [A write cut off part way](#a-write-cut-off-part-way)
- [Undo and redo](#undo-and-redo)
- [Bringing a playlist back](#bringing-a-playlist-back)
- [Editing both databases](#editing-both-databases)
- [Edits after an export](#edits-after-an-export)
- [The refresh job](#the-refresh-job)
- [The prep job](#the-prep-job)
- [How a job runs](#how-a-job-runs)
- [First run with setup](#first-run-with-setup)
- [Setting up a drive with provision](#setting-up-a-drive-with-provision)
- [Merge, pack and convert](#merge-pack-and-convert)

## Plan, confirm, apply

How a planned change is worked out, shown, and made only as it was shown.

```mermaid
sequenceDiagram
    participant F as Front end
    participant A as Core library
    participant G as Plugins' guards
    participant D as Drive
    participant C as Other clones
    F->>A: Plan this change
    A->>D: Read the library, hash<br/>what the plan depends on
    opt A plan that edits a library
        A->>G: These edits?
    end
    opt A big write to an external drive
        A->>D: Time it, if it hasn't been or that's stale,<br/>and a front end will ask: speed.json
    end
    A-->>F: Plan: id, summary with how long<br/>it'll take, warnings, clones
    F->>F: Show it and ask
    F->>A: Apply this plan
    A->>D: Measure it all again
    alt Blocked by a guard, or something changed
        A-->>F: Refused, nothing written
    else All as planned
        A->>D: The one write routine
        opt 1 GiB or more, read from this computer
            A->>D: Correct the drive's rate
        end
        A->>C: Copy onto each clone
        A-->>F: Report
    end
```

Source: [`crates/locallibrary/src/op.rs`](../../../../crates/locallibrary/src/op.rs), [`crates/locallibrary/src/commit.rs`](../../../../crates/locallibrary/src/commit.rs), [`crates/locallibrary/src/plugin/checks.rs`](../../../../crates/locallibrary/src/plugin/checks.rs), [`crates/locallibrary/src/drive_speed.rs`](../../../../crates/locallibrary/src/drive_speed.rs), [`crates/locallibrary/src/api/drive_speed.rs`](../../../../crates/locallibrary/src/api/drive_speed.rs), [`src/output.rs`](../../src/output.rs)

## A plan's life

The states a plan goes through, whichever front end holds it. Not confirmed means a dry run, a no, or nobody to ask.

```mermaid
stateDiagram-v2
    state "Refused: archive or retired drive" as Refused
    state "Stale, changed or gone" as Stale
    [*] --> Made
    [*] --> Refused: while planning
    Made --> NothingToDo: no change
    Made --> Shown: a front end shows it
    Shown --> NotApplied: not confirmed
    Shown --> Blocked: the command line won't ask
    Shown --> Checking: confirmed
    Checking --> Blocked: a blocking guard
    Checking --> Stale: measured again
    Checking --> Refused: checked again
    Checking --> Writing: all as planned
    Writing --> Applied: report, then clones copied
    Writing --> Failed: an error or a cancel
```

Source: [`crates/locallibrary/src/op.rs`](../../../../crates/locallibrary/src/op.rs), [`crates/locallibrary/src/fleet.rs`](../../../../crates/locallibrary/src/fleet.rs), [`src/output.rs`](../../src/output.rs)

## The one write routine

How a change made in place reaches a drive, and what a failure leaves.

```mermaid
flowchart LR
    subgraph before["Before the swap"]
        direction TB
        lock["Lock the drive,<br/>check its databases"] --> build["Build and read back<br/>in memory"]
        build --> backup["Back up what changes, but for repairs,<br/>firmware, recordings, demo samples' files"]
        backup --> marker["Mark it as being written"]
        marker --> stage["Stage files and databases,<br/>flush, set times"]
    end
    subgraph after["The swap and after: no cancelling"]
        direction TB
        swap["Swap in files,<br/>then databases"] --> delete["Delete what nothing<br/>references"]
        delete --> seal["Note in the backup<br/>what the write left"]
        seal --> lineage["Record the lineage,<br/>clear the marker"]
        lineage --> journal["Add to the journal,<br/>prune old backups"]
    end
    before -->|"all staged"| after
    before -->|"failed or cancelled"| rollback["Taken away:<br/>the drive is as it was"]
    after -->|"failed"| cutoff["The marker stays:<br/>a write cut off"]
```

Source: [`crates/locallibrary/src/commit.rs`](../../../../crates/locallibrary/src/commit.rs), [`crates/locallibrary/src/backup.rs`](../../../../crates/locallibrary/src/backup.rs), [`crates/locallibrary/src/device.rs`](../../../../crates/locallibrary/src/device.rs)

## Copying a whole library

How `sync` and `library push` write a drive, so it stays playable if the copy stops, unless `--delete-first` took its databases off.

```mermaid
flowchart LR
    subgraph before["Up to the swap"]
        direction TB
        stored["push: refused at the plan<br/>if the store lacks a file"] --> fits["Check it fits"]
        fits --> marker["Mark it as being written, keep its player name,<br/>remove what a cut-off write left"]
        marker --> early["--delete-first: take the databases off,<br/>delete stale files. No library until the swap"]
        early --> note["Note the new files<br/>in writing.json"]
        note --> copy["Copy files up to 1 MiB 8 at a time,<br/>then the rest one by one"]
        copy --> newfile["A new file goes under its own name. One the drive has,<br/>a clone, or any on Windows: a temp file, then a rename"]
        newfile --> flush["Flush together, then set their times,<br/>forget writing.json"]
    end
    subgraph after["The swap and after"]
        direction TB
        swap["Remove OneLibrary's old WAL,<br/>swap the databases in"] --> delete["Delete database files<br/>the source lacks, then stale files"]
        delete --> index["Save the hash index,<br/>put the player name back"]
        index --> carry["Carry the journal over,<br/>or forget it for a snapshot"]
        carry --> lineage["Take on the source's lineage,<br/>clear the marker"]
    end
    before --> after
```

Source: [`crates/locallibrary/src/sync.rs`](../../../../crates/locallibrary/src/sync.rs), [`crates/locallibrary/src/library_write.rs`](../../../../crates/locallibrary/src/library_write.rs), [`crates/locallibrary/src/fsutil.rs`](../../../../crates/locallibrary/src/fsutil.rs), [`crates/locallibrary/src/writing.rs`](../../../../crates/locallibrary/src/writing.rs), [`crates/locallibrary/src/device.rs`](../../../../crates/locallibrary/src/device.rs), [`crates/locallibrary/src/player.rs`](../../../../crates/locallibrary/src/player.rs)

## Which way a sync goes

How `sync A B`, with two drives named, decides what's copied onto what. A library or no drives goes through [Syncing a library's drives](#syncing-a-librarys-drives).

```mermaid
flowchart TD
    start["Two drives named"] --> blank{"One has no library?"}
    blank -->|yes| fill["The one with a library<br/>onto the blank one"]
    blank -->|no| given{"--from?"}
    given -->|yes| forced["That drive onto the other,<br/>whichever is newer"]
    given -->|no| relation{"Compare lineage<br/>and evidence"}
    relation -->|identical| same["Nothing to do"]
    relation -->|"one newer"| sure{"Confident?"}
    sure -->|yes| newer["The newer onto the older"]
    sure -->|"no, unless --allow-guess"| refuse["Refused: say which"]
    relation -->|diverged| set{"One only back<br/>from a set?"}
    set -->|yes| back["The exported drive onto it,<br/>its state saved first"]
    set -->|no| refuse
    relation -->|undetermined| refuse
    relation -->|unrelated| merge["Refused: merge,<br/>or --from"]
```

Source: [`crates/locallibrary/src/api.rs`](../../../../crates/locallibrary/src/api.rs), [`crates/locallibrary/src/compare.rs`](../../../../crates/locallibrary/src/compare.rs), [`crates/locallibrary/src/after_set.rs`](../../../../crates/locallibrary/src/after_set.rs)

## Back from a set

What `sync A B`, `prep` and `refresh --sync` do when one drive came back from a set and the other has a new export. `sync` with a library or no drives refuses instead, and says to sync the pair first.

```mermaid
sequenceDiagram
    participant E as Exported drive
    participant S as Drive back from the set
    participant T as Store
    S->>T: Pull it: history lists and cues are kept
    E->>S: Copy the library over it
    T->>T: Update both drives in the registry
    E->>T: Pull it: the export is the newest snapshot
```

Source: [`crates/locallibrary/src/api.rs`](../../../../crates/locallibrary/src/api.rs), [`crates/locallibrary/src/after_set.rs`](../../../../crates/locallibrary/src/after_set.rs), [`crates/locallibrary/src/sync.rs`](../../../../crates/locallibrary/src/sync.rs)

## Syncing a library's drives

What `sync` does with no drives, a library's name, or three or more drives: one source, written to several targets at once.

```mermaid
flowchart TD
    start["sync or sync LIBRARY"] --> groups["Each mounted library with two drives<br/>or more, but archive and retired ones"]
    three["3 drives or more named: only those,<br/>an archive refused as it is applied"] --> newest{"Its newest drive?"}
    groups --> newest
    newest -->|"can't tell, diverged,<br/>or back from a set"| out["Refused. With several<br/>libraries, left out, exit 5"]
    newest -->|found| plans["A plan per target. One question: Update N drives?"]
    plans --> parallel["Source scanned once, targets written<br/>at once, up to one per core"]
    parallel --> each["A target that fails doesn't stop the others: exit 5"]
```

Source: [`src/main.rs`](../../src/main.rs), [`src/drive_cmd.rs`](../../src/drive_cmd.rs), [`crates/locallibrary/src/api.rs`](../../../../crates/locallibrary/src/api.rs), [`crates/locallibrary/src/sync.rs`](../../../../crates/locallibrary/src/sync.rs)

## A change reaches the other clones

How a plan on a library's lead clone is copied onto its other drives plugged in.

```mermaid
flowchart TD
    lead["A library's name: its lead clone, the newest plugged<br/>in, the primary when identical or they can't be ordered"] --> plan["Plan the change on the lead"]
    plan --> each{"Each other mounted clone"}
    each -->|"archive or retired,<br/>or no library on it"| out["Left out"]
    each -->|"newer, diverged, from a set,<br/>or which is newer can't be told"| alone["Left alone, a warning in the plan"]
    each -->|"older or identical"| listed["Named in the plan"]
    listed --> apply["Apply on the lead"]
    apply --> copy["Copy the lead onto the named clones.<br/>One that can't take it is a warning"]
```

Source: [`crates/locallibrary/src/api/jobs.rs`](../../../../crates/locallibrary/src/api/jobs.rs), [`crates/locallibrary/src/op.rs`](../../../../crates/locallibrary/src/op.rs), [`crates/locallibrary/src/fleet.rs`](../../../../crates/locallibrary/src/fleet.rs)

## A write cut off part way

The marker in a drive's `state.json`, and what a drive holding one lets through. `tracks`, the player, `check` and `doctor` still read it, and all but `tracks` say so. A write that takes over first removes the temp files and the new files in `writing.json` that the cut-off one never finished.

```mermaid
stateDiagram-v2
    state "No marker" as Clean
    state "Being written" as Writing
    state "Cut off" as CutOff
    [*] --> Clean
    Clean --> Writing: a write begins
    Writing --> Clean: it finishes, or an in-place write fails before its swap
    Writing --> CutOff: crash, pulled plug, a copy stopped or failed, an error from the swap on
    CutOff --> Writing: sync, push, pack or export takes over
    CutOff --> Writing: undo puts its backup back
    CutOff --> Writing: any write, if the cut-off write had no backup and wasn't a copy
    CutOff --> CutOff: other writes refused, and so is using it as a source, or pull without --force
    CutOff --> CutOff: drive tidy removes the files it never finished
```

Source: [`crates/locallibrary/src/device.rs`](../../../../crates/locallibrary/src/device.rs), [`crates/locallibrary/src/commit.rs`](../../../../crates/locallibrary/src/commit.rs), [`crates/locallibrary/src/sync.rs`](../../../../crates/locallibrary/src/sync.rs), [`crates/locallibrary/src/writing.rs`](../../../../crates/locallibrary/src/writing.rs), [`crates/locallibrary/src/tidy.rs`](../../../../crates/locallibrary/src/tidy.rs)

## Undo and redo

How `undo` picks a backup and puts it back.

```mermaid
flowchart TD
    pick["undo picks the last change's<br/>backup the drive still holds"] --> changed{"Drive changed since, or audio<br/>gone that no backup left out?"}
    changed -->|"yes, no --force"| refused["Refused"]
    changed -->|"no, or --force"| plan["Plan: files, playlists, history lists<br/>dropped, journal entries taken off,<br/>tracks coming back without audio"]
    plan --> redo["Once confirmed, back up what it<br/>replaces: putting that back is the redo"]
    redo --> write["Put the backup back through<br/>the one write routine"]
    write --> journal["Put the backup's journal back"]
    journal --> clones["Copy onto the library's other clones"]
```

Source: [`crates/locallibrary/src/api/undo.rs`](../../../../crates/locallibrary/src/api/undo.rs), [`crates/locallibrary/src/backup.rs`](../../../../crates/locallibrary/src/backup.rs), [`src/restore_cmd.rs`](../../src/restore_cmd.rs)

## Bringing a playlist back

`drive restore --playlist`, and `enter` in the player's Snapshots view: one playlist or folder from an earlier state.

```mermaid
flowchart TD
    ask["A playlist or folder to bring back"] --> from{"From where?"}
    from -->|"--backup"| backup["One of the drive's backups"]
    from -->|"--snapshot"| snapshot["That snapshot"]
    from -->|"neither"| newest["The newest snapshot of<br/>the library that has it"]
    backup --> match["Entries matched to the drive's tracks by<br/>audio file. Those it lacks are left out and listed"]
    snapshot --> match
    newest --> match
    match --> edit["An ordinary playlist edit: planned,<br/>backed up, journaled, undone like any"]
```

Source: [`crates/locallibrary/src/api/restore.rs`](../../../../crates/locallibrary/src/api/restore.rs), [`src/restore_cmd.rs`](../../src/restore_cmd.rs), [`src/drive_cmd.rs`](../../src/drive_cmd.rs), [`src/tui/drives.rs`](../../src/tui/drives.rs)

## Editing both databases

How one edit lands in `export.pdb` and OneLibrary, whose row ids differ. What one database lacks is skipped there, with a warning.

```mermaid
flowchart TD
    edit["An edit, naming playlists by path<br/>and tracks by audio file"] --> plan["Plan against export.pdb,<br/>else OneLibrary"]
    plan --> pdb["export.pdb edited in memory,<br/>exportExt.pdb after it: if that<br/>fails, a warning"]
    plan --> one["OneLibrary: a decrypted working copy,<br/>WAL replayed, edited with SQLite"]
    pdb --> model["export.pdb and OneLibrary read back:<br/>each must match the model of what it should hold"]
    one --> model
    one --> sealed["integrity_check, encrypt and decrypt<br/>round trip, the drive's own salt"]
    sealed --> commit
    model --> commit["The one write routine, under one backup"]
    commit --> journal["One entry in the journal"]
```

Source: [`crates/locallibrary/src/edit.rs`](../../../../crates/locallibrary/src/edit.rs), [`crates/locallibrary/src/onelibrary/mod.rs`](../../../../crates/locallibrary/src/onelibrary/mod.rs), [`crates/locallibrary/src/pdb.rs`](../../../../crates/locallibrary/src/pdb.rs)

## Edits after an export

Why edits come back after rekordbox replaces a drive's databases.

```mermaid
flowchart TD
    edit["An edit is applied"] --> journal[(".locallibrary/edits.json")]
    export["rekordbox exports again"] --> gone["The drive's databases are replaced,<br/>the journal stays"]
    journal --> gone
    gone --> ways{"What brings them back"}
    ways --> refresh["refresh"]
    ways --> replay["edits replay"]
    ways --> offer["The player's offer, y"]
    refresh --> plan["Plan the recorded edits again.<br/>One that no longer applies is skipped"]
    replay --> plan
    offer --> plan
    plan --> write["One backup, then each step made.<br/>Not recorded again"]
```

Source: [`crates/locallibrary/src/edit.rs`](../../../../crates/locallibrary/src/edit.rs), [`crates/locallibrary/src/api/editing.rs`](../../../../crates/locallibrary/src/api/editing.rs), [`crates/locallibrary/src/api/jobs.rs`](../../../../crates/locallibrary/src/api/jobs.rs), [`src/tui/app/drives.rs`](../../src/tui/app/drives.rs)

## The refresh job

What `refresh` runs after a rekordbox export, in order. Each step that has nothing to do sits out.

```mermaid
flowchart TD
    clean["clean<br/>demo tracks"] --> replay["replay<br/>recorded edits"]
    replay --> smart["smart<br/>rule-based playlists"]
    smart --> player["player<br/>name and colour"]
    player --> settings["settings<br/>My Settings"]
    settings --> ops["operations<br/>those --then asks for"]
    ops --> pull["pull<br/>save it in the store"]
    pull --> sync["sync, with --sync<br/>the library's other drives"]
```

Source: [`crates/locallibrary/src/api/jobs.rs`](../../../../crates/locallibrary/src/api/jobs.rs), [`src/jobs_cmd.rs`](../../src/jobs_cmd.rs)

## The prep job

What `prep` runs the night before a set. One plan, one confirmation.

```mermaid
flowchart TD
    pull["pull<br/>save every drive, history lists first"] --> pick["Find the drive with the newest library"]
    pick --> sync["sync<br/>copy it onto the others"]
    pick -->|"none is newest:<br/>a warning, no copy"| check
    sync --> check["check<br/>against the players"]
    check --> ok{"Problems?"}
    ok -->|yes| stop["Stop, drives stay mounted"]
    ok -->|no| eject["eject<br/>unless --no-eject"]
```

Source: [`crates/locallibrary/src/api/jobs.rs`](../../../../crates/locallibrary/src/api/jobs.rs), [`src/jobs_cmd.rs`](../../src/jobs_cmd.rs)

## How a job runs

The rules `refresh` and `prep` share.

```mermaid
stateDiagram-v2
    state "Next step" as Next
    state "Planned again" as Again
    state "Running" as Running
    [*] --> Next
    Next --> Again: an earlier step changed its drive
    Next --> Running: its plan still holds
    Next --> Running: pull and sync, planned as they run
    Again --> Running: nothing else changed the drive
    Again --> Stopped: an outside write, stale_plan
    Running --> Next: done
    Running --> Stopped: it failed, or a check found problems
    Running --> Cancelled: Ctrl-C
    Next --> [*]: no steps left
    Stopped --> [*]: the report says what was done
    Cancelled --> [*]
```

Source: [`crates/locallibrary/src/api/jobs.rs`](../../../../crates/locallibrary/src/api/jobs.rs)

## First run with setup

The questions `setup` asks, in order.

```mermaid
flowchart TD
    drives["Which plugged-in drives are copies<br/>of your library, and its name"] --> ids["Give them identities: a known drive's back,<br/>or a new one"]
    ids --> roles["Which is the primary,<br/>and the others' roles"]
    roles --> pull["Pull the primary into the library,<br/>then the others that hold it, but archives"]
    pull --> settings["Make the primary's My Settings<br/>your profile"]
    settings --> names["The Device Name for each drive,<br/>and your contact details"]
    names --> players["The player models you play on"]
    players --> watch["Save drives as they're plugged in:<br/>watch --install"]
```

Source: [`src/jobs_cmd.rs`](../../src/jobs_cmd.rs), [`src/autostart.rs`](../../src/autostart.rs)

## Setting up a drive with provision

The steps of `provision`, one plan for a new drive. Steps not asked for sit out, and a failed step leaves the ones before it done.

```mermaid
flowchart TD
    format["format<br/>with --format: erase and repartition"] --> name["name<br/>identity, under its volume name or --name"]
    name --> library["library<br/>the latest snapshot, or another drive with --like"]
    library --> settings["settings<br/>write My Settings"]
    settings --> role["role<br/>what the drive is for"]
    role --> player["player<br/>name and colour players show"]
    player --> verify["verify<br/>read every file back"]
    verify --> eject["eject"]
```

Source: [`crates/locallibrary/src/api/drives.rs`](../../../../crates/locallibrary/src/api/drives.rs), [`crates/locallibrary/src/format/mod.rs`](../../../../crates/locallibrary/src/format/mod.rs), [`src/drive_cmd.rs`](../../src/drive_cmd.rs)

## Merge, pack and convert

The three commands that write a library from another one, and how they differ from a sync.

```mermaid
flowchart TD
    merge["merge: another library's tracks into this one"] --> present{"Target has the track?<br/>Same path and size, or artist, title, duration"}
    present -->|yes| keep["Keep the target's. The plan<br/>lists those that differ"]
    present -->|no| add["Copy audio, analysis, artwork,<br/>add a row to each database"]
    add --> playlists["Playlists: one new folder,<br/>by name, or none"]
    keep --> playlists
    playlists --> whole{"Nothing of the<br/>source left out?"}
    whole -->|"yes: the source's state goes in<br/>the target's lineage, MergedIn"| backup
    whole -->|no| backup
    pack["pack: some playlists onto another drive"] --> cut["Cut the databases down in memory"]
    cut --> saved["Save what players left on it in the store"]
    saved --> packed["If it holds a library, back up databases, analysis, artwork,<br/>not audio: undo needs --force if tracks were removed"]
    packed --> transcode["Copy what differs. With --transcode:<br/>lossless copies the players can play"]
    convert["convert: one database from the other"] --> rows["Add what it lacks,<br/>or rebuild with --replace"]
    rows --> backup["The one write routine, backed up first"]
```

Source: [`crates/locallibrary/src/merge.rs`](../../../../crates/locallibrary/src/merge.rs), [`crates/locallibrary/src/commit.rs`](../../../../crates/locallibrary/src/commit.rs), [`crates/locallibrary/src/pack.rs`](../../../../crates/locallibrary/src/pack.rs), [`crates/locallibrary/src/pack/transcode.rs`](../../../../crates/locallibrary/src/pack/transcode.rs), [`crates/locallibrary/src/convert.rs`](../../../../crates/locallibrary/src/convert.rs)
