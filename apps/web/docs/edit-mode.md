# Edit mode

How a page's people and staff change a page from the page itself: edit mode, the edit bar, and creating something as an empty page. This replaces "changes happen in the dashboard" ([ADR 0017](../../../docs/adr/0017-pages-are-edited-in-place.md) supersedes [0001](../../../docs/adr/0001-changes-happen-in-the-dashboard.md)) for every page that shows one thing. The dashboard keeps the inbox, bookings, plans, settings, onboarding and the lists of your things.

Storybook shows every state below under **Patterns/Edit mode** (`EditBar.stories.svelte`, on the small act page in `EditBarFrame.svelte`), at 1280 px in the dark palette and 390 px in the light one. `EditPageHarness.svelte` is the same page with the real `EditMode` around it, which its tests (`edit-mode.svelte.test.ts`) drive by hand.

## Decisions

Settled on 2026-11-15:

1. The bar floats at the bottom of the window.
2. The switcher stays in the header's account menu.
3. Drafts are new record data on acts and parties, landed separately first.
4. Unsaved edits are kept in this browser with `FormDraft`, and come back after leaving or signing in again.
5. E opens the editor while keyboard shortcuts are on; Cmd/Ctrl+S saves.
6. Staff get the marked staff part and Edit as staff, and every staff session and change is logged with field-level before and after values.
7. Fields in edit mode are a dashed outline around the text, in its own font and size.

## The model

A page that shows one thing has three ways of being drawn for someone who may change it:

| Phase   | What it shows                                                                                            | The bar                                                         |
| ------- | -------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| View    | The page as everyone sees it, plus what only its people see (booking details, a "not public yet" notice) | Edit, View as visitor, the menu                                 |
| Edit    | The same layout, each section's text swapped for a field in the same box                                 | What's unsaved, Discard, Save (a draft: Save draft and Publish) |
| Visitor | The page with the viewer's rights set aside: no private sections, no notices                             | One pill, Back to editing                                       |

Edit is a mode, never a separate page. Its URL is the page's own with `?edit`, so a reload, a link from onboarding (`/@me?edit#bio`) or a no-script visit lands in it. Everything in edit mode is one form that posts to the page's own `?/save` action, which calls the service the dashboard calls today (`profiles.save`, `sharedSpaces.updatePage`, `parties.updatePage`, `events.update`). Without scripts the form still saves; scripts add the bar's states, the toast and keeping a draft.

There's one Save. Nothing writes as you type: an autosave would trip the stale-write check every time two people edit, and it would mean a toast or spinner on every field.

## Components

All in `src/lib/components/edit/`.

| Component       | What it is                                                                                                                                                                                                                                                                                      |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `EditMode`      | What a page wraps its content in. Draws the page alone for visitors; for its people and staff, the bar, and in edit mode the one form. Loads the editor (`editor.ts`) on demand.                                                                                                                |
| `EditSession`   | The editor once loaded: the draft, what's unsaved, saving and its answers, the leave question, Cmd/Ctrl+S.                                                                                                                                                                                      |
| `EditBar`       | The one bar for owners and staff, fixed to the bottom of the window. Owns the phase buttons and the status line. A `panel` snippet opens above its row, attached to it: the menu or a conflict.                                                                                                 |
| `EditMenu`      | The bar's menu: the page's own part (who it shows to, people, inbox, the way to its dashboard page) and the marked staff part.                                                                                                                                                                  |
| `InlineText`    | Text turned into its own field: same font, size and box as the text, a dashed outline drawn outside the box, `field-sizing: content` so it grows with what's typed. One line or several. Its type class goes on it (`class="text-display"`), since the forms plugin sets every textarea's size. |
| `AddSlot`       | Where an empty section sits in edit mode, so an empty page still has every section in its place.                                                                                                                                                                                                |
| `ConflictPanel` | The stale-write choice, field by field.                                                                                                                                                                                                                                                         |
| `SaveToast`     | What a save, publish or discard says, always at the bottom middle, above the bar.                                                                                                                                                                                                               |
| `LeaveDialog`   | The question before leaving with unsaved changes by a link inside the site.                                                                                                                                                                                                                     |
| `types.ts`      | `EditPhase`, `EditActivity`, `PageStatus`, `EditRole`, `ConflictField`, `PageContext`, `EditView`, `EditRefusal`.                                                                                                                                                                               |
| `fields.ts`     | Reading and writing a form's values, and a conflict's merge.                                                                                                                                                                                                                                    |

Sections that need more than text (place, genres, fees, slots, links) use the site's existing controls (`input`, `chip-toggle`, `select.input`, the place picker) at `btn-sm` height, inside the card the section already has. Fields that don't fit a card in place (plays on, set length, rooms, travel) sit behind one ghost button at the card's foot that opens them in the card.

## States

Every state has a story at 390 and 1280 px, dark and light.

| State                         | What happens                                                                                            | Where it says so                                                                                 |
| ----------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| View                          | Edit is drawn by the server from the page's `can`                                                       | Bar: the page's name and who it shows to                                                         |
| Opening                       | Edit was pressed and the editor's code is loading. Usually under 100 ms after a hover or focus prefetch | Edit shows a spinner in place of its icon                                                        |
| Editor failed to load         | The chunk didn't arrive                                                                                 | Bar: "Couldn't open the editor", Try again                                                       |
| Editing, nothing changed      | Discard reads "Done", Save is disabled                                                                  | Bar: "Editing"                                                                                   |
| Unsaved                       | A dot and "Unsaved changes", kept in this browser (`FormDraft`) until saved or discarded                | Bar                                                                                              |
| Saving                        | Fields go read-only, Save shows a spinner and "Saving", everything else in the bar is disabled          | Bar                                                                                              |
| Saved                         | Back to view, focus returns to Edit                                                                     | Toast: "Saved"                                                                                   |
| Refused field                 | The server named a field: it gets the reason under it and focus                                         | The field, plus the bar's status                                                                 |
| Save failed                   | Network or server error; nothing lost                                                                   | Bar: "Couldn't save" and the reason, Save stays enabled                                          |
| Conflict                      | The `swap` CID or `cid` didn't match: someone saved since editing began                                 | The bar's panel opens with each field both changed, theirs and yours; Save again keeps the picks |
| Sign-in ran out, on load      | The page knows from the session before drawing (`needsSignIn`)                                          | Bar: "Your sign-in ran out", Sign in to edit in place of Edit                                    |
| Sign-in ran out, on save      | A rare race: the session ended while editing                                                            | Bar: "Sign in again to save", the changes stay in this browser and come back after sign-in       |
| Leaving                       | A link inside the site, with changes                                                                    | `LeaveDialog`, Keep editing focused. Reload or leaving the site gets the browser's own question  |
| View as visitor               | `can` emptied for drawing only                                                                          | The bar shrinks to one pill                                                                      |
| Discarded                     | Changes dropped, back to view                                                                           | Toast: "Changes discarded" with Undo for 8 seconds                                               |
| Draft                         | A page being created, visible only to its people                                                        | Notice at the top, bar: "Draft", Save draft and Publish                                          |
| Draft missing required fields | Publish stays pressable but says what's missing and focuses the field                                   | Bar: "Needs a name", the field's error                                                           |
| Published                     | Draft becomes unlisted (acts wait for staff to list them)                                               | Toast                                                                                            |

Failures never toast. A toast goes away by itself; an error has to stay until it's dealt with, so it stays in the bar, which is always in the same place.

## Creating as an empty page

`/party/new` and `/party/<key>/new` for an event, and later `/act/new`, draw the real page layout with nothing in it, straight into edit mode. Every section is in its place: text fields show placeholder copy, cards with nothing in them become `AddSlot`s, the picture is a dashed square with a camera button. A notice at the top says it's a draft only you can see.

Saving a draft creates the thing (`createAct`, `parties.start`, `events.create`) with a draft status, and from then on the URL is the real one. Publish needs the required fields (a name; for an event a venue and a date, which the bar lists as what the draft still needs) and moves it on: an act to unlisted, waiting for staff, and a party or event to public. The draft status is new data on act and party records. It lands as its own small change before acts move to edit mode, so the page work only reads it.

## The edit bar

One bar for everyone who can change the page they're on, drawn by the server whenever the page's load gives the viewer a capability on it, or the viewer is staff. Nobody else's page data says the bar exists (the same rule as the staff toolbar's `pageSubject`).

- **Place.** Fixed to the bottom. On a phone it's full width with the safe-area inset; from 640 px it's a floating card, `max-w-3xl`, 16 px off the bottom. It never covers the page's end: the body gets the bar's height as bottom padding, drawn with the page, and `scroll-padding-bottom` keeps a focused field from scrolling under it.
- **One row, one height.** 64 px in every state. Left: what's going on in two lines (title, detail), with an icon for errors and a dot for unsaved. Right: the buttons, the primary last, so Save and Edit are always under the same thumb.
- **Phone.** The row keeps the status, the menu and the two buttons that move the edit on. "View as visitor" is an icon button with its label for screen readers, and while editing it moves into the menu; a draft's Discard does too, so Save draft and Publish fit at 360 px.
- **Panel.** The menu and a conflict open above the row, inside the bar's own card, capped at 30 rem and scrolling inside. It's not a dialog: the page stays usable.

### Owner part

1. **Shows to**: draft (its people), unlisted (its people and staff, with "Ask to list it"), everyone, or for a private event anyone with its link.
2. **People**: the first few, and Manage, which opens the people panel.
3. **Inbox**: what's waiting about this page, linking to the inbox filtered to it.
4. **Dashboard**: bookings, press kits and settings for this thing.

### Staff part

Marked with a Staff label, a warning-tinted background and "Everything here is logged as staff". Today it holds Edit as staff and "This page's audit log" (the staff console's log searched for the page's URI). Once the staff toolbar (LOC-514) lands, it takes what that holds: listing (List it, Hide), the flags that show on this kind of page ("Shows on", formerly flag contexts) as chip switches, and the record (in the directory, copy URI). The staff toolbar's action registry (one file per action, `contexts` by subject kind, `needs`) becomes the source for both parts, with `needs` taking a capability for owner actions and `admin` or a staff permission for staff ones.

Staff on a page they aren't one of the people of see only the staff part, a Staff mark beside the page's name, and "Edit as staff". Staff who are also one of the page's people see both parts, owner first, and edit as one of its people.

A person's profile lives in their own repo and only their own session can write it, so staff get no Edit there: the bar says "Only its owner can edit it" and the menu's staff part is all they have. Edit as staff applies to what the root writes: acts, parties, venues and events.

### Logging staff edits

Every Edit as staff session is in the audit log, from opening to the last save, so nobody has to trust that a staff change was small.

1. **Opening.** The load of a page at `?edit` writes a `page.editAsStaff` entry before the editor draws (`openEditor`): the staff member's DID and role, the page as subject (its URI, and its kind in the details), and the time. Every way in goes through that load (the button, a link, a reload, no scripts), and the staff Edit link isn't preloaded on hover. It's written even if nothing is saved after, so looking is on record too.
2. **Each save.** The save writes `page.update` (or the page kind's own action) under the staff member's role, with `changes` listing every field that changed and its before and after values, the same field-level shape the audit log keeps for revised requests: `savedChanges(before, after)` in [diff.ts](../src/lib/server/services/diff.ts), the record it read against the one it writes. An act's `page.update` ([shared-spaces.ts](../src/lib/server/services/shared-spaces.ts)) already does, for its people's saves too.
3. **Conflicts and publishes** are saves, so they log the same way. A conflict's entry carries the fields as saved, after the picks.
4. **Where it shows.** The staff console's audit log, filtered to the page from the menu's "This page's audit log", lists the session as its entries with the staff role marked; each entry's page shows the changes table. Like every audit entry, both copies (the actor's and the root's) go through the hash chain.
5. **What the editor sees.** The bar carries a Staff mark and says so the whole time: "Opening this was logged", then "Logged as staff on save" once there are changes. Nothing about it is hidden in a menu.

### What else the bar could centralize

Proposed, in the order they'd add value:

1. **Publish state**: in, as above.
2. **People**: in, as a row linking to the people panel. Editing roles stays in the panel.
3. **Inbox count for this page**: in, as a row. The site header keeps the global count.
4. **Flags**: staff part only, as above.
5. **The switcher** (acting as one of your acts or parties): stays in the header's account menu. It changes who you are on every page, signed in or editing, so it belongs where it's the same everywhere.
6. **Press kit and booking shortcuts**: later, as owner actions from the registry ("Add an event", "New press kit"), once those pages exist.

## Motion

Short and quiet; nothing on the page itself moves.

- Switching phases swaps content in place. There's no transition on the page, since a fade would read as loading.
- The toast rises 8 px and fades in over 180 ms (`cubic-bezier(0.16, 1, 0.3, 1)`), and goes without motion. Under reduced motion it only fades.
- Spinners (`LoaderCircle`) spin only under `motion-safe`.
- The bar's panel opens without animation, so the row it's attached to never moves.

## Keyboard and focus

- **E** opens the editor while the footer's keyboard shortcuts switch is on (ignored while typing), and the Edit button carries `aria-keyshortcuts` only then, as the staff toolbar's Shift+S does.
- **Cmd/Ctrl+S** saves in edit mode (and saves a draft), and the browser's own save is prevented there.
- **Escape** closes the bar's panel and returns focus to its button. In edit mode with nothing open it does nothing, so Escape never throws work away.
- **Entering edit mode** focuses the first field (the name), so typing starts at once.
- **Leaving edit mode** (Save, Discard, Done) returns focus to Edit.
- **A refused save** focuses the field the server named and puts the reason under it, through the same `aria-invalid` and `aria-describedby` that `FormActions` uses.
- **Order.** The page's fields in reading order, then the bar: menu, View as visitor, Discard, Save. The bar comes last in the DOM, so Tab from the last field reaches Save.
- **Announcements.** The bar's status line is `role=status` and `aria-live=polite`; the toast is its own polite status. Errors are announced by the status line, never by an alert dialog.
- **Fields without visible labels** (the name, the tagline) carry an `sr-only` label naming them, since they sit where their text was.
- **Leave dialog** is a native `<dialog>` with Keep editing autofocused, so Enter and Escape both stay.

## Loading only for editors

What every visitor downloads, about 173 KiB of uncompressed JavaScript, may grow by 1 KiB in a pull request ([benchmarks/budgets/web.json](../../../benchmarks/budgets/web.json), measured by [scripts/bundle-sizes.ts](../scripts/bundle-sizes.ts)). Edit mode spends almost none of it.

- **In the first load**: nothing. The bar is server-rendered HTML.
- **In each page's own chunk**: `EditMode`, `EditBar`, `InlineText` and `AddSlot`, which the server draws the bar and a `?edit` form with, and the dynamic import call.
- **Loaded for editors only**, with `import('./editor.ts')`, when Edit is pressed (prefetched when the pointer or focus reaches it), the menu opens, or a page opens at `?edit`: `EditSession` with `LeaveDialog`, `EditMenu`, `ConflictPanel` and `SaveToast`, one editing chunk. Everything in `src/lib/components/edit/` is kept out of the `shared` group in `vite.config.ts`, so it never reaches what every visitor downloads.
- **Without scripts**: `?edit` is drawn by the server with plain fields and a submit button, and saving goes through the form action.
- A build test asserts the editing chunk is not in any visitor's initial load, as it already does for the staff toolbar.

The CSS follows the same split. Vite gives a chunk loaded with `import()` its own stylesheet, and its preload helper links that stylesheet and waits for it before the import resolves, so the editor's styles are in place when it draws. What a visitor's page gets from edit mode is only what the server-drawn bar needs: `EditBar`'s classes and its rule that keeps the bar's height free at the end of the page.

- **Classes.** Tailwind writes one stylesheet, which every page loads, from the classes in every file. The components only editing draws (the editor's, `InlineText` and `AddSlot`) use only classes the rest of the site uses too, so they add nothing to it; what's theirs alone (the toast's place, the menu's staff tint and overlapping faces) is in their own `<style>`, which goes with the editor's chunk.
- **The fields.** `InlineText` draws its box (no padding, border or background, `field-sizing: content`, the type from its `class`) with the site's classes, so a page opened at `?edit` has the box before any script runs and nothing moves. The dashed outline and its hover, focus, read-only and refused states are `inline-text.css`, which `editor.ts` imports. Until the editor arrives, and without scripts, the fields have the box but no outline.
- **The check.** `tests/build.test.ts` fails when a class only these components use lands in the stylesheet every page loads, or when their own styles (Svelte's scoping class for each, or an attribute their stylesheet selects) land in any page's. The CSS budget counts what visitors can download, so the editor's stylesheet isn't in it.

## Why Edit is drawn by the server

Lighthouse fails a page whose cumulative layout shift is over 0.02 ([lighthouserc.cjs](../lighthouserc.cjs)). If the bar or the Edit button appeared after hydration, every editor's page would shift once on each load, and the body padding that keeps the bar off the footer would land late too. So the load works out `can` for the viewer, the server draws the bar and the padding into the first HTML, and hydration only makes it interactive. Visitors get neither, so their pages are unchanged.

Inside the page the same rule holds: each field takes the box its text took, the outline is drawn outside the box (`outline-offset`), and an empty section's `AddSlot` exists only in edit mode, which is a deliberate switch the editor asked for. A signed-in owner run joins the Lighthouse pages (`pages.json` supports `account`) once the first page has edit mode, to check the view phase with the bar.

## Caching

`cacheBriefly` lets a signed-in viewer's browser keep a page for 60 seconds (300 on mixes). Someone who can edit a page would then see the old page after saving and reloading, so a page passes the viewer's `can` (`cacheBriefly(event, 60, can)`) and answers with `no-store` whenever it isn't empty.

## What the server adds

From `src/lib/server/edit-mode.ts`. Every page that shows one thing spreads one helper into its load data, which the staff toolbar's staff-only `pageSubject` folds into when it lands:

```ts
const context = pageContext(locals, 'party', uri, can);
// { subject: { kind, uri }, can: Capability[], role: 'owner' | 'owner-staff' | 'staff' }
// or nothing at all for anyone without a capability who isn't staff
return { ...context, editing: await openEditor(locals, url, context, name), ... };
```

`can` comes from the space's person record for acts, parties, venues and events (`capabilitiesIn` on the viewer's person record), and is everything the space offers for staff. The page draws Edit when `can` includes `page.edit`; on an event that's whoever holds the party's `events.manage`, since editing an event is running the party's events. `openEditor` says whether the page opens in edit mode (`?edit`, and `page.edit`) and logs staff opening it.

The page's one action is `save`, through `savePage(event, form, save, latest)`: `save(version)` calls the page's service with the hidden `cid` the form opened on, `latest()` reads the page's current values and version for a conflict. A saved page redirects back to itself without `?edit`; a refused one returns an `EditRefusal` (the field, a sign-in, the conflict's latest values, or a `choice` the save needs, which the page asks as buttons that save again with the answer, so it works without scripts too; an event asks it when its times would move onto another clock). Services throw a stale write as a `UserError` with status 409.

## Order of pages

After this design is approved: profile, act, party and event (built in place from the start), press kit, venue, mix, show. Projects are skipped, since they become performers.

## Still to change elsewhere

1. [ADR 0002](../../../docs/adr/0002-shared-press-kits-in-root-owned-spaces.md): the press kit page asks before overwriting a field, not the dashboard, once press kits move to edit mode.
2. `ManageLink.svelte` goes once every page type has edit mode.
