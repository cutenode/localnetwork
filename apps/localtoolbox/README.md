# localtoolbox

A multi-purpose Chrome extension for DJs, part of [Local Network](../../README.md). Clicking the toolbar icon opens a small
menu of tools; picking one opens that tool's own panel, which the arrow in
its header leads back out of. It looks like the [website](../web): the same
dark ink surfaces, white type, Instrument Sans and JetBrains Mono, from the
shared theme in `packages/tokens/theme.css`. Light and dark follow the
browser unless the button in the menu's header says otherwise.

The header of every view — the menu and each tool — also carries the
**session queue**: the tracks picked out while working through a page, held
together until something is done with the whole batch. The button says how
many are in it and opens them as a sheet over whatever is on screen, so a
batch can be looked at, marked or handed on without leaving the tool that
filled it.

Beside it is **Settings**, a sheet of the same kind, for the choices that
belong to the extension rather than to any one tool: the theme, whether
SoundCloud's own download buttons are replaced with this extension's (see
[Replacing SoundCloud's own download buttons](#replacing-soundclouds-own-download-buttons)),
whether your own Likes page is opened as a list rather than a grid of badges
(see [Opening your likes as a list](#opening-your-likes-as-a-list)),
what the third-party downloader hands a gate that asks for it — the email
address, the name beside it, and whether saving a file marks its track —
which shops the Manager's search badges point at (see
[Artist searches](#artist-searches); the [artists](#artists) tool's opened
rows carry the same badges), and whether an opened row's words are
put into Title Case (see [Title Case](#title-case)). Those are typed or ticked once and never
thought about again, which is what makes them settings rather than one panel's
options; a failed row that wants one opens this sheet. One section is not a setting at all: everything the
extension has saved — the marks, the looked-up details, the Manager's list,
the session queue and every panel option — goes out to one JSON file there and
reads back in, adding to what is here unless a switch is turned on to replace
it (see [Saving and restoring it all](#saving-and-restoring-it-all)). The last,
**about**, says which version this is (and the commit it was built from), and
its **report a bug** link opens GitHub's bug form in a tab with the version,
the browser and the tool you were in already filled in. Only one sheet is up
at a time.

Current tools:

- **SoundCloud playlist to text** — extracts all song titles and artist names
  from a SoundCloud playlist page, Likes page or track page as plain text, CSV,
  TSV, or JSON.
- **Manager** — keeps a **list of tracks**, and shows where each of them can be
  downloaded from. It opens on any page and nothing goes onto the list by
  itself: the bar at the top says what the tab in front is offering — a
  playlist, Likes page or single track page, by name and by count — and **add**
  is what brings it in, along with the session queue as a second source. What is added stays,
  tab to tab and browser restart to browser restart, until it is taken off.
  The list is grouped by the site its links point to (SoundCloud's own
  "Download file", Hypeddit, Droploud, Bandcamp, …) or shown in the order it
  was built; the tracks with no link carry shortcuts to search for the artist
  elsewhere, and the ones nobody has asked SoundCloud about say so. Opening a
  link puts that track in the session queue, a batch of rows can be ticked to
  queue, mark or remove in one go, and the marks are painted on the SoundCloud
  page itself: a "Downloaded" pill on the cover art of tracks you have
  downloaded, a "nodl" pill on tracks with no link, and a "+ queue" button that
  comes up when you hover cover art, to fill the queue without leaving the page
  — which stays up reading "queued" on the tracks already in it. SoundCloud's
  own download buttons are replaced with this extension's, which does on the
  page what the panel's button does for the same track; the switch for it is in
  Settings. Clicking a row's cover art copies that track's artwork at full
  size; ⌘-clicking opens it in a tab. The list can be searched, and the
  downloaded tracks hidden, to see what is still to get.
- **third-party downloader** — walks the download links behind a page and saves
  what comes back: Hypeddit, ToneDen, Droploud, Pump Your Sound, Laylo,
  Feature.fm and FanLink. Takes its links from the SoundCloud page in the
  active tab (the same ones the Manager tool groups by site), from the session
  queue, from a tab on one of those sites, or from pasted text, and works
  through them one after another. Not every site ends in a file, and it says
  which: a link that resolves to somewhere to go rather than something to save
  says so and offers to open it.
- **artists** — keeps a **list of artists**: the uploaders behind the tracks
  you add, from the page in front, from the Manager's list or from the session
  queue, and asks SoundCloud about each one for their public profile and their
  **socials** — the links they put on it (Instagram, Bandcamp, a website,
  whatever is there) and the addresses they wrote into their bio. A row is the
  artist's picture, name and a line about them, then their links as lettered
  badges, each opening that page; it opens out into every address to copy, the
  bio, the place, the follower count, the tracks that brought the artist in,
  and the same search badges the Manager offers. The list is searchable,
  sortable, kept for good, and copied out as text in one press.

## Install

```
pnpm install         # once, at the repo root (Node 24, pinned by .node-version there)
pnpm build           # in this folder; writes dist/
```

1. Open `chrome://extensions`.
2. Enable **Developer mode** (top right).
3. Click **Load unpacked** and select the **`dist/`** folder — not this
   folder, which has no manifest precisely so that it cannot be loaded by
   mistake.

`pnpm dev` does the same build and rebuilds on every change; Chrome picks
new files up when you hit reload on the extension's card.

## Structure

All of it is TypeScript. The popup is a [Svelte](https://svelte.dev) app,
styled with Tailwind CSS on the Local Network theme (`packages/tokens/theme.css`,
which the website uses too) and built with Vite into ES modules (`assets/*.mjs` in
`dist/`, which Chrome serves as JavaScript); its components come from
[shadcn-svelte](https://shadcn-svelte.com), which copies them into the project
rather than pulling them from a package, so they can be read and changed like
any other file here. Everything that runs **inside the SoundCloud page** is
still a plain classic script — a content script has no imports, and the marks
have to be written by the same code on both sides — so it is not bundled at
all. The build only blanks its types out (ts-blank-space puts spaces where the
annotations were) and ships each file under a `.js` name, `content.ts` as
`content.js` and the two shared ones likewise, with every line and column
where it was: an error on soundcloud.com points at the same place in the
source, and there is no sourcemap to carry. The manifest names the `.js`
files, which exist only in `dist/`.

```
content.ts             SoundCloud content script (see below); ships as content.js
content.css            styles for what the content script adds to the page:
                       the markers, the queue button, the download button
downloaded-store.ts    the set of downloaded tracks (extension storage), shared by
                       content.ts and the popup as the TbxDownloaded global
state-transfer.ts      import/export of the whole saved state as JSON — the
                       marks, the lookups, the list, the queue and the panel
                       options (the TbxState global). Adds by default;
                       replacing is an opt-in the settings sheet asks for
src/
  manifest.json        extension manifest (under src/, so this folder is not
                       loadable as an extension — it names files the build makes)
  popup.html           the page the toolbar icon opens; main.ts mounts App
  background.ts        the background worker: walks a download gate and saves
                       its file, for a caller outside the popup
  offscreen.html, offscreen.ts  the page that worker walks the gate in
  app.css              the shared Local Network theme and fonts, plus what only
                       the popup needs (its width, `field`, `icon-btn`)
  App.svelte           the shell: the tool menu, and whichever tool is open
  lib/                 what the panels share
    tools.ts           the registry: one entry per tool (name, icon, tone, and
                       how to load its panel and start what that panel waits on)
    types.d.ts         the shapes that cross a module boundary — a track at each
                       hop, the page's messages, what is kept — for both sides
    queue.svelte.ts    the session queue, shared by the tools that fill and read it
    queue-sheet.ts     fetches the queue sheet's chunk, the way tools.ts does a panel's
    settings.svelte.ts the extension's own settings, in chrome.storage.local so
                       the content script can read them too
    downloader.svelte.ts  the Downloader's settings — the address a gate is
                       given, the name beside it, whether saving marks the
                       track — set in the settings sheet and read by the panel
    search-sites.json  the sites a link-less track offers a search on: a list of
                       plain objects, which is all adding one takes
    search-sites.ts    reads that file and fills a search URL's {variables} in
    search-sites.svelte.ts  which of those badges are switched on, set in the
                       settings sheet and read by the Manager panel
    settings-sheet.ts  fetches the settings sheet's chunk
    bug-report.ts      the settings sheet's report a bug link: GitHub's bug
                       form, with the version, browser and open tool filled in
    source.ts          which of the two a panel is reading: the page, or the queue
    components/        PanelShell, PanelGhost, TrackRow, FilterBar, SourcePicker,
                       QueueButton and SettingsButton (in every header),
                       QueueSheet and SettingsSheet (over every view), …
    components/StateTransfer.svelte  the settings sheet's backup section: the
                       whole saved state out to a JSON file and back
    details.svelte.ts  how many tracks have been looked up, for the count that
                       section shows beside the marks and the list
    components/ui/     shadcn-svelte's components, copied in
    selection.svelte.ts  the tracks ticked in a list, and the mode that shows
                       the tick boxes — the batch a marker is then put on
    chrome.ts soundcloud.ts downloaded.svelte.ts prefs.svelte.ts filter.ts …
  tools/<id>/          one folder per tool: Panel.svelte and its own logic
    manager/listed.svelte.ts   the Manager's own list of tracks, kept for good
                       in chrome.storage.local: what is on it, where each
                       track came in from, and when
    manager/AddBar.svelte      what the Manager can add right now — the tab in
                       front and the session queue — and the buttons that do it
    third-party-downloads/sites/   one module per site the Downloader walks
    third-party-downloads/walked.svelte.ts   the Downloader's list, kept for
                       the session: what became of every link it has walked
    artists/artists.svelte.ts  the artists tool's own list of uploaders, kept
                       for good in chrome.storage.local, each with what
                       SoundCloud said about the account
    artists/profiles.ts        who is behind a track, which network an address
                       is on, and the links written into a bio
    artists/lookup.ts          asking a SoundCloud tab for the profiles
    artists/AddBar.svelte      the three sources — the tab, the Manager's list,
                       the session queue — and the buttons that add from them
docs/                  the JSON Schema for the exported state file
scripts/               validate-state.mts, behind `pnpm validate:state` —
                       run by Node as it is, which strips the types itself
test/                  the test suite (see "Tests" below)
package.json           the scripts below; the workspace and its lockfile are
                       at the repo root
.oxlintrc.json         the lint config: the repo's shared rules, extended
tsconfig.json          what the type check holds the browser code to: src/ and
                       the three page scripts, strict, against Chrome's types
tsconfig.node.json     the same for what runs in Node: the configs, scripts/
                       and the tests
dist/                  the built extension — what Chrome loads
```

The tools used to be a page each, which the popup window navigated between —
a reload, and a fresh round of lookups, every time one went back. They are one
app now: picking a tool swaps the view, going back is instant, and the marks
and the queue, read from storage once, are shared by all of them. The queue
sheet uses that to hand a batch to another tool: it sets the shared source
option and opens it.

The queue is the one part of the popup that is not a tool and has no route of
its own. It was the fourth entry in the menu until it moved into the header,
which put the one thing that is _about_ the other tools on the same footing as
them — a batch picked out in Manager could only be looked at by leaving
Manager, and coming back meant reading the page again. Now the button is in
every header (`QueueButton`), the sheet comes up over whichever tool is open
(`QueueSheet`, drawn by `App.svelte`), and closing it leaves that tool exactly
as it was. The sheet is its own chunk for the reason the panels are: it brings
the component library with it, and the menu the popup boots into is
deliberately free of that — which is also why the button beside it is written
in plain HTML, as the theme toggle is.

### How fast it opens

A browser-action popup is built from nothing every time it is opened, and in
these tools it is opened constantly — every click that goes to a tab closes
it. So what it does before it can draw is the whole of how it feels, and it
is kept to as little as possible.

**Each tool's panel is its own chunk.** The bundle the popup boots from is
the shell: the menu, the registry's names and tones, the stores, the ghost.
The four panels and the component library under them — every one of which is
bigger than the shell — are fetched when a tool is opened, and at most once
each however often it is opened and closed. The tool menu has no tooltip on
it for exactly this reason: the provider bits-ui wants above one sits in
`PanelShell` rather than at the root of the app, so the library it comes
with is not in the chunk that has to compile before anything is on screen.

Vite's own preloading is turned off for the same page (`modulePreload` in
`vite.config.ts`): it would put a `<link rel="modulepreload" crossorigin>` in
the head for each of a chunk's dependencies, and Chrome counts a crossorigin
preload on a `chrome-extension://` page as a different request from the
module fetch that follows it, so every one is fetched, discarded and warned
about. There is nothing to preload for either — these are files inside the
extension, read from disk.

**The route is known synchronously.** Which tool was open lives in
`chrome.storage.session`, which is a round trip away, and the popup used to
draw nothing at all until it came back. It is mirrored into `localStorage`
too, which is not: the header is drawn, the panel's chunk
fetched and the page read started on the first turn, before anything has been
awaited. The mirror cannot know when a browsing session ended, so the session
is still what settles it — that is what corrects the first opening of a new
session to the menu.

**A ghost stands in for the panel** until its chunk has arrived and the
session has been read (panels restore their own place as they mount, so
mounting one before the session is back would lose it). The ghost is the
tool's real header (its name, its icon, its tone, a back arrow that works)
over a shape where the body will be.

**The page read starts before the panel exists.** Reading a SoundCloud page
is a round trip to the browser process for the tab and another to the content
script, which for download details is a good many requests to SoundCloud on
top; a chunk compiling is no reason for them to wait. Each registry entry
says what its tool will ask the page for (`warm`), the shell starts it as
soon as it knows which tool is opening, and the panel is handed that read
rather than starting a second one.

### What the popup remembers

Three kinds of thing, kept in three places, because they are wanted for three
different lengths of time.

**Options** are settings, and are kept for good in the popup page's own
`localStorage` (`$lib/prefs.svelte.ts`): the output format, the sort order,
the two Hide toggles, the email and name some gates and drops ask for,
which search badges are switched on, and whether an opened row's words are
title-cased.
No storage permission is asked for on the user's behalf to hold them, and the
keys are the ones the exported state file carries, so they travel to another
install with the marks (see "Saving and restoring it all") — bar the address,
the name, the Downloader's mark, the search-badge switches and the title-case
switch, which are deliberately not in the file. An
import sets one only where nothing is set already, so a file carries an option
to an install that has never been asked rather than overruling a choice made
here; asking the import to replace is what overrules them, and then it puts
the ones the file is silent about back to their defaults too.

Where an option is _set_ is a separate question from where it is kept. Most
sit on the panel they are about, a foot from the list they change. The three
the Downloader hands to a gate do not: they are typed on the day one gate asks
and never again, so they are in the settings sheet, made once in
`$lib/downloader.svelte.ts` and imported by both the sheet that writes them and
the panel that reads them — one object, so what is typed in the sheet is the
answer the panel gives a gate a moment later, with no reopening in between.
The search-badge switches are the same arrangement, in
`$lib/search-sites.svelte.ts`: thrown in the sheet, read by the Manager panel
underneath it, which redraws its rows as they are. So is the title-case
switch (`$lib/title-case.svelte.ts`, over the transform in
`$lib/title-case.ts`), which is about how a row's words are drawn rather than
which rows there are.

**What the tools know** outlives the browser, and lives in the extension's own
storage (`chrome.storage.local`): the tracks marked as downloaded
(`downloaded-store.ts`), the cached download details (`content.ts`), the
Manager's own list of tracks (`src/tools/manager/listed.svelte.ts`) and the
artists tool's list of uploaders (`src/tools/artists/artists.svelte.ts`), which
is the same arrangement again. The Manager's list is the reason it can open anywhere: it
used to _be_ whichever page was in front, so the panel had nothing to show on
the gate tab a row had just sent you to, and every opening cost SoundCloud a
few hundred requests about tracks nobody had asked about. Now a page is a
source you add from, and what you added is there tomorrow.

**Places** are where someone is in a piece of work, and are kept in
`chrome.storage.session` (`$lib/view.svelte.ts`), beside the session queue and
for the same reason: which tool is open, whether the queue sheet is up over it,
the search typed into a filter bar,
the batch ticked in the Manager list (`$lib/selection.svelte.ts`), the rows
opened in it, the two
drawers — the Downloader's, with whatever has been pasted into it, and the
settings sheet's import box — and how far down each list the reader had got. A browser-action popup is destroyed on every click that takes the focus
off it, which working through a page is mostly made of — opening a download
link, going to a gate tab — and it used to come back at the tool menu with
every panel blank. Now it comes back where it was. Closing the browser ends
the session, and the next one opens on the menu.

Which tool is open is the one of these also mirrored into `localStorage`, so
the popup has a route to draw before the session has answered; see "How fast
it opens". The mirror is a guess about the session and never the other way
round — the session read corrects it, and wipes it when a new session starts.

The Downloader's whole list is kept the same way
(`src/tools/third-party-downloads/walked.svelte.ts`), because that panel's job
is handing work back to a person and the way it does that is a tab: which
links were saved, which resolved to a page to open, which failed and why, and
the links pasted in by hand, which no source would ever offer again. It used
to be rebuilt from whatever tab was in front — and the tab in front was
usually the gate a row had just opened, which is a link this tool claims, so
twenty rows came back as one. Now the list that was there is the list that
comes back, and the source is read again only for what is new to it.
Switching the source is still a choice, and a choice empties the list.

What a page owns is read again, because by then it may be another page
altogether: what the tab holds is read afresh every time, to say what there is
to add. (The Manager's _list_ is not a page's, and is not read again — nothing
enters it that somebody did not press a button for.) What a run owns is kept
as far as it got, but never resumed — a walk that was still going when the
popup closed comes back as the row it was before **Get** was pressed, since
nothing on this side can know how far it went. A Clear button that was armed
comes back disarmed. The settings sheet's import box is the one text box not
remembered, though the drawer around it is — what goes into it is a whole
exported state file, which can be megabytes of marks and looked-up details, and
it is emptied the moment it is read. Nor is the **replace** switch beside it:
that is the one control in the extension that can lose something, and a popup
reopened an hour later must not still be armed.

The scrolled position belongs to the shell rather than to any panel
(`PanelShell`, which owns the one thing that scrolls). It is put back as the
list arrives rather than at mount, since at mount there is nothing to scroll
yet, and given up on if the list turns out never to get that long — a shorter
page than the one it was left on.

### Adding a tool

1. Create `src/tools/<id>/Panel.svelte`. It takes two props, `tool` and
   `onback`, and wraps its content in `PanelShell`, which draws the header,
   the way back, and the scrolling body.
2. Register it in `TOOLS` in `src/lib/tools.ts`: id, name, description, a
   [Lucide](https://lucide.dev) icon, a `tone`, and a `load` that
   imports the panel — `() => import('../tools/<id>/Panel.svelte')`, which is
   what makes it a chunk of its own. If the panel opens by waiting on
   something slow, give the entry a `warm` that starts it (`warmPage` for a
   read of the page, `warmActiveTab` for the tab alone); the shell calls it
   before the panel is fetched.
3. If the tool needs a content script or new permissions, add them to
   `src/manifest.json`, which names files as they will be in `dist/`. A file
   that must not be compiled goes in `unbundled` in `vite.config.ts`; a classic
   script goes in `pageScripts` there instead, under the `.js` name the
   manifest gives it, and ships with its types blanked out and nothing else.

To add a **site** to the third-party downloader: write
`src/tools/third-party-downloads/sites/<id>.ts` exporting the shape the other
six do (see `sites/index.ts` for the contract, including what a thrown error
offers the person next), add it to `SITES` there, and add its hostnames
to `host_permissions` in `src/manifest.json` — without which every fetch it
makes is blocked. A test in `test/consistency.test.ts` checks those three
agree.

The tone is the only thing a panel says about its own look: a `bg-*` class
for one of the DJ-data colours in `packages/tokens/colors.json`, which is
the background of the tool's icon tile (`ToolTile`), the way the website puts
a DJ's monogram on a track colour. The Local Network theme has no brand
accent, so the buttons, the focus ring and the headers are the same in every
tool. The sheets have no tone at all: their tiles are plain, because they are
not tools but something you open where you stand.

Panels reach the shared state through `$lib`: `downloaded` (the marks, a
reactive mirror of `downloaded-store.ts`), `queue` (the session queue),
`createFilter()` (the search and "Hide downloaded" toggle behind `FilterBar`),
`createSelection()` (the ticked tracks behind **Select**, and the Shift-click
that takes in a run of rows),
`readPage()` (asks the content script for the active tab's tracks, or hands
back the sentence explaining why it cannot), `flagPref`/`choicePref`/`textPref`
(options remembered in localStorage), and `toasts` (the messages that slide in
at the bottom).

`downloaded-store.ts` and `state-transfer.ts` stay classic scripts: they define
page globals, which only a classic script can do, and the content script loads
the first of them too. The build ships them as `downloaded-store.js` and
`state-transfer.js` and injects both into `popup.html` ahead of the bundle,
and `src/lib/globals.ts` is the one file that reaches them by name.

## Tests

The extension is two kinds of code, and the tests treat it as such (Vitest +
jsdom). What runs in the page is loaded the way Chrome loads it — a plain
script against a prepared `document`, `chrome` API mock and `TbxDownloaded`
global, with its types blanked out by the same call the build makes, so what
is tested is the file that ships — and driven through its messages, clicks and
storage changes. The popup is compiled by the same plugin that builds it, so a
panel is tested as the component it is: mounted into jsdom with the chrome
APIs mocked, and asserted on through what a user would see.

```
pnpm test            # run the suite
pnpm test:coverage   # run it with coverage; the report is in coverage/
pnpm lint            # oxlint, type-aware, with the repo's shared rules
pnpm format          # oxfmt, with the repo's shared settings
pnpm check           # strict TypeScript, by svelte-check over both tsconfigs
pnpm knip            # unused files, exports and dependencies
pnpm test:shuffle    # the same suite in a random order, to catch tests that
                     # only pass because of the ones before them
pnpm test:repeat     # every test twice, to catch state kept between runs
pnpm verify          # format, lint, types, knip, coverage, shuffled, repeated
                     # and the build: the whole gate
pnpm test:mutation   # Stryker; slow, so CI runs it weekly. The report is in
                     # reports/mutation/
pnpm zip             # the release build, zipped as localtoolbox.zip
pnpm validate:state <file.json>   # check an exported state file
```

Everything that is not a component — the content script, the shared classic
scripts, and every `.ts` file under `src/` — is held to 100% of statements,
branches, functions and lines, file by file. Components clear a floor instead
(see `vitest.config.ts`): Svelte compiles every interpolated attribute into a
"has this changed?" branch whose unchanged half a freshly mounted component
never takes, so a component's branch percentage says more about the compiler
than about the test. The few branches in the logic that no caller can reach —
defensive guards — carry a `v8 ignore` hint next to a comment saying why.

Coverage says every line ran. It does not say the line was right, that it is
still right at five hundred rows, that the page it read was the page
SoundCloud served, or that what shipped is what was tested. So the suite asks
in several other ways as well:

- **Examples** — `content/`, `panels/`, `lib/`, `tools/`. One layout, one
  option, one click at a time: what the code does, said in cases.
- **Properties** (`property/`, fast-check) — what must hold for _every_ input.
  Some are round-trips (a CSV, a JSON state file, a base64 payload, a Next.js
  flight payload all come back as what went in); some are robustness (fed
  arbitrary bytes, lone surrogates and truncated markup, nothing throws). A
  failure prints the smallest input that causes it.
- **Integration** (`integration/`) — the content script and a panel in one
  document, talking over a real `chrome.tabs.sendMessage`, sharing one storage.
  Everywhere else one of the two is a fixture, and two fixtures can agree with
  each other while both being wrong.
- **The artifact** (`build.test.ts`) — runs the real build and inspects `dist/`:
  the manifest against a Manifest V3 schema, every file it names, the page
  scripts (their sources with the types blanked out, and nothing else
  changed), the popup's asset paths (relative, or the extension origin breaks
  them), no `eval` the V3 policy would refuse, no sourcemap, no absolute path
  from the machine that built it, icons that really are PNGs of the size they
  claim.
- **Accessibility** (`a11y.test.ts`) — every panel, in every state a test can
  put it in, through an audit: a name for every control, a state attribute for
  every toggle, no duplicate ids, nothing focusable hidden from a screen
  reader. Rows here are icons and colour; without this none of that is checked.
- **Failure** (`resilience.test.ts`) — storage that will not read or write,
  a tab that has gone, site data blocked, a clipboard that refuses, a content
  script left over from before an update. Each panel has to draw something a
  person can read and leave no promise rejected.
- **Size** (`scale.test.ts`) — five hundred rows. The assertions are on work
  done (requests made in batches of fifty, one marker per marked row, writes
  that do not grow with tracks), because that is the part that goes wrong.
- **The exact bytes** (`golden.test.ts`) — snapshots of every output format,
  every saved filename and the whole exported state file, so a file exported
  last year still reads.
- **Agreement** (`consistency.test.ts`, `docs.test.ts`) — the manifest, the
  build config, the tool registry, the stylesheets and the README, checked
  against each other and against the code.
- **Mutation** (`stryker.config.mjs`, weekly): the logic changed one small
  edit at a time (a `<` made `<=`, a branch emptied) and the tests rerun
  against each change. A change no test notices is behaviour nothing pins down.

Every test is also watched for a promise nobody handled (`setup.ts`): a panel
whose load rejected draws nothing and says nothing, and an assertion on what it
_did_ draw would pass.

`pnpm lint` is one of two checks that read the code rather than run it. It runs
oxlint with the configuration the whole repo shares (`.oxlintrc.json` at its
root, which this folder's extends): type-aware rules and the vendored anti-slop
plugin, the same set the website is held to.

The other is `pnpm check`: svelte-check over the TypeScript — the `.ts`
files and the components' `<script lang="ts">` — with `strict` on, so a
`| null` in a type is enforced rather than taken on trust. It runs twice,
because the code runs in two places: `tsconfig.json` holds what runs in the
browser (`src/` and the three page scripts, against Chrome's types), and
`tsconfig.node.json` what runs in Node (the configs, `scripts/` and the tests,
which bring `src/` in through their imports and are checked against it).
Nothing about the types reaches the build — Vite compiles the popup's away,
and the page scripts' are blanked out. The shapes that cross a module boundary
are in `src/lib/types.d.ts`; everything else is typed where it lives.

```
test/
  setup.ts               fresh chrome mock, clean document, and an unhandled
                         rejection fails the test it happened in
  helpers/chrome.ts      in-memory chrome.storage/runtime/tabs
  helpers/dom.ts         SoundCloud page builders (playlist, Likes, track pages, hydration)
  helpers/api.ts         fetch stub for the SoundCloud API
  helpers/load.ts        loads the classic scripts fresh, with cleanup of
                         listeners/observers
  helpers/panel.ts       mounts a panel (or the whole popup) against a mocked
                         chrome, and reads it back
  helpers/a11y.ts        the accessibility audit, and what a keyboard reaches
  content/               page detection, extraction, download details, artist
                         profiles, markers, the queue button, the replaced
                         download buttons, and the Likes page's view
  panels/                one file per tool panel, and one for what they all
                         remember between openings of the popup
  lib/, tools/           the logic each panel is built on, on its own
                         (tools/listed.test.ts is the Manager's list: what an
                         entry keeps, and which of two answers about a track
                         wins when a page is added twice; tools/artists.test.ts,
                         tools/profiles.test.ts and tools/lookup.test.ts are the
                         artists tool's list, its reading of a profile, and its
                         asking for one)
  property/pure-logic.test.ts   every parser and formatter, over generated input
  property/state.test.ts        the state file and the marks, over generated documents
  property/dom-fuzz.test.ts     the content script over generated and truncated pages
  integration/page-to-panel.test.ts  page → content script → panel, and back
  a11y.test.ts           every panel through the accessibility audit
  resilience.test.ts     every panel against a browser that says no
  scale.test.ts          five hundred rows: requests, writes, markers, rows
  golden.test.ts         snapshots of every output the extension hands over
  build.test.ts          the real build, inspected as Chrome would load it
  docs.test.ts           this README against the code it describes
  app.test.ts            the tool menu, the routing between tools, where the
                         popup was left, the theme
  queue-sheet.test.ts    the session queue's sheet and the button that opens it:
                         marking, taking out, clearing, handing a batch on
  settings-sheet.test.ts the settings sheet: what the page obeys, what the
                         Downloader asks for, the search badges, title case,
                         the report a bug link
  background.test.ts     the worker behind the page's Download button: the file
                         saved, or the gate opened, one walk at a time
  offscreen.test.ts      the page it walks a gate in: which links, which settings
  saved-state.test.ts    the settings sheet's backup section: what it exports,
                         what it says about a file before importing it, and the
                         difference adding and replacing make
  state-transfer.test.ts the JSON state file: what it exports, what it refuses
  state-schema.test.ts   the same documents through Ajv and through the importer,
                         which must agree (see "The file format" below)
  downloaded-store.test.ts, consistency.test.ts
```

---

## SoundCloud playlist to text

Lives in `src/tools/soundcloud-playlist-to-text/`. Works on playlist pages, on
Likes pages (your own or any user's) and on a single track's own page. It is a
**pure DOM observer**: the content script (`content.ts`, shipped as
`content.js`) runs in Chrome's isolated world, reads the already-rendered
elements, and never executes anything in the page's own JavaScript context,
never mutates the page, and never makes network requests.
All post-processing happens inside the extension popup.

### Use

1. Open a SoundCloud playlist page (`soundcloud.com/<user>/sets/<playlist>`,
   including auto-generated ones under `soundcloud.com/discover/sets/…`), a
   Likes page — your own at `soundcloud.com/you/likes` (either the **Badges**
   grid or the **List** view) or any user's public likes at
   `soundcloud.com/<user>/likes` — or a track's own page
   (`soundcloud.com/<user>/<track>`), which offers the one track it is about.
2. **Scroll to the bottom of the list first if it is long** — the extension
   assumes the page is fully loaded and does not scroll or trigger
   lazy-loading itself. SoundCloud only renders more rows (or tiles) as you
   scroll; this applies to Likes pages just as much as to playlists.
3. Click the extension icon and pick **SoundCloud playlist to text**. The
   panel shows the extracted track list.

### Options

- **This page / Session queue** — which tracks to turn into text: the ones on
  the page in the active tab, or the ones in the session queue (see that tool
  below). The choice is remembered, and shared with the Downloader tool.
- **Format** — `artist - title` lines, `title - artist`, `title only`, a
  `numbered list`, CSV, TSV, or JSON.
- **Use artist from "Artist - Title" titles** — many SoundCloud uploads are
  posted by channels/promoters, with the real artist embedded in the track
  title (e.g. uploader `Twisted`, title
  `Justin Timberlake ft. Timbaland - Sexyback (EDIT)`). When enabled, titles
  containing a dash separator (`-`, `–`, `—`) are split, and the embedded
  artist replaces the uploader name.
- **Remove duplicate tracks** — drops repeated artist+title pairs
  (case-insensitive).

**Copy** puts the current output on the clipboard; **Download** saves it as a
file named after the playlist (or `Your likes` / `Likes by <user>` on Likes
pages, or `session-queue` when the queue is the source) with the extension
matching the format.

### How it works

`content.ts` first works out what kind of page is on screen. SoundCloud is a
single-page app, and its address bar and its content do not always agree:
the URL can already (or still) name another page while something else is
rendered — `/you/insights` has been seen showing the Likes page. So the page
type is read from the rendered layout, and the URL is only consulted when
nothing recognisable is rendered (yet):

| Page                                | Recognised by                                                                                                                                                                                                                    | Identified by                                                                                                                                                                                                                                       |
| ----------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Playlist (regular, album or system) | the hero's tracks summary (`.fullHero .fullHero__tracksSummary`) or the rendered track list (`.listenDetails .trackList`, `.systemPlaylistDetails`)                                                                              | the `?in=<user>/sets/<name>` query on every row's title link (`?in_system_playlist=<id>` on system playlists, i.e. `/discover/sets/<id>`)                                                                                                           |
| Likes, `/<user>/likes`              | the active **Likes** tab in the page's tab bar: `a.g-tabs-link.active` whose `href` is `/<user>/likes` (`li.networkTabs__likes` in `ul.userNetworkTabs`, next to `h1.userNetworkTop__title`)                                     | the tab's `href`                                                                                                                                                                                                                                    |
| Likes, `/you/likes`                 | the active tab, when the Library's tab bar renders it as `a.g-tabs-link.active` with `href="/you/likes"`; otherwise the Library layout itself — `.collectionSection` with the Badges/List **View** toggle (`.listDisplayToggle`) | the signed-in user                                                                                                                                                                                                                                  |
| Track                               | the hero without a tracks summary (`.l-listen-hero .fullHero`) or the engagement bar (`.listenEngagement`)                                                                                                                       | the URL, accepted only when it is a `/<user>/<track>` path under the uploader the hero names (`.fullHero .soundTitle__username a`); it is the one track the page offers, and it attributes a download made from the page and marks the hero artwork |
| Anything else                       | —                                                                                                                                                                                                                                | the URL as a last resort: `/sets/` in the path means a playlist, `/<anything>/likes` a Likes page                                                                                                                                                   |

Row selectors alone never decide the page type: profile pages embed the same
`.sound` rows and `.trackItem` previews as Likes and playlist pages. The
response carries `pageType` (`playlist`, `likes`, `track` or `other`),
`pageKey` (the playlist, Likes page or track as a path) and `pageDetectedBy`
(`dom` or `url`).

It then reads these elements (selectors verified against soundcloud.com in
September 2026):

**Playlist pages**

| Data              | Selector                                                                                                                                                    |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Track row         | `li.trackList__item .trackItem` (regular playlists) or `li.systemPlaylistTrackList__item .trackItem` (system playlists)                                     |
| Artist            | `.trackItem__username` within the row                                                                                                                       |
| Title             | `.trackItem__trackTitle` within the row                                                                                                                     |
| Artwork thumbnail | inline `background-image` of `.trackItem__image span.image__full` within the row (used to copy the cover art)                                               |
| Playlist title    | `.fullHero .soundTitle__title` (regular) or `.fullHero .fullHero__titleTextTitle` (system)                                                                  |
| Curator           | `.fullHero .soundTitle__username a` (regular; direct text nodes only, to skip the "Verified" badge) or `.fullHero .fullHero__titleTextLineSmall a` (system) |

If no rows match on a `/sets/` page (e.g. SoundCloud renames the list
wrappers), it falls back to any `.trackItem` on the page; the fallback is
skipped on non-playlist pages, whose feeds embed playlist previews with their
own `.trackItem` rows.

**Likes pages** come in two layouts. Public `/<user>/likes` and the **List**
view of `/you/likes` render a "sound list" of rows (layout A); the default
**Badges** view of `/you/likes` renders a grid of tiles (layout B). Only one
is on the page at a time, but both are scanned.

| Data                  | Selector                                                                                                                                                         |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Liked item (A)        | `li.soundList__item > div > .sound`                                                                                                                              |
| Artist (A)            | `.soundTitle__usernameText` within the row (the display artist, which can differ from the uploader account in the link), falling back to `.soundTitle__username` |
| Title (A)             | `.soundTitle__title` within the row                                                                                                                              |
| Artwork thumbnail (A) | inline `background-image` of `.sound__coverArt .image__full` within the row                                                                                      |
| Genre (A)             | `.soundTitle__tagContainer .sc-tagContent` within the row — the tag SoundCloud renders beside the title, which is the track's genre                              |
| Liked item (B)        | `li.badgeList__item > .audibleTile` (empty grid fillers are `.audibleTilePlaceholder`, so they never match)                                                      |
| Artist (B)            | `.playableTile__usernameHeading` within the tile                                                                                                                 |
| Title (B)             | `.playableTile__mainHeading` within the tile (direct text nodes only, to skip the nested icon span)                                                              |
| Artwork thumbnail (B) | inline `background-image` of `.playableTile__image .image__full` within the tile                                                                                 |
| Genre (B)             | none: a tile renders no tag, so the genre is unknown from the page there                                                                                         |
| Page title            | `h1.userNetworkTop__title` ("Likes by <user>") on `/<user>/likes`; `/you/likes` shows no username, so the fixed title `Your likes` is used                       |

Likes pages have no `.fullHero`, so the curator is always empty there.

A Likes page can mix liked tracks with liked **playlists**. Liked playlists
are skipped, and the panel reports how many were skipped. In layout A a
playlist row carries the `playlist` class (`.sound.playlist`); the title
link's `href` is checked for a `/sets/` segment as well, in case that class
goes away. In layout B track and playlist tiles share exactly the same
classes, so the only tell is a `/sets/` segment in the heading or artwork
link `href`. A liked playlist row also embeds a preview of its tracks as
`ul.trackList__item > li.compactTrackList__item > .trackItem` — the same
`.trackItem` structure as a playlist page, but under a `ul`, not an `li`, so
the playlist selectors above cannot match it, and the Likes selectors only
read the row's own `.soundTitle` block, which sits above the preview. Those
nested rows are therefore never picked up.

**Track pages** (`/<user>/<track>`) list nothing: the page is about one song,
and the hero is the only thing on it that names that song. It is read as a
list of the one, so a page with a single track answers in the shape a page
with fifty does and everything downstream — the count, the lookup, the
Manager's list — works on it without knowing which kind of page it came off.

| Data                 | Selector                                                                                                                                                                   |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Track                | `.l-listen-hero .fullHero` — the hero itself, one per page                                                                                                                 |
| Artist               | `.soundTitle__username` within the hero (the uploader)                                                                                                                     |
| Title                | `.soundTitle__title` within the hero                                                                                                                                       |
| Artwork              | inline `background-image` of `.listenArtworkWrapper__artwork .image__full` within the hero (320 px, not a thumbnail)                                                       |
| Genre                | `.sc-tagContent` within the hero — the same tag a Likes row carries, without a tag container of its own                                                                    |
| Liked / reposted     | `.sc-button-like` / `.sc-button-repost` in `.listenEngagement`, which is a sibling _below_ the hero rather than inside it                                                  |
| Purchase link        | `a.soundActions__purchaseLink` (legacy `.sc-buylink`) in `.listenEngagement`, unwrapped from `gate.sc`; its label is the `aria-label`, falling back to the link's own text |
| Page title / curator | the hero's `.soundTitle__title` and `.soundTitle__username a`, read as every hero is — so the page's title is the track's, and its curator the uploader                    |

The track's permalink is the page's own key: the URL, accepted only when the
uploader in it is the one the hero names (see the table above). Where the two
disagree the track is still reported, with an empty permalink — exactly as a
Likes row whose title is not a link is. A track with no permalink has no
identity, so the Manager's list will not hold it and does not count it as
something to add. A hero that has not rendered yet, or one that names neither
a title nor an artist, offers no track at all rather than an empty one.

The panel asks the content script for the data via
`chrome.tabs.sendMessage`, so extraction happens on demand against the
current DOM — client-side (SPA) navigation between playlists and Likes pages
is handled naturally, and so is a URL that lags behind or runs ahead of the
content, since the page type comes from the content.

Permissions: `activeTab` (to read the active tab's URL when you click the
icon), `clipboardWrite` (for the Copy button), `storage` (the Manager tool's own
list of tracks and its set of downloaded ones; see that tool), `downloads` (the third-party
downloader saves the files it unlocks), and `offscreen` (the page the background
worker walks a download gate in, since a service worker cannot parse HTML; see
`src/background.ts`). The host patterns are the content
script's match on `soundcloud.com`, plus one `host_permissions` entry per site
the Downloader reads gate pages from — `hypeddit.com`, `toneden.io`,
`fanlink.tv`, `droploud.com` and its `api.` host, `pumpyoursound.com`,
`laylo.com` with the two CDNs its drop pages read from
(`d21i0hc4hl3bvt.cloudfront.net` for the drop, `d3oyaxbt9vo0fg.cloudfront.net`
for the artist) and `events.laylo.com`, where a signup is posted, and `ffm.to`.
Chrome reports those as site access on exactly those sites. No other host
access, no remote code.

### Limitations

- Only rows present in the DOM are extracted; long playlists must be scrolled
  to the end manually first (SoundCloud renders roughly 15 rows up front,
  and roughly 10 liked items on Likes pages).
- On Likes pages, liked playlists are skipped (and counted), not expanded —
  their embedded track previews are partial, and the page's subject is the
  liked items, not their contents.
- The **Filter** box on `/you/likes` narrows what SoundCloud renders, so
  only the filtered items are extracted while a filter is active.
- `/you/likes` shows no username on the page, so its title (and the download
  file name) is the fixed `Your likes`.
- SoundCloud class names are not a public API and may change; the selectors
  above are documented so they are easy to fix.
- Page detection reads SoundCloud's layout. Should another Library page
  render the same `.collectionSection` + **View** toggle as `/you/likes`
  without a tab bar to tell them apart, it would be read as a Likes page
  (verified on `/you/likes` itself; the other Library pages were not checked).
- On playlist pages the "artist" is the uploader's SoundCloud account name
  (Likes pages show SoundCloud's display artist, which can differ from the
  uploader); use the split option when lists are full of channel uploads.

---

## Manager

Lives in `src/tools/manager/`. It keeps a **list of tracks** — its own, held
in the extension's storage and kept for good, built from playlist pages, Likes
pages, single track pages and the session queue — and for each of them finds out
two things: whether SoundCloud itself offers a **Download file** for it, and
where its **purchase link** (the "Buy" / "Free Download" button SoundCloud
shows for tracks whose uploader set one) points. What it lists is every link
that adds up to, grouped by site.

**It opens on any page, and nothing goes onto the list by itself.** Those two
are one decision. The panel used to _be_ whichever tab was in front: opening it
read that page, asked SoundCloud about every track on it, drew the answer, and
threw the lot away when the popup closed — which meant it had nothing at all to
say on the gate tab a row had just sent you to (the tab you are on for most of
the work), and that opening the tool was itself a few hundred requests to
SoundCloud about tracks nobody had asked about, paid again on every opening.
Now the page is a source you add from. The bar at the top of the panel says
what the tab in front is offering and how much of it the list already holds;
**add** brings it in, and asking SoundCloud anything is something somebody
pressed a button for.

This is the one tool that is not a pure observer: when you add a page it asks
SoundCloud's public API about that page's tracks, and it paints a marker on the
SoundCloud page. Both are spelled out below.

### Use

1. Click the extension icon and pick **Manager**. It opens wherever you are,
   with whatever is on the list — on a first run, with an explanation of what
   the list is and nothing on it.
2. The bar at the top is where tracks come in, and it says what there is:
   - **this tab**, named — the playlist's own title, the Likes page's heading,
     or a track page's track — with the curator (the uploader, on a track
     page), the number of tracks on it, how many of those the list already
     holds, and the number of liked playlists skipped where there are any. The
     button says **add 12** when twelve of them are new, and **refresh 42**
     when the page is wholly on the list already, which is what pressing it
     then really does: it asks SoundCloud about all of them again, so their
     links are up to date. A track page offers exactly one track, so the
     button there reads **add 1**.
   - **queue (7)** beside it when the session queue has anything in it. Queued
     tracks come in with whatever was known when they were queued; nothing is
     looked up, because a lookup runs on the page a track is on and the point
     of this source is that that page need not be open at all.
   - When the tab has nothing to offer, the bar says why — not a playlist,
     Likes or track page, a page that has not finished loading, a tab that
     needs a reload — and the button is disabled. The list below it is untouched: a tab
     with nothing on it is not a reason to lose what you have.

   Open a playlist, Likes or track page; scroll a long list to the end before
   adding it, since the extension reads the rows the page has rendered and
   SoundCloud renders more only as you scroll. A track page has nothing to
   scroll: its one track is in the hero.

3. Adding puts the page's tracks on the list **straight away**, from what the
   page itself renders, and then asks SoundCloud where each one can be had — a
   progress counter runs on long lists. So the rows are up before the first
   answer and settle where they already are as the answers land, rather than
   appearing all at once at the end. A track already on the list is not added
   twice: it keeps its place and takes the better of the two answers (a full
   answer beats a Likes row's half one; between two of the same kind, the
   newer). When the lookup cannot run at all the rows still go on, and the
   note says what is missing from them rather than that nothing happened.
4. **Sort** chooses the layout, and is remembered:
   - **By site** (the default): one section per site, **soundcloud.com**
     first, then the others (hypeddit.com, droploud.com, bandcamp.com, …)
     by size, then the tracks without links, then the ones nobody has asked
     SoundCloud about. A track can appear in two sections — once for its
     SoundCloud download and once for its purchase link.
   - **List order**: every track once, in the order it was added, with all of
     its links on the one row; the row's detail line says where each link goes.
     Tracks with no link sit in their place in the list, with their search
     badges. Handy for working down a page from top to bottom.

     A track can offer two links, and two buttons of the same weight leave a
     row no room for the title beside them. So one link leads and the other
     trails: the leading one is the download button, the trailing one a
     small arrow button next to it, carrying the same link and the same
     tooltip (the detail line has already named its site). The **SoundCloud
     download** leads, being the file itself with no
     gate to walk — unless its download limit is used up, when a button that
     gives nothing is not the one to put forward and the purchase link leads
     instead. `splitEntries()` in `src/tools/manager/links.ts` decides. In the
     **By site** layout every row has one link by construction, so nothing
     trails there.
5. In an external site's section, the download button opens the purchase link
   and puts the track in
   the **session queue** — not in the downloaded ones: at that moment nothing
   has been saved, and the gate behind the link has not even been walked. The
   queue is where it waits until a whole batch of them is marked at once (see
   the session queue, in the header). Links open in background tabs by default so the
   panel stays open and several can be opened in a row; turn **Background
   tabs** off to open them in front instead (which closes the panel, as any
   popup).

   The button is **the same download icon on every row**, whatever the uploader
   called their link. SoundCloud renders that label on its own page ("Free
   Download", "Buy", "🎧 Stream / Download", and nothing at all when the
   uploader gave none), and a row that wrote it across a button was a row whose
   width changed with somebody else's wording — the room it took was the room
   the title wanted, and "Buy" and "🎧 Stream / Download" mean the same thing
   here anyway: this opens, and the track is queued. It is not thrown away. The
   button's tooltip quotes it, the row's detail line names the site the link
   points at, and `downloadLabel()` in `src/tools/manager/links.ts` is what the
   button says instead — "download from hypeddit.com", which is what it does.

6. In the **soundcloud.com** section, the download button opens the track's
   page. SoundCloud's own downloads have no link: "Download file" is a button
   in each track's **More** menu (on the track page, and on each row of a
   playlist or Likes page). Use it wherever you like — the track is marked as
   downloaded when that button is clicked, which is one of the few moments the
   extension can be sure a file really was saved. Tracks whose download limit
   is used up are listed with a "no downloads left" note.
7. **No download links** lists the tracks that offer neither a SoundCloud
   download nor a purchase link. Each row carries a badge per search site that
   is switched on, opening a search for the track's artist name: **BP**
   Beatport, **bc** Bandcamp, **Z** ZIPDJ, **M** MyMP3Pool, all four on to
   begin with (URLs below, and the switches in Settings). Opening a search
   neither queues nor marks anything.
8. The last section, **Not checked**, is the one a kept list needs and a page
   never did: tracks nobody has an answer about. A track that came in from the
   session queue on a tab that was not SoundCloud, one whose page was added
   before it finished loading, one SoundCloud would not describe — the row says
   _not checked_ rather than pretending to be either of the two above, because
   "no download links" is a claim and this is the absence of one. Add the page
   such a track is on to find out; while an add is running the rows there say
   they are being checked, and leave a few at a time as the answers arrive.
9. Each row carries two toggles, and each works on the one row it is on. The
   **list** button puts the track in the session queue, or takes it out again —
   the way to collect a batch without opening anything. The **check** button
   marks it as downloaded by hand (for a misclick, or a download made some
   other way).
10. For several tracks at once, **Select several tracks** — the tick-box button
    in the header, which carries no words because the tool's own name wants the
    room. A tick box appears on every row, and a bar above the list says how
    many are ticked and what can be done to them:

- **Queue** puts the lot in the session queue (tracks it already holds are
  not queued twice, and the toast says how many were really added);
  **Unqueue** takes them out again, marks untouched.
- **Mark** marks them all as downloaded — here and on the SoundCloud page —
  and **Unmark** takes that mark off again.
- **Remove** takes them off the list. Their marks and their places in the
  queue stay: this is the list and only the list, and adding the page again
  brings them back. It is also the way to empty the list — tick the box at
  the left of the bar, then Remove.

Each of the five is **one write**, however many tracks are ticked. The
tracks are dealt with in list order rather than the order they were ticked
in, so a batch handed to the queue reads down the list.

Ticking is nothing but ticking: the batch can be looked over before anything
happens to it, and the ticks _stay_ after a marker is put on, so the same
batch can be queued and then marked. (Remove is the exception, and unticks
its own — a selection of rows that are gone is not a selection.)
**Shift-click** a tick box to take in
every track between it and the last one ticked. The box at the left of the
bar ticks every track the list is showing — what the filter hides is not
ticked — and unticks them again, as does **Untick all**. **Tick this page**
beneath it takes in only the rows the tab in front is showing: the list
spans every page it was built from, and the one you are looking at is rarely
all of it. It says how many it would take, leaves ticks on tracks from other
pages alone, and turns into **Untick this page** once they are all ticked.
It is off when the tab has no SoundCloud page on it, when nothing that page
shows is on the list yet, or when the filter is hiding all of it — the
button says which on hover. A track the list
draws twice (once per section in the **By site** layout, when it offers both
kinds of link) is one tick and one track. **Done** — in the bar, or the
header button again — closes the mode and unticks everything.

A half-made selection survives the popup closing, as the search and the
scroll position do (see
[What the popup remembers](#what-the-popup-remembers)): opening a download
link is a click that closes the popup, and a selection of twenty tracks is
not something to ask for twice. It goes when the browser does. 11. The **chevron** at the end of every row opens it downwards into the rest
of what is known about the track, one field a line:

| field                 | where it comes from                                                                                                             |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| **title**, **artist** | the page's own row                                                                                                              |
| **genre**             | the tag the page renders beside the row, where it renders one; else the lookup — SoundCloud's API (`genre` on the track object) |
| **link**              | the track's address, `https://soundcloud.com` + its permalink                                                                   |
| **from**              | the list itself: the page the track was added from, or "the session queue"                                                      |

**Clicking any of them copies that one field** to the clipboard, which is
the point of the thing: these are the words about to be typed into a
spreadsheet, a message or a search box, and they are already on screen. The
**link** is the exception that also opens: a small arrow beside it opens
the track in a tab (following **Background tabs**, and queueing and marking
nothing — looking at a track is not getting it).

The three that are words — title, artist and genre — can be shown in Title
Case instead of as the uploader typed them, which is a switch in Settings
and off until it is asked for (see [Title Case](#title-case)).

**Liked and reposted** are not fields down there. They are a yes or a no
rather than words anybody pastes anywhere, and two lines reading "not
liked" and "not reposted" spent a third of the opened row saying nothing
had happened — so they are a pair of small icons on the last line instead,
a heart and the repost arrows, beside the way off the list. Each has three
states and is drawn as three different things: **filled** for yes, the
**outline** alone for no, and a **dashed box** for a row that rendered no
such button. Point at one and it says which in words.

That third state is the badge tiles of `/you/likes` in Badges view, which
render neither button — so nothing on that page says whether you liked a
track, and telling you that you have not would be a claim about your own
account made from a page that never mentioned it. Switch that page to
**List** view and the rows say — or have Settings do it (see
[Opening your likes as a list](#opening-your-likes-as-a-list)).

A field nobody has an answer for says **unknown** and cannot be copied, so
that no claim the extension never made ends up pasted somewhere. There are
two ways that happens, and they are different things:

- **genre** on a track whose page rendered no tag for it and which nobody
  has looked up. Playlist rows and the badge tiles render none; Likes rows
  and a track page's hero do, and that tag _is_ the track's genre — so a
  Likes page in **List** view answers this field for every row it draws,
  with SoundCloud never asked. (A track cached by a version of this extension from before
  the genre was kept does _not_ read as unknown: such an entry counts as
  stale however recent it is, and is asked about again. See
  [When a cache entry goes off](#when-a-cache-entry-goes-off).)
- **genre** where the uploader set none reads **none** instead, because
  that _is_ an answer and it is the reason the field is empty.

The row's way **off the list** is here too, under the fields: four buttons
is as many as a 400px row will take, and this is where there is room to say
what it does — the track goes, its mark and its place in the queue stay.

Which rows are open is remembered for the browsing session, as the search
and a half-made selection are (see
[What the popup remembers](#what-the-popup-remembers)) — opening a download
link closes the popup, and a row opened to read an address off is exactly
the row to come back to. A track drawn in both sections of the **By site**
layout is one track: it opens in both places at once. 12. Each row's **cover art** is also its handle on the artwork. Clicking it
fetches that track's artwork at full size (1080 × 1080 — what SoundCloud's
own player uses for hero banners, the largest it serves) and puts it on the
clipboard as an image; the thumbnail shows a spinner while that one fetch
runs, then a check. **⌘-click** (Ctrl-click on Windows/Linux) opens the
full-size image in a tab instead, and fetches nothing here — it follows the
**Background tabs** toggle like any other link. A row whose thumbnail never
loaded on the page takes its cover from what SoundCloud said about the
track instead; only a track that has no cover at all shows a placeholder.
How the full-size address is arrived at is below. 13. **Unmark** in the header takes every downloaded mark off, on every page;
it carries the number of marks there are, and is the eraser button beside
the tick-box one. It asks once more: the first click arms it (it turns red
and asks), a second click within a few seconds clears. It says _unmark_
rather than _clear_ because there is a list on screen now, and a button
called Clear beside one reads as the thing that empties it — which this has
never touched. (Emptying the list is Select, the box at the left of the
bar, then **Remove**.) 14. The filter bar above the sort control narrows the list. The **search
box** keeps the rows whose title, artist, link site or source page
contains every word typed (case and accents ignored, in any order —
`hypeddit` lists the Hypeddit links, `bicep glue` finds the track, and a
playlist's name narrows the list to what came in from it). **Hide
downloaded**
drops the tracks already marked, so what is left is what is still to
get; a track marked while the toggle is on disappears from the list at
once. Sections that end up
empty are hidden, the others show how many of their rows are (`2 of
    5`), and a line under the bar says how many of the tracks are showing.
The toggle is remembered for good (it is shared with the Downloader
tool); the search lasts as long as the browsing session (see
[What the popup remembers](#what-the-popup-remembers)).

The line under the tool's name is about the list rather than about any page:
how many tracks are on it, how many download links they add up to, how many
have none, how many are not checked, and — once the list has been built from
more than one page — how many sources it came from.

What an add came to is said either way round. A clean one is a toast, because
the rows that just appeared are the report; anything that has to be read — a
page that could not be reached, tracks SoundCloud would not describe, tracks
that did not fit — is a note that stays above the list until something replaces
it.

### How a full-size artwork URL is derived

SoundCloud serves every artwork at multiple sizes, addressed purely by the
last `-<size>` token of the filename on its image CDN:

```
https://i1.sndcdn.com/artworks-<hash>-t120x120.jpg     playlist row thumbnail
https://i1.sndcdn.com/artworks-<hash>-t500x500.jpg     og:image size; Likes rows and tiles
https://i1.sndcdn.com/artworks-<hash>-t1080x1080.jpg   player hero banner
```

`src/lib/artwork.ts` rewrites that token in the thumbnail URL the content
script read from the DOM (the inline `background-image` SoundCloud sets once a
thumbnail has lazy-loaded), so no request is ever needed to _discover_ a
full-size URL — and a URL whose shape it does not recognise is passed through
untouched rather than mangled.

A page lazy-loads its thumbnails as they scroll into view, so most rows of a
list read without scrolling through all of it render no thumbnail at all. Those
rows are not left with a placeholder: every track object SoundCloud sends the
lookup — the page's own hydration, `/tracks?ids=`, the likes listing,
`/resolve` — names the track's artwork (`artwork_url`, at its legacy `-large`
size, which is one of the tokens rewritten above), or its uploader's avatar
where the track has no artwork of its own, which is the same picture the site's
own rows fall back to. That address is kept with the rest of the details, at no
extra request, and stands in wherever the row showed nothing. The row's own URL
wins where there is one: that rendition is already in the browser's cache. A
track with neither still shows the placeholder.

Load on SoundCloud's servers: the rows display the thumbnails using the exact
URLs the page already rendered, so they are typically served from the browser's
cache; at worst the same thumbnails are re-requested from the image CDN (~120 px
on playlist pages; 500 px on Likes pages, whose rows and tiles render larger
artwork). The full-size image is fetched **only when a thumbnail is clicked —
exactly one image per click**; nothing is prefetched, and a ⌘-click fetches
nothing at all, since the browser makes that request itself when it loads the
tab. Chrome's async clipboard accepts only PNG, so a JPEG is re-encoded from
its decoded pixels before it goes on the clipboard. (The artwork CDN,
`i1.sndcdn.com`, serves `Access-Control-Allow-Origin: *`, so the on-click fetch
needs no extra host permissions.)

### The markers on the page

A track you have downloaded gets a translucent overlay on its cover art with
a white **Downloaded** pill in the bottom-right corner, wherever that track
appears: playlist rows, Likes rows (List view), Likes tiles (Badges view), a
track page's own hero artwork, and the player bar. Playlist rows show 32 px
thumbnails and the player bar a 30 px one, too small for the word, so there
the pill shrinks to a check mark in the same corner.

A track page's hero is marked from the page's own identity rather than from a
link, because nothing in the hero holds the track's path — its title is a
heading, not a link (see [How it works](#how-it-works) for how a track page
is recognised and named). That is also what keeps a _playlist_ page's hero
clear: the same hero renders there, and a playlist is not a track to mark. In the player bar the title is
a link for a real track and a bare span for an ad, which names nothing, so an
ad is never marked.

Waiting in the session queue is not a pill: the queue button below says that
itself, in the corner where a click can do something about it.

### Queueing from the page

Hovering a track's cover art brings up a **+ queue** button in its _top_-right
corner — the corner opposite the pills, and clear of the play button SoundCloud
puts in the middle. It drops that track in the session queue without leaving
the page. Same toggle as the list button on every row of the Manager panel,
same refusal to queue a track twice, on the page where the track is.

That button is also where the page says a track **is** in the queue. On a
queued track it is there without hovering, reading **queued** in the queue's
green; putting the pointer on it turns it into the **unqueue** it would become,
word and colour together (white, as a track leaving the queue). One control
carrying the state and the click that changes it, rather than a pill in one
corner and a button disagreeing in the other. It says so the moment the track
is queued — from the popup, or from the button itself — and stops the moment it
leaves, in every tab at once; it is gone with the queue when the browser
closes. On playlist rows and in the player bar, where the artwork is 32 px or
so, there is room for a sign instead: a queued track keeps the **+** the pill
used to carry, and hovering makes it a **−**.

It sits on every picture of a track the markers do: playlist rows, Likes rows
and tiles, a track page's hero, the player bar. It is only _there_ while the
art is hovered or the button itself has keyboard focus — tab to it and it
appears, so it is reachable without a pointer — and a click on it neither
plays the track nor follows the link the artwork sits in.

The track goes into the queue with what the row says about it (title, artist,
cover art) plus whatever download details have been looked up for it, which
is the same entry the panel would have made — so the Downloader can find its
gate later without the page. A row showing no title or artist queues with
those fields empty, exactly as extracting the page would give them.

The session queue is the one thing the content script writes — this button
fills it, and so does the download button below. It writes to the extension's
own session storage, never to the page, and Chrome keeps that area shut to a
content script until a trusted context opens it — so until the popup has been
opened once in this browser session there is no button at all, and with it no
sign on the page of what is in the queue. Open the toolbox once and it
appears.

A track that has been looked up and offers no download link at all gets a
dark **nodl** pill in the same corner (artwork left as is), so the ones not
worth opening are visible at a glance. One pill per track: a downloaded mark
wins over "nodl", being the end of the story. The nodl
pills follow the known details, which the extension keeps for
thirty days (see below), so they appear as soon as a page loads for tracks
checked (or harvested from a page) before, and disappear once the details
expire and are checked again. A track whose purchase link is only known
from the page never gets a "nodl" pill: its SoundCloud download is unknown.

What "downloaded" means: SoundCloud's own **Download file** button or a
purchase-link button was clicked on a SoundCloud page itself, the
third-party downloader saved the file, or the mark was made in a panel — one track at a time, or
a queued batch at once. Opening a link from the Manager panel is not one of
them: it queues the track instead, since the extension cannot see what happens
on Hypeddit or Bandcamp afterwards and the download has not happened yet.

Where the marks live: in the extension's own storage (`chrome.storage.local`,
key `downloaded`, a map of track permalink path → time marked) — not in the
page's cookies or localStorage, which SoundCloud's scripts could read. (The
session queue sits in `chrome.storage.session` instead, and is gone when the
browser closes; see that tool. Chrome keeps that area shut to content scripts
until a trusted context opens it, so the popup does — once, as the queue
starts — which is what lets the page offer its queue button at all. SoundCloud's own scripts still cannot reach it: the door is
opened to the extension's content script, not to the page.) They
survive reloads and browser restarts, are per browser profile (not synced),
and go away with the extension (or with **Clear**). The known details
sit next to them (key `details`, permalink path → the four fields above
plus the time they were learned), expiring after thirty days.

**`pnpm dev` and the loaded extension.** The watch build writes into the
same `dist/` Chrome has loaded, and Chrome reads an unpacked extension's files
from disk as it needs them. So the watch must not empty that folder between
writes: with `emptyOutDir` on, every rebuild took the extension out from under
the browser for a moment, and clicking the action in that window opened
nothing at all — no popup, just the icon's pressed state — until the files came
back. `emptyOutDir` is therefore on only for a release build. If the popup ever
fails to appear while developing, stop the watch, run `pnpm build`, and
reload the extension: that is the difference between a folder being rewritten
under Chrome and one that is simply there.

**Never `:has()`.** The overlay needs its artwork to be a containing block,
which for a while was `.sc-artwork:has(> .tbx-mark) { position: relative }` —
one line, and by a wide margin the slowest thing the extension did. `:has()`
turns every `.sc-artwork` on the page into a candidate to restyle whenever any
`.tbx-mark` is added or removed, so a playlist scrolled to the end, where the
lookup inserts hundreds of markers as the answers arrive, invalidated the
document on each one. Measured on a 479-track playlist, a single repaint took
**17 seconds** — and it is the content script's main thread, so nothing else in
the page moved either: answers coming back from SoundCloud sat unhandled, the
next batch of requests was not sent, and the panel's progress bar stopped. The
whole lookup took 10–35 seconds, of which under 7 were SoundCloud's.

The script knows which artwork it has just marked, so it says so: a
`tbx-mark-host` class on that element, which invalidates that element and
nothing else. The same lookup then takes **2.5 seconds** and the slowest
repaint **7 milliseconds**. Anything painted onto a page this size wants the
same care — the page is not the extension's to make slow.

The artwork elements marked, in each layout: `.trackItem__image .sc-artwork`
(playlist rows), `.sound__coverArt .sc-artwork` (Likes rows),
`.playableTile__image .sc-artwork` (Likes tiles),
`.listenArtworkWrapper__artwork .sc-artwork` (a track page's hero — the outer
of the two nested there) and `.playbackSoundBadge__avatar .sc-artwork` (the
player bar, whose track is named by `a.playbackSoundBadge__titleLink`).

Nothing is sent to SoundCloud for this. The marker is a DOM element the
content script appends inside the artwork element (SoundCloud's scripts can
see any node an extension adds to their page, as with any extension, but
nothing is transmitted), it ignores pointer events so the play button and
links beneath keep working, and clicks on SoundCloud's buttons are only
observed: a capture-phase listener notes them and never stops or alters
them. A MutationObserver re-applies markers as SoundCloud renders rows
lazily or navigates in-app, so they also show up on the stream and other
pages that reuse the same row layouts, and the player bar's pill follows
whatever it re-renders for the next track. Every layout's artwork element is
positioned by SoundCloud already, but the marked one is made a containing
block in `content.css` all the same, so an overlay can never land in the
wrong box; on an element with no offsets of its own that moves nothing.

### Replacing SoundCloud's own download buttons

SoundCloud's own way to a file is two controls: **Download file**, an item in
each track's More menu, and the purchase link the uploader set — a pill saying
**Buy** or **Free Download** on Likes rows and on an anonymous track page, a
**cart** in the action row of a signed-in one. With **replace SoundCloud's download buttons**
on in Settings — it is, by default — each is hidden and this extension's own
button takes its place, doing on the page what the same track's button does in
the Manager panel: the track goes into the **session queue**, and then the link
opens or the file downloads.

Why the queue and not a mark: opening a download link says nothing that is
known yet — the file has not been saved, and often the gate behind the link
has not even been walked — so the panel queues the track and the queue sheet
marks a whole batch of them once the files really are there. A click on the
page means what a click in the panel means, so it does the same thing.

SoundCloud draws the purchase link in two quite different places, and they are
counted as one control here because they are one link: the **cart** in a
signed-in track page's action row, and the pill — whatever the uploader called
it — everywhere else. See
[The signed-in track page](#the-signed-in-track-page) for why they are not the
same page at all.

The controls are not quite alike underneath:

| SoundCloud's control                                                                                                                                   | What replaces it                                                                                                                                         | What a click does                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Purchase link (`a.soundActions__purchaseLink`, legacy `.sc-buylink`)                                                                                   | an `<a>` to the link's **real** destination — SoundCloud wraps outbound links in its `gate.sc` redirector, and this unwraps it exactly as the panel does | queues the track, and the browser follows the link (so ⌘-click still opens it behind the page)                                                                       |
| The cart, which is the same link in the signed-in track page's own frame (the one link leaving the site from its `section[aria-label="Track header"]`) | the same `<a>`                                                                                                                                           | the same                                                                                                                                                             |
| **Download file** (`.sc-button-download`, inside the More menu)                                                                                        | a `<button>`                                                                                                                                             | queues the track, then presses SoundCloud's own button, hidden beneath it — which the click detection above sees, so the track is marked downloaded as it always was |

The panel's button for a SoundCloud download opens the _track page_, because
"Download file" has no URL of its own to open; on the track page itself that
would be a button that reopens the page it is on and downloads nothing. So the
replacement presses the real button instead. The end state is the panel's
(queued, then marked), one step shorter.

Every one of them says the same word, **Download**, behind the same download
icon — the panel's own rule, for the panel's own reason. SoundCloud renders
whatever the uploader called their link ("Free Download", "Buy", "🎧 Stream /
Download", and nothing at all when they gave none), and a button whose width
changes with somebody else's wording takes the room the row wanted; they all
mean one thing here anyway, which is that this downloads and the track is
queued.

What differs between one of these buttons and the next is where the file would
come from, and that is the **tooltip**: the domain the link really points at
once the `gate.sc` wrapper is off (`hypeddit.com`, `bandcamp.com`, …, without
its `www.`), or **soundcloud.com** — and **soundcloud.com (no downloads left)**
when the track's SoundCloud download limit for the period is used up, the same
thing the panel's row says about it. A screen reader gets it as the button's
label ("download from hypeddit.com"), since a tooltip is no use there. A spent
limit learned after the page was drawn is filled in when the answer arrives.

The button is drawn as tall as **the other buttons in the row**, so it sits in
that row rather than on top of it. SoundCloud sizes its buttons differently
from one layout to the next — small in a Likes row's actions, taller in a track
page's engagement bar, a full-width row inside the More menu — so the height is
measured from the page rather than written down here: from the controls
standing _beside_ ours (anything carrying a form of SoundCloud's `sc-button`
class, looked for among the siblings and then a level or two out, since a menu
puts each item in a row of its own), falling back to the control being replaced
where a row holds nothing else. The neighbours rather than the replaced
control, because they are what it has to line up with — SoundCloud does not
always size a purchase link like the buttons it sits among — and because they
are still measurable on a later pass, which is what lets a menu that was still
opening the first time round be measured once it has settled. The answer is
remembered for the rest of the controls of that kind on the page: reading an
element's height makes the browser lay the page out, and a Likes page has
hundreds of rows.

Nothing is removed from the page. SoundCloud's control is hidden with a class
of this extension's (`tbx-dl-replaced`, the one `!important` in `content.css`,
because which of two equally weighted class rules wins is a question of
stylesheet order and that is not this extension's to decide) and pressed from
beneath. Turn the setting off and it is shown again untouched, in every open
tab at once — as it is, immediately, if a control turns out to name no track
or to point nowhere, which is the one thing that must never leave an empty
space where a download button was. The More menu renders only when it is
opened, so for the tenth of a second the repaint is coalesced into, a
freshly-opened menu shows SoundCloud's own button; clicking it in that moment
does what it always did.

### The signed-in track page

Signed in, a track page is not the page this extension grew up reading. The
tab's own document keeps the site header and the player bar, and the track
itself — hero, actions, comments — arrives in an iframe: `iframe.webiIframe`
loading `soundcloud.com/n/<user>/<track>?v2_layout=true`, a React app that
shares no markup with the page an anonymous visitor gets. There is no
`.fullHero`, no `.listenEngagement`, no `sc-button` anywhere; the buttons are
MUI's, and the class that would name one (`mui-163v5oj`) is a hash of the
stylesheet that built it, so it is no more nameable than a minified variable.
The purchase link is drawn there as a **cart** rather than a pill.

Two things follow.

The content script runs **in every frame** (`all_frames` in the manifest), or
it would never see that page at all. What it reads there is the
**accessibility tree**, which is the part SoundCloud cannot rename without
breaking a screen reader: `section[aria-label="Track header"]` around the
hero, the `<h1>` for the title, the one link in that header that leaves the
site for the purchase link, and `.MuiIconButton-root` — MUI's own API rather
than a build artefact — for the row whose height ours matches. The uploader is
the header's link to a profile **on this site**, and the origin is what tells
it apart: `https://hypeddit.com/x` resolves to `/x` against the page and would
otherwise read exactly like one. The frame's path carries an extra segment
(`/n/<user>/<track>`); the track is the same track, and `/n` is dropped.

Whether you have liked or reposted the track is the one thing that layout does
not say. It is drawn as the word on a button — "Like" against "Unlike" — and
nowhere else: no pressed state, no class of its own. That is SoundCloud's
English rather than a fact about the track, so this reports "nobody said"
instead of reading a label back, which is what a badge tile reports too.

And **only the tab's own document answers the panels**. A panel asks with
`chrome.tabs.sendMessage` and no frame id, which asks every frame in the tab
and keeps whichever answers first — so a frame that also answered would make
the panel read the page sometimes and the frame other times. The frame does
the work that is about the page it is in (the buttons, the marks); what a
panel asks for comes from the top. Which is also the limit of this: the
panels still read the classic layout, so on a signed-in track page the
Manager's bar has no track to offer and **add** has nothing to add. The button
in the page works there; the panel does not yet.

### Opening your likes as a list

`soundcloud.com/you/likes` has two views, picked with SoundCloud's own **View**
toggle above the list: **Badges**, a grid of tiles, and **List**, the rows
every other Likes page draws. SoundCloud remembers which was last picked and
opens the page in it; the one it ships with is Badges.

A tile says considerably less than a row does, and everything this extension
reads off that page it reads out of what SoundCloud drew:

|                                                       | Badges (a tile)                                                                                         | List (a row)                                             |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Genre                                                 | none — the tag is not rendered, so every track has to be looked up for it                               | the tag beside the title, which _is_ the genre           |
| Liked / reposted                                      | no buttons, so the panel draws its “nobody said” mark rather than making a claim about your own account | both buttons, read from the page                         |
| Purchase link                                         | none, so there is nothing to put a download button in the place of                                      | **Buy** / **Free Download**, replaced as everywhere else |
| Title, artist, artwork, marks, the **+ queue** button | all there                                                                                               | all there                                                |

So **open your likes as a list**, in Settings. It is **off** unless it is asked
for. Turned on, the content script presses the **List** half of SoundCloud's
own toggle whenever that page comes up showing the grid — the same press a
person would make, so SoundCloud draws the page and SoundCloud remembers the
choice, which is exactly why it is not on by default: it outlives the visit it
was made in.

It presses **once per arrival at the page**, and pressing **Badges** afterwards
is left alone: a person asking for the grid this time must not have it taken
back from under them. The guard lifts when the page is left (SoundCloud is a
single-page app, so "left" means the toggle is no longer rendered) and when the
setting itself is changed, so turning it on with the page already open applies
it there and then.

Nothing else is touched. The page is only ever asked for by pressing
`.listDisplayToggle__listToggle` inside the `.collectionSection
.listDisplayToggle` that only your own Likes page renders — a public
`/<user>/likes` is a list already and has no toggle at all — and what is on
screen is read from `.collectionSection .badgeList` rather than from the
toggle's own pressed state, because the grid is the thing that has to change.
Should SoundCloud rename or rework that control, the press finds nothing and
the page stays exactly as it was drawn.

### Title Case

Uploaders type what they like. One playlist is "glue", "Glue", "GLUE (ORIGINAL
MIX)" and "Drum & Bass" at once, and somebody pasting those into one document
wants one house style rather than four. **title case the title, artist and
genre**, in Settings, is that switch. It is **off** unless it is asked for:
what the uploader typed is what the page shows, and an extension that quietly
retypes it draws fields that no longer match the page they came off.

It reaches the **opened row only** — the three fields there that are words —
and what a click on one of them copies, so the row and the clipboard say the
same thing. Not the **link**, which is an address, and case is meaning in one.
Not **from**, which is this list's own sentence about where a track came in.
Not the list itself, nor any export, nor anything the content script paints on
a SoundCloud page: those go on saying exactly what SoundCloud said, and the
switch changes nothing that is stored — turning it off puts every field back
as it was.

Converting case is guesswork, so the guessing is written down
(`src/lib/title-case.ts`) rather than left to be discovered a track at a time.
Two rules:

1. **A capital somebody typed is left as typed.** SBTRKT, DnB, McCoy, EDM — a
   capital is a claim about the word, and nothing here can check one. So a word
   carrying one anywhere is untouched, and this only ever _raises_ a lower-case
   word. `the best of it` becomes `The Best of It`; `The Best Of It` is already
   somebody's choice and stays.
2. **Unless the whole field is in capitals**, which is the one case where the
   first rule has nothing to keep: if every letter is a capital then no capital
   says anything, and the field is re-cased from scratch. `GLUE (ORIGINAL MIX)`
   becomes `Glue (Original Mix)`. A short list of words whose capitals really
   are the word survives that anyway — AKA, BPM, DJ, DNB, EDM, EP, ID, IDM,
   LA, LP, MC, NYC, OST, RMX, TV, UK, UKG, US, USA, VIP and the roman numerals
   II to IX — so `HARDCORE EDM MIX` becomes `Hardcore EDM Mix`. One lower-case
   letter anywhere and the field is not shouted at all, and rule 1 has it back.

Articles, conjunctions and the short prepositions stay down in the middle of a
title and are raised at either end, which is what makes `fly me to the moon`
into `Fly Me to the Moon` rather than a row of capitals. Words are split on
everything that is not a letter, a digit or an apostrophe inside one, so
`hip-hop` becomes `Hip-Hop` and `Drum & Bass` is spaced as it was typed.

The switch is the popup's own (`$lib/title-case.svelte.ts`, `pref:title-case`),
made once and read by both the sheet that throws it and the Manager panel under
it — so a row that is already open re-draws as it moves. It is not in the
exported state file: how somebody likes their own tracklist capitalised is not
worth a line in a format every install has to keep reading.

### Saving and restoring it all

Everything above lives in the extension's own storage, which goes away when
the extension does. Reinstalling it, or moving to another browser profile,
otherwise means losing every mark and paying for every lookup again. **your
data**, a section of the Settings sheet, takes the lot out to a JSON
file and reads one back (`src/lib/components/StateTransfer.svelte`).

It was the Manager panel's own footer, which was the wrong place for it twice
over. The file carries the marks a SoundCloud page paints, the content
script's cache of lookups, the [session queue](#session-queue) behind every
header's count, the [artists](#artists) tool's list and three panels' options
— one of its members is the Manager's list and the rest are not — and a
backup is something one takes _before_ opening a playlist rather than while
looking at one. The panel's footer still
points at it, since the list is the biggest thing in the file and so where
somebody goes looking. The section opens with a row of counts — marks, tracks
looked up, tracks listed, artists, tracks queued — because a button reading
"export" says nothing at all about how much there is to lose by not pressing
it.

**Export** writes `manager-<date>.json` through an ordinary download link, so
it needs no extra permission. The queue is in there because it is the part
with the shortest life of all: closing the browser ends it, so a file is the
only way a batch picked out but not yet worked through survives the day it was
picked. What goes into the file is the queue and the list the popup is
_showing_, handed over by the stores that hold them rather than read out of
storage — a browser that will not keep them for the extension is exactly the
browser whose queue and list are worth exporting, and both would be missing
from a file written the other way.

**Import…** opens a box that takes either a chosen file or pasted text; both
routes fill the same box, so what is about to be imported is always the text
on screen. (Pasting is there because a browser popup can close when a file
dialog takes the focus; opening the popup in a tab avoids that entirely.) The
box is capped in height and scrolls past the cap — a state file is tens of
thousands of lines, and a box that grew to hold one would push every control
under it off the bottom of a popup that is only as tall as what is in it.

The text in the box is **read as it lands**, not when Import is pressed:
whatever is there is put through the same parser the import uses, and the line
under the box says either what the file holds ("a backup from 2026-09-08 — 128
marks, 96 looked-up tracks, 42 tracks listed") or what is wrong with it. The
button that would write it is off until it reads. Nothing is written by any of
that — `parse` only ever refuses — so a file chosen by mistake says so before
it has touched anything.

#### Importing adds

By default an import **never removes anything**:

- marks are added, and a track already marked keeps the earlier of the two
  times, so re-importing an old backup cannot make a mark look newer than it
  is;
- details are taken only where they are newer than what is already known —
  the same rule the content script applies to another tab's lookups;
- listed tracks are appended to the list, in the order the file has them, and
  a track already on it keeps its place and takes the better of the two
  answers — the same rule adding a page twice follows. A restored entry keeps
  the source and time the file gives it rather than being stamped "just now,
  from nowhere", so a restored list is the list that was exported;
- artists are appended to the artists tool's list the same way, and one
  already on it keeps its place, takes the file's tracks on top of its own and
  the better of the two answers about the account — a profile over a
  remembered failure, and the newer of two profiles;
- queued tracks are appended to the queue, in the order the file has them, and
  a track the queue already holds is left exactly as it was queued — the
  permalink is the identity, so the same track never lands in it twice, and
  neither does a file that lists it twice or one imported twice. That is the
  queue's own doing: a file is restored by queueing its tracks, the same call
  a row's list button makes, rather than by writing over the queue;
- options are set only where nothing is set already. An option you have chosen
  here is a thing you have, and an import that is adding to what you have does
  not overwrite it — which makes a file a way of carrying an option to an
  install that has never been asked, rather than a way of losing one.

#### Replacing, which is the opt-in

The switch in the import box turns every one of those the other way round, and
it is off every time the popup opens: the marks become the file's marks, the
details cache becomes the file's cache, the list, the artists and the queue
become the file's, and every option goes to what the file says — or, where the file says
nothing about one, back to its default. What was here and is not in the file
is gone. The button changes with it, from **add to what's here** to **replace
everything**.

That is the way to restore an exact snapshot. Doing it by hand still works
(**Unmark** for the marks, Select → the box at the left of the bar → **Remove**
for the list) and reaches the same place. A SoundCloud tab that is already
open keeps its old markers either way until it is reloaded.

A file exported before the tool kept a list carries no `listed` member, and
still reads: adding such a file restores the marks and the lookups as it
always did and leaves the list as it is, and replacing with one empties the
list, which is what replacing with a file that holds no list means. The
`artists` member is newer still, and a file without one reads the same way.

Nothing is imported unless the whole file is valid — a bad entry is reported
by name and nothing is written.

#### The file format

The format is specified in
[`docs/manager-state.schema.json`](docs/manager-state.schema.json), plain
[JSON Schema 2020-12](https://json-schema.org/), so any conformant validator
can check a file. A convenience script uses Ajv from the dev
dependencies:

```
pnpm validate:state manager-2026-09-09.json
```

```json
{
	"format": "toolbox.manager.state",
	"version": 1,
	"exportedAt": "2026-09-09T10:04:00.000Z",
	"downloaded": { "/bicep/glue": "2026-09-08T12:00:00.000Z" },
	"details": {
		"/bicep/glue": {
			"id": 1234,
			"permalink": "/bicep/glue",
			"downloadable": true,
			"hasDownloadsLeft": true,
			"purchaseUrl": "https://hypeddit.com/x",
			"purchaseTitle": "Free Download",
			"genre": "Techno",
			"artworkUrl": "https://i1.sndcdn.com/artworks-abc-large.jpg",
			"at": 1757332800000
		},
		"/someone/unreachable": { "unavailable": true, "at": 1757332800000 }
	},
	"queue": [
		{
			"permalink": "/bicep/glue",
			"title": "Glue",
			"artist": "Bicep",
			"artworkUrl": "https://i1.sndcdn.com/artworks-abc-t120x120.jpg",
			"details": {
				"id": 1234,
				"permalink": "/bicep/glue",
				"downloadable": true,
				"hasDownloadsLeft": true,
				"purchaseUrl": "https://hypeddit.com/x",
				"purchaseTitle": "Free Download",
				"genre": "Techno",
				"artworkUrl": "https://i1.sndcdn.com/artworks-abc-large.jpg",
				"at": 1757332800000
			},
			"genre": "Techno",
			"liked": true,
			"reposted": null
		}
	],
	"artists": [
		{
			"permalink": "/bicep",
			"name": "Bicep",
			"tracks": [{ "permalink": "/bicep/glue", "title": "Glue" }],
			"profile": {
				"id": 5678,
				"permalink": "/bicep",
				"username": "Bicep",
				"fullName": "",
				"city": "Belfast",
				"country": "GB",
				"followers": 123456,
				"trackCount": 42,
				"verified": true,
				"avatarUrl": "https://i1.sndcdn.com/avatars-abc-large.jpg",
				"bio": "bookings: agent@example.com",
				"links": [{ "network": "instagram", "title": "", "url": "https://www.instagram.com/bicepmusic/" }],
				"at": 1757332800000
			},
			"from": "Late Night Cuts",
			"at": 1757332800000
		}
	],
	"prefs": { "background": true, "sort": "site", "hideDownloaded": false }
}
```

Only `format` and `version` are required; the data members default to
empty, so a hand-written file restoring nothing but a few marks is valid. A
`details` entry is either what SoundCloud said about a track or a remembered
lookup failure (`unavailable`), and `additionalProperties` is `false`
throughout, so a typo is refused rather than silently dropped. `genre` and
`artworkUrl` are the optional members of a details entry, and only because of
the files that already exist: entries cached before either was kept carry no
such key, so a backup taken then would otherwise stop reading back. `""` in
them is an answer — the uploader set no genre; the track has neither artwork
nor an uploader avatar. An imported entry without the key is accepted, and then
asked about again on the next page that lists the track — see
[When a cache entry goes off](#when-a-cache-entry-goes-off).

A `queue` entry is a track as the queue keeps one, and only its `permalink` is
required — a track queued from a page that showed no title carries none. The
details it was queued with are either the ones SoundCloud gave (as above) or
the partial ones a Likes row renders by itself, which carry `"partial": true`
and `null` for everything only the API could answer. A remembered lookup
failure is refused there: it is not an answer about the track, so it is never
what one was queued with.

Its `genre`, `liked` and `reposted` are what the page it was queued from said,
and the reason a queue entry carries them at all is the Manager's list, which
adds from the queue: by then that page is gone, no lookup answers those three,
and a track that arrived this way would otherwise never know them. `null` is
"the page never said" — a badge tile renders no tag and neither button — as is
their absence, in a queue written before they were kept. A `listed` entry
carries the same three, with what only a list knows on top: `domPurchaseTitle`,
`from` and `at`.

An `artists` entry is an uploader as the artists tool keeps one, and only its
`permalink` — an artist's path, one segment (`/bicep`) rather than a track's
two — is required. Its `profile` is what SoundCloud said about the account
(`artistProfile` in the schema, of which only `at` is required), a remembered
failure to find one (`unavailable`), or `null` for an artist nobody has asked
about yet; the importer refuses a profile under a track's path, as it refuses
a track's permalink where an artist's belongs.

The extension ships no dependencies, so `state-transfer.ts` enforces that
schema by hand rather than running a validator in the browser. The two are
kept in step by `test/state-schema.test.ts`, which feeds the same documents
to Ajv and to the importer and requires the same verdict from each. There are
two deliberate asymmetries, each pinned down by its own test: JSON Schema
cannot say "this property must equal that one", so the importer alone refuses
a `details` entry naming a track other than the one it is filed under, and a
`queue` entry whose details name a track other than the one queued. Filed
wrongly, the first would answer for the wrong track ever after and the second
would send the Downloader to another track's gate.

A file listing the same track twice is _not_ one of them: the schema accepts
it, the importer accepts it, and the queue drops the second copy when the
tracks are queued. "Unique by key" is not something JSON Schema can say
either, but it is not the format's business — the queue is the thing that
knows what is already in it.

### How it works, and what it costs SoundCloud

Which tracks are downloadable is not visible in playlist or Likes rows, nor
in a track page's hero: "Download file" only renders inside a row's More menu
when it is opened, and playlist rows show no purchase link at all. The direct download URL of
years past (`soundcloud.com/<user>/<track>/download`) now answers `410
Gone`, signed in or not. So the extension uses everything the page already
holds first, asks SoundCloud only for what is left, and remembers the
answers.

**Free, from the page (no request):**

1. **The page's hydration data.** SoundCloud server-renders a `<script>`
   element with `window.__sc_hydration = [...]`, the JSON the page was built
   from. Being a DOM element, its text can be read from the isolated world
   and parsed as JSON (it is never executed). It holds a full track object
   for a track page's track and for the first few tracks of a playlist
   (5 to 19 in practice), bare `{id}` stubs for the playlist's other tracks,
   and on every page the public API client id the page itself uses. Every
   full track in it is harvested into the cache when the page loads — so
   just browsing SoundCloud fills the cache for free. It describes the page
   loaded by the last full navigation, so after in-app navigation it is
   stale (the client id in it stays valid).
2. **Likes rows** (List view) render the purchase link as
   `a.soundActions__purchaseLink`, wrapped in SoundCloud's `gate.sc`
   redirector, with SoundCloud's label for it in `aria-label` ("Buy",
   "Free Download", …). That settles purchase links for those rows without
   asking anyone; only "can SoundCloud itself download it" is unknown. A
   **track page** renders the same link in its engagement bar
   (`.listenEngagement`, legacy class `.sc-buylink`), read the same way — so
   a song page after in-app navigation, whose hydration is stale, still
   answers for its own purchase link for free.
3. **Track identity** is the `href` of each row's title link, minus the
   `?in=<playlist>` query SoundCloud adds on playlist pages (that query is
   also how the extension knows which playlist the rows belong to, whatever
   the address bar says).
4. **Liked and reposted** come from the row's own `.sc-button-like` and
   `.sc-button-repost`, which say whether they are on twice over — the
   `sc-button-selected` class and `aria-pressed` — and either is taken, so a
   redesign that drops one of them still reads correctly. This is page state
   rather than track metadata (it is about the signed-in account, and it
   changes the moment either button is clicked), so unlike the fields above it
   is never cached: it is read afresh every time the panel asks for the page.
   A row that renders no such button — every badge tile on `/you/likes` in
   Badges view, which
   [Opening your likes as a list](#opening-your-likes-as-a-list) is there to
   avoid — answers `null`, which is "nobody knows" rather than "no".
5. **The genre** is the tag SoundCloud renders beside a row's title
   (`.soundTitle__tagContainer .sc-tagContent` on Likes rows,
   `.sc-tagContent` in a track page's hero). It is the track's `genre` field
   and not one of its free tags — both were read off soundcloud.com and
   checked against the same tracks' own page data — so a Likes page names the
   genre of every row it draws and nothing has to be asked for it. Playlist
   rows and badge tiles render no tag, and answer `null`: "nobody knows",
   which is not the uploader's "no genre set" (`''`). Like the two above, it
   is read off the page rather than taken from the details cache, and the
   panel prefers it to a lookup's answer.

**Asked of SoundCloud** — only when the panel is opened, only for tracks the
cache does not know, all `GET`s to its public API (`api-v2.soundcloud.com`,
the one the page calls), anonymous (that client id, no cookies —
`credentials: 'omit'` — no auth header), returning public track metadata of
which `downloadable`, `has_downloads_left`, `purchase_url`, `purchase_title`,
`genre` and `artwork_url` (else the uploader's `avatar_url`) are kept:

| Page                              | Request                   | When                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| --------------------------------- | ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Playlist                          | `/tracks?ids=…`           | for the hydration's stubs, 50 per request, in playlist order, only as far as the rendered rows need — the first request on its own (it usually settles every rendered row, and the other nine of a 479-track playlist are never made), then four at a time                                                                                                                                                                                       |
| Playlist, after in-app navigation | `/resolve?url=<playlist>` | once per page, for the track ids (74 KB for a 479-track playlist; re-fetching the page would be 149 KB of server-rendered HTML). The answer is kept for as long as the page is open, so a second panel spends its requests on rows rather than on asking what playlist this is again                                                                                                                                                             |
| Likes                             | `/users/<id>/likes`       | the listing the page itself loads, in page order; the first request asks for exactly as many likes as the page shows (plus a little slack), later ones for 200, the API's maximum, until every rendered row is covered or 2,000 likes deep. One request instead of one per track. The user id is in the hydration; after in-app navigation to another user's likes, one `/resolve` of the profile URL, also kept for as long as the page is open |
| Track page                        | nothing of its own        | the one track is in the page's own hydration, so a song page loaded fresh is answered without a request at all; after in-app navigation it falls to the row below, which is one request for one track                                                                                                                                                                                                                                            |
| Anything left over                | `/resolve?url=<track>`    | private tracks, a filtered Likes list, rows past the listing's reach — four at a time                                                                                                                                                                                                                                                                                                                                                            |

The listing is skipped when resolving the few pending tracks one by one
would cost fewer requests. `429` answers are backed off and retried a couple
of times. The API accepts cross-origin calls from `soundcloud.com` only,
which is why the content script makes them (from the page's origin) rather
than the panel.

**A page the API cannot see.** These requests are anonymous — the extension
asks SoundCloud the way a signed-out visitor would, which is what keeps it from
needing the user's account — so a page whose tracks are not public is one the
API will not describe however it is asked. The ordinary case is somebody
looking at their own private playlist. The bulk request comes back empty, and
resolving each row in turn then fails the same way one request at a time: on a
private 90-track playlist that was ninety round trips, a minute of "still
checking", and an answer of no.

So when the bulk source settles nothing at all, five rows are resolved before
the rest. If even one comes back, the rest follow as they always did — the page
is visible and the bulk source simply did not reach it (a filtered list, rows
past a listing's reach). If none do, the lookup stops and says why: those
tracks are not ones SoundCloud will describe, and whatever the page itself
renders for them is still listed.

**The listing is carried on, not restarted.** Scrolling a Likes page and
opening the panel again is the ordinary way to use it, and every opening
brings rows nobody has asked about before. Walking the listing from the top
each time would cost 55 requests over ten such scrolls of a 2,000-like page,
for the ten pages of data SoundCloud actually had to send; instead the page's
cursor is kept and the next walk asks only for what the last one had not
reached, which makes it ten. It resumes only when every row now wanted lies
past what the earlier walk covered — a filtered list, or a like added at the
top since, starts from the top as before, and a cursor SoundCloud no longer
honours is dropped rather than retried.

**Nothing is looked up until somebody asks.** Opening the panel asks the
content script one thing — `EXTRACT_TRACKS`, a scan of the page's own DOM that
comes back in a few milliseconds and costs SoundCloud nothing — and that is
only to say what the tab is offering: its name, its curator, how many tracks
it has. `GET_DOWNLOAD_DETAILS`, which is the lookup below, runs when **add** is
pressed. It used to run on every opening of the tool, over every track of
whatever page happened to be in front.

**The rows do not wait for the lookup either.** Adding puts the page's tracks
on the list from that first scan — title, artist, cover art, and on Likes pages
the purchase links the page itself renders — and the lookup fills them in
underneath. Rows nothing is known about yet say so ("Not checked" sorted by
site, "checking…" in list order) rather than being called link-less. Answers
arrive a few at a time on the `DOWNLOAD_DETAILS_PROGRESS` message the lookup
was already sending its progress on, and each row settles where it stands.

The panel used to await the lookup and show a spinner in place of the list
until it finished, which on a scrolled playlist was several seconds of an
empty popup. What is left is the shell in about ten milliseconds and the list
as fast as storage answers — and a long list is painted
a screenful at a time (sixty rows on the first frame, two hundred on each one
after) because five hundred rows are the best part of a second of DOM work
and the popup shows eight of them.

**Four at a time** is what the lookup costs in wall clock. These requests are
round trips and nothing else — SoundCloud answers one in half a second on a
good day and in two on a bad one — so a lookup takes as long as the number
of them it waits through end to end, and the panel counts them out on a
progress bar above the list. Measured against the live API, ten `/tracks` batches one
after another took 10.3 s and the same ten four at a time took 2.1 s; twenty
`/resolve` calls went from 5.0 s to 1.3 s. Six at a time was no faster than
four (the server becomes the queue), and nothing at any of these rates drew
a single `429`. Four is where the extension stops, being polite rather than
fastest: the requests are anonymous `GET`s of public metadata, but they are
made in the user's name on the user's page.

**Remembered:** every answer is kept in extension storage for thirty days
(uploaders occasionally add a purchase link later, so not forever), on every
tab; a track that could not be looked up is remembered for a day so it is
not retried on every panel open. Reopening the panel, reloading, scrolling
further and coming back, another tab — none of it asks again for a track
already known. Answers go into the cache as they arrive, not when a lookup
ends, and are written back within a second, so a lookup cut short by a
reload keeps what it had learned, "nodl" pills appear as answers come in,
and a later request benefits from an earlier one's progress. The hydration
is parsed once per page.

The cache holds 20,000 tracks — about 6 MB of the 10 MB `storage.local`
allows, leaving the marks room beside it — and drops the entries it learned
longest ago to stay under that. The cap is there for SoundCloud's sake, not
for tidiness: nothing waits on the write, so a cache that outgrew the area
would fail its write in silence and be empty on the next visit, and an empty
cache asks about every track on every page all over again, forever. Forgetting
pages not seen in weeks costs a handful of requests; refusing to forget costs
all of them. If the area still will not take the write, half as much is
written rather than nothing.

#### When a cache entry goes off

Age is not the only way. An entry also goes off when it is missing a field
this version keeps, whatever its age — because it will never gain one by
waiting.

The fields kept per track have grown: the **genre** was added after the cache
had been filling up for a month, and the **cover** after that. An entry written
before either has no `genre` or `artworkUrl` key, and nothing about being
thirty days fresh would ever fix it — nothing would ask about the track again
until it expired, so for a month the panel would have said it did not know the
genre of a track SoundCloud was about to name the genre of anyway, in a request
it was going to make regardless.

So `hasEveryField()` in `content.ts` checks an entry against the list of
fields `detailsOf()` writes, and one that is short of any of them is treated
exactly as an expired entry is: dropped on load, dropped on write-back, and
looked up again. Remembered failures are exempt — they are not details and
have none of those fields, and their own day-long lifetime is what expires
them.

What this costs is one round of looking up, once, on each page whose tracks
were cached by an older version — and most of it never becomes a request,
because the hydration harvest runs on every page load and refills those
entries from the page itself for nothing. What it buys is that adding a field
to `detailsOf()` is the whole of the change: there is no separate cache
version to remember to bump, and no release in which the panel quietly says
"unknown" about something it could have asked for.

**One lookup at a time:** the panel can be closed and reopened, or the page
left and come back to in-app, while a long lookup is still running — and
each reopen asks again. A request whose tracks are all covered by a lookup
already queued or running joins it (the panel says "continuing a lookup
already under way") and is answered as soon as its own tracks are settled,
even while that lookup goes on with the rest; a request for other tracks
gets a lookup of its own, run after the ones before it and asking only for
what is still unknown by then. Two lookups never run at once from one tab,
and nothing is asked twice — the price being that a request for a
different page waits for the lookup in progress to finish.

So in practice: a fresh playlist page costs one request per 50 tracks past
the hydrated ones (often zero for short playlists); a Likes page costs one
request per 200 rendered likes; a track you have already seen anywhere costs
nothing. The panel itself fetches nothing to draw the list (thumbnails are the URLs the
page already rendered), and no request is ever made to any of the external
sites: their links are only opened when clicked. The one fetch it ever makes is
for a cover art click, described above.

### Keeping it above board

- Reads only what a signed-out visitor can see: public track metadata, via
  the public API the site's own front end uses, with the public client id
  the page itself was served with. No cookies, no session, no auth header —
  nothing that would let a request act as you.
- `GET` only. Nothing is ever written to SoundCloud, and no data of yours
  (marks, cache, clicks) leaves the browser.
- No code runs in the page's own JavaScript context, no network traffic of
  the page is intercepted, and none of SoundCloud's controls are driven
  automatically — the only change to the page is the inert marker overlay,
  and the only thing watched is which of the page's own buttons you click.
- Asks little and never twice: bulk endpoints sized to what is on screen,
  nothing prefetched beyond the rendered rows, no polling or background
  activity (requests happen only when the panel is opened), a month-long
  cache shared by all tabs, failures not retried for a day, one lookup at a
  time per tab with low concurrency within it, and back-off on `429`.

Elements the marker and click detection rely on (verified September 2026):

| What                                      | Selector                                                                                                                                         |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| Artwork element to overlay (playlist row) | `.trackItem__image .sc-artwork` (32 × 32, made `position: relative` by the `tbx-mark-host` class the script adds — never by `:has()`, see above) |
| Artwork element to overlay (Likes row)    | `.sound__coverArt .sc-artwork` (160 × 160)                                                                                                       |
| Artwork element to overlay (Likes tile)   | `.playableTile__image .sc-artwork` (180 × 180)                                                                                                   |
| Track identity                            | `href` of `.trackItem__trackTitle`, `.soundTitle__title`, or `.playableTile__mainHeading` / `.playableTile__artworkLink`                         |
| Purchase link in a Likes row              | `a.soundActions__purchaseLink` (`href` via `gate.sc`, label in `aria-label`)                                                                     |
| Purchase link on a track page             | the same, in `.listenEngagement` below the hero (legacy `.sc-buylink`; the link's own text where there is no `aria-label`)                       |
| More menu button                          | `.sc-button-more` (the menu itself renders detached, appended to `<body>`, so the row whose More button was clicked last is remembered)          |
| Download file button                      | `.sc-button-download` (inside the More menu)                                                                                                     |
| Purchase link (click detection)           | `a.soundActions__purchaseLink` (legacy `.sc-buylink` is checked too)                                                                             |

The markers' classes are prefixed `tbx-mark` (`tbx-mark--downloaded`,
`tbx-mark--nodl`, `tbx-mark--small`), and the queue button's `tbx-queue`
(`tbx-queue--small`, the resting word in a `tbx-queue__text` span, plus a
`data-state` of `<variant>:yes` or `<variant>:no` and, where hovering swaps the
word, a `data-hover` the stylesheet renders in its place); styles for both in
`content.css`. The artwork they sit in is given `tbx-mark-host`, which is what
makes it their containing block. Every rule that reaches the queue button
repeats its class (`.tbx-queue.tbx-queue…`), for the reason the replacement
download button does the same: the element is this extension's, but the page
can still reach it as a plain `button` inside SoundCloud's own artwork, and
`.sound__coverArt .sc-artwork > button` (0,2,1) would outweigh a single class
of ours. That matters most for the rule holding a queued track's button up
with no pointer on it — lose there and the button still appears on hover and
never otherwise, which is the page saying nothing at all about what is in the
queue. The button that stands in for SoundCloud's own
download controls is `tbx-dl` (`tbx-dl--soundcloud`, `tbx-dl--external`, with
`tbx-dl__icon` and `tbx-dl__text` inside it), and the control it stands in for
is given `tbx-dl-replaced`, which hides it. Nothing else on the page is
touched.

### Grouping

Sections are keyed by the link's registrable domain, so `dani3l.bandcamp.com`
and `www.beatport.com` file under `bandcamp.com` and `beatport.com`; the
full host is shown on the row when it differs. A purchase link wrapped in
`gate.sc` is unwrapped to its real destination first.

### Artist searches

The badges in the **No download links** section open a search on each site
that is switched on in **Settings → search badges**. All four ship on; turning
one off takes its badge off every row and its name out of the sentence above
the section. Turn them all off and a link-less row carries nothing but its own
toggles, which the panel and the sheet both say out loud.

The sites are not written into the panel. They are a list of plain objects in
[`src/lib/search-sites.json`](src/lib/search-sites.json), read and checked by
[`src/lib/search-sites.ts`](src/lib/search-sites.ts), and adding one is an
entry in that file and nothing else — no code change, and no manifest change
either, since a badge opens a tab and never fetches.

| Badge | Site      | URL                                                         |
| ----- | --------- | ----------------------------------------------------------- |
| BP    | Beatport  | `https://www.beatport.com/search?q={artist}`                |
| bc    | Bandcamp  | `https://bandcamp.com/search?q={artist}`                    |
| Z     | ZIPDJ     | `https://www.zipdj.com/app/search?q={artist}`               |
| M     | MyMP3Pool | `https://mp3poolonline.com/results/artists?search={artist}` |

An entry is:

| Field                 | What it is                                                                                                                         |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `id`                  | what its switch is remembered under (`pref:search:<id>`). Stable — renaming it forgets whether somebody had turned that badge off. |
| `name`                | the site's name, as the settings sheet and the panel's hints say it.                                                               |
| `badge`               | the two or three characters on the button. It is 28px wide; anything longer will not fit.                                          |
| `url`                 | the search address, with `{variables}` in it.                                                                                      |
| `background`, `color` | the badge's colour and the ink on it, as `#rgb` or `#rrggbb`. Optional: a site naming neither gets a grey badge.                   |
| `on`                  | `false` to ship the badge switched off. Optional, and `true` when it is missing.                                                   |

The variables a `url` may carry, each URL-encoded where it lands:

| Variable   | Filled with             |
| ---------- | ----------------------- |
| `{artist}` | the track's artist      |
| `{title}`  | the track's title       |
| `{query}`  | both, as `artist title` |

A badge whose variables the track has nothing for is drawn disabled rather
than opening a search for the empty string — which on a playlist row with no
artist on it is every `{artist}` badge. An entry that cannot be used is left
out rather than taken as far as a broken button: one with no `name` or
`badge`, an address that is not `https:`, one with no variable in it at all,
one naming a variable that does not exist, or a second entry repeating an `id`
already used. The rest of the file is still read.

The badges are lettered monograms in each site's colour rather than the
sites' logos, so the panel ships no third-party artwork and fetches nothing.
On playlist pages the "artist" is the uploader's account name, as in the
other tools, so a channel upload searches for the channel.

### Limitations

- The extension cannot download SoundCloud's own files for you: that goes
  through SoundCloud's signed-in "Download file" button (More menu), which
  the tool points you to and watches.
- "Downloaded" for an external link means the link was opened; gated pages
  (follow-to-download and the like) may not end in a download.
- Same rendered-rows rule as the other tools: only tracks present in the DOM
  when a page is added go onto the list. Scroll a long page to the end first,
  or add it twice — a page added again brings in what it has grown since.
- The list is per browser profile, holds five thousand tracks, and refuses
  what will not fit rather than dropping the oldest: what is on it was put
  there on purpose. The add says how many did not fit; taking some off is
  Select → the box at the left of the bar → **Remove**.
- The lookup runs in the SoundCloud tab, over the tracks that tab renders, so
  a track is checked by adding a page it is on — its own page counts, and is
  the way to check one track. A track put on the list from the session queue on
  some other tab keeps whatever was known when it was queued, and reads as
  _not checked_ until a page it is on is added.
- A track page whose address SoundCloud has left pointing somewhere else (its
  URL naming one uploader while the hero names another, which in-app navigation
  can do for a moment) offers a track with no permalink, and the list cannot
  hold one: the bar counts it among the page's tracks but the button has
  nothing to add. Reloading the tab settles it.
- Private tracks (secret links) cannot be looked up anonymously and are
  reported as not checked (their purchase link, if any, still comes from the
  page on Likes pages and track pages); the attempt is not repeated for a day.
- Marks are stored per browser profile and are not synced.
- On playlist rows the Downloaded pill is a check mark, not the word — the
  thumbnail is 32 px ("nodl" still fits) — and the queue button there is a plus
  or a minus for the same reason.
- The queue button needs the popup to have been opened at least once this
  browsing session: it is the popup that opens session storage to the content
  script, and until it has, the page can neither read the queue nor write to
  it. So until then the page shows no button and no sign of what is in the
  queue. Open the toolbox once and it appears on every SoundCloud tab.
- The queue button follows the cover art, so it reaches only tracks whose art
  is on screen — there is no button in a search result's text row or the
  sidebar's related tracks.
- "nodl" pills only appear for tracks whose details are known — looked up by
  opening the panel on a page listing them, or harvested from a page that
  was served with them — within the last thirty days.
- SoundCloud's class names and hydration format are not a public API and may
  change; everything relied on is listed above.

## third-party downloader

Lives in `src/tools/third-party-downloads/`. A free download on SoundCloud
usually is not on SoundCloud: the "Buy" button points at a **download gate**,
where the artist puts the track behind a few steps — follow me, like and
repost this, drop your email, open my Instagram — with the download button at
the end. Collecting a playlist's worth of those is a lot of clicking through
the same four screens, on seven different sites that each do it differently.

This tool does the walking. Give it a set of links and it takes each one as far
as it goes, and hands the browser whatever file comes back.

Seven sites, and they are not the same
--------------------------------------

The honest summary first, because it decides what the tool can do for each:

| Site                                                            | What the link is                                                      | Ends in a file?                |
| --------------------------------------------------------------- | --------------------------------------------------------------------- | ------------------------------ |
| **Hypeddit** `hypeddit.com`                                     | A download gate whose social steps are decoration, apart from Spotify | Yes                            |
| **Droploud** `droploud.com`                                     | A download gate whose steps are self-reported, bar one                | Yes                            |
| **Pump Your Sound** `pumpyoursound.com`                         | A download gate that is a plain HTML form                             | Yes                            |
| **ToneDen** `toneden.io`                                        | A download gate with a real OAuth step                                | Only sometimes — see below     |
| **FanLink** `fanlink.tv` (and the dead `fanlink.to`, rewritten) | ToneDen's smart link; sometimes a gate in disguise                    | Only sometimes                 |
| **Laylo** `laylo.com`                                           | A notify-me signup, not a gate at all — so the tool signs up          | No: the artist mails it to you |
| **Feature.fm** `ffm.to`                                         | A smart link: a chooser of streaming and shop links                   | Rarely                         |

A row's badge says which site it is, and a row that resolves to somewhere to go
rather than something to save says so and offers an **Open** button instead of
reporting a failure it did not have.

("DropCloud" is how the name gets written down; the site spells it Droploud,
and so does the link SoundCloud carries.)

### Use

1. Open the tool from the menu. With **This page** chosen it looks at the
   active tab and picks up what it finds:
   - a **tab on one of the seven sites** — that one link;
   - a **SoundCloud playlist, Likes or track page** — every link on it that
     one of the sites claims, with the track each belongs to (a track page
     offers the one it is about). These are the same
     links the Manager tool groups by site, read the same way (it asks the
     content script for the page's download details).
2. With **Session queue** chosen it takes the links of the queued tracks
   instead, whatever the active tab happens to be — the way to work through a
   batch picked out earlier without the page it came from being open at all.
   Switching between the two rebuilds the list; a link already saved says so
   when it comes back. Closing the popup does not rebuild it — the list, and
   what became of each row, is kept for the browsing session (see
   [What the popup remembers](#what-the-popup-remembers)).
3. Or open **add links by hand**, the drawer at the bottom, and paste links
   into it — a list, a chat log, prose; anything pointing at one of the seven
   is picked out, trailing punctuation trimmed, and SoundCloud's `gate.sc`
   wrapper unwrapped. The
   drawer stays open, and holds what was pasted into it, until the links are
   added or the browser closes (see [What the popup remembers](#what-the-popup-remembers)).
   Links added this way are the ones no source can offer again, so the list
   puts them back itself. The drawer opens itself when there is nothing else
   to show, since it is the only other place a link comes from — but a drawer
   closed by hand stays closed.
4. Two toggles above the list narrow it, and are remembered. **Hide
   downloaded** drops the links whose track is already marked — the same
   option the Manager tool keeps, so it is on here if it is on there. **Hide queued** drops the ones whose track is waiting in the session
   queue; what is left is what has not been picked out yet. A row that has not
   run says which of the two it is in, and a link with no SoundCloud track
   behind it (from a gate tab, or the paste box) is never dropped by either.
   When something is hidden, a line says how many of the links are showing.
5. **Download all**, or **Get** on a single row. Rows run one at a time and
   report as they go: _Unlocking…_, _Saving…_, _Saved_, _Signed up_ for a
   Laylo drop the address was left with, the page it turned out to point at,
   or the reason it did not work. **Download all** runs what is
   listed, so the toggles are also how a batch is chosen — and with **Hide
   downloaded** on, each row goes as soon as saving it marks its track.
6. A row that failed says what to do about it, and offers to do it. Most of
   these gates fail because they want something a fetch cannot give them — a
   connected account, a CAPTCHA, a step the page does to itself — so the failed
   row leads with **Open**, which takes you to the gate itself, or to the file
   when the walk got as far as the file and it was the transfer that broke (an
   interrupted download, no room on the disk). Opening one puts its track in
   the session queue on the way out, so a link left half-done is still on a
   list once the popup has closed behind the new tab, and can be marked from
   the [session queue](#session-queue) when the file has landed. **Get**
   stays beside it, for the failures worth another go on their own.

   The failures that are not a trip to a tab are the ones Settings already
   has the answer to: a Hypeddit gate or a Laylo drop that wants an email
   address and has not been given one offers **add email**, and a drop that
   asks for a name with it offers **add name**. Either opens the settings
   sheet, over the panel and with the list behind it untouched, at the fields
   where those are typed.

Files land in a folder named for the site (`Hypeddit`, `Droploud`,
`PumpYourSound`, `ToneDen`, `Laylo`, `FeatureFM`) under the browser's download
directory, named as the artist named them where the site says so — Hypeddit and
Droploud both sign the real filename into the link, so the name is known before
a byte is fetched.

A name is made safe first, because Chrome refuses a download outright —
_Invalid filename_ — rather than cleaning one up, and holds every name to what
Windows would accept whatever the machine is: separators and the reserved
`"*:<>?|` become `-`, invisible characters (soft hyphens, zero-width spaces,
bidi marks) and dots or spaces at the ends go, device names like `NUL` get a
`_`, and the whole thing is cut to 180 characters without splitting the last
one. If Chrome still will not take it the file is asked for again with no name
at all, so it lands in the download folder under whatever the response calls it
— the row says _Saved, but Chrome renamed it_ — rather than not landing.

Rows that came from a SoundCloud page, or from the queue, carry their track,
so:

- a row whose track is already marked as downloaded, or is waiting in the
  session queue, says so before you spend the download;
- opening a failed one by hand queues that track, which is what the Manager
  tool does when a gate is opened from there — with **Hide queued** on, the row
  goes as it is taken in hand;
- saving one marks that track — the same mark the Manager tool uses and the
  content script paints on the SoundCloud page. **mark the SoundCloud track
  downloaded**, in Settings, turns that off. A row that only resolved to a page
  marks nothing: nothing has been downloaded yet;
- the **check button** on the row makes that mark by hand, and clicking it
  again takes it off. It is on every row with a track behind it, in whatever
  state the walk left it — which is the point, because the tracks that need it
  are the ones a walk did not finish: a Laylo or Feature.fm row that only ever
  resolved to a page you then went to yourself, a gate you opened and clicked
  through in a tab, a file that was on the disk before any of this started, a
  row saved with that setting off. It also undoes a mark made in
  error. With **Hide downloaded** on the row goes as the mark is made, the same
  way a saved one does — so take a mark off before hiding, not after. The same
  button, doing the same thing, is on every row of the [session
  queue](#session-queue) and the Manager tool.

### How each site works

Every site is one module under `sites/`, and they all answer the same two
ways — _here is a file to save_, or _here is somewhere to go_ — or throw with
the sentence the row should show. A thrown error sends the person to the gate
itself, which is what the failed row's **Open** does; a gate that failed for
want of something the panel can ask for instead says so by throwing a `Wanting` from
`next-step.ts` — today the email address, and the name a Laylo drop sometimes
asks for beside it. A third answer joins the two above for Laylo alone: _the
address has been left with the artist_, which is neither a file nor a page.

**Hypeddit.** A gate page is an ordinary form. Every value the download button
posts is already in the page as a hidden input, so the panel fetches the page,
reads them out, and posts them back: `GET /<gate>` for the page (the gate id
`current_download_file_listner`, a per-visit nonce `wrndk`, a visit token
`gvt`, the numeric `fan_gate_id`, the step list `nwSteps`, and the Laravel CSRF
token from the `csrf-token` meta tag); `POST /gate/ge` with `{vt, uid}`, which
is what the page fires on first paint to register the visit; `POST
/verifyEmailAddress` for the gates that ask; then `POST /gate/download/ul` with
the gate's own fields, which answers `{download_status: true, URL}` where `URL`
is a pre-signed S3 link carrying a `Content-Disposition` with the real
filename. The session cookie the first response sets is what ties the nonce to
this visit, so every later request goes out with `credentials: 'include'`.

**What the gate actually checks:** most of it, nothing. The social steps are
decoration — their buttons open the artist's SoundCloud (or Instagram, or
TikTok) page in a popup and remove an `undone` class from themselves. Nothing
is reported back, nothing is recorded, and the download post is not asked about
any of it. Two steps are real, and the tool fakes neither.

**The email step.** A gate whose `nwSteps` include `email` will not unlock
until the address has been through `/verifyEmailAddress`, which answers
`{"status":"T"}` only for an address it believes is deliverable. That address
goes to the artist; it is their side of the bargain and the reason the gate
exists. So the tool asks for one in Settings, remembers it, and sends it only
to the gates that ask. After too many tries on one gate Hypeddit answers
`{"attempts":"over"}` and wants a CAPTCHA; the row says so, and the way out is
to open that gate in a tab once by hand.

**The Spotify step** (`sp`). This one is not reported by the page either — it
is not reported at all. The step's Connect button sends the visitor through
Spotify's own OAuth (`accounts.spotify.com/authorize`, scopes
`user-follow-modify` and `user-library-modify`, back to
`hypeddit.com/spotify_callback`), and Hypeddit then does the following and the
library-saving itself, on a token of its own. Until that has happened
`/gate/download/ul` answers `{"download_status": false, "URL": ""}`, and no
field in the post changes it: a shortened `steps`, a `skip_gate_steps[]`
carrying `sp`, and the page's own `additional_sp_user_id[]` all get the same
refusal, because the check is on the session rather than on the post. So the
tool walks the step rather than claiming it. The gate page carries the step's
own authorize URL, `state` and all, inside the Connect button's `onclick`; the
panel sends a background tab to it and watches for the tab to come back to
hypeddit.com, which is where Spotify leaves it once the app has been allowed.
A visitor who has said yes to Hypeddit's Spotify app before is sent straight
back, about a second and no clicking; one who has not gets a login or a consent
screen a background tab has no business asking them to answer, so the trip is
abandoned, the tab closed, and the row says to press Connect on the gate once —
after which every later `sp` gate goes through without a word. **The follow and
the library save really happen**, which is the point: it is the artist's side
of that bargain, exactly as the address is.

**Droploud.** A `/gate/<track id>` or `/track/<track id>` page ships its whole
gate in the HTML, as part of the Next.js server-component payload: the track,
the artist, and a `steps` list saying which follows, likes, reposts and saves
it asks for. Walking it is `POST api.droploud.com/api/gate/<id>/step` with
`{session_id, step}` for each step, then `POST /gate/<id>/download` with
`{session_id}`, which answers `{download_url, type}`. The session id is not a
credential: the page invents a UUID on first paint and keeps it in
`sessionStorage`, and the server only uses it to tie one visitor's steps
together, so a gate walked here is a fresh visitor exactly as a private window
would be.

**What the gate actually checks:** the download is refused (`403 "Gate not
completed"`) until the steps have been posted, but the steps themselves are
self-reported — the page marks one done the moment its button is clicked, the
click opens the artist's page in a popup, and nothing checks what happens
there. Posting the step is worth exactly what clicking the button is worth,
which is why the tool does it and says so here rather than pretending
otherwise. **One step is different.** A `soundcloud_repost` step is verified
for real: Droploud wants a SoundCloud account connected by OAuth and then reads
that account's reposts back. There is nothing to self-report and nothing here
signs in as anybody, so the tool takes Droploud's own way out — the "give up"
its page offers after a couple of failed checks (`POST
/soundcloud/verify-repost` with `give_up: true`), which drops the step rather
than claiming it. A gate that will not open without it says so on its row, and
is one to open in a tab.

**Pump Your Sound.** The simplest of the set. A fangate page
(`/f/<channel>/<slug>/<id>`) is server-rendered PHP with an ordinary Nette form
in it, `frm-freeDownloadForm`, posting back to the page it is on with
`_do=freeDownloadForm-submit`. The tool reads the form's fields off the page
and posts them, which is exactly what pressing Download does; the answer is the
file, or a page with the link in it. The steps beside the button — follow on
SoundCloud, follow on Instagram, leave a comment — are separate OAuth flows of
their own, not part of the form, and the button is not disabled while they are
undone. Nothing is posted to SoundCloud by submitting the form.

**ToneDen.** The one whose steps have teeth, and the tool does not pretend
otherwise. A `toneden.io/<artist>/post/<slug>` gate says "Follow on SoundCloud"
(or Spotify, or Facebook) and leads to `/auth/<platform>`, an OAuth round trip
that ends with ToneDen holding a token for that account; it performs the follow
itself and opens the gate for the fan it just met.
`GET /api/v1/attachments/<id>/download` answers `401 "Please login to your
account to continue."` to everyone else. So there is nothing to walk, and what
the tool does instead is:

- read the post through ToneDen's own public lookup
  (`GET /api/v1/posts?author_username=&link_path=`), which needs no account, so
  a row can say what the gate is;
- finish the gates whose `download_source` is `redirect` — those are not files
  behind a login at all, they are the artist pointing at a Dropbox link or a
  shop, and that destination is in the post;
- for the rest, ask for the download with whatever ToneDen session the browser
  already has. If you have been through a ToneDen gate once in this browser,
  that is a real unlocked session and the file comes back. If not, the row says
  to open the gate and connect once — which is the artist's side of the bargain
  and not something to fake.

A refusal is not one shape, and the row should not report it as a status code.
Nobody signed in at all gets `401 "Please login to your account to continue."`;
a browser ToneDen _does_ know, whose fan has not been through this particular
gate, gets a `400` with a sentence of its own. So the answer body is read either
way, and a download that comes back no is followed by
`GET /api/v1/attachments/<id>/gateMetadata`, which is public and answers the two
questions worth asking: `is_gate_open` — is this browser through this gate — and
`viewer_id` — does ToneDen know this browser at all. With those the row names
the account the gate wants (`platform`, or `platforms` with `is_or_condition`
for a multi-action gate) instead of the status it answered with: _"this gate
wants Twitch, and ToneDen doesn't know this browser"_. A gate that is open and
still says no keeps ToneDen's own words. A gate whose `platform` is `email`
wants an address left on ToneDen's page rather than an account, and says that;
one with `skip_download` set has no file behind it at all.

**FanLink** (`fanlink.tv`) is ToneDen's smart-link product on the same backend:
a chooser pointing at Spotify, SoundCloud, Beatport and so on, with no file of
its own. The page ships the whole thing as `window.preloadLink`, so the tool
reads the destinations out and offers the one worth opening — a direct file if
there is one, else Bandcamp, Beatport or iTunes, else where the link itself
points. (Not every destination is a web address: a "music" link can carry a
`spotify:album:…` URI, which is for the app and not for a tab, and those are
skipped.)

Two shapes of FanLink are more than a chooser, and the tool follows both. One
has an unlock hung off it (`rsvp_attachment`), which is a ToneDen gate like any
other. The other has no services at all and a `target_url` pointing at a
`toneden.io/…/post/…` — a gate wearing a smart link's clothes; following it is
the difference between offering a page to open and finishing the job.

**fanlink.to**, ToneDen's earlier domain for these links, is gone: every path on
it now answers "Brand not found". The paths came across unchanged, so a `.to`
link is a working `.tv` one with a different suffix, and the tool rewrites it
before the request rather than reporting a dead link. Nothing ever fetches
fanlink.to — which is why the manifest names only `fanlink.tv`. In the registry
that is the site's `aliases`: hostnames it claims but never reaches, and so
deliberately outside `host_permissions`. (`fanlink.to` links written before the
move are still all over SoundCloud, and all six in the export this was built
against resolve on `.tv`.)

**Laylo.** Not a download gate, and the tool does not pretend it is one. A
`laylo.com/<artist>/<slug>` drop is a notify-me page: you leave an email
address or a phone number, and the artist's link reaches you later, by mail or
by text. There is no file on the page and no request that produces one — which
means the signup _is_ the walk, and that is the walk the tool makes.

Reading the drop first. The page is a shell that fetches its own JSON from
Laylo's CDN — the title, the artist's id, which channels the drop is
collecting, what else it asks for — and then fetches the artist's own record
from a second CDN, for their display name and whether they have asked Laylo to
confirm an address by mail before adding it. Both URLs are read off the page
rather than guessed at.

Then the signup, which is the same single POST the page's own RSVP button
makes: `{type: 'rsvp', payload: {productId, creatorId, email, optIn, dropDate,
fingerprintId, …}}` to `events.laylo.com/actions/rsvp/rsvp`, answered
`{success}`. `creatorId` is the one field the endpoint refuses outright
without; everything past it is checked later and out of sight, so `success`
means _taken_, not _subscribed_ — and the row says the former. `optIn` is true
unless the artist's record asks for a confirmation mail first, in which case
the row says to look for it. The `fingerprintId` is a fresh random one per
signup rather than the cookie the page keeps, which exists to recognise the
same visitor from one artist's drop to the next.

The page also attaches a captcha token (reCAPTCHA v3, or Turnstile). That field
is optional — the endpoint takes a signup carrying none — and a forged one is
refused, so none is sent rather than one faked. If Laylo ever makes it
compulsory, the signups come back refused and the row goes back to offering the
drop.

Some drops it leaves alone, and says why: one collecting only a phone number,
one that confirms by text (Laylo offers double opt-in over SMS alone), and one
asking questions beyond a name. There is no phone number here to give and no
way to answer the rest. A drop of the _link_ kind, where the artist put a
destination on it rather than a signup, has somewhere to go, and that goes back
as a link with nothing signed up for.

**Feature.fm.** Another smart link. An `ffm.to` page is a list of the places a
release can be played or bought, and the "download" on it is usually iTunes or
Beatport rather than a file. Every button points at
`api.ffm.to/sl/e/c/<slug>?cd=<payload>` — a click tracker that counts the click
and then redirects — and the payload is base64url JSON with the real
destination (`destUrl`) inside it. So the tool reads every destination out of
the page it already fetched, without a single request to the tracker: faster,
and one fewer fake click in somebody's dashboard. It then offers the best of
them — a direct file, else Bandcamp, Beatport, iTunes or Amazon, else the page.

### Keeping it above board

- **Nothing is faked that anyone can check.** No fabricated follows, likes or
  reposts are sent to SoundCloud or anywhere else. On Hypeddit the social steps
  are neither asked for nor recorded, so there is nothing to fake; on Droploud
  the steps posted are the same self-reported clicks the page posts, and the
  one step Droploud really verifies is given up rather than claimed; on ToneDen
  the gate is enforced by an OAuth account and the tool simply does not walk
  it.
- **A step that is really checked is really done.** Hypeddit's Spotify step is
  walked by going through Spotify's own sign-in in a tab, so the artist is
  genuinely followed and the track genuinely saved to your library — the gate's
  price, paid rather than dodged. It happens only on the gates that ask for it,
  and only on the first press of **Get** for each one.
- **The email step is honoured with a real address, or the gate is not
  unlocked.** The address in Settings goes to two places, both of them the
  artist: Hypeddit gates with an email step, which check it is deliverable
  before they unlock, and Laylo drops, where the signup is the whole of what
  the page is. Nothing else here is ever sent it, it is not written into an
  exported state file, and a row with no address set signs up for nothing — it
  asks for one instead.
- **A Laylo signup is a mailing list, and Settings says so where the address is
  typed.** Handing an address to a drop puts it on that artist's list, which is
  what the drop is for and the price the artist set for the link. The hint
  under the field says it in as many words, so it is not something a **Download
  all** does quietly.
- **One link at a time, a handful of requests each** — roughly what opening the
  page yourself costs the site, minus the pixel and analytics traffic. Every
  one of these is somebody's small server.
- The files are the ones the artist put behind the gate for free download.
  Following the artists whose tracks you keep is still the point of the thing;
  this tool just stops you doing it four clicks at a time.

### Limitations

- **A Laylo drop still ends in your inbox, not in a file.** The tool leaves
  the address; the link is the artist's to send, in their own time, and nothing
  here can hurry it. A signed-up row says so and marks nothing as downloaded.
  Drops collecting only a phone number, confirming by text, or asking questions
  beyond a name are left for you to do on the page.
- **A Hypeddit gate with a Spotify step follows the artist from your account.**
  That is what makes it open, and there is no version of it that does not: the
  check is Hypeddit's, on its own Spotify token, and it is the reason the
  artist put the step there. The first such gate needs one press of **Connect**
  on the page, to say yes to Hypeddit's Spotify app; after that the rest go
  through from the panel, each one opening a tab for about a second. A gate
  wanting Deezer or Apple Music instead is not walked at all — the row names
  what it wants and offers to open it.
- **ToneDen and FanLink gates need one visit by hand** unless the browser
  already carries a ToneDen session that has been through _that_ gate — a
  session alone is not enough, since the follow is recorded per gate. The row
  names the account it wants ("this gate wants Twitch") and offers to open it;
  going through it once in a tab makes that gate, and later gates by the same
  route, work.
- **Feature.fm and FanLink are smart links**, not gates — expect a page to open
  rather than a file, unless the artist put a direct file behind one, or the
  FanLink turns out to point at a ToneDen gate.
- **The fanlink.to rewrite assumes the paths stayed put**, which is what ToneDen
  did when it moved. A `.to` link whose path was _not_ carried over comes back
  as "not a ToneDen FanLink" rather than as a dead domain.
- A **link gate** anywhere (where the "download" is a redirect to Spotify,
  Patreon or a shop rather than a file) has no file to save; the row reports
  where it points and offers to open it.
- A Hypeddit gate behind a CAPTCHA has to be visited once in a tab, which is
  what the failed row's **Open** offers.
- **Pump Your Sound's success path is inferred, not observed.** The form is
  read off the page and submitted as the page submits it; what the server hands
  back for a satisfied gate has not been exercised end to end here, so a row
  that says the form was taken but no file came back may mean the gate wants
  its SoundCloud step done in a tab — or may mean the answer has a shape this
  does not recognise yet.
- A row says _Saved_ when the browser has accepted the transfer, not when the
  bytes have landed — the popup usually closes first. If a transfer is
  interrupted while the panel is still open, the row turns red and offers the
  file itself to open. A transfer that breaks after the popup has closed is the
  browser's own download list to deal with; nothing here hears about it.
- Only tracks rendered in the DOM are read off a SoundCloud page, the same rule
  as the other tools; scroll a long playlist first.
- None of these sites' markup or endpoints is a public API. Everything relied
  on is listed above, and a change to any of it will show up as rows that
  report "not a … download gate", "would not unlock", or a status code.

---

## artists

Lives in `src/tools/artists/`. The other tools are about tracks; this one is
about who made them. Working through a playlist means, sooner or later,
wanting to find the people behind it somewhere other than SoundCloud — to
follow them, to ask for a track nobody will sell you, to credit them — and
SoundCloud puts that on each profile page, one page and a scroll to the links
at a time. This tool collects the uploaders behind the tracks you add, asks
SoundCloud about each of them once, and keeps the answer: a searchable list of
artists, each with their **socials**.

An artist, here, is the uploader of a track — the account the track's
permalink names, which on a playlist page is also the name the row shows.
That path is the artist's identity throughout the tool: it is what SoundCloud
resolves to a profile, what the list holds an artist once under, and what
takes one off again.

### Use

1. Open the tool from the menu. It opens on any tab, with whatever is on the
   list; nothing goes onto the list by itself. The bar at the top says what the
   tab in front is offering — a playlist, Likes page or track page, by name and
   by how many artists are behind its tracks — and **add** is what brings them
   in. Beside it, when either holds anything, are the two sources that work
   from anywhere: **Manager**, the artists behind every track on the Manager's
   list, and **queue**, the ones behind the session queue. Each button counts
   what it would add, and the tab's says **refresh** instead once its artists
   are all here.
2. Adding puts the artists on the list at once, from what the source itself
   says — a name and the tracks — and then, if the tab in front is a SoundCloud
   page, asks SoundCloud about each one it has no answer for. The rows are up
   before the first answer and fill in as the answers land, with a progress
   bar under the header. On any other tab the rows say **not looked up**, and
   a line above the list says how many, and that a SoundCloud tab is needed;
   open one (any page of it) and the **look up** button on that line asks
   about them all.
3. A row is the artist's picture, name (with SoundCloud's verified mark where
   it has one) and a line about them — where they say they are, how many
   follow them, how many links they offer — and then the links as lettered
   badges, up to five on the row and a **+n** for the rest. Each badge opens
   that page, behind the popup or in front depending on the **background
   tabs** switch the Manager also has. An artist SoundCloud had no public
   profile for says **no public profile**.
4. The chevron opens the row into everything else, one field a line, each a
   button that copies it: the SoundCloud profile and every link as an address
   (with an arrow to open it), SoundCloud's own name for the account where the
   page called it something else, the place, the follower count, the bio, the
   tracks that brought the artist here, and where they came in from. Below
   that are the search badges switched on in **Settings → search badges** —
   the same shops the Manager searches, given the artist's name — and the two
   ways out: **look up again**, which asks SoundCloud about this one artist
   afresh, and **off the list**.
5. The search box finds an artist by anything known about them: name, path,
   place, bio, the address or name of any link, the titles of their tracks.
   The sort menu offers **by name**, **list order** (the order they were added)
   and **most followed**. **copy** in the header puts every artist the search
   leaves showing on the clipboard as text — a block per artist: the name, the
   profile, then every link on its own line.

The list is kept for good, in the extension's own storage beside the Manager's
(see [What the popup remembers](#what-the-popup-remembers)), and travels in
the state file with it (see [Saving and restoring it all](#saving-and-restoring-it-all)).
It holds two thousand artists and refuses past that, saying how many did not
fit, rather than dropping the oldest.

### Where the socials come from

Two places, and the row does not distinguish them: the links the uploader put
on their SoundCloud profile, which the API lists, and whatever addresses they
wrote into their bio — which is where a good many of them put the links that
matter. The bio is read for full addresses, `www.` ones, a known host with a
path after it (`linktr.ee/bicep`), and email addresses; a bare word with a dot
in it is left alone. An address the profile already lists is not listed twice
for appearing in the bio too.

Which network a link is on is read off its address rather than trusted from
SoundCloud's own `network` field, so a link written into a bio is filed exactly
as one put on the profile would be, and the field is the fallback for a host
the tool does not know. The badges are lettered monograms in each network's
colour rather than the sites' logos, so the panel ships no third-party artwork
and fetches nothing:

| Badge | Network                              |
| ----- | ------------------------------------ |
| IG    | Instagram                            |
| X     | X (Twitter)                          |
| fb    | Facebook                             |
| YT    | YouTube                              |
| TT    | TikTok                               |
| TH    | Threads                              |
| SP    | Spotify                              |
| AM    | Apple Music                          |
| bc    | Bandcamp                             |
| BP    | Beatport                             |
| MC    | Mixcloud                             |
| RA    | Resident Advisor                     |
| DG    | Discogs                              |
| TW    | Twitch                               |
| DC    | Discord                              |
| TG    | Telegram                             |
| SN    | Snapchat                             |
| PT    | Patreon                              |
| LT    | Linktree                             |
| sc    | SoundCloud                           |
| @     | an email address                     |
| www   | any other website, named by its host |

The catalogue is `NETWORKS` in `src/tools/artists/profiles.ts`; adding one is
an entry there.

### How it works, and what it costs SoundCloud

Two requests per artist, and only when somebody presses a button for it: a
`/resolve` of the profile URL for the user object — id, name, avatar, place,
follower and track counts, bio — and then `/users/<id>/web-profiles` for the
links. Both go to the same public API the Manager's lookups use, on the same
terms: from the SoundCloud page's own origin (the content script makes them,
`GET_ARTIST_PROFILES`), with the public client id the page was served with,
anonymously, `GET` only. Four artists are asked about at a time, the
concurrency the Manager's lookups were measured at; a `429` is backed off and
retried; a `401` — the page's client id no longer accepted — stops the lot,
since it says nothing about any one artist. The `/resolve` half is shared with
the Likes lookup and kept for the life of the page, so a profile asked about
twice on one page is one request.

Nothing is cached in the content script for this. The panel keeps what it is
told on the entry it asked for, asks only about the artists it has no answer
for, and a failure to find a profile is remembered on the entry too, so a
deleted or private account is not asked about on every press — **look up
again** on the row is what asks about one afresh. A lookup cut short by the
popup closing keeps the answers that had arrived, bar the last second's: the
list is written back once per burst of answers rather than once per answer,
and the rest say **not looked up** until the button is pressed again.

What the panel keeps of a profile is the subset above — the bio cut off at a
thousand characters, at most twenty links, and only those that are `http(s)`
addresses.

### Keeping it above board

- Reads only what a signed-out visitor can see: the public profile and the
  links the uploader chose to put on it, via the public API the site's own
  front end uses. No cookies, no session, no auth header.
- `GET` only, and only on a press. Nothing is prefetched, nothing runs in the
  background, and nothing about the artists — the list, the answers — leaves
  the browser.
- The bio is read as text and never executed; the addresses found in it are
  opened only when a badge is pressed.

### Limitations

- An artist is an uploader. On a playlist of reposts and bootlegs the
  uploader is the channel, not the person who made the track, and that is
  whose profile comes back; the tool does not read artists out of titles the
  way the text tool's "pull the artist out" option does.
- A SoundCloud tab has to be in front for a lookup — any page of it — because
  the API takes requests from the page's origin and from nowhere else. The
  list itself is on screen on any tab.
- What counts as a link in a bio is a guess at prose: an address with a
  scheme, a `www.`, or a known host with a path. A bare domain with none of
  those is not picked up, and an address a sentence ends with a bracket is
  trimmed of the bracket.
- The networks table above is what gets a lettered badge; anything else is a
  **www** badge named by its host, which is not wrong but is not a name.
- Only tracks rendered in the DOM are read off a SoundCloud page, the same
  rule as the other tools; scroll a long playlist first.

---

## session queue

Not a tool: a button in the header of every view, and a sheet it opens over
whatever is on screen. `src/lib/components/QueueButton.svelte` and
`QueueSheet.svelte`, drawn by `App.svelte`. The tracks picked out while working
through a page, held together until something is done with the whole batch.

Why it exists: opening a download link used to mark its track as downloaded on
the spot, which said something that was not known yet — the file had not been
saved, and often the gate behind the link had not even been walked. The mark is
worth more than that: it is what the "Downloaded" pill on the SoundCloud page
means, and what **Hide downloaded** hides. So a track opened from the Manager
tool goes into this queue instead, and the mark is made from here, once, for
the batch — after the files really have been saved.

Why it is in the header: it was the fourth entry in the tool menu to begin
with, which made the one thing that is _about_ the other tools a place you had
to go instead of something you had to hand. Filling it from Manager and then
looking at it meant leaving Manager, and coming back meant reading the page
again. In the header it is in front of you wherever you are — the button
carries the count — and the sheet goes away leaving the tool under it exactly
as it was.

### Use

1. Fill it from the **Manager** tool: opening a download link queues that
   track, the list button on a row queues one without opening anything, and the
   tick-box button there selects a batch of rows that **Queue** then puts in
   here in one write.
2. Open the queue from the header of any view — the menu, or whichever tool is
   in front. Every queued track is listed with its artwork, title and artist,
   and the ones already marked as downloaded are tinted green; the line under
   the heading says how many. Escape, the **×**, or a click on the ground
   behind the sheet puts it away.
3. **Mark N tracks as downloaded** marks every track in the queue that is not
   marked yet, in one write. They stay listed, so the batch can be worked
   through further; the button then says _all marked downloaded_.
4. **Take N marked out** — under it, and only while something in the queue is
   marked — takes exactly the marked tracks out of the queue in one write, so
   what is left is what is left to do. The marks are untouched: they live in
   local storage and outlast the session, which is why this asks nothing
   before doing it, unlike **Clear**. When every track in the queue is marked
   it reads _take all marked out_, and the queue is empty afterwards.
5. **Send the queue somewhere** hands the batch to the Playlist to Text or
   third-party downloader tool. Each opens on the queue rather than on the
   active tab (the picker at the top of those two panels switches back); the
   tab can be anything at all, or nothing. The sheet closes behind it: it was a
   way of getting there, not somewhere to come back to.
6. Per row: the check button toggles that one mark, the **×** takes the track
   out of the queue (its mark, if any, stays). **Clear** in the sheet's header
   empties the queue — it asks once more, and it removes no marks.

Whether the sheet is up is kept for the session like everything else about
where you were (see "What the popup remembers"), so the popup closing behind a
click does not put it away.

### Where it lives

In `chrome.storage.session`, key `queue`, an array of entries in the order they
were queued. Each is the track as the tools that read the queue need it: its
permalink path, title, artist, artwork URL and the download details that were
looked up for it (which is how the Downloader finds its gate without a page
to read).

Session storage is the extension's own, for the length of the browsing session:
the queue survives the popup closing — which it does on every click that opens
a tab — and going back and forth between tools, and is gone when the browser
is. That is the whole point of a _session_ queue: it is a working list, not a
library. The library is the Manager's own list, and the marks are what a
worked-through queue leaves behind; both live in `chrome.storage.local`. A
batch worth keeping past today is one to add to that list — the queue is a
second source in the Manager's add bar for exactly that.

The queue is nonetheless in the [exported state
file](#saving-and-restoring-it-all), which is the one way past that: exporting
before the browser closes and importing afterwards carries a
half-worked-through batch over, and importing adds to whatever is queued then
rather than replacing it — unless the import is one that was asked to replace,
which makes the queue the file's queue and nothing else. It restores a working
list, not part of the library — the tracks come back queued, not marked.

Both directions go through the queue rather than through the storage under it.
Exporting asks it for what it holds, so a queue it could only keep in memory —
this store carries on without session storage, minus what the page shows of
it —
is in the file like any other; importing queues the file's tracks with the
same call a row's list button makes, which is what keeps a track that is
already in the queue from landing in it a second time. (A replacing import
goes through the same store by the other door, which empties it first and then
takes the file's tracks — a track the file names twice still lands once.)

The content script does see the queue, which is why hovering cover art there
offers a **+ queue** button, and why on a track already in the queue that
button is there without hovering, reading _queued_ (see
[queueing from the page](#queueing-from-the-page)). Session storage is closed
to page contexts until a trusted one opens it, and the popup does that as the
queue starts (`setAccessLevel`); the page reads the queue, and writes it only
through that one button, which replaces the whole list the same way the store
does. Until the popup has been opened once in a browsing session the page can
do neither, and shows no button at all.

### Limitations

- One queue, not one per playlist. It is a working list for the session, and
  clearing it is a click.
- The sheet is as tall as the popup lets it be. A browser-action popup is only
  as tall as its content, so the shell gives the view underneath a floor while
  the sheet is up rather than letting a short tool make a short sheet.
- A track with no permalink cannot be queued: there would be nothing to tell
  it apart from another, and nothing to mark when its turn came. Cover art on
  a page that names no track — an ad in the player bar — carries no queue
  button for the same reason.
- Nothing expires from it, and marking a track does not take it out on its
  own — what was worked through stays visible until it is taken out, either by
  **Take N marked out**, by the row's **×**, or by clearing the queue. That is
  deliberate: a mark made from here is often made before the file is safely
  saved, and a row that vanished as it was marked would take the way back with
  it.
