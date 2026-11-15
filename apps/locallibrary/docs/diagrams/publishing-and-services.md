# locallibrary diagrams: Publishing and services

Records, sets, bug reports, online services and other programs. The rest are listed in the [README](../../README.md#diagrams).

- [Publishing a record](#publishing-a-record)
- [What each command publishes](#what-each-command-publishes)
- [Publishing a set](#publishing-a-set)
- [Publishing at a set destination](#publishing-at-a-set-destination)
- [Reporting a bug](#reporting-a-bug)
- [Online services of the core library](#online-services-of-the-core-library)
- [Online services of plugins and the browser](#online-services-of-plugins-and-the-browser)
- [Programs on this computer](#programs-on-this-computer)

## Publishing a record

How My Settings, the profile and a set reach the DJ account. Reads go through Local Network's directory, writes to the account's own server. A record unchanged since it was published stops before asking.

```mermaid
sequenceDiagram
    participant L as locallibrary
    participant D as Directory
    participant W as Website
    participant S as Account's server
    opt First publish from this account here, if it allows it
        L->>S: A token for requestIndex
        L->>W: directory.requestIndex
        W->>D: Read this account again
    end
    L->>D: What's published here now?
    L->>L: Plan the exact record,<br/>checked against its Lexicon
    opt The account can't write it yet
        L->>S: Sign in again, allowing it
        L->>L: Plan again, for<br/>whoever approved
    end
    L->>L: Ask: publish as this account?
    L->>S: Write it, only over<br/>the version the plan read
    S-->>L: at:// URI and CID
```

Source: [`crates/locallibrary/src/publish.rs`](../../../../crates/locallibrary/src/publish.rs), [`crates/locallibrary/src/api/publish.rs`](../../../../crates/locallibrary/src/api/publish.rs), [`crates/locallibrary/src/atmosphere/directory.rs`](../../../../crates/locallibrary/src/atmosphere/directory.rs), [`crates/locallibrary/src/atmosphere/repo.rs`](../../../../crates/locallibrary/src/atmosphere/repo.rs), [`crates/locallibrary/src/atmosphere/lexicon.rs`](../../../../crates/locallibrary/src/atmosphere/lexicon.rs), [`src/publish_cmd.rs`](../../src/publish_cmd.rs)

## What each command publishes

The record types locallibrary writes to the DJ account. Per-drive overrides, notes, paths and sessions stay on this computer.

```mermaid
flowchart LR
    settings["settings publish"] --> settingsrec["computer.localnetwork.locallibrary.settings<br/>key self: the profile's base values"]
    profile["profile publish"] --> profilerec["computer.localnetwork.dj.profile<br/>key self: the profile document"]
    sets["sets publish"] --> setrec["computer.localnetwork.dj.set<br/>keyed by the set's id or an older<br/>plays publish key, hidden tracks unnamed"]
    upload["sets publish --upload"] --> recording["computer.localnetwork.dj.recording<br/>the audio as a blob"]
    upload --> setrec
    delete["sets delete"] --> gone["Deletes both from the account"]
    bug["bug"] --> discussion["app.userinput.discussion<br/>a bug report"]
    plugin["A command running a plugin<br/>with atmosphere:write:NSID"] --> pluginrec["NSID records, each one<br/>you confirm in a terminal"]
```

Source: [`crates/locallibrary/src/publish.rs`](../../../../crates/locallibrary/src/publish.rs), [`crates/locallibrary/src/atmosphere/lexicon.rs`](../../../../crates/locallibrary/src/atmosphere/lexicon.rs), [`crates/locallibrary/src/api/publish.rs`](../../../../crates/locallibrary/src/api/publish.rs), [`docs/account.md`](../account.md), [`crates/locallibrary/src/plugin/atmosphere.rs`](../../../../crates/locallibrary/src/plugin/atmosphere.rs), [`lexicons/computer/localnetwork`](../../../../lexicons/computer/localnetwork)

## Publishing a set

The ways `sets publish` can go, in the order things happen.

```mermaid
flowchart TD
    start["sets publish SET"] --> to{"--to?"}
    to -->|yes| dest["The plugin's destination"]
    dest -->|"signed in, its record offered"| lookup
    to -->|no| upload{"--upload?"}
    upload -->|yes| encode["AAC with FFmpeg, if it<br/>won't play or fit"]
    encode --> blob["Upload the blob, publish<br/>the recording's record"]
    blob --> lookup["Unless --no-lookup or --dry-run:<br/>look up MusicBrainz IDs"]
    upload -->|no| lookup
    lookup --> record["Publish the set's record"]
```

Source: [`src/sets_cmd.rs`](../../src/sets_cmd.rs), [`src/publish_cmd.rs`](../../src/publish_cmd.rs), [`src/plugins/roles.rs`](../../src/plugins/roles.rs), [`crates/locallibrary/src/publish/recording.rs`](../../../../crates/locallibrary/src/publish/recording.rs), [`crates/locallibrary/src/api/publish.rs`](../../../../crates/locallibrary/src/api/publish.rs)

## Publishing at a set destination

How a plugin publishes a set's recording. Publishing needs a yes: asked, or `--yes`.

```mermaid
sequenceDiagram
    participant L as locallibrary
    participant P as Plugin
    participant S as Service
    L->>L: Do its accounts stay signed in long enough?
    L->>P: set/publish, as a dry run
    P-->>L: What would happen
    L->>L: Show it with the tracks left unnamed, ask
    L->>L: Check its accounts again
    L->>P: set/publish, the recording lent to it
    P->>L: net/fetch, sending the recording
    L->>S: Upload with the account's token
    S-->>L: Where it is
    L-->>P: The answer
    P-->>L: Its link and name, maybe a record
    L->>L: Keep them on the set
```

Source: [`crates/locallibrary/src/plugin/destinations/publish.rs`](../../../../crates/locallibrary/src/plugin/destinations/publish.rs), [`crates/locallibrary/src/plugin/accounts/ready.rs`](../../../../crates/locallibrary/src/plugin/accounts/ready.rs), [`src/sets_cmd.rs`](../../src/sets_cmd.rs), [`plugins/youtube/src/render.rs`](../../../../plugins/youtube/src/render.rs)

## Reporting a bug

Where `bug` sends a report. Until Local Network has a space on userinput.app, every report goes to GitHub.

```mermaid
flowchart TD
    bug["locallibrary bug"] --> space{"Can it post?"}
    space -->|"no space, signed out, no title"| github["GitHub's form,<br/>filled in, in the browser"]
    space -->|yes| show["Show the report"]
    show --> scope{"Allowed to?"}
    scope -->|no| again["Sign in again, allowing it"]
    again -->|"not done"| github
    again --> confirm
    scope -->|yes| confirm["Post it? Anyone can read it"]
    confirm -->|no| stop["Not posted"]
    confirm -->|yes| post["An app.userinput.discussion<br/>record in your repository"]
    post -->|"it fails"| github
```

Source: [`src/bug_cmd.rs`](../../src/bug_cmd.rs), [`crates/locallibrary/src/atmosphere/bug_report.rs`](../../../../crates/locallibrary/src/atmosphere/bug_report.rs)

## Online services of the core library

What the core library reaches itself. Nothing here is needed to sync drives.

```mermaid
flowchart LR
    core["Core library"] --> pds["The account's server:<br/>sign-in, records, blobs"]
    core --> handle["A handle's DNS or web<br/>address: finding its DID"]
    core --> plc["plc.directory: a did:plc<br/>account's server"]
    core --> happyview["happyview.localnetwork.computer:<br/>the directory"]
    core --> website["localnetwork.computer:<br/>requestIndex"]
    core --> client["locallibrary.localnetwork.computer:<br/>client document"]
    core --> iroh["iroh and n0's relays:<br/>store serve, store fetch"]
```

Source: [`crates/locallibrary/src/atmosphere/mod.rs`](../../../../crates/locallibrary/src/atmosphere/mod.rs), [`crates/locallibrary/src/atmosphere/identity.rs`](../../../../crates/locallibrary/src/atmosphere/identity.rs), [`crates/locallibrary/src/atmosphere/directory.rs`](../../../../crates/locallibrary/src/atmosphere/directory.rs), [`crates/locallibrary/src/atmosphere/repo.rs`](../../../../crates/locallibrary/src/atmosphere/repo.rs), [`crates/locallibrary/src/peers/mod.rs`](../../../../crates/locallibrary/src/peers/mod.rs)

## Online services of plugins and the browser

What the plugin host reaches for plugins and account plugins, and what opens in your browser.

```mermaid
flowchart LR
    host["Plugin host"] --> exchange["localnetwork.computer:<br/>token exchange"]
    host --> oauth["Google, SoundCloud,<br/>Mixcloud: sign-in"]
    host --> upload["YouTube, SoundCloud, Mixcloud,<br/>plyr.fm, Internet Archive: uploads"]
    host --> mb["MusicBrainz, Cover Art<br/>Archive: lookups"]
    browser["Your browser"] --> oauth
    browser --> pds["The account's server: sign-in"]
    browser --> github["GitHub's bug form"]
```

Source: [`crates/locallibrary/src/plugin/net.rs`](../../../../crates/locallibrary/src/plugin/net.rs), [`crates/locallibrary/src/plugin/accounts.rs`](../../../../crates/locallibrary/src/plugin/accounts.rs), [`src/bug_cmd.rs`](../../src/bug_cmd.rs), [`plugins/accounts`](../../../../plugins/accounts)

## Programs on this computer

The other programs locallibrary runs, and what for.

```mermaid
flowchart LR
    identify["Which disk a drive is on: adopt,<br/>drive disks, drive speed, doctor"] --> which["diskutil, ioreg; lsblk;<br/>PowerShell's Storage cmdlets"]
    format["drive format, provision"] --> disk["diskutil, newfs_msdos, newfs_exfat, plutil,<br/>ioreg; sfdisk, mkfs, lsblk, udevadm, sync,<br/>umount; PowerShell's Storage cmdlets"]
    fsck["drive fsck"] --> check["diskutil; fsck.vfat, fsck.exfat, sudo,<br/>udisksctl; chkdsk"]
    eject["eject"] --> ej["diskutil; udisksctl, umount;<br/>PowerShell, Explorer's Eject"]
    upload["sets publish --upload"] --> ffmpeg["ffmpeg"]
    youtube["youtube plugin"] --> ffmpeg
    youtube --> ffprobe["ffprobe"]
    watch["watch --install"] --> login["launchctl, systemctl"]
    open["Opening a page or file"] --> opener["open, rundll32, xdg-open"]
    pager["Long lists"] --> less["sh running $PAGER, else less"]
```

Source: [`crates/locallibrary/src/format/mod.rs`](../../../../crates/locallibrary/src/format/mod.rs), [`crates/locallibrary/src/fleet.rs`](../../../../crates/locallibrary/src/fleet.rs), [`crates/locallibrary/src/api/drive_speed.rs`](../../../../crates/locallibrary/src/api/drive_speed.rs), [`crates/locallibrary/src/fscheck.rs`](../../../../crates/locallibrary/src/fscheck.rs), [`crates/locallibrary/src/eject.rs`](../../../../crates/locallibrary/src/eject.rs), [`crates/locallibrary/src/publish/recording.rs`](../../../../crates/locallibrary/src/publish/recording.rs), [`plugins/youtube/src/render.rs`](../../../../plugins/youtube/src/render.rs), [`src/autostart.rs`](../../src/autostart.rs), [`crates/locallibrary/src/opener.rs`](../../../../crates/locallibrary/src/opener.rs), [`src/pager.rs`](../../src/pager.rs)
